//! The native methods of `HomeActivity` (ADR 0005, ADR 0007) and `HomeViews` (ADR 0006,
//! ADR 0008), called by Android on the main thread.
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

use crate::{
    Direction, HomeFrame, HomeView, Launcher, Layout, MenuItem, PointerEvent, PointerKind,
    SavedHome, SettingsGroup, ShellIntent,
};

/// JNI's `jboolean`.
type JBoolean = u8;

thread_local! {
    static LAUNCHER: RefCell<Option<Launcher>> = const { RefCell::new(None) };
}

/// `boolean nativeStart(Bridge bridge, String filesDir, int savedView, int savedAppsScrollPx)`:
/// starts MeeGone, once per process. Returns whether this call started it.
///
/// `savedView` is the view kept in the activity's saved state, or -1 if there is none. When
/// this call starts MeeGone with one, the system had ended the process, and the home comes
/// back as it was (REL-05). A launcher that is already running keeps its own state.
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
    saved_view: i32,
    saved_apps_scroll_px: i32,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment and live local references, which are
    // used only during this call.
    let (env, bridge, files_dir) = unsafe {
        (
            Env::from_raw(env),
            LocalRef::from_raw(bridge),
            LocalRef::from_raw(files_dir),
        )
    };
    let mut started = false;
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            if launcher.is_none() {
                let platform = android::platform(env, &bridge, &files_dir)
                    .map_err(|error| format!("MeeGone couldn't start: {error}"))?;
                let mut new = Launcher::start(platform);
                if let Some(view) = usize::try_from(saved_view)
                    .ok()
                    .and_then(HomeView::from_index)
                {
                    new.dispatch(ShellIntent::Restore(SavedHome {
                        view,
                        apps_scroll_px: f64::from(saved_apps_scroll_px),
                    }));
                }
                *launcher = Some(new);
                started = true;
            }
            Ok(())
        })
    });
    JBoolean::from(started)
}

/// `void nativeResumed(boolean returning, boolean byHome)`: MeeGone came to the foreground
/// (HOME-01, ONB-04). `returning` is true when an app or a system screen had covered the home,
/// and `byHome` when the system Home action brought it back meanwhile (HOME-06, ADR 0007).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeResumed(
    env: RawEnv,
    _activity: JObject,
    returning: JBoolean,
    by_home: JBoolean,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Resumed);
        if returning != 0 {
            launcher.dispatch(ShellIntent::Returned { home: by_home != 0 });
        }
        launcher.frame();
    });
}

/// `void nativeHome()`: the system Home action while the home is on screen (HOME-07).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeHome(
    env: RawEnv,
    _activity: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Home);
        launcher.frame();
    });
}

/// `void nativeBack()`: the system Back action on the home (HOME-08).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeBack(
    env: RawEnv,
    _activity: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Back);
        launcher.frame();
    });
}

/// `void nativeSave(int[] saved)`: fills `saved` with what the activity keeps in case the system
/// ends the process (REL-05): the current view, 0 to 2, then how far Applications is scrolled,
/// in whole pixels.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeSave(
    env: RawEnv,
    _activity: JObject,
    saved: JObject,
) {
    // SAFETY: the JVM passes this thread's environment and a live local reference to an
    // `int[]`, used only during this call.
    let (env, saved) = unsafe { (Env::from_raw(env), LocalRef::from_raw(saved)) };
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            let home = launcher.shell().saved();
            let slots = [home.view.index() as i32, home.apps_scroll_px.round() as i32];
            env.set_int_array(&saved, &slots)
                .map_err(|error| format!("Couldn't save the home: {error}"))
        })
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
/// the home views, with `MotionEvent`'s action. Returns whether the carousel or a long-press
/// has taken the gesture, so the content below gets no more of it (HOME-03, HOME-10).
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
    let mut taken = false;
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x: f64::from(x),
            y: f64::from(y),
            at: uptime(time_nanos),
        }));
        launcher.frame();
        let carousel = launcher.shell().state().carousel;
        taken = carousel.dragging || carousel.held;
    });
    JBoolean::from(taken)
}

/// `boolean nativeLongPress()`: the finger that is down has stayed on empty space for the
/// platform's long-press time (HOME-10). Returns whether the long-press was taken, so the
/// content below gets no more of the gesture.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeLongPress(
    env: RawEnv,
    _views: JObject,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut taken = false;
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::LongPress);
        launcher.frame();
        taken = launcher.shell().state().carousel.held;
    });
    JBoolean::from(taken)
}

/// `boolean nativeOpenMenu()`: opens the home menu without a gesture, for screen readers
/// (A11Y-05). Returns false if the current view has none.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeOpenMenu(
    env: RawEnv,
    _views: JObject,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut opened = false;
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::OpenMenu);
        launcher.frame();
        opened = launcher.shell().state().menu.is_some();
    });
    JBoolean::from(opened)
}

/// `void nativeMenu(int item)`: an item of the home menu was chosen, 0 to 2 in [`MenuItem::ALL`]
/// order, or -1 for a touch outside it, which closes it (HOME-10).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeMenu(
    env: RawEnv,
    _views: JObject,
    item: i32,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let intent = match usize::try_from(item).ok().and_then(MenuItem::from_index) {
        Some(item) => ShellIntent::ChooseMenuItem(item),
        None => ShellIntent::CloseMenu,
    };
    with_launcher(env, |launcher| {
        launcher.dispatch(intent);
        launcher.frame();
    });
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
/// | 10 | The home content's scale in ten-thousandths, 10 000 at rest (ANIM-09) |
/// | 11 | The home content's opacity, 0 to 255 (ANIM-09) |
/// | 12 | The home menu's items, as bits 1 << [`MenuItem::index`]; 0 while it is closed (HOME-10) |
/// | 13 | 1 when the current view offers the home menu and nothing covers it (A11Y-05) |
/// | 14–15 | Where the home menu opens, in px from the top left, or -1 to centre it |
/// | 16 | The surface colour of menus and Settings, as opaque ARGB |
/// | 17 | Settings: 0 closed, 1 open, 2 open at its Lock-screen image group |
fn frame_slots(frame: &HomeFrame) -> [i32; 18] {
    let [events, applications, recent] = frame.pages;
    let menu_items = frame.menu.map_or(0, |menu| {
        menu.items
            .iter()
            .fold(0, |bits, item| bits | 1 << item.index())
    });
    let (menu_x, menu_y) = frame.menu.and_then(|menu| menu.anchor).unwrap_or((-1, -1));
    let settings = match (frame.settings_open, frame.settings_group) {
        (false, _) => 0,
        (true, None) => 1,
        (true, Some(SettingsGroup::LockScreenImage)) => 2,
    };
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
        i32::from(frame.content_scale),
        i32::from(frame.content_alpha),
        menu_items,
        i32::from(frame.menu_offered),
        menu_x,
        menu_y,
        argb(frame.surface),
        settings,
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
