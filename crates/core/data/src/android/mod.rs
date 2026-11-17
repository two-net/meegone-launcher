//! The platform adapters on Android (ARCH-04, ADR 0005).
//!
//! The app's entry points (`src/android.rs`) build the [`Platform`] here once per process.
//! Adapters ask Android through a Java `Bridge` object over a minimal, hand-written JNI binding
//! ([`jni`]), and keep MeeGone's files in the folder the activity passes in, its private
//! `Context.getFilesDir()` (PRIV-05). Only `jni.rs` contains `unsafe` code.
//!
//! - [`system`]: the home-app role, the clock, and web search (HOME-01, ONB-04, SRCH-05).
//! - [`apps`]: the app catalogue, launching and search keys (APPS-01, APPS-10, SRCH-02,
//!   ADR 0010).
//! - [`notifications`]: notification access, the active notifications, opening and dismissing
//!   them, and an app's notification settings (EVT-03 to EVT-08, ADR 0020, ADR 0021).
//! - [`usage`]: usage access and the apps used recently (REC-02, REC-03, ADR 0023).
//! - [`stubs`]: every adapter not built yet, answering with its "if unavailable" value.

mod apps;
mod jni;
mod notifications;
mod stubs;
mod system;
mod usage;

use std::sync::Arc;

pub use jni::{Env, JObject, Local, LocalRef, RawEnv};

use crate::adapters::Platform;
use crate::model::DataError;
use crate::storage::FileStorage;

/// Every adapter, built on the thread that runs the native call that passed `bridge` and
/// `files_dir` (a `String`).
pub fn platform(env: Env, bridge: &LocalRef, files_dir: &LocalRef) -> Result<Platform, DataError> {
    let files_dir = env.string(files_dir)?;
    let not_built = Arc::new(stubs::NotBuiltYet);
    Ok(Platform {
        apps: Arc::new(apps::AndroidApps::new(env, bridge)?),
        notifications: Arc::new(notifications::AndroidNotifications::new(env, bridge)?),
        usage: Arc::new(usage::AndroidUsage::new(env, bridge)?),
        wallpaper: not_built.clone(),
        network: not_built.clone(),
        scheduler: not_built,
        system: Arc::new(system::AndroidSystem::new(env, bridge)?),
        storage: Arc::new(FileStorage::new(files_dir)),
    })
}

/// What a request answers until its issue builds it.
fn not_yet() -> DataError {
    DataError::Unsupported("Not available yet".into())
}
