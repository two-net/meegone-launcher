//! The acceptance scenarios of SRS section 9, run on the whole app with fake platform adapters
//! (MAINT-03). Each test carries its scenario's id; each feature adds its own here.
//!
//! Not automated yet, because the behaviour isn't built:
//! - AC-01: first-run setup (#35)
//! - AC-12: Clear in Events (#20)
//! - AC-13, AC-14: Recent density and close mode (#23, #24)
//! - AC-15 to AC-17: the daily image (#27 to #30)
//!
//! AC-18 to AC-20 (animation scale, screen reader, timing) need a device; they belong to the
//! device matrix (#43). Their core logic is tested without one: the carousel with animations
//! removed and its screen-reader actions in `meegone-shell-state`, and the folders'
//! screen-reader actions here (A11Y-05).

use std::time::Duration;

use meegone_core_data::fakes::{Fakes, WORK_PROFILE, app, app_id, work_app};
use meegone_core_data::storage::format::seal;
use meegone_core_data::{
    App, DataError, HomeTarget, Notification, Profile, ProfileId, RecentUse, Rect, Rgb, Shortcut,
    StoredFile, ThemeChoice, accent,
};
use meegone_core_state::{Access, Health, Theme};
use meegone_core_ui::tokens;
use meegone_launcher::{
    AppMenuItem, BADGE_ROWS, Bounds, CellKey, Direction, FolderId, GridLayout, HomeView, Launcher,
    Layout, LiftFrame, MenuItem, PointerEvent, PointerKind, SettingsGroup, ShellIntent, Surface,
};

/// A phone 1080 × 2400 px at 2.75 px per dp.
const WIDTH: f64 = 1080.0;
const HEIGHT: f64 = 2400.0;

fn notification(key: &str, package: &str) -> Notification {
    Notification {
        key: key.into(),
        package: package.into(),
        title: "Title".into(),
        text: "Text".into(),
        posted_at_ms: 1,
        clearable: true,
        ongoing: false,
    }
}

/// The home views laid out on that phone.
fn phone() -> Layout {
    Layout {
        width_px: WIDTH,
        density: 2.75,
        touch_slop_px: 22.0,
        rtl: false,
    }
}

/// A launcher laid out on a phone, with the time at 0 ms.
fn on_a_phone(fakes: &Fakes) -> (Launcher, u64) {
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Layout(phone()));
    (launcher, 0)
}

/// The display draws frames until nothing moves. `clock` is the time in ms.
fn come_to_rest(launcher: &mut Launcher, clock: &mut u64) {
    while launcher.frame().animating {
        *clock += 8;
        launcher.dispatch(ShellIntent::Frame(Duration::from_millis(*clock)));
    }
}

/// A finger drags from `from` to `to` over `length` ms, rests for `pause` ms and lifts; then the
/// display draws frames until nothing moves. `clock` is the time in ms.
fn swipe(launcher: &mut Launcher, clock: &mut u64, from: f64, to: f64, length: u64, pause: u64) {
    let mut touch = |kind, x, at| {
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x,
            y: 1200.0,
            at: Duration::from_millis(at),
        }));
        launcher.frame();
    };
    let steps = length / 8;
    touch(PointerKind::Down, from, *clock);
    for step in 1..=steps {
        let x = from + (to - from) * step as f64 / steps as f64;
        touch(PointerKind::Move, x, *clock + step * 8);
    }
    *clock += steps * 8 + pause;
    touch(PointerKind::Up, to, *clock);
    for _ in 0..500 {
        *clock += 8;
        launcher.dispatch(ShellIntent::Frame(Duration::from_millis(*clock)));
        if !launcher.frame().animating {
            return;
        }
    }
    panic!("still moving");
}

/// A finger held still on empty space at `(x, y)` until the host's long-press time passes,
/// then lifted (HOME-10).
fn long_press(launcher: &mut Launcher, clock: &mut u64, x: f64, y: f64) {
    let touch = |launcher: &mut Launcher, kind, at| {
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x,
            y,
            at: Duration::from_millis(at),
        }));
        launcher.frame();
    };
    touch(launcher, PointerKind::Down, *clock);
    *clock += 400;
    launcher.dispatch(ShellIntent::LongPress);
    launcher.frame();
    *clock += 300;
    touch(launcher, PointerKind::Up, *clock);
}

fn grid(launcher: &Launcher) -> Vec<String> {
    let state = launcher.shell().apps().state();
    state
        .items
        .iter()
        .map(|item| item.label().to_owned())
        .collect()
}

#[test]
fn ac02_swiping_right_to_left_three_times_goes_recent_events_applications() {
    let fakes = Fakes::default();
    let mut launcher = Launcher::start(fakes.platform());
    assert_eq!(launcher.shell().state().current, HomeView::Applications);

    let mut seen = Vec::new();
    for _ in 0..3 {
        launcher.dispatch(ShellIntent::Swipe(Direction::Next));
        launcher.frame();
        seen.push(launcher.shell().state().current);
    }

    assert_eq!(
        seen,
        [HomeView::Recent, HomeView::Events, HomeView::Applications]
    );
}

#[test]
fn ac02_dragging_right_to_left_three_times_goes_recent_events_applications() {
    // HOME-03: the same scenario with the finger, each view settling in full view.
    let (mut launcher, mut clock) = on_a_phone(&Fakes::default());

    let mut seen = Vec::new();
    for _ in 0..3 {
        swipe(&mut launcher, &mut clock, 900.0, 300.0, 400, 100);
        let frame = launcher.frame();
        assert_eq!(frame.pages[frame.current.index()], 0);
        seen.push(frame.current);
    }

    assert_eq!(
        seen,
        [HomeView::Recent, HomeView::Events, HomeView::Applications]
    );
}

#[test]
fn anim07_a_drag_past_35_percent_or_a_fling_settles_and_a_short_slow_drag_returns() {
    // Issue #5: 35 % of 1080 px is 378 px, after the 22 px touch slop.
    let (mut launcher, mut clock) = on_a_phone(&Fakes::default());
    let current = |launcher: &Launcher| launcher.shell().state().current;

    swipe(&mut launcher, &mut clock, 900.0, 900.0 - 410.0, 400, 100);
    assert_eq!(current(&launcher), HomeView::Recent);

    swipe(&mut launcher, &mut clock, 500.0, 650.0, 64, 0);
    assert_eq!(current(&launcher), HomeView::Applications);

    swipe(&mut launcher, &mut clock, 900.0, 900.0 - 380.0, 400, 100);
    assert_eq!(current(&launcher), HomeView::Applications);
    assert_eq!(launcher.frame().pages, [-1080, 0, 1080]);
}

#[test]
fn ac03_home_from_an_app_opened_from_recent_shows_recent_with_it_at_the_top_left() {
    // Opening an app from its card arrives with #22; here the user leaves Recent and uses Mail.
    let fakes = Fakes::default();
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("maps", "Maps")];
    *fakes.system.now_ms.lock().unwrap() = 10_000_000;
    let used = |package: &str, at: u64| RecentUse {
        app: app_id(package),
        last_used_ms: at,
    };
    *fakes.usage.uses.lock().unwrap() = vec![used("maps", 9_000_000), used("mail", 8_000_000)];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    assert_eq!(launcher.shell().recent().state().cards[0].app.label, "Maps");

    *fakes.system.now_ms.lock().unwrap() = 10_060_000;
    fakes.usage.uses.lock().unwrap()[1].last_used_ms = 10_060_000;
    launcher.dispatch(ShellIntent::Resumed);
    launcher.dispatch(ShellIntent::Returned { home: true });
    launcher.frame();

    assert_eq!(launcher.shell().state().current, HomeView::Recent);
    assert_eq!(launcher.shell().recent().state().cards[0].app.label, "Mail");
}

#[test]
fn ac04_home_on_events_shows_applications_and_home_again_scrolls_it_to_the_top() {
    let (mut launcher, mut clock) = on_a_phone(&Fakes::default());
    launcher.view().apps().scrolled(Surface::Grid, 600.0);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    launcher.frame();
    assert_eq!(launcher.shell().state().current, HomeView::Events);

    launcher.dispatch(ShellIntent::Home);
    launcher.frame();
    assert_eq!(launcher.shell().state().current, HomeView::Applications);
    assert_eq!(launcher.shell().apps().state().scroll_px, 600.0);
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(launcher.frame().pages, [-1080, 0, 1080]);

    launcher.dispatch(ShellIntent::Home);

    assert_eq!(launcher.shell().state().current, HomeView::Applications);
    assert_eq!(launcher.shell().apps().state().scroll_px, 0.0);
}

