//! The native methods of `HomeActivity` (ADR 0005) and `HomeViews` (ADR 0006), called by
//! Android on the main thread.
//!
//! The process has one [`Launcher`], kept on the main thread. Every `HomeActivity` shares it:
//! the one opened from the app drawer and the one Home starts can be alive at once, and two
//! launchers would both write MeeGone's files.
//!
//! `HomeViews` draws the home views with Android's own views. It passes touches, frame times
//! and its layout in, then reads what to draw with `nativeRead`, whose slots are listed in
//! [`frame_slots`] and mirrored in `HomeViews.java`.
//!
//! A bug in MeeGone becomes a Java `RuntimeException` instead of an abort, so its message
//! reaches logcat. Containing it keeps `panic = "unwind"`, as ARCH-09 already requires.

#![allow(non_snake_case)] // JNI names native methods `Java_<class>_<method>`.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
use std::time::Duration;

use meegone_core_data::Rgb;
use meegone_core_data::android::{self, Env, JObject, LocalRef, RawEnv};

use crate::{Direction, HomeFrame, Launcher, Layout, PointerEvent, PointerKind, ShellIntent};

/// JNI's `jboolean`.
type JBoolean = u8;

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

/// `void nativeLayout(int widthPx, float density, int touchSlopPx, boolean rtl)`: the home
/// views were laid out, at start or after a size or direction change (L10N-02).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeLayout(
    env: RawEnv,
    _views: JObject,
    width_px: i32,
    density: f32,
    touch_slop_px: i32,
    rtl: JBoolean,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Layout(Layout {
            width_px: f64::from(width_px),
            density: f64::from(density),
            touch_slop_px: f64::from(touch_slop_px),
            rtl: rtl != 0,
        }));
        launcher.frame();
    });
}

/// `boolean nativePointer(int action, float x, float y, long timeNanos)`: the first finger on
/// the home views, with `MotionEvent`'s action. Returns whether the carousel has taken the
/// gesture, so the content below gets no more of it (HOME-03).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativePointer(
    env: RawEnv,
    _views: JObject,
    action: i32,
    x: f32,
    y: f32,
    time_nanos: i64,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    // `MotionEvent.ACTION_DOWN`, `ACTION_UP`, `ACTION_MOVE` and `ACTION_CANCEL`.
    let kind = match action {
        0 => PointerKind::Down,
        1 => PointerKind::Up,
        2 => PointerKind::Move,
        3 => PointerKind::Cancel,
        _ => return 0,
    };
    let mut dragging = false;
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x: f64::from(x),
            y: f64::from(y),
            at: uptime(time_nanos),
        }));
        launcher.frame();
        dragging = launcher.shell().state().carousel.dragging;
    });
    JBoolean::from(dragging)
}

/// `void nativeFrame(long frameTimeNanos)`: the display is about to draw a frame (ANIM-04).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeFrame(
    env: RawEnv,
    _views: JObject,
    frame_time_nanos: i64,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Frame(uptime(frame_time_nanos)));
        launcher.frame();
    });
}

/// `boolean nativeShow(boolean next)`: shows the next or previous view without a gesture, for
/// screen readers (A11Y-05). Returns false if there is none, past an end with wrap-around off.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeShow(
    env: RawEnv,
    _views: JObject,
    next: JBoolean,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let direction = if next != 0 {
        Direction::Next
    } else {
        Direction::Previous
    };
    let mut moved = false;
    with_launcher(env, |launcher| {
        let before = launcher.shell().state().current;
        launcher.dispatch(ShellIntent::Swipe(direction));
        launcher.frame();
        moved = launcher.shell().state().current != before;
    });
    JBoolean::from(moved)
}

/// `void nativeRead(int[] frame)`: fills `frame` with what to draw now, in the slots of
/// [`frame_slots`].
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeRead(
    env: RawEnv,
    _views: JObject,
    frame: JObject,
) {
    // SAFETY: the JVM passes this thread's environment and a live local reference to an
    // `int[]`, used only during this call.
    let (env, frame) = unsafe { (Env::from_raw(env), LocalRef::from_raw(frame)) };
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            env.set_int_array(&frame, &frame_slots(&launcher.frame()))
                .map_err(|error| format!("Couldn't pass the frame: {error}"))
        })
    });
}

/// A `HomeFrame` as the `int[]` that `HomeViews.java` reads, slot by slot:
///
/// | Slot | Holds |
/// |---|---|
/// | 0–2 | Events', Applications' and Recent's offset from where it fills the screen, in px |
/// | 3 | The view indicator's opacity, 0 to 255 |
/// | 4 | The indicator's highlighted segment, 0 to 2 |
/// | 5 | The current view, 0 to 2 |
/// | 6 | 1 while something moves, so another frame is needed; otherwise 0 |
/// | 7–9 | The background, foreground and dim colours, as opaque ARGB |
fn frame_slots(frame: &HomeFrame) -> [i32; 10] {
    let [events, applications, recent] = frame.pages;
    [
        events,
        applications,
        recent,
        i32::from(frame.indicator_alpha),
        frame.indicator_selected.index() as i32,
        frame.current.index() as i32,
        i32::from(frame.animating),
        argb(frame.background),
        argb(frame.foreground),
        argb(frame.dim),
    ]
}

/// An opaque colour as Android's `int` ARGB.
fn argb(colour: Rgb) -> i32 {
    let rgb = u32::from(colour.r) << 16 | u32::from(colour.g) << 8 | u32::from(colour.b);
    (0xFF00_0000 | rgb) as i32
}

/// A time on the host's monotonic clock, from `System.nanoTime()` or a `MotionEvent`.
fn uptime(nanos: i64) -> Duration {
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(0))
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
