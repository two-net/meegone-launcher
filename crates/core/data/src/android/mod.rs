//! The platform adapters on Android (ARCH-04, ADR 0005).
//!
//! The app's entry points (`src/android.rs`) build the [`Platform`] here once per process.
//! Adapters ask Android through a Java `Bridge` object over a minimal, hand-written JNI binding
//! ([`jni`]), and keep MeeGone's files in the folder the activity passes in, its private
//! `Context.getFilesDir()` (PRIV-05). Only `jni.rs` contains `unsafe` code.
//!
//! - [`system`]: the home-app role, the clock, the device's locale, free storage, and web search
//!   (HOME-01, ONB-04, WALL-05, SRCH-05).
//! - [`apps`]: the app catalogue, launching and search keys (APPS-01, APPS-10, SRCH-02,
//!   ADR 0010).
//! - [`notifications`]: notification access, the active notifications, opening and dismissing
//!   them, and an app's notification settings (EVT-03 to EVT-08, ADR 0020, ADR 0021).
//! - [`usage`]: usage access and the apps used recently (REC-02, REC-03, ADR 0023).
//! - [`network`]: the network's state and HTTPS requests (SEC-01, SEC-02, ADR 0027).
//! - [`wallpaper`]: the device's screens, cropping the daily image (WALL-06, ADR 0028), and
//!   setting, reading and resetting the lock-screen wallpaper (WALL-01, WALL-12 to WALL-14,
//!   ADR 0030).
//! - [`scheduler`]: the daily image's job, and "Update now" (ARCH-07, WALL-03, WALL-11,
//!   ADR 0029, ADR 0030).
//!
//! What isn't built yet answers with its "if unavailable" value (SRS 8.1), through
//! [`not_yet`].

mod apps;
mod jni;
mod network;
mod notifications;
mod scheduler;
mod system;
mod usage;
mod wallpaper;

use std::sync::Arc;

pub use jni::{Env, JObject, Local, LocalRef, RawEnv};

use crate::adapters::Platform;
use crate::model::DataError;
use crate::storage::FileStorage;

/// Every adapter, built on the thread that runs the native call that passed `bridge` and
/// `files_dir` (a `String`).
pub fn platform(env: Env, bridge: &LocalRef, files_dir: &LocalRef) -> Result<Platform, DataError> {
    let files_dir = env.string(files_dir)?;
    Ok(Platform {
        apps: Arc::new(apps::AndroidApps::new(env, bridge)?),
        notifications: Arc::new(notifications::AndroidNotifications::new(env, bridge)?),
        usage: Arc::new(usage::AndroidUsage::new(env, bridge)?),
        wallpaper: Arc::new(wallpaper::AndroidWallpaper::new(env, bridge)?),
        network: Arc::new(network::AndroidNetwork::new(env, bridge)?),
        scheduler: Arc::new(scheduler::AndroidScheduler::new(env, bridge)?),
        system: Arc::new(system::AndroidSystem::new(env, bridge)?),
        storage: Arc::new(FileStorage::new(files_dir)),
    })
}

/// What a request answers until its issue builds it.
fn not_yet() -> DataError {
    DataError::Unsupported("Not available yet".into())
}
