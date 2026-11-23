//! Applications data layer: the layout store, the one store Applications owns (SRS 7.3).

use std::collections::BTreeSet;
use std::sync::Arc;

use meegone_core_data::storage::format::{Fields, Line};
use meegone_core_data::storage::{
    Decode, Encode, Format, Migration, Reader, Stored, Writer, read, salvage, save,
};
use meegone_core_data::{AppId, DataError, StorageAdapter, StoredFile};

/// Version of the saved layout format (MAINT-04). Changing the format means a new version, a
/// migration in [`LAYOUT_MIGRATIONS`] and a fixture (ARCHITECTURE.md, "Persistence").
pub const LAYOUT_VERSION: u32 = 3;

/// Brings layouts saved by older versions up to date, one version at a time.
const LAYOUT_MIGRATIONS: [Migration; LAYOUT_VERSION as usize - 1] =
    [add_new_apps, add_installed_and_restore];

/// Version 2 added `new` records (APPS-07). A version 1 body is a version 2 body without any.
fn add_new_apps(body: &str) -> Option<String> {
    Some(body.to_owned())
}

/// Version 3 added the `installed` and `restore` records (APPS-21). A version 2 body is a
/// version 3 body without them: only this copy of MeeGone could have saved it, since earlier
/// versions took no part in backups.
fn add_installed_and_restore(body: &str) -> Option<String> {
    Some(body.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FolderId(pub u32);

/// One cell of the grid. A folder holds apps, never folders (FOLD-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutEntry {
    App(AppId),
    Folder {
        id: FolderId,
        name: String,
        apps: Vec<AppId>,
    },
}

/// The user's arrangement: order and folders (APPS-18). Position is the index in `entries`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    pub entries: Vec<LayoutEntry>,
    /// Apps that joined the grid after it was first set up and haven't been opened from MeeGone
    /// since. Each carries a dot (APPS-07).
    pub new: BTreeSet<AppId>,
    /// When the copy of MeeGone that saved this layout was first installed. A layout saved by
    /// another copy came from a backup (APPS-21). `None` in a layout saved before version 3,
    /// which only this copy could have saved.
    pub installed_ms: Option<u64>,
    /// After a restore, the backed-up apps that aren't installed yet (APPS-21).
    pub restore: Option<Restore>,
}

/// The apps a restored layout is still waiting for (APPS-21, ADR 0018). Each keeps its place,
/// out of sight, until it is installed or the wait ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restore {
    /// When the wait ends. Apps still not installed then leave the layout.
    pub until_ms: u64,
    pub waiting: BTreeSet<AppId>,
}

/// What the layout store holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedLayout {
    /// No layout has ever been saved: this is the first start (APPS-04, APPS-05).
    Missing,
    Read(Layout),
    /// The saved layout couldn't be read whole. These are the cells recovered from it, in their
    /// order; the grid is rebuilt around them (REL-02).
    Recovered(Layout),
}

pub trait LayoutStore: Send + Sync {
    /// The saved layout. Fails only when the storage itself can't be read; a damaged, cut short
    /// or newer file is [`SavedLayout::Recovered`] (MAINT-04, REL-02).
    fn load(&self) -> Result<SavedLayout, DataError>;
    /// Saves the layout atomically (REL-03).
    fn save(&self, layout: &Layout) -> Result<(), DataError>;
}

/// Keeps the layout in [`StoredFile::Layout`].
pub struct PersistentLayoutStore {
    storage: Arc<dyn StorageAdapter>,
}

impl PersistentLayoutStore {
    pub fn new(storage: Arc<dyn StorageAdapter>) -> Self {
        Self { storage }
    }
}

impl LayoutStore for PersistentLayoutStore {
    fn load(&self) -> Result<SavedLayout, DataError> {
        Ok(match read(&*self.storage, StoredFile::Layout)? {
            Stored::Missing => SavedLayout::Missing,
            Stored::Read(layout) => SavedLayout::Read(layout),
            Stored::Unreadable(bytes) => SavedLayout::Recovered(recover(&bytes)),
        })
    }

    fn save(&self, layout: &Layout) -> Result<(), DataError> {
        save(&*self.storage, StoredFile::Layout, layout)
    }
}

/// Every cell and mark of a damaged file that still reads on its own (REL-02).
fn recover(bytes: &[u8]) -> Layout {
    salvage::<Layout>(bytes)
        .into_iter()
        .fold(Layout::default(), |mut layout, part| {
            layout.entries.extend(part.entries);
            layout.new.extend(part.new);
            layout.installed_ms = part.installed_ms.or(layout.installed_ms);
            layout.restore = part.restore.or(layout.restore);
            layout
        })
}

/// One record per grid cell, in order: `app <app>` or `folder <id> <name> <apps>`. Then one
/// `new <app>` record per app not yet opened, then `installed <ms>` and
/// `restore <until_ms> <apps>` if there are. Each record stands alone, so a damaged file can be
/// recovered record by record.
impl Format for Layout {
    const KIND: &'static str = "layout";
    const VERSION: u32 = LAYOUT_VERSION;
    const MIGRATIONS: &'static [Migration] = &LAYOUT_MIGRATIONS;

