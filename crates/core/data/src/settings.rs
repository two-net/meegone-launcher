//! The settings store (SRS 7.4).
//!
//! Several features keep their options here, so the store lives in core while the Settings
//! feature owns the screens (ADR 0002). It is the single source of truth for every setting
//! (ARCH-06).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::model::{AppId, DataError};

/// Version of the [`Settings`] format, for forward migrations (MAINT-04).
pub const SETTINGS_VERSION: u32 = 1;

/// The user's theme choice (THEME-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

/// What Home shows when the user comes back from an app (HOME-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeTarget {
    #[default]
    LastView,
    Applications,
}

/// Columns in Recent (REC-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecentDensity {
    #[default]
    ThreeColumns,
    TwoColumns,
}

/// Lock-screen image options (SET-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageOptions {
    /// Off until the user confirms it in setup or Settings (WALL-02).
    pub enabled: bool,
    /// Feed region code; `None` means Automatic (WALL-05).
    pub region: Option<String>,
    pub use_mobile_data: bool,
    pub show_card: bool,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            region: None,
            use_mobile_data: false,
            show_card: true,
        }
    }
}

/// Every setting (SRS 7.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub version: u32,
    pub theme: ThemeChoice,
    pub home_target: HomeTarget,
    pub wrap_around: bool,
    pub recent_density: RecentDensity,
    pub recent_window_hours: u32,
    pub hidden_apps: BTreeSet<AppId>,
    pub image: ImageOptions,
    /// Apps the user removed from Recent, with the time of removal (PRIV-03).
    pub removed_from_recent: BTreeMap<AppId, u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: ThemeChoice::default(),
            home_target: HomeTarget::default(),
            wrap_around: true,
            recent_density: RecentDensity::default(),
            recent_window_hours: 24,
            hidden_apps: BTreeSet::new(),
            image: ImageOptions::default(),
            removed_from_recent: BTreeMap::new(),
        }
    }
}

/// Reads and writes [`Settings`]. Changes take effect at once (SET-03).
pub trait SettingsStore: Send + Sync {
    /// The current settings, or the defaults if none could be read (MAINT-04).
    fn get(&self) -> Settings;
    /// Applies `change` and saves the result atomically (REL-03), returning the new settings.
    fn update(&self, change: &mut dyn FnMut(&mut Settings)) -> Result<Settings, DataError>;
}

/// Keeps settings in memory. Used until the persistent store lands (REL-03, MAINT-04).
#[derive(Debug, Default)]
pub struct MemorySettingsStore(Mutex<Settings>);

impl MemorySettingsStore {
    fn lock(&self) -> MutexGuard<'_, Settings> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SettingsStore for MemorySettingsStore {
    fn get(&self) -> Settings {
        self.lock().clone()
    }

    fn update(&self, change: &mut dyn FnMut(&mut Settings)) -> Result<Settings, DataError> {
        let mut current = self.lock();
        // Change a copy, so a change that panics part-way leaves the settings as they were.
        let mut next = current.clone();
        change(&mut next);
        *current = next.clone();
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_srs() {
        let settings = Settings::default();

        assert_eq!(settings.theme, ThemeChoice::System); // THEME-01
        assert!(settings.wrap_around); // HOME-04
        assert!(!settings.image.enabled); // WALL-02
        assert_eq!(settings.recent_window_hours, 24); // REC-03
    }

    #[test]
    fn a_change_that_panics_leaves_the_settings_unchanged() {
        let store = MemorySettingsStore::default();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = store.update(&mut |settings| {
                settings.wrap_around = false;
                panic!("change failed part-way");
            });
        }));

        assert!(result.is_err());
        assert!(store.get().wrap_around);
    }
}
