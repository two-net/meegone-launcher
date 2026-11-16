//! The native methods of `HomeActivity` (ADR 0005, ADR 0007) and `HomeViews` (ADR 0006,
//! ADR 0008), called by Android on the main thread. The one exception is `AppAccents`'
//! `nativeAccent`, which is pure and runs on its own thread (ADR 0011).
//!
//! The process has one [`Launcher`], kept on the main thread. Every `HomeActivity` shares it:
//! the one opened from the app drawer and the one Home starts can be alive at once, and two
//! launchers would both write MeeGone's files.
//!
//! `HomeViews` draws the home views with Android's own views. It passes touches, frame times
//! and its layout in, then reads what to draw with `nativeRead`, whose slots are listed in
//! [`frame_slots`] and mirrored in `HomeViews.java`. Its Applications grid, `AppsGrid`, reads
//! its cells with `nativeAppsCells` whenever their revision changes, in the fields of
//! [`cell_fields`] (ADR 0010).
//!
//! A bug in MeeGone becomes a Java `RuntimeException` instead of an abort, so its message
//! reaches logcat. Containing it keeps `panic = "unwind"`, as ARCH-09 already requires.

#![allow(non_snake_case)] // JNI names native methods `Java_<class>_<method>`.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
use std::time::Duration;

use meegone_core_data::android::{self, Env, JObject, LocalRef, RawEnv};
use meegone_core_data::{Rect, Rgb, accent};

use crate::{
    Cell, Direction, HomeFrame, HomeView, Launcher, Layout, MenuItem, PointerEvent, PointerKind,
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

/// `static void nativeAppsChanged()`: the system reported that an app was installed, updated,
/// removed, suspended or unsuspended, or that a profile was added, removed, paused or resumed
/// (APPS-09, ADR 0010). `Bridge` coalesces a burst of them into one call.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeAppsChanged(
    env: RawEnv,
    _class: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::AppsChanged);
        launcher.frame();
    });
}

/// `static int nativeAccent(int[] argb)`: the accent colour of an app's icon, drawn in
/// MeeGone's shape, from its pixels as `0xAARRGGBB`. Returns `0xRRGGBB` (APPS-17, ADR 0011).
///
/// Unlike every other native method, `AppAccents` calls it on its own background thread, once
/// per icon. It is pure: it reads only its argument and never touches the [`Launcher`], which
/// belongs to the main thread.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppAccents_nativeAccent(
    env: RawEnv,
    _class: JObject,
    icon: JObject,
) -> i32 {
    // SAFETY: the JVM passes this thread's environment and a live local reference to an
    // `int[]`, used only during this call.
    let (env, icon) = unsafe { (Env::from_raw(env), LocalRef::from_raw(icon)) };
    let mut rgb = 0;
    guarded(env, || {
        let pixels = env
            .int_array(&icon)
            .map_err(|error| format!("Couldn't read the icon: {error}"))?;
        // Android's `int` pixels are `0xAARRGGBB` bit for bit.
        let pixels: Vec<u32> = pixels.into_iter().map(|pixel| pixel as u32).collect();
        rgb = argb(accent::derive(&pixels)) & 0x00FF_FFFF;
        Ok(())
    });
    rgb
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

/// `String[] nativeAppsCells()`: the Applications grid's cells, in order, as of the last frame,
/// each in the fields of [`cell_fields`] (APPS-01).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppsCells(
    env: RawEnv,
    _grid: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut cells = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow(|launcher| {
            let launcher = launcher.as_ref().ok_or("MeeGone hasn't started")?;
            let fields: Vec<String> = launcher
                .view()
                .apps()
                .cells()
                .flat_map(cell_fields)
                .collect();
            let array = env
                .new_string_array(fields.iter().map(String::as_str))
                .map_err(|error| format!("Couldn't pass the apps: {error}"))?;
            cells = array.into_raw();
            Ok(())
        })
    });
    cells
}

/// The fields of one cell in `nativeAppsCells`, mirrored in `AppsGrid.java`: its label, then
/// the package, activity and profile serial number of the app it launches (empty for a
/// folder), then its flags: 1 an app, 2 available (APPS-09), 4 in a work profile (APPS-20),
/// 8 new (APPS-07). Last, its place in the cells before, or -1 if it just joined (ANIM-21).
fn cell_fields(cell: Cell<'_>) -> [String; 6] {
    let flags = u8::from(cell.app.is_some())
        | u8::from(cell.available) << 1
        | u8::from(cell.work) << 2
        | u8::from(cell.new) << 3;
    let from = cell
        .from
        .and_then(|from| i32::try_from(from).ok())
        .unwrap_or(-1);
    let (package, activity, profile) = match cell.app {
        Some(app) => (
            app.package.clone(),
            app.activity.clone(),
            app.profile.0.to_string(),
        ),
        None => Default::default(),
    };
    [
        cell.label.to_owned(),
        package,
        activity,
        profile,
        flags.to_string(),
        from.to_string(),
    ]
}

