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
//! [`cell_fields`] (ADR 0010). An app's long-press menu is read the same way, with
//! `nativeAppMenu`, in the fields of [`menu_item_fields`] (ADR 0013). In edit mode, the grid
//! names a cell by revision and place when its ✕ is tapped or a screen reader moves it
//! (ADR 0014). Search's field sends each change of its text, and its results are a third grid
//! (ADR 0016). Events' rows are read the same way, with `nativeEventsList`, and named by
//! revision and place when tapped or long-pressed (ADR 0020, ADR 0021).
//!
//! A bug in MeeGone becomes a Java `RuntimeException` instead of an abort, so its message
//! reaches logcat. Containing it keeps `panic = "unwind"`, as ARCH-09 already requires.

#![allow(non_snake_case)] // JNI names native methods `Java_<class>_<method>`.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
use std::time::Duration;

use meegone_core_data::android::{self, Env, JObject, LocalRef, RawEnv};
use meegone_core_data::{App, Rect, Rgb, accent};

use crate::{
    AppMenuItem, BADGE_ROWS, Bounds, Cell, CellKey, Direction, EventsListMode, EventsRow,
    FOLDER_COLUMNS, FolderId, GridLayout, HomeFrame, HomeView, Launcher, Layout, MenuItem,
    NotificationMenuItem, PREVIEW_APPS, PointerEvent, PointerKind, RowTarget, SavedHome,
    SettingsGroup, ShellIntent, Surface,
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

/// `static void nativeClockChanged()`: the date, the time, the time zone, the 12- or 24-hour
/// setting or the next alarm changed (EVT-01, EVT-02, L10N-03, ADR 0019). `Bridge` coalesces a
/// burst of them into one call.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeClockChanged(
    env: RawEnv,
    _class: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::ClockChanged);
        launcher.frame();
    });
}

/// `static void nativeNotificationsChanged()`: a notification was posted or removed, or
/// MeeGone's listener was connected or disconnected, as when notification access is granted or
/// revoked (EVT-10, ADR 0020). `Bridge` coalesces a burst of them into one call, and makes none
/// before the home has started.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeActivity_nativeNotificationsChanged(
    env: RawEnv,
    _class: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::NotificationsChanged);
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
/// the home views, with `MotionEvent`'s action. Returns whether the carousel, a long-press or
/// the search field's pull has taken the gesture, so the content below gets no more of it
/// (HOME-03, HOME-10, SRCH-01).
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
        let state = launcher.shell().state();
        taken = state.carousel.dragging || state.carousel.held || state.search.pulling;
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

/// `String[] nativeAppMenu()`: the items of the app menu as of the last frame, in order, each in
/// the fields of [`menu_item_fields`] (APPS-11). Empty while it is closed.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeAppMenu(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut items = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow(|launcher| {
            let launcher = launcher.as_ref().ok_or("MeeGone hasn't started")?;
            let fields: Vec<String> = launcher
                .view()
                .apps()
                .menu_items()
                .iter()
                .flat_map(menu_item_fields)
                .collect();
            let array = env
                .new_string_array(fields.iter().map(String::as_str))
                .map_err(|error| format!("Couldn't pass the menu: {error}"))?;
            items = array.into_raw();
            Ok(())
        })
    });
    items
}

/// The fields of one item of a cell's menu in `nativeAppMenu`, mirrored in `HomeViews.java`:
/// its kind, 0 App info, 1 Uninstall, 2 Edit applications, 3 a shortcut, 4 Rename folder or
/// 5 Hide (APPS-19), then the shortcut's label, or nothing for the others, whose text is in
/// Android's string resources (L10N-01).
fn menu_item_fields(item: &AppMenuItem) -> [String; 2] {
    let (kind, label) = match item {
        AppMenuItem::AppInfo => (0, ""),
        AppMenuItem::Uninstall => (1, ""),
        AppMenuItem::EditApplications => (2, ""),
        AppMenuItem::Shortcut(shortcut) => (3, shortcut.label.as_str()),
        AppMenuItem::RenameFolder => (4, ""),
        AppMenuItem::Hide => (5, ""),
    };
    [kind.to_string(), label.to_owned()]
}

/// `String nativeAppMenuChosen(int revision, int item)`: item `item` of the app menu at
/// `revision` was chosen (APPS-11). Returns `null`, or a short message to show when the system
/// refused, such as an App info screen that couldn't open.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeAppMenuChosen(
    env: RawEnv,
    _views: JObject,
    revision: i32,
    item: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let Ok(item) = usize::try_from(item) else {
        return std::ptr::null_mut();
    };
    let mut message = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            // The revision travels as Java's `int`.
            if !launcher
                .view()
                .apps()
                .menu_item_chosen(revision as u32, item)
            {
                return Ok(());
            }
            launcher.frame();
            message = problem(env, launcher)?;
            Ok(())
        })
    });
    message
}

/// `String[] nativeHiddenApps()`: Settings → Applications → Hidden apps as of the last frame,
/// in order, each in the fields of [`hidden_app_fields`] (APPS-19). Empty while Applications
/// isn't working (ARCH-09).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeHiddenApps(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut apps = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow(|launcher| {
            let launcher = launcher.as_ref().ok_or("MeeGone hasn't started")?;
            let fields: Vec<String> = if launcher.shell().apps_ready() {
                let hidden = launcher.view().hidden_apps().apps();
                hidden.iter().flat_map(hidden_app_fields).collect()
            } else {
                Vec::new()
            };
            let array = env
                .new_string_array(fields.iter().map(String::as_str))
                .map_err(|error| format!("Couldn't pass the hidden apps: {error}"))?;
            apps = array.into_raw();
            Ok(())
        })
    });
    apps
}

