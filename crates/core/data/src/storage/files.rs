//! [`FileStorage`]: MeeGone's files on the device.
//!
//! This is the only code in the app that touches the file system, so it is on arch-check's
//! `OS_ACCESS_ALLOWED` list (ARCH-04, ADR 0004).

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::{MAX_STORED_BYTES, StorageAdapter, StoredFile, check_budget};
use crate::model::DataError;

/// Keeps each [`StoredFile`] as a file under one root folder.
///
/// A write goes to a temporary file next to the target, which is flushed to the device and then
/// renamed over the target. The rename is atomic, so a crash or power loss at any point leaves
/// either the old file or the new one (REL-03).
pub struct FileStorage {
    root: PathBuf,
    budget: u64,
    /// One write or removal at a time, so two writes never share a temporary file.
    writing: Mutex<()>,
    #[cfg(test)]
    crash: Mutex<Option<Crash>>,
}

impl FileStorage {
    /// Storage under `root`, which must be the app's private internal storage, such as
    /// Android's `Context.getFilesDir()`. The system deletes it when MeeGone is uninstalled or
    /// its storage is cleared, which removes everything MeeGone stored (PRIV-05). Never shared
    /// or external storage (SEC-03).
    ///
    /// Nothing is read or created until a file is written.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            budget: MAX_STORED_BYTES,
            writing: Mutex::new(()),
            #[cfg(test)]
            crash: Mutex::new(None),
        }
    }

    fn path(&self, file: StoredFile) -> PathBuf {
        file.path()
            .split('/')
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    /// The bytes every stored file but `except` takes.
    fn stored_bytes_except(&self, except: StoredFile) -> io::Result<u64> {
        let mut total = 0;
        for file in StoredFile::ALL.into_iter().filter(|file| *file != except) {
            total += match fs::metadata(self.path(file)) {
                Ok(metadata) => metadata.len(),
                Err(error) if error.kind() == ErrorKind::NotFound => 0,
                Err(error) => return Err(error),
            };
        }
        Ok(total)
    }

    fn replace(&self, file: StoredFile, bytes: &[u8]) -> Result<(), DataError> {
        let path = self.path(file);
        let folder = path.parent().unwrap_or(&self.root);
        fs::create_dir_all(folder).map_err(|error| failure(&error))?;
        let others = self
            .stored_bytes_except(file)
            .map_err(|error| failure(&error))?;
        check_budget(others, bytes.len(), self.budget)?;

        let temporary = temporary(&path);
        self.write_temporary(&temporary, bytes)
            .and_then(|()| self.crash_before_rename())
            .and_then(|()| fs::rename(&temporary, &path))
            .and_then(|()| sync_folder(folder))
            .map_err(|error| failure(&error))
    }

    fn write_temporary(&self, temporary: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut out = File::create(temporary)?;
        #[cfg(test)]
        if let Some(Crash::AfterBytes(written)) = *self.crash.lock().unwrap() {
            out.write_all(&bytes[..written.min(bytes.len())])?;
            return Err(io::Error::other("crashed while writing"));
        }
        out.write_all(bytes)?;
        out.sync_all()
    }

    #[cfg(test)]
    fn crash_before_rename(&self) -> io::Result<()> {
        match *self.crash.lock().unwrap() {
            Some(Crash::BeforeRename) => Err(io::Error::other("crashed before renaming")),
            _ => Ok(()),
        }
    }

    #[cfg(not(test))]
    fn crash_before_rename(&self) -> io::Result<()> {
        Ok(())
    }
}

impl StorageAdapter for FileStorage {
    fn read(&self, file: StoredFile) -> Result<Option<Vec<u8>>, DataError> {
        match fs::read(self.path(file)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(failure(&error)),
        }
    }

    fn write(&self, file: StoredFile, bytes: &[u8]) -> Result<(), DataError> {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        self.replace(file, bytes)
    }

