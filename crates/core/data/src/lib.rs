//! Data layer shared by every feature (SRS 7.3).
//!
//! - [`model`]: data that several features read, such as [`App`].
//! - [`adapters`]: narrow traits for everything MeeGone asks of the operating system (ARCH-04),
//!   bundled in [`Platform`].
//! - [`settings`]: the settings store, which several features share (ADR 0002).
//!
//! Nothing here knows about state holders or the UI (ARCH-02).

pub mod adapters;
#[cfg(feature = "fakes")]
pub mod fakes;
pub mod model;
pub mod settings;

pub use adapters::{
    AppsAdapter, NetworkAdapter, NotificationsAdapter, Platform, SchedulerAdapter,
    SystemConfigAdapter, UsageAdapter, WallpaperAdapter,
};
pub use model::{
    Access, App, AppId, DataError, ImageJob, JobNetwork, NetworkState, Notification, Profile,
    ProfileId, RecentUse, Rgb, Shortcut, Size,
};
pub use settings::{
    HomeTarget, ImageOptions, MemorySettingsStore, RecentDensity, SETTINGS_VERSION, Settings,
    SettingsStore, ThemeChoice,
};