/// The fields of one hidden app in `nativeHiddenApps`, mirrored in `HiddenAppsList.java`:
/// 1. its label;
/// 2. to 4. its package, activity and profile serial number, which its icon is loaded by;
/// 5. its flags: 1 in a work profile (APPS-20), 2 available (APPS-09).
fn hidden_app_fields(app: &App) -> [String; 5] {
    let flags = u8::from(app.work) | u8::from(app.available) << 1;
    [
        app.label.clone(),
        app.id.package.clone(),
        app.id.activity.clone(),
        app.id.profile.0.to_string(),
        flags.to_string(),
    ]
}

/// `String nativeUnhide(int revision, int index)`: Unhide was tapped on app `index` of the
/// hidden apps at `revision` (APPS-19). It shows in the grid again, in its old place. Returns
/// `null`, or a short message to show when it couldn't be saved. A list that changed
/// meanwhile does nothing.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeUnhide(
    env: RawEnv,
    _views: JObject,
    revision: i32,
    index: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let Ok(index) = usize::try_from(index) else {
        return std::ptr::null_mut();
    };
    let mut message = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            // A failed Applications drops the intent, and its old problem means nothing now
            // (ARCH-09). The revision travels as Java's `int`.
            if !launcher.shell().apps_ready()
                || !launcher
                    .view()
                    .hidden_apps()
                    .unhide_tapped(revision as u32, index)
            {
                return Ok(());
            }
            launcher.frame();
            message = problem(env, launcher)?;
            Ok(())
        })
    });
    message
}

/// Applications' problem as a Java string, or `null` if there is none.
fn problem(env: Env, launcher: &Launcher) -> Result<JObject, String> {
    message(env, launcher.shell().apps().state().problem.as_deref())
}

/// A message for the host to show as a Java string, or `null` for none.
fn message(env: Env, text: Option<&str>) -> Result<JObject, String> {
    match text {
        Some(text) => env
            .new_string(text)
            .map(|text| text.into_raw())
            .map_err(|error| format!("Couldn't pass the message: {error}")),
        None => Ok(std::ptr::null_mut()),
    }
}

/// `String[] nativeEventsHeader()`: the Events header as of the last frame (EVT-01, EVT-02),
/// mirrored in `EventsHeader.java`:
/// 1. today's weekday;
/// 2. today's day and month;
/// 3. when the next alarm rings, or empty to hide the alarm row.
///
/// Empty while Events isn't working (ARCH-09).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeEventsHeader(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut fields = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow(|launcher| {
            let launcher = launcher.as_ref().ok_or("MeeGone hasn't started")?;
            let header = launcher.view().events().header();
            let texts: Vec<&str> = if launcher.shell().events_ready() {
                vec![
                    header.weekday.as_str(),
                    header.date.as_str(),
                    header.alarm.as_deref().unwrap_or_default(),
                ]
            } else {
                Vec::new()
            };
            let array = env
                .new_string_array(texts.into_iter())
                .map_err(|error| format!("Couldn't pass the Events header: {error}"))?;
            fields = array.into_raw();
            Ok(())
        })
    });
    fields
}

/// `String nativeOpenAlarm()`: the alarm row of the Events header was tapped, so the app that
/// set the alarm opens (EVT-02). Returns `null`, or a short message to show when it couldn't.
/// Does nothing while Events isn't working (ARCH-09).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeOpenAlarm(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut shown = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            if !launcher.shell().events_ready() {
                return Ok(());
            }
            launcher.view().events().alarm_tapped();
            launcher.frame();
            shown = message(env, launcher.shell().events().state().problem.as_deref())?;
            Ok(())
        })
    });
    shown
}

/// `String[] nativeEventsList()`: the app groups below the Events header as of the last frame,
/// newest first, each in the fields of [`events_row_fields`] (EVT-04). Empty unless the frame's
/// slot 82 says the list shows. The fields hold what each newest notification says, so they
/// stay in memory, and no error message carries them (EVT-13).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeEventsList(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut rows = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow(|launcher| {
            let launcher = launcher.as_ref().ok_or("MeeGone hasn't started")?;
            let fields: Vec<String> = if launcher.shell().events_ready() {
                let rows = launcher.view().events().rows();
                rows.iter().flat_map(events_row_fields).collect()
            } else {
                Vec::new()
            };
            let array = env
                .new_string_array(fields.iter().map(String::as_str))
                .map_err(|error| format!("Couldn't pass the Events list: {error}"))?;
            rows = array.into_raw();
            Ok(())
        })
    });
    rows
}

/// The fields of one row in `nativeEventsList`, mirrored in `EventsList.java`:
/// 1. where it was in the rows last read, which keeps its view, or -1 for a row that comes in
///    (ANIM-20);
/// 2. the app's name;
/// 3. its flags: 1 in a work profile (APPS-20); 2 one notification of the expanded group, below
///    its group's row; 4 a group's row while it is expanded (EVT-06);
/// 4. to 6. its package, launcher activity and profile serial number, which its icon is loaded
///    by; the activity is empty for an app with no icon in the grid, whose icon the system gives;
/// 7. the marker's colour, as opaque ARGB in decimal (EVT-09);
/// 8. how many notifications its group holds, or 1 for a notification's row;
/// 9. and 10. what it says: a group's newest notification's title and text, empty while the
///    group is expanded, or the notification's own.
fn events_row_fields(row: &EventsRow) -> [String; 10] {
    let flags = u8::from(row.work) | u8::from(row.notification) << 1 | u8::from(row.expanded) << 2;
    [
        row.from
            .and_then(|at| i32::try_from(at).ok())
            .unwrap_or(-1)
            .to_string(),
        row.label.clone(),
        flags.to_string(),
        row.package.clone(),
        row.activity.clone().unwrap_or_default(),
        row.profile.0.to_string(),
        argb(row.accent).to_string(),
        row.count.to_string(),
        row.title.clone(),
        row.text.clone(),
    ]
}