    fn remove(&self, file: StoredFile) -> Result<(), DataError> {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let path = self.path(file);
        match fs::remove_file(&path) {
            Ok(()) => sync_folder(path.parent().unwrap_or(&self.root)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
        .map_err(|error| failure(&error))
    }
}

/// `path` with `.tmp` added, in the same folder, so the rename stays on one file system.
fn temporary(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(OsString::from).unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Makes a rename or removal in `folder` survive a power loss.
#[cfg(unix)]
fn sync_folder(folder: &Path) -> io::Result<()> {
    File::open(folder)?.sync_all()
}

#[cfg(not(unix))]
fn sync_folder(_folder: &Path) -> io::Result<()> {
    Ok(())
}

fn failure(error: &io::Error) -> DataError {
    match error.kind() {
        ErrorKind::StorageFull => DataError::Failed("Not enough storage space".into()),
        _ => DataError::Failed(format!("Couldn't use saved data ({error})")),
    }
}

/// Where a test makes a write stop, as if the process had died there.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
enum Crash {
    /// After this many bytes of the temporary file.
    AfterBytes(usize),
    /// After the temporary file is complete, before it replaces the target.
    BeforeRename,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::fakes::app_id;
    use crate::settings::{Settings, ThemeChoice};
    use crate::storage::{encode, load, save};

    /// A fresh folder, deleted when the test ends.
    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "meegone-storage-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }

        /// Every file under the folder, relative to it.
        fn files(&self) -> BTreeSet<String> {
            fn walk(root: &Path, folder: &Path, files: &mut BTreeSet<String>) {
                for entry in fs::read_dir(folder).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        walk(root, &path, files);
                    } else {
                        let relative = path.strip_prefix(root).unwrap();
                        files.insert(relative.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
            let mut files = BTreeSet::new();
            walk(&self.0, &self.0, &mut files);
            files
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn crashing(folder: &Folder, crash: Crash) -> FileStorage {
        let storage = FileStorage::new(&folder.0);
        *storage.crash.lock().unwrap() = Some(crash);
        storage
    }

    fn settings(theme: ThemeChoice) -> Settings {
        let mut settings = Settings {
            theme,
            ..Settings::default()
        };
        settings.hidden_apps.insert(app_id("games"));
        settings
    }

    #[test]
    fn creating_storage_touches_nothing() {
        let folder = Folder::new();

        let storage = FileStorage::new(&folder.0);

        assert_eq!(storage.read(StoredFile::Settings), Ok(None));
        assert!(!folder.0.exists());
    }

    #[test]
    fn a_write_replaces_the_file_and_a_removal_deletes_it() {
        let folder = Folder::new();
        let storage = FileStorage::new(&folder.0);

        storage.write(StoredFile::ImageDetails, b"first").unwrap();
        storage.write(StoredFile::ImageDetails, b"second").unwrap();
        assert_eq!(
            storage.read(StoredFile::ImageDetails),
            Ok(Some(b"second".to_vec()))
        );

        storage.remove(StoredFile::ImageDetails).unwrap();
        assert_eq!(storage.read(StoredFile::ImageDetails), Ok(None));
        assert_eq!(storage.remove(StoredFile::ImageDetails), Ok(()));
    }

    #[test]
    fn a_crash_part_way_through_a_write_leaves_the_previous_data() {
        // REL-03
        let folder = Folder::new();
        save(
            &FileStorage::new(&folder.0),
            StoredFile::Settings,
            &settings(ThemeChoice::Dark),
        )
        .unwrap();
        let next = encode(&settings(ThemeChoice::Light));

        for written in [0, 1, next.len() / 2, next.len() - 1, next.len()] {
            let storage = crashing(&folder, Crash::AfterBytes(written));
            assert!(storage.write(StoredFile::Settings, &next).is_err());

            // Start again, as after the crash.
            let restarted = FileStorage::new(&folder.0);
            assert_eq!(
                load::<Settings>(&restarted, StoredFile::Settings),
                settings(ThemeChoice::Dark),
                "crashed after {written} bytes"
            );
        }
    }

    #[test]
    fn a_crash_before_the_rename_leaves_the_previous_data() {
        // REL-03
        let folder = Folder::new();
        save(
            &FileStorage::new(&folder.0),
            StoredFile::Settings,
            &settings(ThemeChoice::Dark),
        )
        .unwrap();

        let storage = crashing(&folder, Crash::BeforeRename);
        assert!(
            save(
                &storage,
                StoredFile::Settings,
                &settings(ThemeChoice::Light)
            )
            .is_err()
        );

        let restarted = FileStorage::new(&folder.0);
        assert_eq!(
            load::<Settings>(&restarted, StoredFile::Settings),
            settings(ThemeChoice::Dark)
        );
    }

    #[test]
    fn the_next_write_after_a_crash_succeeds_and_leaves_no_temporary_file() {
        let folder = Folder::new();
        let storage = crashing(&folder, Crash::AfterBytes(3));
        assert!(storage.write(StoredFile::Layout, b"layout").is_err());
        assert_eq!(folder.files(), BTreeSet::from(["layout.tmp".into()]));
        assert_eq!(
            FileStorage::new(&folder.0).read(StoredFile::Layout),
            Ok(None)
        );

        let restarted = FileStorage::new(&folder.0);
        restarted.write(StoredFile::Layout, b"layout").unwrap();

        assert_eq!(
            restarted.read(StoredFile::Layout),
            Ok(Some(b"layout".to_vec()))
        );
        assert_eq!(folder.files(), BTreeSet::from(["layout".into()]));
    }

    #[test]
    fn everything_stays_in_the_three_stores_under_the_root() {
        // SRS 7.1, PRIV-05: deleting the root removes everything MeeGone stored.
        let folder = Folder::new();
        let storage = FileStorage::new(&folder.0);

        for file in StoredFile::ALL {
            storage.write(file, b"data").unwrap();
            storage.write(file, b"more data").unwrap();
        }

        assert_eq!(
            folder.files(),
            BTreeSet::from([
                "images/a".into(),
                "images/b".into(),
                "images/details".into(),
                "layout".into(),
                "settings".into(),
            ])
        );
    }

    #[test]
    fn a_write_over_the_budget_changes_nothing() {
        // PERF-08
        let folder = Folder::new();
        let storage = FileStorage {
            budget: 10,
            ..FileStorage::new(&folder.0)
        };
        storage.write(StoredFile::Layout, b"123456").unwrap();
        storage.write(StoredFile::Settings, b"1234").unwrap();

        assert!(storage.write(StoredFile::Settings, b"12345").is_err());

        assert_eq!(
            storage.read(StoredFile::Settings),
            Ok(Some(b"1234".to_vec()))
        );
        storage.write(StoredFile::Layout, b"1").unwrap();
        storage.write(StoredFile::Settings, b"123456789").unwrap();
    }

    #[test]
    fn file_system_errors_are_reported_in_plain_words() {
        let folder = Folder::new();
        fs::create_dir_all(folder.0.join("settings")).unwrap();
        fs::write(folder.0.join("images"), b"not a folder").unwrap();
        let storage = FileStorage::new(&folder.0);

        let Err(DataError::Failed(reason)) = storage.read(StoredFile::Settings) else {
            panic!("reading a folder should fail");
        };
        assert!(reason.starts_with("Couldn't use saved data"), "{reason}");
        assert!(storage.write(StoredFile::ImageDetails, b"x").is_err());
        assert!(storage.write(StoredFile::Settings, b"x").is_err());
        assert!(storage.remove(StoredFile::Settings).is_err());
        assert_eq!(
            failure(&io::Error::from(ErrorKind::StorageFull)),
            DataError::Failed("Not enough storage space".into())
        );
    }
}