#[test]
fn home06_with_always_applications_home_from_an_app_shows_applications() {
    // Chosen in Settings as "Home button opens: Always Applications".
    let (mut launcher, _) = on_a_phone(&Fakes::default());
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher
        .view()
        .settings()
        .home_target_chosen(HomeTarget::Applications);
    launcher.frame();
    launcher.dispatch(ShellIntent::Back);
    // The user left while Recent was still sliding in.
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    launcher.frame();

    launcher.dispatch(ShellIntent::Resumed);
    launcher.dispatch(ShellIntent::Returned { home: true });
    let frame = launcher.frame();

    // Shown at once, with nothing moving.
    assert_eq!(frame.current, HomeView::Applications);
    assert_eq!(frame.pages, [-1080, 0, 1080]);
    // The content comes back scaled down and faded out (ANIM-09).
    assert_eq!((frame.content_scale, frame.content_alpha), (9_400, 0));
    assert!(frame.animating);
}

#[test]
fn home10_long_pressing_the_empty_row_below_a_long_grid_opens_the_home_menu() {
    // Issue #7, HOME-14. The grid's views (#9) find what is under the finger; here the host
    // reports a finger held on the empty row. Rows are 96 dp high, four to a row.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = (0..40)
        .map(|i| app(&format!("app{i:02}"), &format!("App {i:02}")))
        .collect();
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    let row_px = 96.0 * 2.75;

    // Given a grid longer than the screen, which ends with an empty row,
    let rows = launcher.shell().apps().state().rows();
    assert_eq!(rows, 11);
    assert!(rows as f64 * row_px > HEIGHT);
    // when the user scrolls to the bottom and long-presses the empty row,
    let bottom = rows as f64 * row_px - HEIGHT;
    launcher.view().apps().scrolled(Surface::Grid, bottom);
    launcher.frame();
    let empty_row = (rows - 1) as f64 * row_px - bottom + row_px / 2.0;
    long_press(&mut launcher, &mut clock, WIDTH / 2.0, empty_row);

    // then the home menu opens there.
    let menu = launcher.frame().menu.expect("the home menu is open");
    assert_eq!(menu.items, MenuItem::ALL);
    assert_eq!(menu.anchor, Some((540, empty_row.round() as i32)));
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 1);

    // Edit applications enters edit mode where the grid was.
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
    let frame = launcher.frame();
    assert_eq!(frame.menu, None);
    assert!(launcher.shell().apps().state().edit_mode);
    assert_eq!(launcher.shell().apps().state().scroll_px, bottom);
}

#[test]
fn home10_in_events_the_home_menu_has_no_edit_applications() {
    // Issue #7
    let (mut launcher, mut clock) = on_a_phone(&Fakes::default());
    swipe(&mut launcher, &mut clock, 200.0, 800.0, 400, 100);
    assert_eq!(launcher.shell().state().current, HomeView::Events);

    long_press(&mut launcher, &mut clock, 400.0, 1800.0);

    let menu = launcher.frame().menu.expect("the home menu is open");
    assert_eq!(
        menu.items,
        [MenuItem::LauncherSettings, MenuItem::LockScreenImage]
    );

    // Lock-screen image opens Settings at its group; Back closes it (SET-02, HOME-08).
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::LockScreenImage));
    let frame = launcher.frame();
    assert!(frame.settings_open);
    assert_eq!(frame.settings_group, Some(SettingsGroup::LockScreenImage));
    launcher.dispatch(ShellIntent::Back);
    let frame = launcher.frame();
    assert!(!frame.settings_open);
    assert_eq!(frame.current, HomeView::Events);
}

#[test]
fn home10_in_recent_a_long_press_enters_close_mode_instead() {
    // Issue #7, REC-07
    let fakes = Fakes::default();
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    swipe(&mut launcher, &mut clock, 900.0, 300.0, 400, 100);
    assert_eq!(launcher.shell().state().current, HomeView::Recent);

    long_press(&mut launcher, &mut clock, 540.0, 1200.0);

    assert_eq!(launcher.frame().menu, None);
    assert!(launcher.shell().recent().state().close_mode);
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 1);
}

#[test]
fn comp03_folding_or_unfolding_keeps_the_view_scroll_position_and_edit_mode() {
    // Issue #8. Folded, the cover screen is the phone; unfolded, the inner screen is wider, at
    // another density. Folders arrive with #14; `meegone-shell-state` checks that an open one
    // is kept too.
    let (mut launcher, mut clock) = on_a_phone(&Fakes::default());
    launcher.view().apps().scrolled(Surface::Grid, 600.0);
    long_press(&mut launcher, &mut clock, 540.0, 1800.0);
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
    launcher.frame();
    let unfolded = Layout {
        width_px: 2208.0,
        density: 2.375,
        touch_slop_px: 19.0,
        rtl: false,
    };

    for (layout, width) in [(unfolded, 2208), (phone(), 1080)] {
        launcher.dispatch(ShellIntent::Layout(layout));
        let frame = launcher.frame();

        assert_eq!(frame.current, HomeView::Applications);
        assert_eq!(frame.pages, [-width, 0, width]);
        let apps = launcher.shell().apps().state();
        assert!(apps.edit_mode);
        assert_eq!(apps.scroll_px, 600.0);
    }
}

#[test]
fn home12_rotating_a_tablet_adapts_the_layout_and_keeps_state() {
    // Issue #8. A tablet 1600 × 2560 px at 2 px per dp, turned from portrait to landscape.
    let mut launcher = Launcher::start(Fakes::default().platform());
    let mut clock = 0;
    let portrait = Layout {
        width_px: 1600.0,
        density: 2.0,
        touch_slop_px: 16.0,
        rtl: false,
    };
    launcher.dispatch(ShellIntent::Layout(portrait));
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    come_to_rest(&mut launcher, &mut clock);
    long_press(&mut launcher, &mut clock, 400.0, 2000.0);
    assert_eq!(launcher.frame().menu.unwrap().anchor, Some((400, 2000)));

    launcher.dispatch(ShellIntent::Layout(Layout {
        width_px: 2560.0,
        ..portrait
    }));
    let frame = launcher.frame();

    // The views fill the new width, and Events still shows its menu, now centred.
    assert_eq!(frame.current, HomeView::Events);
    assert_eq!(frame.pages, [0, 2560, -2560]);
    let menu = frame.menu.expect("the home menu is still open");
    assert_eq!(
        menu.items,
        [MenuItem::LauncherSettings, MenuItem::LockScreenImage]
    );
    assert_eq!(menu.anchor, None);
}

#[test]
fn comp02_home_and_back_behave_the_same_in_every_navigation_mode() {
    // Issue #8. Only whether the system animates the return home depends on the navigation
    // mode: it does with gestures, and doesn't with two or three buttons (ADR 0007).
    let mut outcomes = Vec::new();
    for gestures in [true, false] {
        let fakes = Fakes::default();
        *fakes.system.animates_return_home.lock().unwrap() = gestures;
        let (mut launcher, mut clock) = on_a_phone(&fakes);
        launcher.view().apps().scrolled(Surface::Grid, 600.0);
        let mut steps = Vec::new();
        let mut step = |launcher: &mut Launcher, intent| {
            launcher.dispatch(intent);
            let frame = launcher.frame();
            let scroll_px = launcher.shell().apps().state().scroll_px;
            steps.push((frame.current, frame.menu.is_some(), scroll_px));
        };

        // Back closes the home menu, then does nothing.
        long_press(&mut launcher, &mut clock, 540.0, 1800.0);
        step(&mut launcher, ShellIntent::Back);
        step(&mut launcher, ShellIntent::Back);
        // Home from Events shows Applications; again, it scrolls it to the top.
        step(&mut launcher, ShellIntent::Swipe(Direction::Previous));
        step(&mut launcher, ShellIntent::Home);
        step(&mut launcher, ShellIntent::Home);
        // Home from an app shows the view the user left.
        step(&mut launcher, ShellIntent::Swipe(Direction::Next));
        come_to_rest(&mut launcher, &mut clock);
        launcher.dispatch(ShellIntent::Resumed);
        step(&mut launcher, ShellIntent::Returned { home: true });

        // The content comes back with MeeGone's own motion only with buttons (ANIM-09).
        let frame = launcher.frame();
        assert_eq!(frame.content_alpha == 255, gestures);
        outcomes.push(steps);
    }

    assert_eq!(outcomes[0], outcomes[1]);
    assert_eq!(
        outcomes[0],
        [
            (HomeView::Applications, false, 600.0),
            (HomeView::Applications, false, 600.0),
            (HomeView::Events, false, 600.0),
            (HomeView::Applications, false, 600.0),
            (HomeView::Applications, false, 0.0),
            (HomeView::Recent, false, 0.0),
            (HomeView::Recent, false, 0.0),
        ]
    );
}