    fn write(&self, out: &mut Writer) {
        for entry in &self.entries {
            match entry {
                LayoutEntry::App(app) => out.line("app", app),
                LayoutEntry::Folder { id, name, apps } => out.line("folder", (id, name, apps)),
            };
        }
        for app in &self.new {
            out.line("new", app);
        }
        if let Some(installed_ms) = self.installed_ms {
            out.line("installed", installed_ms);
        }
        if let Some(Restore { until_ms, waiting }) = &self.restore {
            let waiting: Vec<&AppId> = waiting.iter().collect();
            out.line("restore", (until_ms, waiting));
        }
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        let mut entries = Vec::new();
        loop {
            match input.peek_key() {
                Some(key @ "app") => entries.push(LayoutEntry::App(input.line(key)?)),
                Some(key @ "folder") => {
                    let (id, name, apps) = input.line(key)?;
                    entries.push(LayoutEntry::Folder { id, name, apps });
                }
                _ => break,
            }
        }
        let new = input.lines("new")?.into_iter().collect();
        let installed_ms = match input.peek_key() {
            Some(key @ "installed") => Some(input.line(key)?),
            _ => None,
        };
        let restore = match input.peek_key() {
            Some(key @ "restore") => {
                let (until_ms, waiting): (u64, Vec<AppId>) = input.line(key)?;
                Some(Restore {
                    until_ms,
                    waiting: waiting.into_iter().collect(),
                })
            }
            _ => None,
        };
        Some(Self {
            entries,
            new,
            installed_ms,
            restore,
        })
    }
}

impl Encode for FolderId {
    fn encode(&self, line: &mut Line<'_>) {
        self.0.encode(line);
    }
}

impl Decode for FolderId {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        u32::decode(fields).map(FolderId)
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::Problem;
    use meegone_core_data::fakes::{FakeStorage, app_id};
    use meegone_core_data::storage::format::seal;
    use meegone_core_data::storage::{decode, encode};

    use super::*;