/// What row `index` of Events' rows at `revision` stands for, if they are still the rows the
/// host last read and Events works (ARCH-09).
fn row_at(launcher: &Launcher, revision: i32, index: i32) -> Option<RowTarget> {
    let index = usize::try_from(index).ok()?;
    if !launcher.shell().events_ready() {
        return None;
    }
    // The revision travels as Java's `int`.
    launcher.view().events().row_target(revision as u32, index)
}

/// Asks something of Events with `step`, applies it, and returns Events' `problem` as a message
/// for the host to show, or `null`. Does nothing while Events isn't working (ARCH-09). The
/// message never carries what a notification says (EVT-13).
fn with_events_message(env: Env, step: impl FnOnce(&Launcher) -> bool) -> JObject {
    let mut shown = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            if !launcher.shell().events_ready() || !step(launcher) {
                return Ok(());
            }
            launcher.frame();
            shown = message(env, launcher.shell().events().state().problem.as_deref())?;
            Ok(())
        })
    });
    shown
}

/// `String nativeEventsRowTapped(int revision, int index, int left, int top, int right, int
/// bottom)`: row `index` of Events' rows at `revision` was tapped, and is at that rect on the
/// screen (EVT-06). A notification opens, its window growing from the row (ANIM-08), and a group
/// of several expands or collapses. Returns `null`, or a short message to show when nothing
/// could open. Rows that changed since the host drew them do nothing.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // The JNI calling convention.
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeEventsRowTapped(
    env: RawEnv,
    _views: JObject,
    revision: i32,
    index: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let from = Rect {
        left,
        top,
        right,
        bottom,
    };
    with_events_message(env, |launcher| {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        // The revision travels as Java's `int`.
        launcher
            .view()
            .events()
            .row_tapped(revision as u32, index, from)
    })
}

/// `boolean nativeEventsRowLongPressed(int revision, int index)`: the finger that is down has
/// stayed on row `index` of Events' rows at `revision` for the platform's long-press time
/// (EVT-08). Returns whether the long-press was taken: the row's menu opened, and the rest of
/// the gesture is the shell's, so lifting the finger taps nothing. Rows that changed since the
/// host drew them take nothing.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeEventsRowLongPressed(
    env: RawEnv,
    _views: JObject,
    revision: i32,
    index: i32,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut taken = false;
    with_launcher(env, |launcher| {
        if let Some(target) = row_at(launcher, revision, index) {
            launcher.dispatch(ShellIntent::LongPressNotification(target));
            launcher.frame();
            let state = launcher.shell().state();
            taken = state.events_menu_open && state.carousel.held;
        }
    });
    JBoolean::from(taken)
}

/// `boolean nativeEventsMenuRequested(int revision, int index)`: opens the menu of row `index`
/// of Events' rows at `revision` without a gesture, for screen readers (A11Y-05). Returns
/// whether it opened.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeEventsMenuRequested(
    env: RawEnv,
    _views: JObject,
    revision: i32,
    index: i32,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut opened = false;
    with_launcher(env, |launcher| {
        if let Some(target) = row_at(launcher, revision, index) {
            launcher.dispatch(ShellIntent::OpenNotificationMenu(target));
            launcher.frame();
            opened = launcher.shell().state().events_menu_open;
        }
    });
    JBoolean::from(opened)
}

/// `String nativeEventsMenuChosen(int item)`: an item of the open notification menu was chosen,
/// by its code in [`notification_menu_bit`]: 0 Dismiss, 1 Notification settings (EVT-08).
/// Returns `null`, or a short message to show when the system refused.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeEventsMenuChosen(
    env: RawEnv,
    _views: JObject,
    item: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let item = match item {
        0 => NotificationMenuItem::Dismiss,
        1 => NotificationMenuItem::NotificationSettings,
        _ => return std::ptr::null_mut(),
    };
    with_events_message(env, |launcher| {
        launcher.view().events().menu_item_chosen(item);
        true
    })
}

/// `String nativeClearNotifications()`: Clear was tapped on Events' section header, so every
/// clearable notification goes (EVT-07). Returns `null`, or a short message to show when the
/// system refused.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeClearNotifications(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_events_message(env, |launcher| {
        launcher.view().events().clear_tapped();
        true
    })
}

/// `String nativeAllowNotifications()`: Allow was tapped on the Events explainer card, so the
/// system's notification-access screen opens (EVT-03). Returns `null`, or a short message to
/// show when it couldn't. Does nothing while Events isn't working (ARCH-09).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeAllowNotifications(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_events_message(env, |launcher| {
        launcher.view().events().allow_access_tapped();
        true
    })
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

/// The surface a grid native names: 0 the Applications grid, 1 the open folder's panel
/// (ADR 0015).
fn surface(code: i32) -> Option<Surface> {
    match code {
        0 => Some(Surface::Grid),
        1 => Some(Surface::Folder),
        2 => Some(Surface::Search),
        _ => None,
    }
}

