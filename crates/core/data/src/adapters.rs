//! Platform adapters (ARCH-04): the only way any crate reaches the operating system.
//!
//! There are seven narrow traits, one per group of capabilities in SRS 8.1, plus
//! [`StorageAdapter`] for MeeGone's own files (ADR 0004). Tests replace each one with a fake
//! (MAINT-01). The table in ARCHITECTURE.md maps every capability to its adapter. The Android
//! implementations are in the `android` module (ADR 0005); those not built yet answer with
//! their "if unavailable" value. Storage has `FileStorage`.
//!
//! Adapters only answer questions and carry out requests; they never call back. Changes the
//! system reports, such as an app being installed, reach MeeGone as a `ShellIntent` that the
//! platform's entry point passes to `Launcher::dispatch` (ADR 0003).
//!
//! When the system has no screen or service for a request, the method returns
//! [`DataError::Unsupported`], and the feature shows written instructions or hides the item
//! (SRS 8.1, "If unavailable").

use std::sync::Arc;

use crate::model::{
    Access, App, AppId, DataError, ImageJob, NetworkState, Notification, Profile, RecentUse, Rect,
    Shortcut, Size,
};
use crate::storage::StorageAdapter;

/// The app catalogue: installed apps, profiles, shortcuts and uninstall requests (SRS 8.1).
pub trait AppsAdapter: Send + Sync {
    /// Every launchable app in every available profile (APPS-01).
    fn launchable_apps(&self) -> Result<Vec<App>, DataError>;
    /// Every profile that holds apps, such as personal and work (APPS-09, APPS-20).
    fn profiles(&self) -> Result<Vec<Profile>, DataError>;
    /// The packages of the default phone, messaging, browser and camera apps, in that order,
    /// leaving out any the system doesn't identify (APPS-05).
    fn default_apps(&self) -> Vec<String>;
    /// `text` folded for search exactly as the catalogue folds each label into
    /// [`App::search_key`]: accents removed, case folded, spaces collapsed (SRCH-02, L10N-04).
    fn search_key(&self, text: &str) -> String;
    /// Starts or resumes an app (APPS-10, REC-06). Its window grows from `from`, the tapped icon
    /// or card, with the platform's launch-from-bounds transition (ANIM-08).
    ///
    /// An unavailable app is launched too, so the system explains why it can't open (APPS-09).
    /// Fails with a short message for a status line when the system can't start it.
    fn launch(&self, app: &AppId, from: Option<Rect>) -> Result<(), DataError>;
    /// The app's shortcuts. The system offers them only while MeeGone is the default home app;
    /// otherwise this is `Unsupported` and menus show no shortcuts (APPS-11).
    fn shortcuts(&self, app: &AppId) -> Result<Vec<Shortcut>, DataError>;
    fn launch_shortcut(&self, shortcut: &Shortcut) -> Result<(), DataError>;
    /// Opens the system's uninstall confirmation. MeeGone never uninstalls anything itself
    /// (APPS-13).
    fn request_uninstall(&self, app: &AppId) -> Result<(), DataError>;
    /// Opens the system's App info screen (APPS-11).
    fn open_app_info(&self, app: &AppId) -> Result<(), DataError>;
}

/// Active notifications, read through notification access (EVT-03 to EVT-08).
pub trait NotificationsAdapter: Send + Sync {
    fn access(&self) -> Access;
    fn active(&self) -> Result<Vec<Notification>, DataError>;
    /// Opens the notification's content. The system removes it if it is meant to disappear
    /// when opened (EVT-06).
    fn open(&self, key: &str) -> Result<(), DataError>;
    /// Removes a clearable notification (EVT-07, EVT-08).
    fn dismiss(&self, key: &str) -> Result<(), DataError>;
    /// Opens the system's notification-access screen (EVT-03).
    fn open_access_settings(&self) -> Result<(), DataError>;
    /// Opens the system's notification settings for one app (EVT-08).
    fn open_app_settings(&self, package: &str) -> Result<(), DataError>;
}

/// App-usage history, read through usage access (REC-02, REC-03).
pub trait UsageAdapter: Send + Sync {
    fn access(&self) -> Access;
    /// The last use of each app at or after `since_ms`.
    fn used_since(&self, since_ms: u64) -> Result<Vec<RecentUse>, DataError>;
    /// Opens the system's usage-access screen (REC-02).
    fn open_access_settings(&self) -> Result<(), DataError>;
}

