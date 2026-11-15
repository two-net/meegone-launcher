//! Applications data layer: the layout store, the one store Applications owns (SRS 7.3).

use std::sync::{Mutex, MutexGuard, PoisonError};

use meegone_core_data::{AppId, DataError};

/// Version of the [`Layout`] format, for forward migrations (MAINT-04).
pub const LAYOUT_VERSION: u32 = 1;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub version: u32,
    pub entries: Vec<LayoutEntry>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            version: LAYOUT_VERSION,
            entries: Vec::new(),
        }
    }
}

pub trait LayoutStore: Send + Sync {
    /// The saved layout, or an empty one if none could be read (REL-02).
    fn load(&self) -> Layout;
    /// Saves the layout atomically (REL-03).
    fn save(&self, layout: &Layout) -> Result<(), DataError>;
}

/// Keeps the layout in memory. Used until the persistent store lands (APPS-18, REL-03).
#[derive(Debug, Default)]
pub struct MemoryLayoutStore(Mutex<Layout>);

impl MemoryLayoutStore {
    fn lock(&self) -> MutexGuard<'_, Layout> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl LayoutStore for MemoryLayoutStore {
    fn load(&self) -> Layout {
        self.lock().clone()
    }

    fn save(&self, layout: &Layout) -> Result<(), DataError> {
        *self.lock() = layout.clone();
        Ok(())
    }
}