#[test]
fn ac05_with_theme_system_the_app_turns_dark_with_the_device() {
    let fakes = Fakes::default();
    let mut launcher = Launcher::start(fakes.platform());
    assert_eq!(launcher.shell().theme().state().theme, Theme::Light);

    *fakes.system.dark.lock().unwrap() = true;
    launcher.dispatch(ShellIntent::SystemThemeChanged);

    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);
}

#[test]
fn ac06_with_theme_light_the_app_stays_light_when_the_device_turns_dark() {
    let fakes = Fakes::default();
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().settings().theme_chosen(ThemeChoice::Light);
    launcher.frame();

    *fakes.system.dark.lock().unwrap() = true;
    launcher.dispatch(ShellIntent::SystemThemeChanged);

    assert_eq!(launcher.shell().theme().state().theme, Theme::Light);
}

#[test]
fn ac10_a_newly_installed_app_appears_at_the_end_of_the_grid() {
    // Issue #11: APPS-06, APPS-07, ANIM-21. It scales and fades in, with a dot.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("phone", "Phone")];
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    assert_eq!(grid(&launcher), ["Mail", "Phone"]);
    assert_eq!(dots(&mut launcher), [false, false]);

    fakes.apps.apps.lock().unwrap().push(app("atlas", "Atlas"));
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_eq!(grid(&launcher), ["Mail", "Phone", "Atlas"]);
    let change = launcher.frame().grid_change;
    assert_eq!((change.appear_scale, change.appear_alpha), (5_000, 0));
    assert_eq!(dots(&mut launcher), [false, false, true]);
    // The host draws Atlas as the cell that joined; Mail and Phone keep their places.
    assert_eq!(froms(&mut launcher), [Some(0), Some(1), None]);
    come_to_rest(&mut launcher, &mut clock);
    let change = launcher.frame().grid_change;
    assert_eq!((change.appear_scale, change.appear_alpha), (10_000, 10_000));
}

/// Whether each cell the host draws carries the new-app dot (APPS-07).
fn dots(launcher: &mut Launcher) -> Vec<bool> {
    launcher.frame();
    launcher
        .view()
        .apps()
        .cells(Surface::Grid)
        .map(|cell| cell.new)
        .collect()
}

/// Where each cell the host draws was in the cells before (ANIM-21).
fn froms(launcher: &mut Launcher) -> Vec<Option<usize>> {
    launcher.frame();
    launcher
        .view()
        .apps()
        .cells(Surface::Grid)
        .map(|cell| cell.from)
        .collect()
}

/// A layout saved in today's format, as the user arranged it.
fn saved_layout(fakes: &Fakes, records: &str) {
    fakes
        .storage
        .files
        .lock()
        .unwrap()
        .insert(StoredFile::Layout, seal("layout", 2, records));
}

#[test]
fn ac07_given_maps_was_moved_to_the_first_cell_after_a_restart_it_is_still_first() {
    // Issue #11: APPS-18. Moving it takes edit mode (#13); here the user's arrangement is
    // saved as edit mode will save it.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![
        app("camera", "Camera"),
        app("mail", "Mail"),
        app("maps", "Maps"),
    ];
    let launcher = Launcher::start(fakes.platform());
    assert_eq!(grid(&launcher), ["Camera", "Mail", "Maps"]);
    drop(launcher);
    saved_layout(
        &fakes,
        "app 0 maps maps.Main\napp 0 camera camera.Main\napp 0 mail mail.Main\n",
    );

    let restarted = Launcher::start(fakes.platform());

    assert_eq!(grid(&restarted), ["Maps", "Camera", "Mail"]);
}

#[test]
fn apps06_given_an_app_installed_while_meegone_wasnt_running_it_is_at_the_end() {
    // Issue #11: APPS-06, APPS-07, APPS-18. It keeps its dot until it is opened, and that
    // too survives a restart.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![app("phone", "Phone"), app("mail", "Mail")];
    drop(Launcher::start(fakes.platform()));

    fakes.apps.apps.lock().unwrap().push(app("atlas", "Atlas"));
    let mut launcher = Launcher::start(fakes.platform());

    assert_eq!(grid(&launcher), ["Mail", "Phone", "Atlas"]);
    assert_eq!(dots(&mut launcher), [false, false, true]);
    // Starting shows the grid as it is: nothing moves (ANIM-21).
    assert!(!launcher.frame().animating);

    let revision = launcher.frame().apps.revision;
    assert!(
        launcher
            .view()
            .apps()
            .cell_tapped(Surface::Grid, revision, 2, Rect::default())
    );
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("atlas")]);
    assert_eq!(dots(&mut launcher), [false, false, false]);
    drop(launcher);

    let mut restarted = Launcher::start(fakes.platform());
    assert_eq!(dots(&mut restarted), [false, false, false]);
}

#[test]
fn rel02_given_an_unreadable_layout_meegone_shows_an_alphabetical_grid_and_doesnt_crash() {
    // Issue #11: REL-02, MAINT-04. The rebuilt layout replaces the file at once.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![
        app("zoom", "Zoom"),
        app("mail", "Mail"),
        app("atlas", "Atlas"),
    ];
    fakes
        .storage
        .files
        .lock()
        .unwrap()
        .insert(StoredFile::Layout, b"\xff\x00 not a layout".to_vec());

    let mut launcher = Launcher::start(fakes.platform());

    assert_eq!(*launcher.shell().apps().health(), Health::Ready);
    assert_eq!(grid(&launcher), ["Atlas", "Mail", "Zoom"]);
    assert_eq!(dots(&mut launcher), [false, false, false]);
    let stored = fakes.storage.stored_text();
    assert!(stored.starts_with("meegone-layout 2\n"), "{stored}");
    drop(launcher);
    assert_eq!(
        grid(&Launcher::start(fakes.platform())),
        ["Atlas", "Mail", "Zoom"]
    );
}

#[test]
fn rel02_a_partly_damaged_layout_keeps_what_can_be_recovered() {
    // Issue #11: REL-02. The record after Zoom's was damaged; Zoom stays first and the rest
    // is rebuilt alphabetically.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![
        app("zoom", "Zoom"),
        app("mail", "Mail"),
        app("atlas", "Atlas"),
    ];
    let mut damaged = seal(
        "layout",
        2,
        "app 0 zoom zoom.Main\napp 0 mail mail.Main\napp 0 atlas atlas.Main\n",
    );
    let mail = String::from_utf8_lossy(&damaged)
        .find("app 0 mail")
        .unwrap();
    damaged[mail] = b'?';
    fakes
        .storage
        .files
        .lock()
        .unwrap()
        .insert(StoredFile::Layout, damaged);

    let launcher = Launcher::start(fakes.platform());

    assert_eq!(grid(&launcher), ["Zoom", "Atlas", "Mail"]);
}

#[test]
fn apps08_an_uninstalled_app_leaves_its_folder_and_the_grid_closes_up() {
    // Issue #11: APPS-08, FOLD-06, ANIM-21. Go leaves the games folder, which turns back into
    // Chess; Maps leaves the grid, and Phone slides into its place.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![
        app("chess", "Chess"),
        app("go", "Go"),
        app("maps", "Maps"),
        app("phone", "Phone"),
    ];
    saved_layout(
        &fakes,
        "folder 1 Games 2 0 chess chess.Main 0 go go.Main\n\
         app 0 maps maps.Main\napp 0 phone phone.Main\n",
    );
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    assert_eq!(grid(&launcher), ["Games", "Maps", "Phone"]);
    // The host draws them.
    assert_eq!(froms(&mut launcher), [None, None, None]);

    fakes
        .apps
        .apps
        .lock()
        .unwrap()
        .retain(|app| !["go", "maps"].contains(&app.id.package.as_str()));
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_eq!(grid(&launcher), ["Chess", "Phone"]);
    // Games turns back into Chess in its own cell (ADR 0015); Maps leaves, shrinking; Phone
    // moves up from the third cell.
    assert_eq!(froms(&mut launcher), [Some(0), Some(2)]);
    let change = launcher.frame().grid_change;
    assert_eq!(
        (change.leave_scale, change.leave_alpha, change.shift),
        (10_000, 10_000, 0)
    );
    come_to_rest(&mut launcher, &mut clock);
    let change = launcher.frame().grid_change;
    assert_eq!(
        (change.leave_scale, change.leave_alpha, change.shift),
        (5_000, 0, 10_000)
    );
    assert!(clock <= 208, "{clock} ms");
}