/// The system wallpaper service (WALL-01, WALL-12, WALL-13).
pub trait WallpaperAdapter: Send + Sync {
    /// `Ok` if the lock-screen wallpaper can be set; otherwise the reason it can't (WALL-13).
    fn lock_screen_support(&self) -> Result<(), DataError>;
    /// The size the lock-screen image is cropped to (WALL-06).
    fn lock_screen_size(&self) -> Size;
    /// Sets the lock-screen wallpaper only; the home wallpaper is untouched (WALL-01).
    fn set_lock_screen(&self, image: &[u8]) -> Result<(), DataError>;
    /// Makes the lock screen show the home wallpaper again (WALL-12).
    fn reset_lock_screen(&self) -> Result<(), DataError>;
    /// Opens the system's wallpaper settings (WALL-12).
    fn open_wallpaper_settings(&self) -> Result<(), DataError>;
}

/// HTTPS downloads and network state (SEC-01, WALL-04, WALL-11).
pub trait NetworkAdapter: Send + Sync {
    fn state(&self) -> NetworkState;
    /// Fetches an `https://` URL with the platform's certificate validation.
    ///
    /// Fails with [`DataError::Offline`] without a connection, and with
    /// [`DataError::Invalid`] for a URL or redirect that isn't HTTPS, or a body larger than
    /// `max_bytes` (SEC-01, SEC-02, WALL-06).
    fn get(&self, https_url: &str, max_bytes: usize) -> Result<Vec<u8>, DataError>;
}

/// The one scheduled background job, the daily image (ARCH-07).
pub trait SchedulerAdapter: Send + Sync {
    /// Schedules the image job, replacing any job already scheduled. The system also waits
    /// while battery saver is on or the battery is low (WALL-04).
    ///
    /// `Unsupported` if background scheduling isn't available; the image is then checked at
    /// the next MeeGone start.
    fn schedule_image_job(&self, job: ImageJob) -> Result<(), DataError>;
    fn cancel_image_job(&self) -> Result<(), DataError>;
}

/// Device configuration, the clock, and device-wide services: the home-app role, the next
/// alarm, haptics and the web browser (SRS 8.1).
pub trait SystemConfigAdapter: Send + Sync {
    /// The device's dark-theme setting (THEME-02).
    fn is_dark(&self) -> bool;
    /// The system animation speed: 1.0 is normal and 0.0 means animations are removed
    /// (ANIM-05).
    fn animation_scale(&self) -> f64;
    /// Whether the system animates the home's return from an app itself, so MeeGone's own
    /// return motion would double up (ANIM-09).
    fn animates_return_home(&self) -> bool;
    /// Wall-clock time in milliseconds since the Unix epoch.
    fn now_ms(&self) -> u64;
    /// The local time zone's offset from UTC at `at_ms`, for local midnight (WALL-03) and the
    /// date (EVT-01).
    fn utc_offset_ms(&self, at_ms: u64) -> i64;
    /// The device's language and region as a BCP 47 tag, such as `en-US` (WALL-05, L10N).
    fn locale(&self) -> String;
    /// Whether MeeGone is the default home app (ONB-04).
    fn is_default_home(&self) -> bool;
    /// Opens the system dialog for choosing the default home app, or the default-apps screen
    /// (ONB-01, ONB-04).
    fn request_default_home(&self) -> Result<(), DataError>;
    /// When the next alarm rings, if one is set (EVT-02).
    fn next_alarm_ms(&self) -> Option<u64>;
    /// Opens the app that set the next alarm (EVT-02).
    fn open_next_alarm(&self) -> Result<(), DataError>;
    /// A light haptic tick. Follows the system haptics setting and does nothing without a
    /// vibrator (ANIM-24).
    fn haptic_tick(&self);
    /// Whether a web browser is installed; links are hidden without one.
    fn has_browser(&self) -> bool;
    /// Opens an `https://` link in the external browser (SEC-02, WALL-15).
    fn open_in_browser(&self, https_url: &str) -> Result<(), DataError>;
    /// Searches the web with the device's default search handler (SRCH-05).
    fn web_search(&self, query: &str) -> Result<(), DataError>;
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
    /// Where the layout, settings and image cache are kept (REL-03).
    pub storage: Arc<dyn StorageAdapter>,
}