/// `String[] nativeAppsCells(int surface)`: the cells of the Applications grid or the open
/// folder, in order, as of the last frame, each in the fields of [`cell_fields`] (APPS-01,
/// FOLD-04).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppsCells(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let Some(surface) = surface(surface_code) else {
        return std::ptr::null_mut();
    };
    let mut cells = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow(|launcher| {
            let launcher = launcher.as_ref().ok_or("MeeGone hasn't started")?;
            let fields: Vec<String> = launcher
                .view()
                .apps()
                .cells(surface)
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

/// The fields of one cell in `nativeAppsCells`, mirrored in `AppsGrid.java`:
/// 1. its label, or a folder's name, `""` until the user names it (FOLD-03);
/// 2. to 4. the package, activity and profile serial number of the app it launches, empty for
///    a folder;
/// 5. its flags: 1 an app, 2 available (APPS-09), 4 in a work profile (APPS-20), 8 new
///    (APPS-07), 16 the user can uninstall it, so it has a ✕ in edit mode (APPS-12), 32 a
///    folder;
/// 6. its place in the cells the grid last read, or -1 if it joined since (ANIM-21);
/// 7. how many apps a folder holds, or 0;
/// 8. a folder's first apps, which its icon previews (FOLD-02): each as profile, package and
///    activity separated by `/`, the apps separated by line breaks.
fn cell_fields(cell: Cell<'_>) -> [String; 8] {
    let folder = cell.app.is_none();
    let flags = u8::from(cell.app.is_some())
        | u8::from(cell.available) << 1
        | u8::from(cell.work) << 2
        | u8::from(cell.new) << 3
        | u8::from(cell.uninstallable) << 4
        | u8::from(folder) << 5;
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
    let preview: Vec<String> = cell
        .apps
        .iter()
        .take(PREVIEW_APPS)
        .map(|app| {
            format!(
                "{}/{}/{}",
                app.id.profile.0, app.id.package, app.id.activity
            )
        })
        .collect();
    [
        cell.label.to_owned(),
        package,
        activity,
        profile,
        flags.to_string(),
        from.to_string(),
        cell.apps.len().to_string(),
        preview.join("\n"),
    ]
}

/// `void nativeAppsLayout(int surface, float density, int[] layout, int[] grid)`: the
/// Applications grid, the open folder's or search's results were laid out (APPS-02, APPS-16,
/// ADR 0014 to ADR 0016). `layout` holds, in px:
/// 0. the cells' width inside the margins;
/// 1. the row height;
/// 2. and 3. the room above and below the cells, inside the grid: the safe area;
/// 4. the grid's height on screen;
/// 5. where the cells start, from the home views' left;
/// 6. where the grid's top is, from the home views' top;
/// 7. and 8. each icon's size, and how far below its cell's top it starts;
/// 9. to 12. for a folder, its panel's left, top, right and bottom on screen; for the results,
///    the search field's band's (ANIM-16).
///
/// Fills `grid` with the columns and rows the grid now has, so it lays them out in the same
/// pass.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppsLayout(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
    density: f32,
    layout: JObject,
    grid: JObject,
) {
    // SAFETY: the JVM passes this thread's environment and live local references to two
    // `int[]`s, used only during this call.
    let (env, layout, grid) = unsafe {
        (
            Env::from_raw(env),
            LocalRef::from_raw(layout),
            LocalRef::from_raw(grid),
        )
    };
    let Some(surface) = surface(surface_code) else {
        return;
    };
    guarded(env, || {
        let values = env
            .int_array(&layout)
            .map_err(|error| format!("Couldn't read the layout: {error}"))?;
        let value = |slot: usize| values.get(slot).map_or(0.0, |&value| f64::from(value));
        let laid_out = GridLayout {
            width_px: value(0),
            row_height_px: value(1),
            density: f64::from(density),
            top_px: value(2),
            bottom_px: value(3),
            height_px: value(4),
            left_px: value(5),
            view_top_px: value(6),
            icon_px: value(7),
            icon_top_px: value(8),
        };
        let panel = Bounds {
            left: value(9),
            top: value(10),
            right: value(11),
            bottom: value(12),
        };
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            let apps = launcher.view().apps();
            match surface {
                Surface::Grid => apps.grid_laid_out(laid_out),
                Surface::Folder => apps.folder_laid_out(laid_out, panel),
                Surface::Search => apps.search_laid_out(laid_out, panel),
            }
            let apps = launcher.frame().apps;
            let (columns, rows) = match surface {
                Surface::Grid => (i32::from(apps.columns), i32::from(apps.rows)),
                Surface::Folder => (FOLDER_COLUMNS as i32, i32::from(apps.folder.rows)),
                Surface::Search => (laid_out.columns() as i32, i32::from(apps.results.rows)),
            };
            env.set_int_array(&grid, &[columns, rows])
                .map_err(|error| format!("Couldn't pass the grid: {error}"))
        })
    });
}

/// `void nativeAppsScrolled(int surface, int px)`: the user scrolled the Applications grid, or
/// the open folder's, this far from its top (HOME-07, REL-05).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppsScrolled(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
    px: i32,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let Some(surface) = surface(surface_code) else {
        return;
    };
    with_launcher(env, |launcher| {
        launcher.view().apps().scrolled(surface, f64::from(px));
        launcher.frame();
    });
}

