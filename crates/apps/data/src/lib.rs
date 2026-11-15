//! Applications data layer: the layout store, the one store Applications owns (SRS 7.3).

use std::sync::Arc;

use meegone_core_data::storage::format::{Fields, Line};
use meegone_core_data::storage::{Decode, Encode, Format, Migration, Reader, Writer, load, save};
use meegone_core_data::{AppId, DataError, StorageAdapter, StoredFile};

/// Version of the saved layout format (MAINT-04). Changing the format means a new version, a
/// migration in [`LAYOUT_MIGRATIONS`] and a fixture (ARCHITECTURE.md, "Persistence").
pub const LAYOUT_VERSION: u32 = 1;

/// Brings layouts saved by older versions up to date, one version at a time.
const LAYOUT_MIGRATIONS: [Migration; LAYOUT_VERSION as usize - 1] = [];

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
}

pub trait LayoutStore: Send + Sync {
    /// The saved layout, or an empty one if none could be read (MAINT-04, REL-02).
    fn load(&self) -> Layout;
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
    fn load(&self) -> Layout {
        load(&*self.storage, StoredFile::Layout)
    }

    fn save(&self, layout: &Layout) -> Result<(), DataError> {
        save(&*self.storage, StoredFile::Layout, layout)
    }
}

/// One record per grid cell, in order: `app <app>` or `folder <id> <name> <apps>`.
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
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        let mut entries = Vec::new();
        while let Some(key) = input.peek_key() {
            entries.push(match key {
                "app" => LayoutEntry::App(input.line(key)?),
                "folder" => {
                    let (id, name, apps) = input.line(key)?;
                    LayoutEntry::Folder { id, name, apps }
                }
                _ => return None,
            });
        }
        Some(Self { entries })
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
    use meegone_core_data::fakes::{FakeStorage, app_id};
    use meegone_core_data::storage::format::seal;
    use meegone_core_data::storage::{decode, encode};

    use super::*;

    /// A saved file, and the value it must load as.
    type Fixture = (&'static str, fn() -> Layout);

    /// Every saved version, oldest first, with the layout it must load as (MAINT-04). Never
    /// change or remove a fixture; add one for each new version.
    const FIXTURES: [Fixture; LAYOUT_VERSION as usize] =
        [(include_str!("../fixtures/layout-v1.txt"), sample)];

    fn sample() -> Layout {
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
        }
    }

    fn store() -> (Arc<FakeStorage>, PersistentLayoutStore) {
        let storage = Arc::new(FakeStorage::default());
        (storage.clone(), PersistentLayoutStore::new(storage))
    }

    #[test]
    fn a_saved_layout_is_there_after_a_restart() {
        // APPS-18
        let (storage, store) = store();
        assert_eq!(store.load(), Layout::default());

        store.save(&sample()).unwrap();

        assert_eq!(PersistentLayoutStore::new(storage).load(), sample());
    }

    #[test]
    fn a_failed_save_leaves_the_previous_layout() {
        // REL-03
        let (storage, store) = store();
        store.save(&sample()).unwrap();
        *storage.error.lock().unwrap() = Some(DataError::Failed("Disk error".into()));

        assert!(store.save(&Layout::default()).is_err());

        *storage.error.lock().unwrap() = None;
        assert_eq!(store.load(), sample());
    }

    #[test]
    fn damaged_or_newer_layouts_load_as_empty() {
        // MAINT-04; #11 rebuilds the grid from the installed apps (REL-02).
        let saved = encode(&sample());
        let text = String::from_utf8(saved.clone()).unwrap();
        let body: String = text
            .lines()
            .filter(|line| line.starts_with("app ") || line.starts_with("folder "))
            .map(|line| format!("{line}\n"))
            .collect();
        let mut damaged = saved.clone();
        damaged[40] ^= 0x20;

        for bytes in [
            seal("layout", LAYOUT_VERSION + 1, &body),
            seal("layout", LAYOUT_VERSION, &format!("{body}shelf 1\n")),
            seal("layout", LAYOUT_VERSION, "folder 1 Games 2 0 a a.Main\n"),
            seal("settings", LAYOUT_VERSION, &body),
            damaged,
            saved[..saved.len() - 1].to_vec(),
        ] {
            let (storage, store) = store();
            storage
                .files
                .lock()
                .unwrap()
                .insert(StoredFile::Layout, bytes);

            assert_eq!(store.load(), Layout::default());
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
