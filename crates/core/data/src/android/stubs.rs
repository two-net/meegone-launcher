//! The platform adapters whose Android implementations arrive with later issues.
//!
//! Each answers with its "if unavailable" value from SRS 8.1, so every feature shows its
//! reduced state meanwhile: no apps, missing access, no lock-screen image, offline. Shortcuts
//! are `Unsupported`, so menus show none (APPS-11); their implementation (#12) offers them only
//! while MeeGone is the default home app.

use crate::adapters::{
    AppsAdapter, NetworkAdapter, NotificationsAdapter, SchedulerAdapter, UsageAdapter,
    WallpaperAdapter,
};
use crate::model::{
    Access, App, AppId, DataError, ImageJob, NetworkState, Notification, Profile, RecentUse,
    Shortcut, Size,
};

/// Every adapter not built yet.
pub struct NotBuiltYet;

fn not_yet() -> DataError {
    DataError::Unsupported("Not available yet".into())
}

/// The app catalogue arrives with #9, shortcuts with #12, uninstalling with #13.
impl AppsAdapter for NotBuiltYet {
    fn launchable_apps(&self) -> Result<Vec<App>, DataError> {
        Ok(Vec::new())
    }

    fn profiles(&self) -> Result<Vec<Profile>, DataError> {
        Ok(Vec::new())
    }

    fn launch(&self, _app: &AppId) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn shortcuts(&self, _app: &AppId) -> Result<Vec<Shortcut>, DataError> {
        Err(not_yet())
    }

    fn launch_shortcut(&self, _shortcut: &Shortcut) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn request_uninstall(&self, _app: &AppId) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn open_app_info(&self, _app: &AppId) -> Result<(), DataError> {
        Err(not_yet())
    }
}

/// Notifications arrive with #19 and #20.
impl NotificationsAdapter for NotBuiltYet {
    fn access(&self) -> Access {
        Access::Missing
    }

    fn active(&self) -> Result<Vec<Notification>, DataError> {
        Err(DataError::AccessDenied)
    }

    fn open(&self, _key: &str) -> Result<(), DataError> {
        Err(DataError::AccessDenied)
    }

    fn dismiss(&self, _key: &str) -> Result<(), DataError> {
        Err(DataError::AccessDenied)
    }

    fn open_access_settings(&self) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn open_app_settings(&self, _package: &str) -> Result<(), DataError> {
        Err(not_yet())
    }
}

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