/// `void nativeAppsLayout(int widthPx, int rowHeightPx, float density, int[] grid)`: the grid
/// was laid out this wide inside its margins, with rows this far apart: at start, or after a
/// size, density or font-size change (APPS-02, APPS-16). Fills `grid` with the columns and rows
/// it now has, so the grid lays them out in the same pass.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppsLayout(
    env: RawEnv,
    _grid: JObject,
    width_px: i32,
    row_height_px: i32,
    density: f32,
    grid: JObject,
) {
    // SAFETY: the JVM passes this thread's environment and a live local reference to an
    // `int[]`, used only during this call.
    let (env, grid) = unsafe { (Env::from_raw(env), LocalRef::from_raw(grid)) };
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            launcher.view().apps().grid_laid_out(
                f64::from(width_px),
                f64::from(row_height_px),
                f64::from(density),
            );
            let apps = launcher.frame().apps;
            env.set_int_array(&grid, &[i32::from(apps.columns), i32::from(apps.rows)])
                .map_err(|error| format!("Couldn't pass the grid: {error}"))
        })
    });
}

/// `void nativeAppsScrolled(int px)`: the user scrolled the grid this far from its top
/// (HOME-07, REL-05).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppsScrolled(
    env: RawEnv,
    _grid: JObject,
    px: i32,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.view().apps().grid_scrolled(f64::from(px));
        launcher.frame();
    });
}

/// `void nativeAppPressed(int index, long timeNanos)`: a finger went down on cell `index`, so
/// its icon shrinks (ANIM-10).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppPressed(
    env: RawEnv,
    _grid: JObject,
    index: i32,
    time_nanos: i64,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let Ok(item) = usize::try_from(index) else {
        return;
    };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Press {
            item,
            at: uptime(time_nanos),
        });
        launcher.frame();
    });
}

/// `void nativeAppReleased(long timeNanos)`: the finger lifted or left the pressed cell, which
/// springs back (ANIM-10).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppReleased(
    env: RawEnv,
    _grid: JObject,
    time_nanos: i64,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Release(uptime(time_nanos)));
        launcher.frame();
    });
}

/// `String nativeAppTapped(int revision, int index, int left, int top, int right, int bottom)`:
/// cell `index` of the cells at `revision` was tapped, and its icon is at that rect on the
/// screen (APPS-10, ANIM-08). Launches its app at once (PERF-06). Returns `null`, or a short
/// message to show when the system couldn't start the app and gave no explanation of its own
/// (APPS-09).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // The JNI calling convention.
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppTapped(
    env: RawEnv,
    _grid: JObject,
    revision: i32,
    index: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let Ok(index) = usize::try_from(index) else {
        return std::ptr::null_mut();
    };
    let from = Rect {
        left,
        top,
        right,
        bottom,
    };
    let mut message = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            // The revision travels as Java's `int`.
            if !launcher
                .view()
                .apps()
                .cell_tapped(revision as u32, index, from)
            {
                return Ok(());
            }
            launcher.frame();
            if let Some(problem) = &launcher.shell().apps().state().problem {
                let text = env
                    .new_string(problem)
                    .map_err(|error| format!("Couldn't pass the message: {error}"))?;
                message = text.into_raw();
            }
            Ok(())
        })
    });
    message
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
/// | 18 | Applications' cells revision: when it changes, `AppsGrid` reads its cells again |
/// | 19 | How far Applications is scrolled, in px (HOME-07, REL-05) |
/// | 20 | Applications' columns (APPS-02) |
/// | 21 | Applications' rows, the last one empty (HOME-14) |
/// | 22 | The pressed cell of the current view, or -1 (ANIM-10) |
/// | 23 | The pressed cell's icon scale in ten-thousandths, 10 000 at rest (ANIM-10) |
/// | 24–25 | The scale and opacity of cells that just joined the grid, in ten-thousandths (ANIM-21) |
/// | 26–27 | The scale and opacity of cells that left the grid, in ten-thousandths (ANIM-21) |
/// | 28 | How far the other cells have come from their old places, in ten-thousandths (ANIM-21) |
/// | 29 | The new-app dot's colour, as opaque ARGB (APPS-07) |
fn frame_slots(frame: &HomeFrame) -> [i32; 30] {
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
        // The revision wraps around; the host only compares it.
        frame.apps.revision as i32,
        frame.apps.scroll_px,
        i32::from(frame.apps.columns),
        i32::from(frame.apps.rows),
        frame
            .pressed
            .and_then(|item| i32::try_from(item).ok())
            .unwrap_or(-1),
        i32::from(frame.press_scale),
        i32::from(frame.grid_change.appear_scale),
        i32::from(frame.grid_change.appear_alpha),
        i32::from(frame.grid_change.leave_scale),
        i32::from(frame.grid_change.leave_alpha),
        i32::from(frame.grid_change.shift),
        argb(frame.accent),
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