#[test]
fn apps08_an_updated_app_gets_its_new_label_without_moving() {
    // Issue #11: APPS-08. Also when the update renames its launcher activity.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![
        app("mail", "Mail"),
        app("maps", "Maps"),
        app("phone", "Phone"),
    ];
    let mut launcher = Launcher::start(fakes.platform());

    {
        let mut apps = fakes.apps.apps.lock().unwrap();
        apps[0].label = "Email".into();
        apps[1].id.activity = "maps.Start".into();
        apps[1].label = "Maps Go".into();
    }
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_eq!(grid(&launcher), ["Email", "Maps Go", "Phone"]);
    assert_eq!(dots(&mut launcher), [false, false, false]);
    drop(launcher);
    assert_eq!(
        grid(&Launcher::start(fakes.platform())),
        ["Email", "Maps Go", "Phone"]
    );
}

#[test]
fn apps05_the_first_start_puts_the_default_phone_messaging_browser_and_camera_first() {
    // Issue #11: APPS-04, APPS-05. A tablet without a phone app leaves it out.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![
        app("atlas", "Atlas"),
        app("browser", "Browser"),
        app("camera", "Camera"),
        app("messages", "Messages"),
        app("zoom", "Zoom"),
    ];
    *fakes.apps.default_apps.lock().unwrap() =
        vec!["messages".into(), "browser".into(), "camera".into()];

    let launcher = Launcher::start(fakes.platform());

    assert_eq!(
        grid(&launcher),
        ["Messages", "Browser", "Camera", "Atlas", "Zoom"]
    );
}

#[test]
fn l10n04_apps_are_sorted_in_the_locales_alphabetical_order() {
    // Issue #11: L10N-04. The platform's collation keys decide: here, a locale that sorts
    // "Ö" after "Z", as Swedish does, and ignores the accent of "Él".
    let fakes = Fakes::default();
    let with_key = |package: &str, label: &str, key: &[u8]| App {
        sort_key: key.to_vec(),
        ..app(package, label)
    };
    *fakes.apps.apps.lock().unwrap() = vec![
        with_key("orter", "Örter", b"z\x01"),
        with_key("zoom", "Zoom", b"z"),
        with_key("el", "Él", b"el"),
        with_key("atlas", "Atlas", b"atlas"),
    ];

    let launcher = Launcher::start(fakes.platform());

    assert_eq!(grid(&launcher), ["Atlas", "Él", "Zoom", "Örter"]);
}

#[test]
fn apps18_a_locked_work_profile_keeps_its_apps_places_until_it_unlocks() {
    // Issue #11: APPS-18. After a restart, a work profile with its own lock lists no apps.
    let fakes = Fakes::default();
    let work_profile = |locked| {
        *fakes.apps.profiles.lock().unwrap() = vec![
            Profile {
                id: ProfileId(0),
                work: false,
                paused: false,
                locked: false,
            },
            Profile {
                id: WORK_PROFILE,
                work: true,
                paused: false,
                locked,
            },
        ];
    };
    work_profile(false);
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), work_app("docs", "Docs")];
    saved_layout(&fakes, "app 10 docs docs.Main\napp 0 mail mail.Main\n");
    drop(Launcher::start(fakes.platform()));

    work_profile(true);
    fakes.apps.apps.lock().unwrap().retain(|app| !app.work);
    let mut launcher = Launcher::start(fakes.platform());
    assert_eq!(grid(&launcher), ["Mail"]);

    work_profile(false);
    fakes
        .apps
        .apps
        .lock()
        .unwrap()
        .push(work_app("docs", "Docs"));
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_eq!(grid(&launcher), ["Docs", "Mail"]);
    assert_eq!(dots(&mut launcher), [false, false]);
}

#[test]
fn ac11_after_access_is_granted_notifications_replace_the_explainer_card() {
    let fakes = Fakes::default();
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert_eq!(launcher.shell().events().state().access, Access::Missing);

    launcher.view().events().allow_access_tapped();
    launcher.frame();
    assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![notification("m1", "mail")];
    launcher.dispatch(ShellIntent::NotificationsChanged);

    let events = launcher.shell().events().state();
    assert_eq!(events.access, Access::Granted);
    assert_eq!(events.groups.len(), 1);
}

#[test]
fn ac21_revoking_notification_access_shows_the_explainer_and_applications_still_works() {
    let fakes = Fakes::default();
    fakes.apps.apps.lock().unwrap().push(app("mail", "Mail"));
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![notification("m1", "mail")];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert_eq!(launcher.shell().events().state().groups.len(), 1);

    *fakes.notifications.access.lock().unwrap() = Access::Missing;
    launcher.dispatch(ShellIntent::NotificationsChanged);

    let events = launcher.shell().events().state();
    assert_eq!(events.access, Access::Missing);
    assert!(events.groups.is_empty());
    assert_eq!(*launcher.shell().events().health(), Health::Ready);

    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    launcher.view().apps().icon_tapped(app_id("mail"), None);
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
}

/// The cells the host draws, after a frame: label, work profile, available.
fn cells(launcher: &mut Launcher) -> Vec<(String, bool, bool)> {
    launcher.frame();
    launcher
        .view()
        .apps()
        .cells(Surface::Grid)
        .map(|cell| (cell.label.to_owned(), cell.work, cell.available))
        .collect()
}

/// A personal profile, and a work profile that is paused or not.
fn with_a_work_profile(fakes: &Fakes, paused: bool) {
    *fakes.apps.profiles.lock().unwrap() = vec![
        Profile {
            id: ProfileId(0),
            work: false,
            paused: false,
            locked: false,
        },
        Profile {
            id: WORK_PROFILE,
            work: true,
            paused,
            locked: false,
        },
    ];
}

#[test]
fn apps01_given_personal_and_work_profiles_every_launchable_app_from_both_appears() {
    // Issue #9: APPS-01, APPS-20. The system badges the work icons; the cell says "work" so
    // screen readers can (A11Y-01).
    let fakes = Fakes::default();
    with_a_work_profile(&fakes, false);
    *fakes.apps.apps.lock().unwrap() = vec![
        app("mail", "Mail"),
        app("maps", "Maps"),
        work_app("mail", "Mail"),
        work_app("docs", "Docs"),
    ];

    let mut launcher = Launcher::start(fakes.platform());

    assert_eq!(
        cells(&mut launcher),
        [
            ("Docs".to_owned(), true, true),
            ("Mail".to_owned(), false, true),
            ("Mail".to_owned(), true, true),
            ("Maps".to_owned(), false, true),
        ]
    );
}

#[test]
fn apps02_given_a_screen_600_dp_wide_or_more_every_cell_is_96_to_128_dp_wide() {
    // Issue #9. The grid sits inside the safe area and 16 dp margins at each side. Phones keep
    // four columns.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = (0..30)
        .map(|i| app(&format!("app{i:02}"), &format!("App {i:02}")))
        .collect();
    let (mut launcher, _) = on_a_phone(&fakes);
    let columns = |launcher: &mut Launcher, screen_dp: f64, insets_dp: f64, density: f64| {
        let grid_dp = screen_dp - insets_dp - 2.0 * 16.0;
        launcher.view().apps().grid_laid_out(GridLayout {
            width_px: grid_dp * density,
            row_height_px: 112.0 * density,
            density,
            ..PHONE_GRID
        });
        let columns = f64::from(launcher.frame().apps.columns);
        (grid_dp, columns)
    };

    for (screen_dp, insets_dp, density) in [
        (600.0, 0.0, 2.0),    // the smallest large screen
        (600.0, 48.0, 2.0),   // in landscape beside a three-button bar
        (673.0, 0.0, 2.625),  // an unfolded foldable
        (800.0, 0.0, 2.0),    // a tablet in portrait
        (1_280.0, 48.0, 2.0), // the same tablet in landscape
        (2_560.0, 0.0, 1.0),  // a large external display
    ] {
        let (grid_dp, columns) = columns(&mut launcher, screen_dp, insets_dp, density);
        let cell_dp = grid_dp / columns;
        assert!(
            (96.0..=128.0).contains(&cell_dp),
            "{screen_dp} dp: {columns} columns of {cell_dp} dp"
        );
    }
    for phone_dp in [320.0, 360.0, 393.0, 412.0, 480.0] {
        assert_eq!(
            columns(&mut launcher, phone_dp, 0.0, 2.75).1,
            4.0,
            "{phone_dp} dp"
        );
    }
}

