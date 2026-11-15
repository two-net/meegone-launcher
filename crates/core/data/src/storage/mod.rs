//! MeeGone's own files (REL-03, MAINT-04, PRIV-05; ADR 0004).
//!
//! MeeGone keeps exactly three persistent stores (SRS 7.1): the layout, the settings and the
//! image cache. [`StoredFile`] names every file they use, so nothing else can be written.
//!
//! - [`StorageAdapter`]: reads and atomically replaces those files. [`FileStorage`] is the one
//!   implementation that touches the file system; tests use `fakes::FakeStorage`.
//! - [`format`]: the versioned, checksummed text format every store saves in, with forward
//!   migrations. [`load`] gives the defaults for a file that is missing, damaged or from a newer
//!   version, so a bad file never crashes MeeGone.

mod files;
pub mod format;

pub use files::FileStorage;
pub use format::{Decode, Encode, Format, Migration, Reader, Writer, decode, encode, load, save};

use crate::model::DataError;

/// Every file MeeGone stores. Each belongs to one of the three persistent stores (SRS 7.1).
///
/// Notification content and app-usage history have no file, so they are never stored
/// (PRIV-03, EVT-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StoredFile {
    /// The layout store: app order and folders (APPS-18).
    Layout,
    /// The settings store, including the apps removed from Recent (PRIV-03).
    Settings,
    /// The image cache: the details of the current and previous images (WALL-09).
    ImageDetails,
}

impl StoredFile {
    pub const ALL: [StoredFile; 3] = [Self::Layout, Self::Settings, Self::ImageDetails];

    /// Where the file lives, relative to the storage root, with `/` between folders. The image
    /// cache has a folder of its own, so the images themselves can join it (#27).
    pub const fn path(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            Self::Settings => "settings",
            Self::ImageDetails => "images/details",
        }
    }
}

/// The most MeeGone stores in total, image cache included (PERF-08).
pub const MAX_STORED_BYTES: u64 = 50_000_000;

/// MeeGone's private files.
pub trait StorageAdapter: Send + Sync {
    /// The file's contents, or `None` if it has never been written.
    fn read(&self, file: StoredFile) -> Result<Option<Vec<u8>>, DataError>;
    /// Replaces the file's contents atomically: after a crash or power loss, a later read
    /// returns either the old contents or the new ones, never a mix (REL-03).
    ///
    /// Fails without changing anything if the stored files would then take more than
    /// [`MAX_STORED_BYTES`] (PERF-08).
    fn write(&self, file: StoredFile, bytes: &[u8]) -> Result<(), DataError>;
    /// Deletes the file. Deleting a file that doesn't exist succeeds.
    fn remove(&self, file: StoredFile) -> Result<(), DataError>;
}

/// Fails if `new` bytes stored next to `others` bytes would take more than `budget` (PERF-08).
pub(crate) fn check_budget(others: u64, new: usize, budget: u64) -> Result<(), DataError> {
    let total = others.saturating_add(u64::try_from(new).unwrap_or(u64::MAX));
    if total <= budget {
        Ok(())
    } else {
        Err(DataError::Failed(format!(
            "Stored data would go over {} MB",
            budget / 1_000_000
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_has_its_own_path() {
        let mut paths: Vec<_> = StoredFile::ALL.iter().map(|file| file.path()).collect();
        paths.sort();
        paths.dedup();

        assert_eq!(paths.len(), StoredFile::ALL.len());
    }

    #[test]
    fn the_budget_allows_up_to_its_limit_and_no_more() {
        // PERF-08
        assert!(check_budget(30_000_000, 20_000_000, MAX_STORED_BYTES).is_ok());
        assert_eq!(
            check_budget(30_000_000, 20_000_001, MAX_STORED_BYTES),
            Err(DataError::Failed("Stored data would go over 50 MB".into()))
        );
        assert!(check_budget(u64::MAX, 1, MAX_STORED_BYTES).is_err());
    }
}