/// `void nativeAppPressed(int surface, int index, long timeNanos)`: a finger went down on cell
/// `index` of the grid or the open folder, so its icon shrinks (ANIM-10).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppPressed(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
    index: i32,
    time_nanos: i64,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let (Some(surface), Ok(item)) = (surface(surface_code), usize::try_from(index)) else {
        return;
    };
    with_launcher(env, |launcher| {
        launcher.dispatch(ShellIntent::Press {
            surface,
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

/// `boolean nativeAppLongPressed(int surface, int revision, int index)`: the finger that is down
/// has stayed on cell `index` of the cells at `revision`, in the grid or the open folder, for the
/// platform's long-press time (APPS-11, ADR 0015). Returns whether the long-press was taken: the
/// app's or folder's menu opened, and the rest of the gesture is the shell's, so lifting the
/// finger taps nothing. Cells that changed since the host drew them take nothing.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppLongPressed(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
    revision: i32,
    index: i32,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut taken = false;
    with_launcher(env, |launcher| {
        if let Some((surface, key)) = key_at(launcher, surface_code, revision, index) {
            launcher.dispatch(ShellIntent::LongPressCell { surface, key });
            launcher.frame();
            taken = launcher.shell().state().carousel.held;
        }
    });
    JBoolean::from(taken)
}

/// `boolean nativeAppMenuRequested(int surface, int revision, int index)`: opens the menu of
/// cell `index` of the cells at `revision` without a gesture, for screen readers (A11Y-05).
/// Returns whether it opened.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppMenuRequested(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
    revision: i32,
    index: i32,
) -> JBoolean {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut opened = false;
    with_launcher(env, |launcher| {
        if let Some((_, key)) = key_at(launcher, surface_code, revision, index) {
            launcher.dispatch(ShellIntent::OpenCellMenu(key));
            launcher.frame();
            opened = launcher.shell().state().app_menu_open;
        }
    });
    JBoolean::from(opened)
}

/// `String nativeAppBadgeTapped(int surface, int revision, int index)`: the ✕ of cell `index`
/// of the cells at `revision` was tapped, in edit mode (APPS-13). The system asks the user to
/// confirm the uninstall; MeeGone never uninstalls anything itself. Returns `null`, or a short
/// message to show when the system refused.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppBadgeTapped(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
    revision: i32,
    index: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let (Some(surface), Ok(index)) = (surface(surface_code), usize::try_from(index)) else {
        return std::ptr::null_mut();
    };
    let mut message = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            // The revision travels as Java's `int`.
            let apps = launcher.view().apps();
            if !apps.badge_tapped(surface, revision as u32, index) {
                return Ok(());
            }
            launcher.frame();
            message = problem(env, launcher)?;
            Ok(())
        })
    });
    message
}

/// `int nativeAppMoved(int surface, int revision, int index, int to)`: a screen reader moved
/// cell `index` of the cells at `revision` to place `to`, in the grid or within the open
/// folder, in edit mode (A11Y-05). Returns the place its cell is at now, or -1 if nothing moved.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppMoved(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
    revision: i32,
    index: i32,
    to: i32,
) -> i32 {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let (Ok(cell), Ok(to)) = (usize::try_from(index), usize::try_from(to)) else {
        return -1;
    };
    let mut place = -1;
    with_launcher(env, |launcher| {
        let Some((surface, key)) = key_at(launcher, surface_code, revision, index) else {
            return;
        };
        // The revision travels as Java's `int`.
        if launcher
            .view()
            .apps()
            .moved(surface, revision as u32, cell, to)
        {
            launcher.frame();
            place = place_now(launcher, surface, &key);
        }
    });
    place
}

/// `int nativeAppGrouped(int revision, int index)`: a screen reader put the app of grid cell
/// `index` of the cells at `revision` into a folder with the cell after it, in edit mode: "Make
/// folder with…" or "Move into…" (A11Y-05). Returns the place in the grid of the folder it is in
/// now, or -1 if nothing happened.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppGrouped(
    env: RawEnv,
    _grid: JObject,
    revision: i32,
    index: i32,
) -> i32 {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let Ok(cell) = usize::try_from(index) else {
        return -1;
    };
    let mut place = -1;
    with_launcher(env, |launcher| {
        let Some((_, CellKey::App(app))) = key_at(launcher, 0, revision, index) else {
            return;
        };
        // The revision travels as Java's `int`.
        if launcher.view().apps().grouped(revision as u32, cell) {
            launcher.frame();
            let apps = launcher.shell().apps().state();
            place = apps
                .items
                .iter()
                .position(|item| match item {
                    meegone_shell_state::GridItem::Folder { apps, .. } => {
                        apps.iter().any(|shown| shown.id == app)
                    }
                    meegone_shell_state::GridItem::App { .. } => false,
                })
                .and_then(|place| i32::try_from(place).ok())
                .unwrap_or(-1);
        }
    });
    place
}

/// `int nativeAppMovedOut(int revision, int index)`: a screen reader took the app of the open
/// folder's cell `index` of the cells at `revision` out of the folder, in edit mode: "Move out
/// of folder" (A11Y-05). Returns its place in the grid now, or -1 if nothing moved.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppMovedOut(
    env: RawEnv,
    _grid: JObject,
    revision: i32,
    index: i32,
) -> i32 {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let Ok(cell) = usize::try_from(index) else {
        return -1;
    };
    let mut place = -1;
    with_launcher(env, |launcher| {
        let Some((_, key)) = key_at(launcher, 1, revision, index) else {
            return;
        };
        // The revision travels as Java's `int`.
        if launcher.view().apps().moved_out(revision as u32, cell) {
            launcher.frame();
            place = place_now(launcher, Surface::Grid, &key);
        }
    });
    place
}

/// `void nativeEditDone()`: Done was tapped, so edit mode ends (APPS-12).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeEditDone(
    env: RawEnv,
    _grid: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.view().apps().done();
        launcher.frame();
    });
}

/// `void nativeCloseFolder()`: a tap outside the open folder closes it (FOLD-04).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeCloseFolder(
    env: RawEnv,
    _views: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.view().apps().close_folder();
        launcher.frame();
    });
}

/// `void nativeRenameRequested(int folder)`: the user tapped the open folder's name, so the host
/// asks for a new one (FOLD-03).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeRenameRequested(
    env: RawEnv,
    _views: JObject,
    folder: i32,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        // The id travels as Java's `int`.
        launcher
            .view()
            .apps()
            .rename_requested(FolderId(folder as u32));
        launcher.frame();
    });
}