#[test]
fn apps09_given_a_paused_work_profile_its_apps_are_dimmed_and_a_tap_defers_to_the_system() {
    // Issue #9. The system shows its own explanation when a paused or suspended app is
    // launched; MeeGone shows a short message only if the launch fails outright.
    let fakes = Fakes::default();
    with_a_work_profile(&fakes, true);
    let mut paused = work_app("mail", "Mail");
    paused.available = false;
    *fakes.apps.apps.lock().unwrap() = vec![app("maps", "Maps"), paused.clone()];
    let mut launcher = Launcher::start(fakes.platform());

    assert_eq!(
        cells(&mut launcher),
        [
            ("Mail".to_owned(), true, false),
            ("Maps".to_owned(), false, true),
        ]
    );

    let icon = Rect {
        left: 44,
        top: 300,
        right: 198,
        bottom: 454,
    };
    let revision = launcher.frame().apps.revision;
    assert!(
        launcher
            .view()
            .apps()
            .cell_tapped(Surface::Grid, revision, 0, icon)
    );
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [paused.id.clone()]);
    assert_eq!(*fakes.apps.launched_from.lock().unwrap(), [Some(icon)]);
    assert_eq!(launcher.shell().apps().state().problem, None);

    *fakes.apps.error.lock().unwrap() = Some(DataError::Unsupported("App isn't available".into()));
    assert!(
        launcher
            .view()
            .apps()
            .cell_tapped(Surface::Grid, revision, 0, icon)
    );
    launcher.frame();
    assert_eq!(
        launcher.shell().apps().state().problem.as_deref(),
        Some("App isn't available")
    );
}

#[test]
fn apps_changed_while_open_reach_the_grid() {
    // Issue #9: the system reports a resumed work profile while MeeGone shows; its apps are
    // no longer dimmed.
    let fakes = Fakes::default();
    with_a_work_profile(&fakes, true);
    let mut paused = work_app("mail", "Mail");
    paused.available = false;
    *fakes.apps.apps.lock().unwrap() = vec![paused];
    let mut launcher = Launcher::start(fakes.platform());
    let before = launcher.frame().apps.revision;

    with_a_work_profile(&fakes, false);
    *fakes.apps.apps.lock().unwrap() = vec![work_app("mail", "Mail")];
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_ne!(launcher.frame().apps.revision, before);
    assert_eq!(cells(&mut launcher), [("Mail".to_owned(), true, true)]);
}

/// Recent with Mail used a minute ago, its accent derived from an icon of these pixels.
fn mail_in_recent(fakes: &Fakes, icon: &[u32]) -> Launcher {
    let mail = App {
        accent: accent::derive(icon),
        ..app("mail", "Mail")
    };
    *fakes.apps.apps.lock().unwrap() = vec![mail];
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.system.now_ms.lock().unwrap() = 10_000_000;
    *fakes.usage.uses.lock().unwrap() = vec![RecentUse {
        app: app_id("mail"),
        last_used_ms: 9_940_000,
    }];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    launcher
}

/// The accent of Recent's first card, as the current theme draws it, and that theme's
/// background (THEME-07).
fn drawn_accent(launcher: &Launcher) -> (Rgb, Rgb) {
    let theme = tokens::tokens(launcher.shell().theme().state().theme);
    let card = &launcher.shell().recent().state().cards[0];
    (theme.app_accent(card.app.accent), theme.background)
}

/// A 48 × 48 icon in one opaque colour, with the shape's four corners transparent.
fn icon_of(rgb: u32) -> Vec<u32> {
    let mut icon = vec![0xFF00_0000 | rgb; 48 * 48];
    for corner in [0, 47, 48 * 47, 48 * 48 - 1] {
        icon[corner] = 0;
    }
    icon
}

#[test]
fn apps17_given_a_near_white_icon_its_light_accent_reaches_3_to_1_against_the_background() {
    // Issue #10, THEME-07
    let fakes = Fakes::default();
    let launcher = mail_in_recent(&fakes, &icon_of(0xFAFAFA));
    assert_eq!(launcher.shell().theme().state().theme, Theme::Light);

    let (accent, background) = drawn_accent(&launcher);

    assert_eq!(background, Rgb::hex(0xF2F2F2));
    assert!(
        accent.contrast_ratio(background) >= 3.0,
        "{accent:?} is {:.2}",
        accent.contrast_ratio(background)
    );
}

#[test]
fn apps17_given_a_near_black_icon_its_dark_accent_reaches_3_to_1_against_the_background() {
    // Issue #10, THEME-07
    let fakes = Fakes::default();
    *fakes.system.dark.lock().unwrap() = true;
    let launcher = mail_in_recent(&fakes, &icon_of(0x080808));
    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);

    let (accent, background) = drawn_accent(&launcher);

    assert_eq!(background, Rgb::hex(0x000000));
    assert!(
        accent.contrast_ratio(background) >= 3.0,
        "{accent:?} is {:.2}",
        accent.contrast_ratio(background)
    );
}

#[test]
fn apps17_recent_shows_the_accent_derived_from_the_icon_and_an_update_derives_it_again() {
    // Issue #10: one accent per app, from the catalogue, so every view shows the same one.
    let fakes = Fakes::default();
    let mut launcher = mail_in_recent(&fakes, &icon_of(0x1877F2));
    let card = |launcher: &Launcher| launcher.shell().recent().state().cards[0].app.accent;
    assert_eq!(card(&launcher), Rgb::hex(0x1877F2));

    // An update changes Mail's icon; the host derives its accent again and reports the change.
    fakes.apps.apps.lock().unwrap()[0].accent = accent::derive(&icon_of(0xEA4335));
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_eq!(card(&launcher), Rgb::hex(0xEA4335));
}

/// A finger goes down on cell `index` of the grid at `(x, y)` and stays there until the host's
/// long-press time passes; the host then reports it as `AppsGrid` does, by the cells' revision
/// and the cell's place (APPS-11). Returns whether the long-press was taken. The finger stays
/// down.
fn long_press_cell(launcher: &mut Launcher, clock: &mut u64, index: usize, x: f64, y: f64) -> bool {
    finger(launcher, clock, PointerKind::Down, x, y);
    *clock += 400;
    let revision = launcher.frame().apps.revision;
    let Some(app) = launcher
        .view()
        .apps()
        .app_at(Surface::Grid, revision, index)
        .cloned()
    else {
        return false;
    };
    launcher.dispatch(ShellIntent::LongPressCell {
        surface: Surface::Grid,
        key: CellKey::App(app),
    });
    launcher.frame().apps.menu.is_some() && launcher.shell().state().carousel.held
}

fn finger(launcher: &mut Launcher, clock: &mut u64, kind: PointerKind, x: f64, y: f64) {
    *clock += 16;
    launcher.dispatch(ShellIntent::Pointer(PointerEvent {
        kind,
        x,
        y,
        at: Duration::from_millis(*clock),
    }));
    launcher.frame();
}

/// The open app menu's items, as the host reads them.
fn app_menu(launcher: &mut Launcher) -> Vec<AppMenuItem> {
    launcher.frame();
    launcher.view().apps().menu_items().to_vec()
}

/// Item `index` of the app menu is tapped, as `HomeViews` reports it.
fn choose(launcher: &mut Launcher, index: usize) {
    let revision = launcher
        .frame()
        .apps
        .menu
        .expect("the menu is open")
        .revision;
    assert!(launcher.view().apps().menu_item_chosen(revision, index));
    launcher.frame();
}

/// Mail, which the user installed, and Settings, a system app.
fn mail_and_settings(fakes: &Fakes) {
    *fakes.apps.apps.lock().unwrap() = vec![
        app("mail", "Mail"),
        App {
            can_uninstall: false,
            ..app("settings", "Settings")
        },
    ];
}

const FIXED: [AppMenuItem; 3] = [
    AppMenuItem::AppInfo,
    AppMenuItem::Uninstall,
    AppMenuItem::EditApplications,
];