    /// A saved file, and the value it must load as.
    type Fixture = (&'static str, fn() -> Layout);

    /// Every saved version, oldest first, with the layout it must load as (MAINT-04). Never
    /// change or remove a fixture; add one for each new version.
    const FIXTURES: [Fixture; LAYOUT_VERSION as usize] = [
        (include_str!("../fixtures/layout-v1.txt"), sample_v1),
        (include_str!("../fixtures/layout-v2.txt"), sample_v2),
        (include_str!("../fixtures/layout-v3.txt"), sample),
    ];

    /// The layout of the version 1 fixture: no app is marked new.
    fn sample_v1() -> Layout {
        Layout {
            entries: vec![
                LayoutEntry::App(app_id("phone")),
                LayoutEntry::Folder {
                    id: FolderId(7),
                    name: "Fun & games".into(),
                    apps: vec![app_id("chess"), app_id("go")],
                },
                LayoutEntry::App(AppId {
                    profile: meegone_core_data::ProfileId(10),
                    package: "com.example.mail".into(),
                    activity: "com.example.mail.Inbox".into(),
                }),
                LayoutEntry::Folder {
                    id: FolderId(8),
                    name: String::new(),
                    apps: Vec::new(),
                },
            ],
            ..Layout::default()
        }
    }

    /// Version 2: Atlas and Zoom were installed later and haven't been opened (APPS-07).
    fn sample_v2() -> Layout {
        let mut layout = sample_v1();
        layout.entries.push(LayoutEntry::App(app_id("atlas")));
        layout.entries.push(LayoutEntry::App(app_id("zoom")));
        layout.new = [app_id("zoom"), app_id("atlas")].into();
        layout
    }

    /// Version 3: saved by the copy of MeeGone installed at 1 763 000 000 000, a week after a
    /// restore that is still waiting for Chess and Go (APPS-21).
    fn sample() -> Layout {
        Layout {
            installed_ms: Some(1_763_000_000_000),
            restore: Some(Restore {
                until_ms: 1_763_604_800_000,
                waiting: [app_id("go"), app_id("chess")].into(),
            }),
            ..sample_v2()
        }
    }

    fn store() -> (Arc<FakeStorage>, PersistentLayoutStore) {
        let storage = Arc::new(FakeStorage::default());
        (storage.clone(), PersistentLayoutStore::new(storage))
    }

    fn put(storage: &FakeStorage, bytes: Vec<u8>) {
        storage
            .files
            .lock()
            .unwrap()
            .insert(StoredFile::Layout, bytes);
    }

    #[test]
    fn a_saved_layout_is_there_after_a_restart() {
        // APPS-18; nothing saved yet is the first start (APPS-04).
        let (storage, store) = store();
        assert_eq!(store.load(), Ok(SavedLayout::Missing));

        store.save(&sample()).unwrap();

        assert_eq!(
            PersistentLayoutStore::new(storage).load(),
            Ok(SavedLayout::Read(sample()))
        );
    }

    #[test]
    fn a_failed_save_leaves_the_previous_layout() {
        // REL-03
        let (storage, store) = store();
        store.save(&sample()).unwrap();
        *storage.error.lock().unwrap() = Some(DataError::Failed(Problem::SavedDataUnusable));

        assert!(store.save(&Layout::default()).is_err());

        *storage.error.lock().unwrap() = None;
        assert_eq!(store.load(), Ok(SavedLayout::Read(sample())));
    }

    #[test]
    fn storage_that_cant_be_read_is_an_error_not_an_empty_layout() {
        // REL-02: a passing read error must never look like a layout to replace.
        let (storage, store) = store();
        store.save(&sample()).unwrap();
        let error = DataError::Failed(Problem::SavedDataUnusable);
        *storage.error.lock().unwrap() = Some(error.clone());

        assert_eq!(store.load(), Err(error));
    }

    /// The text of the saved sample, and where `needle` starts in it.
    fn saved_with(needle: &str) -> (Vec<u8>, usize) {
        let saved = encode(&sample());
        let at = String::from_utf8(saved.clone())
            .unwrap()
            .find(needle)
            .unwrap();
        (saved, at)
    }

    #[test]
    fn a_damaged_layout_keeps_the_cells_that_still_read() {
        // REL-02: the games folder's record is damaged; everything else is recovered in order.
        let (mut damaged, folder) = saved_with("folder 7");
        damaged[folder + 1] ^= 0x20;
        let mut expected = sample();
        expected.entries.remove(1);

        let (storage, store) = store();
        put(&storage, damaged);

        assert_eq!(store.load(), Ok(SavedLayout::Recovered(expected)));
    }

    #[test]
    fn a_layout_cut_short_keeps_its_whole_records() {
        // REL-02, REL-03
        let (saved, atlas) = saved_with("app 0 atlas");
        let mut expected = sample_v1();

        let (storage, store) = store();
        put(&storage, saved[..atlas + 4].to_vec());
        assert_eq!(store.load(), Ok(SavedLayout::Recovered(expected.clone())));

        let (saved, mark) = saved_with("new 0 zoom");
        put(&storage, saved[..mark].to_vec());
        expected = sample_v2();
        expected.new.remove(&app_id("zoom"));
        assert_eq!(store.load(), Ok(SavedLayout::Recovered(expected)));
    }

    #[test]
    fn recovery_keeps_the_install_and_the_restore() {
        // REL-02, APPS-21: each record stands alone, so a damaged cell loses only itself.
        let (mut damaged, phone) = saved_with("app 0 phone");
        damaged[phone + 1] ^= 0x20;
        let mut expected = sample();
        expected.entries.remove(0);

        let (storage, store) = store();
        put(&storage, damaged);

        assert_eq!(store.load(), Ok(SavedLayout::Recovered(expected)));
    }

    #[test]
    fn newer_or_foreign_layouts_recover_nothing() {
        // MAINT-04: the grid is rebuilt from the installed apps (REL-02).
        let saved = encode(&sample());
        let text = String::from_utf8(saved).unwrap();
        let body: String = text
            .lines()
            .filter(|line| !line.starts_with("meegone-") && !line.starts_with("end "))
            .map(|line| format!("{line}\n"))
            .collect();

        for bytes in [
            seal("layout", LAYOUT_VERSION + 1, &body),
            seal("settings", LAYOUT_VERSION, &body),
            b"\x00garbage".to_vec(),
        ] {
            let (storage, store) = store();
            put(&storage, bytes);

            assert_eq!(store.load(), Ok(SavedLayout::Recovered(Layout::default())));
        }
    }

    #[test]
    fn malformed_layouts_are_unreadable() {
        // MAINT-04: parsing stays strict; only recovery is lenient.
        for body in [
            "shelf 1\n",
            "folder 1 Games 2 0 a a.Main\n",
            "new 0 a a.Main\napp 0 a a.Main\n",
            "new 0 a\n",
            "installed 5\nnew 0 a a.Main\n",
            "restore 9 1 0 a a.Main\ninstalled 5\n",
            "installed\n",
            "installed 5 6\n",
            "restore 9\n",
            "restore 9 2 0 a a.Main\n",
        ] {
            assert_eq!(
                decode::<Layout>(&seal("layout", LAYOUT_VERSION, body)),
                None,
                "{body:?}"
            );
        }
    }

    #[test]
    fn every_saved_version_still_loads() {
        // MAINT-04: one fixture per version, each brought up to today's layout.
        for (version, (fixture, expected)) in (1..).zip(FIXTURES) {
            assert_eq!(
                decode::<Layout>(fixture.as_bytes()),
                Some(expected()),
                "layout-v{version}.txt"
            );
        }
    }

    #[test]
    fn the_latest_fixture_is_what_is_saved_today() {
        let (fixture, expected) = FIXTURES[FIXTURES.len() - 1];

        assert_eq!(String::from_utf8(encode(&expected())).unwrap(), fixture);
    }
}
