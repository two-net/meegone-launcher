//! Platform adapters (ARCH-04): the only way any crate reaches the operating system.
//!
//! Each adapter is a narrow trait, so tests can replace it with a fake (MAINT-01). The methods
//! are the minimum the skeleton needs; feature work extends them. Android implementations
//! arrive with the platform work (HOME-01 onwards).

use std::sync::Arc;

use crate::model::{Access, App, AppId, DataError, Notification, RecentUse};

/// Installed apps and launching (SRS 8.1, "App catalogue").
pub trait AppsAdapter: Send + Sync {
    /// Every launchable app in every available profile (APPS-01).
    fn launchable_apps(&self) -> Result<Vec<App>, DataError>;
    /// Starts or resumes an app (APPS-10, REC-06).
    fn launch(&self, app: &AppId) -> Result<(), DataError>;
}

/// Active notifications, read through notification access (EVT-03, EVT-04).
pub trait NotificationsAdapter: Send + Sync {
    fn access(&self) -> Access;
    fn active(&self) -> Result<Vec<Notification>, DataError>;
    /// Opens the system's notification-access screen.
    fn open_access_settings(&self) -> Result<(), DataError>;
}

/// App-usage history, read through usage access (REC-02, REC-03).
pub trait UsageAdapter: Send + Sync {
    fn access(&self) -> Access;
    /// The last use of each app at or after `since_ms`.
    fn used_since(&self, since_ms: u64) -> Result<Vec<RecentUse>, DataError>;
    /// Opens the system's usage-access screen.
    fn open_access_settings(&self) -> Result<(), DataError>;
}

/// The system wallpaper service (WALL-01, WALL-13).
pub trait WallpaperAdapter: Send + Sync {
    /// `Ok` if the lock-screen wallpaper can be set; otherwise the reason it can't.
    fn lock_screen_support(&self) -> Result<(), DataError>;
    fn set_lock_screen(&self, image: &[u8]) -> Result<(), DataError>;
}

/// HTTPS downloads and network state (SEC-01, WALL-04).
pub trait NetworkAdapter: Send + Sync {
    fn is_unmetered(&self) -> bool;
    /// Fetches an `https://` URL, refusing bodies larger than `max_bytes` (WALL-06).
    fn get(&self, https_url: &str, max_bytes: usize) -> Result<Vec<u8>, DataError>;
}

/// The one scheduled background job, the daily image (ARCH-07).
pub trait SchedulerAdapter: Send + Sync {
    /// Schedules the daily image job unless it is already scheduled.
    fn ensure_daily_job(&self) -> Result<(), DataError>;
    fn cancel_daily_job(&self) -> Result<(), DataError>;
}

/// Device configuration and the clock.
pub trait SystemConfigAdapter: Send + Sync {
    /// The device's dark-theme setting (THEME-02).
    fn is_dark(&self) -> bool;
    /// Whether MeeGone is the default home app (ONB-04).
    fn is_default_home(&self) -> bool;
    /// Wall-clock time in milliseconds since the Unix epoch.
    fn now_ms(&self) -> u64;
}

/// Every platform adapter, handed to the Home shell once at start.
#[derive(Clone)]
pub struct Platform {
    pub apps: Arc<dyn AppsAdapter>,
    pub notifications: Arc<dyn NotificationsAdapter>,
    pub usage: Arc<dyn UsageAdapter>,
    pub wallpaper: Arc<dyn WallpaperAdapter>,
    pub network: Arc<dyn NetworkAdapter>,
    pub scheduler: Arc<dyn SchedulerAdapter>,
    pub system: Arc<dyn SystemConfigAdapter>,
}