#[test]
fn apps11_given_a_system_app_when_its_menu_opens_then_it_has_no_uninstall_item() {
    // Issue #12, APPS-13, ANIM-24
    let fakes = Fakes::default();
    mail_and_settings(&fakes);
    let (mut launcher, mut clock) = on_a_phone(&fakes);

    assert!(long_press_cell(&mut launcher, &mut clock, 1, 400.0, 300.0));
    assert_eq!(
        app_menu(&mut launcher),
        [AppMenuItem::AppInfo, AppMenuItem::EditApplications]
    );
    // The host opens it at Settings' cell, and the long-press ticked.
    assert_eq!(launcher.frame().apps.menu.unwrap().cell, Some(1));
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 1);

    // Lifting the finger launches nothing; the menu stays open.
    finger(&mut launcher, &mut clock, PointerKind::Up, 400.0, 300.0);
    assert!(fakes.apps.launched.lock().unwrap().is_empty());
    assert!(launcher.frame().apps.menu.is_some());

    // Mail, which the user installed, can be uninstalled.
    launcher.dispatch(ShellIntent::Back);
    assert!(long_press_cell(&mut launcher, &mut clock, 0, 150.0, 300.0));
    assert_eq!(app_menu(&mut launcher), FIXED);
}

#[test]
fn apps11_given_meegone_isnt_the_default_home_app_then_no_shortcuts_appear() {
    // Issue #12: the system offers shortcuts only to the default home app (SRS 8.1).
    let fakes = Fakes::default();
    mail_and_settings(&fakes);
    let compose = Shortcut {
        app: app_id("mail"),
        id: "compose".into(),
        label: "Compose".into(),
    };
    fakes
        .apps
        .shortcuts
        .lock()
        .unwrap()
        .insert(app_id("mail"), vec![compose.clone()]);
    let (mut launcher, mut clock) = on_a_phone(&fakes);

    assert!(long_press_cell(&mut launcher, &mut clock, 0, 150.0, 300.0));
    assert_eq!(app_menu(&mut launcher), FIXED);
    finger(&mut launcher, &mut clock, PointerKind::Up, 150.0, 300.0);
    launcher.dispatch(ShellIntent::Back);

    // As the default home app, they follow the fixed items, and one launches.
    *fakes.system.default_home.lock().unwrap() = true;
    *fakes.apps.shortcuts_allowed.lock().unwrap() = true;
    assert!(long_press_cell(&mut launcher, &mut clock, 0, 150.0, 300.0));
    let items = app_menu(&mut launcher);
    assert_eq!(items[..3], FIXED);
    assert_eq!(items[3..], [AppMenuItem::Shortcut(compose.clone())]);
    finger(&mut launcher, &mut clock, PointerKind::Up, 150.0, 300.0);
    choose(&mut launcher, 3);
    assert_eq!(*fakes.apps.shortcuts_launched.lock().unwrap(), [compose]);
    assert_eq!(launcher.frame().apps.menu, None);
}

#[test]
fn apps11_given_a_long_press_followed_by_a_drag_then_the_icon_lifts_and_edit_mode_starts() {
    // Issues #12 to #14. With the grid not laid out yet, the icon's centre is the finger, and
    // it takes no new place: dropped, it is back in its cell at once.
    let fakes = Fakes::default();
    mail_and_settings(&fakes);
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    assert!(long_press_cell(&mut launcher, &mut clock, 1, 400.0, 300.0));

    finger(&mut launcher, &mut clock, PointerKind::Move, 520.0, 450.0);

    let frame = launcher.frame();
    assert_eq!(frame.apps.menu, None);
    assert!(launcher.shell().apps().state().edit_mode);
    let lifted = frame.lifted.expect("lifted");
    assert_eq!((lifted.cell, lifted.x, lifted.y), (Some(1), 520, 450));
    // The views stay put, so the drag moves only the icon.
    assert_eq!(frame.pages[HomeView::Applications.index()], 0);
    // ANIM-13: it lifts to 110 %.
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(
        launcher.frame().lifted,
        Some(LiftFrame {
            surface: Surface::Grid,
            cell: Some(1),
            x: 520,
            y: 450,
            scale: 11_000,
            alpha: 255,
            settling: false,
            merging: false,
        })
    );

    finger(&mut launcher, &mut clock, PointerKind::Up, 520.0, 450.0);
    assert_eq!(launcher.frame().lifted, None);
    assert!(launcher.shell().apps().state().edit_mode);
    assert!(fakes.apps.launched.lock().unwrap().is_empty());
}

#[test]
fn apps13_uninstall_asks_the_system_and_the_app_leaves_only_when_the_system_removes_it() {
    // Issue #12: MeeGone never uninstalls anything itself. App info opens the system screen.
    let fakes = Fakes::default();
    mail_and_settings(&fakes);
    let (mut launcher, mut clock) = on_a_phone(&fakes);

    long_press_cell(&mut launcher, &mut clock, 0, 150.0, 300.0);
    finger(&mut launcher, &mut clock, PointerKind::Up, 150.0, 300.0);
    choose(&mut launcher, 0);
    assert_eq!(
        *fakes.apps.app_info_opened.lock().unwrap(),
        [app_id("mail")]
    );

    long_press_cell(&mut launcher, &mut clock, 0, 150.0, 300.0);
    finger(&mut launcher, &mut clock, PointerKind::Up, 150.0, 300.0);
    choose(&mut launcher, 1);
    assert_eq!(
        *fakes.apps.uninstall_requested.lock().unwrap(),
        [app_id("mail")]
    );
    assert_eq!(grid(&launcher), ["Mail", "Settings"]);

    // The user confirms in the system's dialog, and the system reports the removal.
    fakes.apps.apps.lock().unwrap().remove(0);
    launcher.dispatch(ShellIntent::AppsChanged);
    assert_eq!(grid(&launcher), ["Settings"]);
}

#[test]
fn home07_home08_back_or_home_closes_an_apps_menu() {
    // Issue #12: Home goes no further than closing it.
    let fakes = Fakes::default();
    mail_and_settings(&fakes);
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    launcher.view().apps().scrolled(Surface::Grid, 120.0);

    long_press_cell(&mut launcher, &mut clock, 0, 150.0, 300.0);
    finger(&mut launcher, &mut clock, PointerKind::Up, 150.0, 300.0);
    launcher.dispatch(ShellIntent::Back);
    assert_eq!(launcher.frame().apps.menu, None);

    long_press_cell(&mut launcher, &mut clock, 0, 150.0, 300.0);
    finger(&mut launcher, &mut clock, PointerKind::Up, 150.0, 300.0);
    launcher.dispatch(ShellIntent::Home);
    let frame = launcher.frame();
    assert_eq!(frame.apps.menu, None);
    assert_eq!(frame.apps.scroll_px, 120);
    assert_eq!(frame.current, HomeView::Applications);
}

#[test]
fn a11y05_a_screen_reader_opens_an_apps_menu_without_a_gesture() {
    // Issue #12: as `AppsGrid` reports the action, by revision and place. No long-press, so
    // no tick.
    let fakes = Fakes::default();
    mail_and_settings(&fakes);
    let (mut launcher, _) = on_a_phone(&fakes);
    let revision = launcher.frame().apps.revision;

    let app = launcher
        .view()
        .apps()
        .app_at(Surface::Grid, revision, 1)
        .cloned()
        .unwrap();
    launcher.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app)));

    let frame = launcher.frame();
    assert_eq!(frame.apps.menu.map(|menu| menu.cell), Some(Some(1)));
    assert!(!frame.menu_offered);
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 0);
}

/// The phone's grid as `AppsGrid` reports it: 992 px wide inside its 16 dp margins, so 44 px
/// in, four columns of 248 px, rows 308 px apart, the status bar's 82 px above and the
/// navigation bar's 132 px below. Each icon is 154 px from 22 px down its cell.
const PHONE_GRID: GridLayout = GridLayout {
    width_px: 992.0,
    row_height_px: 308.0,
    density: 2.75,
    top_px: 82.0,
    bottom_px: 132.0,
    height_px: HEIGHT,
    left_px: 44.0,
    view_top_px: 0.0,
    icon_px: 154.0,
    icon_top_px: 22.0,
};

fn grid_on_a_phone(launcher: &mut Launcher) {
    launcher.view().apps().grid_laid_out(PHONE_GRID);
    // The host reads the cells it draws.
    let _ = froms(launcher);
}