/// `void nativeFolderRenamed(int folder, String name)`: the user named `folder` `name`
/// (FOLD-03).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeFolderRenamed(
    env: RawEnv,
    _views: JObject,
    folder: i32,
    name: JObject,
) {
    // SAFETY: the JVM passes this thread's environment and a live local reference to a
    // `String`, used only during this call.
    let (env, name) = unsafe { (Env::from_raw(env), LocalRef::from_raw(name)) };
    guarded(env, || {
        let name = env
            .string(&name)
            .map_err(|error| format!("Couldn't read the name: {error}"))?;
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            // The id travels as Java's `int`.
            launcher
                .view()
                .apps()
                .folder_renamed(FolderId(folder as u32), name);
            launcher.frame();
            Ok(())
        })
    });
}

/// `void nativeRenameCancelled()`: the user left the folder's name as it was (FOLD-03).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeRenameCancelled(
    env: RawEnv,
    _views: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.view().apps().rename_cancelled();
        launcher.frame();
    });
}

/// `void nativeOpenSearch()`: a screen reader's "Search apps" opens search without a pull
/// (SRCH-01, A11Y-05).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeOpenSearch(
    env: RawEnv,
    _views: JObject,
) {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_launcher(env, |launcher| {
        launcher.view().apps().open_search();
        launcher.frame();
    });
}

/// `void nativeSearchChanged(String query)`: the user typed, and the field now says `query`
/// (SRCH-02).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeSearchChanged(
    env: RawEnv,
    _views: JObject,
    query: JObject,
) {
    // SAFETY: the JVM passes this thread's environment and a live local reference to a
    // `String`, used only during this call.
    let (env, query) = unsafe { (Env::from_raw(env), LocalRef::from_raw(query)) };
    guarded(env, || {
        let query = env
            .string(&query)
            .map_err(|error| format!("Couldn't read the query: {error}"))?;
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            launcher.view().apps().search_changed(query);
            launcher.frame();
            Ok(())
        })
    });
}

/// `String nativeSearchSubmitted(int left, int top, int right, int bottom)`: the keyboard's
/// action key. The first result launches at once, its window growing from its icon at that
/// rect on the screen, or empty for none (SRCH-03, ANIM-08, PERF-06); with no results, the web
/// is searched if that is offered (SRCH-05). Returns `null`, or a short message to show when
/// the system couldn't do it.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeSearchSubmitted(
    env: RawEnv,
    _views: JObject,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let from = Rect {
        left,
        top,
        right,
        bottom,
    };
    with_message(env, |launcher| {
        launcher
            .view()
            .apps()
            .search_submitted(Some(from).filter(|from| !from.is_empty()));
    })
}

/// `String nativeSearchWeb()`: "Search the web for '…'" (SRCH-05). Returns `null`, or a short
/// message to show when nothing could search.
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeSearchWeb(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    with_message(env, |launcher| launcher.view().apps().search_web())
}

/// `String nativeSearchQuery()`: what the search field says while search is open, or `null`.
/// The host asks only when it shows the field again, such as after a theme change recreated
/// it (THEME-06).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_twonet_meegone_HomeViews_nativeSearchQuery(
    env: RawEnv,
    _views: JObject,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let mut query = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow(|launcher| {
            let launcher = launcher.as_ref().ok_or("MeeGone hasn't started")?;
            if let Some(text) = launcher.view().apps().query() {
                query = env
                    .new_string(text)
                    .map(|text| text.into_raw())
                    .map_err(|error| format!("Couldn't pass the query: {error}"))?;
            }
            Ok(())
        })
    });
    query
}

/// Asks something of the views with `step`, applies it, and returns Applications' `problem`
/// as a message for the host to show, or `null`.
fn with_message(env: Env, step: impl FnOnce(&Launcher)) -> JObject {
    let mut message = std::ptr::null_mut();
    guarded(env, || {
        LAUNCHER.with_borrow_mut(|launcher| {
            let launcher = launcher.as_mut().ok_or("MeeGone hasn't started")?;
            step(launcher);
            launcher.frame();
            message = problem(env, launcher)?;
            Ok(())
        })
    });
    message
}

/// What cell `index` of the cells at `revision`, on the surface `surface_code` names, shows, if
/// it is still that cell's.
fn key_at(
    launcher: &Launcher,
    surface_code: i32,
    revision: i32,
    index: i32,
) -> Option<(Surface, CellKey)> {
    let surface = surface(surface_code)?;
    let index = usize::try_from(index).ok()?;
    // The revision travels as Java's `int`.
    let key = launcher
        .view()
        .apps()
        .key_at(surface, revision as u32, index)?;
    Some((surface, key))
}

/// Where `key`'s cell is on `surface` now, or -1.
fn place_now(launcher: &Launcher, surface: Surface, key: &CellKey) -> i32 {
    launcher
        .shell()
        .apps()
        .state()
        .place_of(surface, key)
        .and_then(|place| i32::try_from(place).ok())
        .unwrap_or(-1)
}

