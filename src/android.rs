//! The native methods of `HomeActivity` (ADR 0005), called by Android on the main thread.
//!
//! The process has one [`Launcher`], kept on the main thread. Every `HomeActivity` shares it:
//! the one opened from the app drawer and the one Home starts can be alive at once, and two
//! launchers would both write MeeGone's files.
//!
//! A bug in MeeGone becomes a Java `RuntimeException` instead of an abort, so its message
//! reaches logcat. Containing it keeps `panic = "unwind"`, as ARCH-09 already requires.

#![allow(non_snake_case)] // JNI names native methods `Java_<class>_<method>`.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};

use meegone_core_data::android::{self, Env, JObject, LocalRef, RawEnv};

use crate::{Launcher, ShellIntent};

thread_local! {
    static LAUNCHER: RefCell<Option<Launcher>> = const { RefCell::new(None) };
}

/// `void nativeStart(Bridge bridge, String filesDir)`: starts MeeGone, once per process.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeStart(
    env: RawEnv,
    _activity: JObject,
    bridge: JObject,
    files_dir: JObject,
) {
    // SAFETY: the JVM passes this thread's environment and live local references, which are
    // used only during this call.
    let (env, bridge, files_dir) = unsafe {
        (
            Env::from_raw(env),
            LocalRef::from_raw(bridge),
            LocalRef::from_raw(files_dir),
        )
    };
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            if launcher.is_none() {
                let platform = android::platform(env, &bridge, &files_dir)
                    .map_err(|error| format!("MeeGone couldn't start: {error}"))?;
                *launcher = Some(Launcher::start(platform));
            }
            Ok(())
        })
    });
}

/// `void nativeResumed()`: MeeGone came to the foreground (HOME-01, ONB-04).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeResumed(
    env: RawEnv,
    _activity: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Resumed);
        launcher.frame();
    });
}

/// `void nativeRequestDefaultHome()`: in debug builds, taps the banner's "make default" button,
/// which isn't drawn yet, so the role request can be checked on a device (ADR 0005).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeRequestDefaultHome(
    env: RawEnv,
    _activity: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.view().home_banner().make_default_tapped();
        launcher.frame();
    });
}

fn with_launcher(env: Env, step: impl FnOnce(&mut Launcher)) {
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| match launcher {
            Some(launcher) => {
                step(launcher);
                Ok(())
            }
            None => Err("MeeGone hasn't started".into()),
        })
    });
}

/// Runs `body`, turning an error or a panic into a Java exception.
fn guarded(env: Env, body: impl FnOnce() -> Result<(), String>) {
    let message = match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => return,
        Ok(Err(message)) => message,
        Err(payload) => panic_message(payload.as_ref()),
    };
    env.throw_runtime_exception(&message);
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "MeeGone failed".to_owned()
    }
}
