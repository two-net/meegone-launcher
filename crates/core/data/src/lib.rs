//! Data layer shared by every feature (SRS 7.3).
//!
//! - [`model`]: data that several features read, such as [`App`].
//! - [`adapters`]: narrow traits for everything MeeGone asks of the operating system (ARCH-04),
//!   bundled in [`Platform`].
//! - [`storage`]: MeeGone's own files and the versioned format its stores save in (REL-03,
//!   MAINT-04).
//! - [`settings`]: the settings store, which several features share (ADR 0002).
//! - `android` (Android builds only): the adapters' Android implementations (ADR 0005).
//!
//! Nothing here knows about state holders or the UI (ARCH-02).

pub mod adapters;
#[cfg(target_os = "android")]
pub mod android;
#[cfg(any(test, feature = "fakes"))]
pub mod fakes;
pub mod model;
pub mod settings;
pub mod storage;

pub use adapters::{
    AppsAdapter, NetworkAdapter, NotificationsAdapter, Platform, SchedulerAdapter,
    SystemConfigAdapter, UsageAdapter, WallpaperAdapter,
};
pub use model::{
    Access, App, AppId, DataError, ImageJob, JobNetwork, NetworkState, Notification, Profile,
    ProfileId, RecentUse, Rgb, Shortcut, Size,
};
pub use settings::{
    HomeTarget, ImageOptions, PersistentSettingsStore, RecentDensity, SETTINGS_VERSION, Settings,
    SettingsStore, ThemeChoice,
};
pub use storage::{FileStorage, MAX_STORED_BYTES, StorageAdapter, StoredFile};
