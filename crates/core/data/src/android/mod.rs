//! The platform adapters on Android (ARCH-04, ADR 0005).
//!
//! The app's entry points (`src/android.rs`) build the [`Platform`] here once per process.
//! Adapters ask Android through a Java `Bridge` object over a minimal, hand-written JNI binding
//! ([`jni`]), and keep MeeGone's files in the folder the activity passes in, its private
//! `Context.getFilesDir()` (PRIV-05). Only `jni.rs` contains `unsafe` code.
//!
//! - [`system`]: the home-app role and the clock (HOME-01, ONB-04).
//! - [`stubs`]: every adapter not built yet, answering with its "if unavailable" value.

mod jni;
mod stubs;
mod system;

use std::sync::Arc;

pub use jni::{Env, JObject, LocalRef, RawEnv};

use crate::adapters::Platform;
use crate::model::DataError;
use crate::storage::FileStorage;

/// Every adapter, built on the thread that runs the native call that passed `bridge` and
/// `files_dir` (a `String`).
pub fn platform(env: Env, bridge: &LocalRef, files_dir: &LocalRef) -> Result<Platform, DataError> {
    let files_dir = env.string(files_dir)?;
    let not_built = Arc::new(stubs::NotBuiltYet);
    Ok(Platform {
        apps: not_built.clone(),
        notifications: not_built.clone(),
        usage: not_built.clone(),
        wallpaper: not_built.clone(),
        network: not_built.clone(),
        scheduler: not_built,
        system: Arc::new(system::AndroidSystem::new(env, bridge)?),
        storage: Arc::new(FileStorage::new(files_dir)),
    })
}