/// Camera, Mail, Maps and Phone, on a phone with the grid laid out, in edit mode, entered from
/// the home menu (APPS-12, HOME-10).
fn editing_four(fakes: &Fakes) -> (Launcher, u64) {
    *fakes.apps.apps.lock().unwrap() = vec![
        App {
            can_uninstall: false,
            ..app("camera", "Camera")
        },
        app("mail", "Mail"),
        app("maps", "Maps"),
        app("phone", "Phone"),
    ];
    let (mut launcher, mut clock) = on_a_phone(fakes);
    grid_on_a_phone(&mut launcher);
    long_press(&mut launcher, &mut clock, 500.0, 1500.0);
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
    come_to_rest(&mut launcher, &mut clock);
    (launcher, clock)
}

#[test]
fn ac07_given_edit_mode_when_maps_is_dragged_to_the_first_cell_then_the_others_move_aside_and_it_stays_first()
 {
    // Issue #13: APPS-12, APPS-18, ANIM-13. "After a reboot" is a new start on the same
    // storage.
    let fakes = Fakes::default();
    let (mut launcher, mut clock) = editing_four(&fakes);
    let frame = launcher.frame();
    assert!(frame.edit.on);
    // ANIM-12: the cells are at 92 %, and every row's badges show.
    assert_eq!(frame.edit.scale, 9_200);
    assert_eq!(frame.edit.badges, [10_000; BADGE_ROWS]);

    assert!(long_press_cell(&mut launcher, &mut clock, 2, 620.0, 300.0));
    // Over the first cell's label: its icon would make a folder.
    finger(&mut launcher, &mut clock, PointerKind::Move, 124.0, 451.0);
    // It waits over the first cell for a moment, then takes it.
    for _ in 0..12 {
        clock += 16;
        launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
        launcher.frame();
    }

    assert_eq!(grid(&launcher), ["Maps", "Camera", "Mail", "Phone"]);
    // The others move aside: the host moves each cell's view from where it was.
    assert_eq!(froms(&mut launcher), [Some(2), Some(0), Some(1), Some(3)]);
    let frame = launcher.frame();
    assert!(frame.animating);
    let lifted = frame.lifted.expect("still lifted");
    // Held 44 px left of its icon's centre and 115 px below it.
    assert_eq!((lifted.cell, lifted.x, lifted.y), (Some(0), 168, 336));

    finger(&mut launcher, &mut clock, PointerKind::Up, 124.0, 451.0);
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(launcher.frame().lifted, None);
    assert_eq!(grid(&launcher), ["Maps", "Camera", "Mail", "Phone"]);
    drop(launcher);

    let restarted = Launcher::start(fakes.platform());
    assert_eq!(grid(&restarted), ["Maps", "Camera", "Mail", "Phone"]);
}

#[test]
fn apps12_dragging_an_icon_near_the_bottom_scrolls_the_grid() {
    // Issue #13: 40 apps in 11 rows; the icon is held at the very bottom of the screen.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = (0..40)
        .map(|i| app(&format!("app{i:02}"), &format!("App {i:02}")))
        .collect();
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    grid_on_a_phone(&mut launcher);

    assert!(long_press_cell(&mut launcher, &mut clock, 5, 400.0, 600.0));
    finger(
        &mut launcher,
        &mut clock,
        PointerKind::Move,
        400.0,
        HEIGHT - 10.0,
    );
    for _ in 0..30 {
        clock += 16;
        launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
        launcher.frame();
    }

    let scrolled = launcher.frame().apps.scroll_px;
    assert!(scrolled > 300, "{scrolled}");
    assert!(launcher.frame().animating);
}

#[test]
fn ac09_given_edit_mode_when_the_user_taps_the_cross_and_confirms_then_the_icon_animates_away() {
    // Issue #13: APPS-12, APPS-13, ANIM-21. System apps show no ✕.
    let fakes = Fakes::default();
    let (mut launcher, _) = editing_four(&fakes);
    let crosses: Vec<bool> = launcher
        .view()
        .apps()
        .cells(Surface::Grid)
        .map(|cell| cell.uninstallable)
        .collect();
    assert_eq!(crosses, [false, true, true, true]);

    let revision = launcher.frame().apps.revision;
    assert!(
        launcher
            .view()
            .apps()
            .badge_tapped(Surface::Grid, revision, 1)
    );
    launcher.frame();
    // MeeGone only asks: the system shows its confirmation.
    assert_eq!(
        *fakes.apps.uninstall_requested.lock().unwrap(),
        [app_id("mail")]
    );
    assert_eq!(grid(&launcher), ["Camera", "Mail", "Maps", "Phone"]);

    // The user confirms, and the system removes Mail.
    fakes
        .apps
        .apps
        .lock()
        .unwrap()
        .retain(|app| app.id != app_id("mail"));
    launcher.dispatch(ShellIntent::AppsChanged);

    let frame = launcher.frame();
    assert!(frame.animating);
    assert_eq!(frame.grid_change.leave_alpha, 10_000);
    assert_eq!(grid(&launcher), ["Camera", "Maps", "Phone"]);
    assert!(launcher.shell().apps().state().edit_mode);
}

#[test]
fn apps12_done_home_or_back_ends_edit_mode() {
    // Issue #13: HOME-07, HOME-08. A tap on an icon launches nothing meanwhile.
    for end in 0..3 {
        let fakes = Fakes::default();
        let (mut launcher, mut clock) = editing_four(&fakes);
        let revision = launcher.frame().apps.revision;
        assert!(
            launcher
                .view()
                .apps()
                .cell_tapped(Surface::Grid, revision, 2, Rect::default())
        );
        launcher.frame();
        assert!(fakes.apps.launched.lock().unwrap().is_empty());

        match end {
            0 => launcher.view().apps().done(),
            1 => launcher.dispatch(ShellIntent::Home),
            _ => launcher.dispatch(ShellIntent::Back),
        }

        let frame = launcher.frame();
        assert!(!frame.edit.on);
        // ANIM-12: reversed on exit.
        assert!(frame.animating);
        come_to_rest(&mut launcher, &mut clock);
        let frame = launcher.frame();
        assert_eq!(frame.edit.scale, 10_000);
        assert_eq!(frame.edit.badges, [0; BADGE_ROWS]);
        // Home only closes it: the grid stays.
        assert_eq!(frame.current, HomeView::Applications);
    }
}

#[test]
fn a11y05_a_screen_reader_moves_an_icon_without_a_gesture() {
    // Issue #13: AC-19's "move an icon". The move is saved.
    let fakes = Fakes::default();
    let (mut launcher, _) = editing_four(&fakes);
    let revision = launcher.frame().apps.revision;

    assert!(launcher.view().apps().moved(Surface::Grid, revision, 3, 0));
    launcher.frame();

    assert_eq!(grid(&launcher), ["Phone", "Camera", "Mail", "Maps"]);
    drop(launcher);
    assert_eq!(
        grid(&Launcher::start(fakes.platform())),
        ["Phone", "Camera", "Mail", "Maps"]
    );
}

/// Where `place`'s icon centre is on screen in [`PHONE_GRID`], out of edit mode.
fn phone_icon(place: usize) -> (f64, f64) {
    let (column, row) = ((place % 4) as f64, (place / 4) as f64);
    (44.0 + column * 248.0 + 124.0, 82.0 + row * 308.0 + 99.0)
}

/// Frames every 16 ms for `ms` ms.
fn wait(launcher: &mut Launcher, clock: &mut u64, ms: u64) {
    for _ in 0..ms / 16 {
        *clock += 16;
        launcher.dispatch(ShellIntent::Frame(Duration::from_millis(*clock)));
        launcher.frame();
    }
}

/// The open folder's grid as its panel reports it: four columns of 248 px from 44 px in, its
/// top 600 px down, one row tall, inside a panel from (30, 500) to (1050, 1000).
fn folder_on_a_phone(launcher: &mut Launcher) {
    launcher.view().apps().folder_laid_out(
        GridLayout {
            top_px: 0.0,
            bottom_px: 0.0,
            height_px: 308.0,
            view_top_px: 600.0,
            ..PHONE_GRID
        },
        Bounds {
            left: 30.0,
            top: 500.0,
            right: 1050.0,
            bottom: 1000.0,
        },
    );
    let _ = launcher.view().apps().cells(Surface::Folder).count();
}