/// `String nativeAppTapped(int surface, int revision, int index, int left, int top, int right,
/// int bottom)`: cell `index` of the cells at `revision`, in the grid or the open folder, was
/// tapped, and its icon is at that rect on the screen. An app launches at once (APPS-10,
/// ANIM-08, PERF-06); a folder opens (FOLD-04). Returns `null`, or a short message to show when
/// the system couldn't start the app and gave no explanation of its own (APPS-09).
///
/// # Safety
///
/// Called only by the JVM, with the arguments of that native method.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // The JNI calling convention.
pub unsafe extern "system" fn Java_io_github_twonet_meegone_AppsGrid_nativeAppTapped(
    env: RawEnv,
    _grid: JObject,
    surface_code: i32,
    revision: i32,
    index: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> JObject {
    // SAFETY: the JVM passes this thread's environment.
    let env = unsafe { Env::from_raw(env) };
    let (Some(surface), Ok(index)) = (surface(surface_code), usize::try_from(index)) else {
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
            let apps = launcher.view().apps();
            if !apps.cell_tapped(surface, revision as u32, index, from) {
                return Ok(());
            }
            launcher.frame();
            message = problem(env, launcher)?;
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

/// How many slots the frame has.
const FRAME_SLOTS: usize = 89;

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
/// | 16 | The surface colour of menus, folders and Settings, as opaque ARGB |
/// | 17 | Settings: 0 closed, 1 open, 2 open at its Lock-screen image group |
/// | 18 | Applications' cells revision: when it changes, `AppsGrid` reads its cells again |
/// | 19 | How far Applications is scrolled, in px (HOME-07, REL-05) |
/// | 20 | Applications' columns (APPS-02) |
/// | 21 | Applications' rows, the last one empty (HOME-14) |
/// | 22 | The pressed cell, on slot 55's surface, or -1 (ANIM-10) |
/// | 23 | The pressed cell's icon scale in ten-thousandths, 10 000 at rest (ANIM-10) |
/// | 24–25 | The scale and opacity of cells that just joined, in ten-thousandths (ANIM-21) |
/// | 26–27 | The scale and opacity of cells that left, in ten-thousandths (ANIM-21) |
/// | 28 | How far the other cells have come from where they were drawn, in ten-thousandths |
/// | 29 | The accent colour, as opaque ARGB: the new-app dot (APPS-07) and Done |
/// | 30 | The cell a menu is open at, on slot 71's surface, or -1: centred, or -2 while it is closed (APPS-11) |
/// | 31 | The menu's revision: when it changes, `HomeViews` reads its items again |
/// | 32 | The dragged icon's cell, on slot 52's surface, which is a gap, or -1 |
/// | 33–34 | Where the dragged icon's centre is on screen, in px |
/// | 35 | The dragged icon's scale in ten-thousandths of an icon (ANIM-13, ANIM-14) |
/// | 36 | 1 while edit mode is on (APPS-12); otherwise 0 |
/// | 37 | Every cell's scale in ten-thousandths: 10 000, or 9 200 in edit mode (ANIM-12) |
/// | 38 | How far into edit mode the cells are, 0 to 10 000: Done's opacity |
/// | 39 | The grid's first visible row, which slot 40 is for |
/// | 40–51 | The ✕ scale of each visible row from slot 39's, in ten-thousandths; rows further down take slot 51's (ANIM-12) |
/// | 52 | The dragged icon's surface: 0 the grid, 1 the open folder |
/// | 53 | The dragged icon's opacity, 0 to 255 |
/// | 54 | 1 while an icon is dragged, settling or going into a folder, so the host draws it; otherwise 0 |
/// | 55 | The pressed cell's surface: 0 the grid, 1 the open folder, 2 search's results |
/// | 56 | The grid cell a dragged app is held over, or -1 (ANIM-14) |
/// | 57 | Its icon's scale in ten-thousandths |
/// | 58 | The grid cell of a folder taking in a dropped app, or -1 (ANIM-14) |
/// | 59 | The preview slot the app goes into, which stays empty until it lands, or -1 |
/// | 60 | 1 if the folder was just made, so its first app shrinks into slot 0 too |
/// | 61 | How far the app has gone into the folder, 0 to 10 000 |
/// | 62 | The open or closing folder's id, or -1 (FOLD-04) |
/// | 63 | 1 while it is open, 0 while it closes: only then does its panel take touches |
/// | 64 | Its grid cell, which the panel grows from and shrinks into, or -1 (ANIM-15) |
/// | 65 | How far the panel has grown from it, 0 to 10 000 |
/// | 66 | The open folder's cells revision: when it changes, its grid reads its cells again |
/// | 67 | How far the open folder's grid is scrolled, in px |
/// | 68 | The open folder's rows |
/// | 69 | The folder being renamed, or -1 (FOLD-03) |
/// | 70 | Its grid cell, whose name the dialog starts with, or -1 |
/// | 71 | The menu's surface: 0 the grid, 1 the open folder |
/// | 72 | The opacity of the black scrim behind an open folder, 0 to 255 |
/// | 73 | Search, as bits: 1 open, so the field has the keyboard (SRCH-01); 2 its results show instead of the grid (SRCH-02); 4 nothing matches; 8 a web search is offered (SRCH-05); 16 a finger pulls the field; 32 Applications offers search to screen readers (A11Y-05) |
/// | 74 | How much of the search field's band shows, in px (ANIM-16) |
/// | 75 | Search's results revision: when it changes, the results grid reads its cells again |
/// | 76 | How far the results grid is scrolled, in px |
/// | 77 | The results grid's rows |
/// | 78 | 1 while Settings offers Hidden apps, which Applications must be working for (APPS-19, ARCH-09); otherwise 0 |
/// | 79 | Hidden apps' revision: when it changes, Settings reads the list again with `nativeHiddenApps` |
/// | 80 | 1 while Events works, so its header is drawn (EVT-01, ARCH-09); otherwise 0 |
/// | 81 | The Events header's revision: when it changes, the header is read again with `nativeEventsHeader` |
/// | 82 | What shows below the Events header: 0 nothing, while Events isn't working (ARCH-09); 1 the notification-access card (EVT-03); 2 "No new events" (EVT-11); 3 the app groups (EVT-04) |
/// | 83 | The Events list's revision: when it changes, the groups are read again with `nativeEventsList` |
/// | 84 | How far the Events list has come in showing groups that came and went, 0 to 10 000 (ANIM-20) |
/// | 85 | The divider colour of hairlines, as opaque ARGB |
/// | 86 | 1 while the Events section header offers Clear (EVT-07); otherwise 0 |
/// | 87 | The notification menu's items, as bits 1 << [`notification_menu_bit`]: 1 Dismiss, 2 Notification settings; 0 while it is closed (EVT-08) |
/// | 88 | The Events row it opens at, in the rows last read, or -1 to centre it |
fn frame_slots(frame: &HomeFrame) -> [i32; FRAME_SLOTS] {
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
    let place = |cell: usize| i32::try_from(cell).unwrap_or(-1);
    let maybe = |cell: Option<usize>| cell.map_or(-1, place);
    let code = |surface: Surface| match surface {
        Surface::Grid => 0,
        Surface::Folder => 1,
        Surface::Search => 2,
    };
    // Ids and revisions travel as Java's `int`; the host only compares them.
    let id = |folder: FolderId| folder.0 as i32;
    let (app_menu, app_menu_revision, app_menu_surface) =
        frame.apps.menu.map_or((-2, 0, 0), |menu| {
            (maybe(menu.cell), menu.revision as i32, code(menu.surface))
        });
    let lift = frame.lifted;
    let edit = frame.edit;
    let hover = frame.hover;
    let merge = frame.merge;
    let folder = frame.folder;
    let renaming = frame.renaming;
    let search = frame.search;
    let search_bits = [
        search.open,
        search.results_shown,
        search.nothing_found,
        search.web_offered,
        search.pulling,
        search.offered,
    ]
    .iter()
    .enumerate()
    .fold(0, |bits, (bit, on)| bits | i32::from(*on) << bit);
    let mut slots = [0; FRAME_SLOTS];
    let fixed = [
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
        frame.apps.revision as i32,
        frame.apps.scroll_px,
        i32::from(frame.apps.columns),
        i32::from(frame.apps.rows),
        maybe(frame.pressed),
        i32::from(frame.press_scale),
        i32::from(frame.grid_change.appear_scale),
        i32::from(frame.grid_change.appear_alpha),
        i32::from(frame.grid_change.leave_scale),
        i32::from(frame.grid_change.leave_alpha),
        i32::from(frame.grid_change.shift),
        argb(frame.accent),
        app_menu,
        app_menu_revision,
        lift.map_or(-1, |lift| maybe(lift.cell)),
        lift.map_or(0, |lift| lift.x),
        lift.map_or(0, |lift| lift.y),
        lift.map_or(10_000, |lift| i32::from(lift.scale)),
        i32::from(edit.on),
        i32::from(edit.scale),
        i32::from(edit.progress),
        i32::from(edit.first_row),
    ];
    slots[..40].copy_from_slice(&fixed);
    for (slot, badge) in slots[40..40 + BADGE_ROWS].iter_mut().zip(edit.badges) {
        *slot = i32::from(badge);
    }
    let rest = [
        lift.map_or(0, |lift| code(lift.surface)),
        lift.map_or(255, |lift| i32::from(lift.alpha)),
        i32::from(lift.is_some()),
        code(frame.pressed_surface),
        hover.map_or(-1, |hover| place(hover.cell)),
        hover.map_or(10_000, |hover| i32::from(hover.scale)),
        merge.map_or(-1, |merge| place(merge.cell)),
        merge.map_or(-1, |merge| maybe(merge.slot)),
        merge.map_or(0, |merge| i32::from(merge.formed)),
        merge.map_or(0, |merge| i32::from(merge.progress)),
        folder.map_or(-1, |folder| id(folder.folder)),
        folder.map_or(0, |folder| i32::from(folder.open)),
        folder.map_or(-1, |folder| maybe(folder.cell)),
        folder.map_or(0, |folder| i32::from(folder.progress)),
        frame.apps.folder.revision as i32,
        frame.apps.folder.scroll_px,
        i32::from(frame.apps.folder.rows),
        renaming.map_or(-1, |renaming| id(renaming.folder)),
        renaming.map_or(-1, |renaming| maybe(renaming.cell)),
        app_menu_surface,
        i32::from(frame.scrim),
        search_bits,
        search.reveal_px,
        frame.apps.results.revision as i32,
        frame.apps.results.scroll_px,
        i32::from(frame.apps.results.rows),
        i32::from(frame.hidden_apps.is_some()),
        // The revision travels as Java's `int`.
        frame.hidden_apps.map_or(0, |revision| revision as i32),
        i32::from(frame.events_header.is_some()),
        frame.events_header.map_or(0, |revision| revision as i32),
        frame.events_list.map_or(0, |list| match list.mode {
            EventsListMode::Explainer => 1,
            EventsListMode::Empty => 2,
            EventsListMode::List => 3,
        }),
        frame.events_list.map_or(0, |list| list.revision as i32),
        i32::from(frame.events_change),
        argb(frame.divider),
        i32::from(frame.events_list.is_some_and(|list| list.clearable)),
        frame.events_menu.map_or(0, |menu| {
            let mut items = 1 << notification_menu_bit(NotificationMenuItem::NotificationSettings);
            if menu.dismiss {
                items |= 1 << notification_menu_bit(NotificationMenuItem::Dismiss);
            }
            items
        }),
        frame
            .events_menu
            .and_then(|menu| menu.row)
            .and_then(|row| i32::try_from(row).ok())
            .unwrap_or(-1),
    ];
    slots[40 + BADGE_ROWS..].copy_from_slice(&rest);
    slots
}

/// A notification menu item's code, mirrored in `HomeViews.java`: its bit in the frame's slot
/// 87, and what `nativeEventsMenuChosen` takes.
fn notification_menu_bit(item: NotificationMenuItem) -> u32 {
    match item {
        NotificationMenuItem::Dismiss => 0,
        NotificationMenuItem::NotificationSettings => 1,
    }
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
