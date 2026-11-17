//! The platform adapters whose Android implementations arrive with later issues.
//!
//! Each answers with its "if unavailable" value from SRS 8.1, so every feature shows its
//! reduced state meanwhile: missing access, no lock-screen image, offline. The app catalogue
//! (`apps.rs`) and notifications (`notifications.rs`) are built.

use super::not_yet;
use crate::adapters::{NetworkAdapter, SchedulerAdapter, UsageAdapter, WallpaperAdapter};
use crate::model::{Access, DataError, ImageJob, NetworkState, RecentUse, Size};

/// Every adapter not built yet.
pub struct NotBuiltYet;

/// Usage history arrives with #22.
impl UsageAdapter for NotBuiltYet {
    fn access(&self) -> Access {
        Access::Missing
    }

    fn used_since(&self, _since_ms: u64) -> Result<Vec<RecentUse>, DataError> {
        Err(DataError::AccessDenied)
    }

    fn open_access_settings(&self) -> Result<(), DataError> {
        Err(not_yet())
    }
}

/// The lock-screen wallpaper arrives with #29.
impl WallpaperAdapter for NotBuiltYet {
    fn lock_screen_support(&self) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn lock_screen_size(&self) -> Size {
        Size::default()
    }

    fn set_lock_screen(&self, _image: &[u8]) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn reset_lock_screen(&self) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn open_wallpaper_settings(&self) -> Result<(), DataError> {
        Err(not_yet())
    }
}

/// Downloads arrive with #26 and #27.
impl NetworkAdapter for NotBuiltYet {
    fn state(&self) -> NetworkState {
        NetworkState::Offline
    }

    fn get(&self, _https_url: &str, _max_bytes: usize) -> Result<Vec<u8>, DataError> {
        Err(DataError::Offline)
    }
}

/// The daily image job arrives with #28.
impl SchedulerAdapter for NotBuiltYet {
    fn schedule_image_job(&self, _job: ImageJob) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn cancel_image_job(&self) -> Result<(), DataError> {
        Err(not_yet())
    }
}