#[test]
fn ac08_given_two_apps_when_one_is_dropped_on_the_other_then_a_folder_holds_both_and_moving_one_out_undoes_it()
 {
    // Issue #14: FOLD-01, FOLD-03, FOLD-05, FOLD-06, ANIM-14.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("maps", "Maps")];
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    grid_on_a_phone(&mut launcher);

    // Maps is held over Mail's icon until it pulses, then dropped.
    let (x, y) = phone_icon(1);
    assert!(long_press_cell(&mut launcher, &mut clock, 1, x, y));
    let (x, y) = phone_icon(0);
    finger(&mut launcher, &mut clock, PointerKind::Move, x, y);
    wait(&mut launcher, &mut clock, 480);
    let hover = launcher.frame().hover.expect("held over Mail");
    assert_eq!(hover.cell, 0);
    finger(&mut launcher, &mut clock, PointerKind::Up, x, y);

    // A folder named "Folder", the host's name for "", holds both, where Mail was.
    assert_eq!(grid(&launcher), [""]);
    let frame = launcher.frame();
    let merge = frame.merge.expect("Maps goes into the folder");
    assert_eq!((merge.cell, merge.slot, merge.formed), (0, Some(1), true));
    come_to_rest(&mut launcher, &mut clock);
    let cell = launcher.view().apps().cells(Surface::Grid).next().unwrap();
    let held: Vec<&str> = cell.apps.iter().map(|app| app.label.as_str()).collect();
    assert_eq!((cell.label, held), ("", vec!["Mail", "Maps"]));

    // Opened, then Maps dragged out of its panel to the grid's second place.
    open_folder(&mut launcher, 0);
    folder_on_a_phone(&mut launcher);
    come_to_rest(&mut launcher, &mut clock);
    assert!(launcher.frame().folder.expect("open").open);
    let (fx, fy) = (44.0 + 248.0 + 124.0, 600.0 + 99.0);
    finger(&mut launcher, &mut clock, PointerKind::Down, fx, fy);
    clock += 400;
    launcher.dispatch(ShellIntent::LongPressCell {
        surface: Surface::Folder,
        key: CellKey::App(app_id("maps")),
    });
    assert!(launcher.frame().apps.menu.is_some());
    finger(&mut launcher, &mut clock, PointerKind::Move, fx, fy - 40.0);
    assert_eq!(
        launcher.frame().lifted.map(|lift| lift.surface),
        Some(Surface::Folder)
    );
    finger(&mut launcher, &mut clock, PointerKind::Move, fx, 300.0);
    assert_eq!(launcher.shell().apps().state().open_folder, None);
    finger(&mut launcher, &mut clock, PointerKind::Up, fx, 300.0);
    come_to_rest(&mut launcher, &mut clock);

    // The folder of one is Mail again.
    assert_eq!(grid(&launcher), ["Mail", "Maps"]);
    drop(launcher);
    assert_eq!(grid(&Launcher::start(fakes.platform())), ["Mail", "Maps"]);
}

#[test]
fn a11y05_a_screen_reader_makes_a_folder_and_takes_an_app_out_of_it() {
    // Issue #14: A11Y-05. "Make folder with Maps", then "Move out of folder". Each step ticks:
    // the long-press, entering edit mode, and the folder made (ANIM-24).
    let fakes = Fakes::default();
    let (mut launcher, _) = editing_four(&fakes);
    let revision = launcher.frame().apps.revision;

    assert!(launcher.view().apps().grouped(revision, 1));
    launcher.frame();
    assert_eq!(grid(&launcher), ["Camera", "", "Phone"]);
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 3);

    // Mail went onto Maps: the folder holds Maps first. Taking Maps out leaves Mail alone,
    // which the folder turns back into, with Maps just after it.
    open_folder(&mut launcher, 1);
    assert_eq!(in_folder(&launcher), ["Maps", "Mail"]);
    let open = launcher.frame().apps.folder.revision;
    assert!(launcher.view().apps().moved_out(open, 0));
    launcher.frame();
    assert_eq!(grid(&launcher), ["Camera", "Mail", "Maps", "Phone"]);
}

/// Games, a folder of Chess and Go, then Maps, on a phone.
fn games_and_maps(fakes: &Fakes) -> (Launcher, u64) {
    *fakes.apps.apps.lock().unwrap() =
        vec![app("chess", "Chess"), app("go", "Go"), app("maps", "Maps")];
    saved_layout(
        fakes,
        "folder 1 Games 2 0 chess chess.Main 0 go go.Main\napp 0 maps maps.Main\n",
    );
    on_a_phone(fakes)
}

/// The folder at grid cell `index` is tapped.
fn open_folder(launcher: &mut Launcher, index: usize) {
    let revision = launcher.frame().apps.revision;
    assert!(
        launcher
            .view()
            .apps()
            .cell_tapped(Surface::Grid, revision, index, Rect::default())
    );
    launcher.frame();
}

fn in_folder(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .apps()
        .cells(Surface::Folder)
        .map(|cell| cell.label.to_owned())
        .collect()
}

#[test]
fn fold04_a_tap_opens_a_folder_and_back_home_or_a_tap_outside_closes_it() {
    // Issue #14: FOLD-04, HOME-07, HOME-08, ANIM-15.
    for end in 0..3 {
        let fakes = Fakes::default();
        let (mut launcher, mut clock) = games_and_maps(&fakes);

        open_folder(&mut launcher, 0);

        let frame = launcher.frame();
        let folder = frame.folder.expect("opening");
        assert_eq!(
            (folder.folder, folder.open, folder.cell),
            (FolderId(1), true, Some(0))
        );
        assert!(frame.animating && !frame.menu_offered);
        come_to_rest(&mut launcher, &mut clock);
        assert_eq!(launcher.frame().folder.unwrap().progress, 10_000);
        assert_eq!(in_folder(&launcher), ["Chess", "Go"]);

        match end {
            0 => launcher.dispatch(ShellIntent::Back),
            1 => launcher.dispatch(ShellIntent::Home),
            _ => launcher.view().apps().close_folder(),
        }

        let closing = launcher.frame().folder.expect("closing");
        assert!(!closing.open);
        come_to_rest(&mut launcher, &mut clock);
        let frame = launcher.frame();
        assert_eq!(frame.folder, None);
        assert_eq!(frame.current, HomeView::Applications);
    }
}

#[test]
fn fold03_a_folders_new_name_survives_a_restart() {
    // Issue #14: FOLD-03. The host asks for the name in a dialog.
    let fakes = Fakes::default();
    let (mut launcher, _) = games_and_maps(&fakes);
    open_folder(&mut launcher, 0);

    launcher.view().apps().rename_requested(FolderId(1));
    let renaming = launcher.frame().renaming.expect("asked for a name");
    assert_eq!((renaming.folder, renaming.cell), (FolderId(1), Some(0)));
    launcher
        .view()
        .apps()
        .folder_renamed(FolderId(1), "  Board games ".into());

    assert_eq!(launcher.frame().renaming, None);
    assert_eq!(grid(&launcher), ["Board games", "Maps"]);
    drop(launcher);
    assert_eq!(
        grid(&Launcher::start(fakes.platform())),
        ["Board games", "Maps"]
    );
}

#[test]
fn fold05_apps_move_within_a_folder_and_the_order_is_kept() {
    // Issue #14: FOLD-05, in edit mode, with a screen reader's "Move earlier".
    let fakes = Fakes::default();
    let (mut launcher, mut clock) = games_and_maps(&fakes);
    long_press(&mut launcher, &mut clock, 500.0, 1500.0);
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
    open_folder(&mut launcher, 0);
    let open = launcher.frame().apps.folder.revision;

    assert!(launcher.view().apps().moved(Surface::Folder, open, 1, 0));
    launcher.frame();

    assert_eq!(in_folder(&launcher), ["Go", "Chess"]);
    drop(launcher);
    let mut restarted = Launcher::start(fakes.platform());
    open_folder(&mut restarted, 0);
    assert_eq!(in_folder(&restarted), ["Go", "Chess"]);
}

#[test]
fn ac05_given_theme_system_and_an_open_folder_the_app_turns_dark_with_the_folder_still_open() {
    // Issue #14: THEME-02, THEME-06.
    let fakes = Fakes::default();
    let (mut launcher, mut clock) = games_and_maps(&fakes);
    launcher.view().apps().scrolled(Surface::Grid, 120.0);
    open_folder(&mut launcher, 0);
    come_to_rest(&mut launcher, &mut clock);
    // The black scrim behind it is 40 % opaque in Light (SRS 5.3)…
    assert_eq!(launcher.frame().scrim, 102);

    *fakes.system.dark.lock().unwrap() = true;
    launcher.dispatch(ShellIntent::SystemThemeChanged);

    let frame = launcher.frame();
    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);
    assert!(frame.folder.expect("still open").open);
    assert_eq!(frame.apps.scroll_px, 120);
    // …and 60 % in Dark.
    assert_eq!(frame.scrim, 153);
}
