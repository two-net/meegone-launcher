//! The acceptance scenarios of SRS section 9, run on the whole app with fake platform adapters
//! (MAINT-03). Each test carries its scenario's id; each feature adds its own here.
//!
//! AC-18 to AC-20 (animation scale, screen reader, timing) need a device to check in full; they
//! belong to the device matrix (#43) and the benchmarks (#40). Their core logic is tested
//! without one: every flow with animations removed (AC-18, ANIM-05) and the screen-reader
//! actions (AC-19, A11Y-05) here, and the carousel's in `meegone-shell-state`.

mod backup;

use std::collections::BTreeMap;
use std::time::Duration;

use meegone_core_data::fakes::{
    self, Fakes, WORK_PROFILE, app, app_id, notification, used, work_app,
};
use meegone_core_data::storage::format::seal;
use meegone_core_data::{
    App, DataError, HomeTarget, JobNetwork, NetworkState, Notification, NotificationsAdapter,
    PersistentSettingsStore, Profile, ProfileId, RecentDensity, Rect, Rgb, SettingsStore, Shortcut,
    Size, StoredFile, ThemeChoice, accent,
};
use meegone_core_state::{Access, Health, Theme};
use meegone_core_ui::tokens;
use meegone_launcher::{
    AccessItem, AccessStatus, Age, AppMenuItem, BADGE_ROWS, Bounds, CardContent, CellKey,
    CheckStatus, DailyImage, DetailsContent, Direction, EventsListMode, EventsMenuFrame, FolderId,
    GridLayout, HomeView, Launcher, Layout, LiftFrame, MenuItem, NotificationMenuItem, PastAction,
    PastContent, PinchEvent, PinchKind, PointerEvent, PointerKind, RECENT_ROWS, RecentFrame,
    RecentMode, RecentWindow, SettingsGroup, SetupStep, Shared, ShellIntent, Surface, UpdateStatus,
};

/// A phone 1080 × 2400 px at 2.75 px per dp.
const WIDTH: f64 = 1080.0;
const HEIGHT: f64 = 2400.0;

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
    let fakes = Fakes::after_setup();
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
    let (mut launcher, mut clock) = on_a_phone(&Fakes::after_setup());

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
    let (mut launcher, mut clock) = on_a_phone(&Fakes::after_setup());
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
    // Issue #22: REC-03, REC-06, HOME-06. Mail is opened from its card, then Home is pressed.
    let fakes = Fakes::after_setup();
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("maps", "Maps")];
    *fakes.system.now_ms.lock().unwrap() = 10_000_000;
    *fakes.usage.uses.lock().unwrap() = vec![used("maps", 9_000_000), used("mail", 8_000_000)];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    let (mode, cards) = recent_cards(&mut launcher);
    assert_eq!(mode, RecentMode::Cards);
    assert_eq!(labels(&cards), ["Maps", "Mail"]);

    let revision = launcher.frame().recent.expect("Recent works").revision;
    assert!(launcher.view().recent().card_tapped(revision, 1, CARD));
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
    assert_eq!(*fakes.apps.launched_from.lock().unwrap(), [Some(CARD)]);

    // The user reads mail for a minute; the system reports it used when it leaves the front.
    *fakes.system.now_ms.lock().unwrap() = 10_060_000;
    fakes.usage.uses.lock().unwrap()[1].last_used_ms = 10_060_000;
    launcher.dispatch(ShellIntent::Resumed);
    launcher.dispatch(ShellIntent::Returned { home: true });

    assert_eq!(launcher.shell().state().current, HomeView::Recent);
    let (_, cards) = recent_cards(&mut launcher);
    assert_eq!(labels(&cards), ["Mail", "Maps"]);
}

#[test]
fn ac04_home_on_events_shows_applications_and_home_again_scrolls_it_to_the_top() {
    let (mut launcher, mut clock) = on_a_phone(&Fakes::after_setup());
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
    let (mut launcher, _) = on_a_phone(&Fakes::after_setup());
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
    let fakes = Fakes::after_setup();
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
    let (mut launcher, mut clock) = on_a_phone(&Fakes::after_setup());
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
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);

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
    let (mut launcher, mut clock) = on_a_phone(&Fakes::after_setup());
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
    let mut launcher = Launcher::start(Fakes::after_setup().platform());
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
        let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
    let mut launcher = Launcher::start(fakes.platform());
    assert_eq!(launcher.shell().theme().state().theme, Theme::Light);

    *fakes.system.dark.lock().unwrap() = true;
    launcher.dispatch(ShellIntent::SystemThemeChanged);

    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);
}

#[test]
fn ac06_with_theme_light_the_app_stays_light_when_the_device_turns_dark() {
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
        .insert(StoredFile::Layout, seal("layout", 3, records));
}

#[test]
fn ac07_given_maps_was_moved_to_the_first_cell_after_a_restart_it_is_still_first() {
    // Issue #11: APPS-18. Moving it takes edit mode (#13); here the user's arrangement is
    // saved as edit mode will save it.
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    assert!(stored.starts_with("meegone-layout 3\n"), "{stored}");
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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

/// What shows below the Events header after a frame, and each group's name and count.
fn events_list(launcher: &mut Launcher) -> (EventsListMode, Vec<(String, usize)>) {
    let list = launcher.frame().events_list.expect("Events works");
    let rows = launcher.view().events().rows();
    let rows = rows
        .iter()
        .map(|row| (row.label.clone(), row.count))
        .collect();
    (list.mode, rows)
}

#[test]
fn ac11_after_access_is_granted_notifications_replace_the_explainer_card() {
    // Issue #19: EVT-03, EVT-04, ONB-02. Access is granted in system settings, and the
    // listener's connection reports it.
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert_eq!(launcher.shell().events().state().access, Access::Missing);
    assert_eq!(
        events_list(&mut launcher),
        (EventsListMode::Explainer, Vec::new())
    );
    // The header still shows (EVT-03).
    assert!(launcher.frame().events_header.is_some());

    launcher.view().events().allow_access_tapped();
    launcher.frame();
    assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![notification("m1", "mail")];
    launcher.dispatch(ShellIntent::NotificationsChanged);

    // Within the same dispatch, well inside 1 s.
    assert_eq!(
        events_list(&mut launcher),
        (EventsListMode::List, vec![("Mail".into(), 1)])
    );
}

#[test]
fn ac11_given_access_granted_while_away_then_coming_back_shows_the_notifications() {
    // Issue #19: EVT-03. The system screen covered the home, and MeeGone resumes.
    let fakes = Fakes::after_setup();
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert_eq!(events_list(&mut launcher).0, EventsListMode::Explainer);

    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![notification("m1", "mail")];
    launcher.dispatch(ShellIntent::Resumed);

    assert_eq!(
        events_list(&mut launcher),
        (EventsListMode::List, vec![("mail".into(), 1)])
    );
}

#[test]
fn ac21_revoking_notification_access_shows_the_explainer_and_applications_still_works() {
    let fakes = Fakes::after_setup();
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
    assert_eq!(
        events_list(&mut launcher),
        (EventsListMode::Explainer, Vec::new())
    );

    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    launcher.view().apps().icon_tapped(app_id("mail"), None);
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
}

#[test]
fn evt04_given_notifications_from_several_apps_then_events_groups_them_newest_first() {
    // Issue #19: EVT-04, EVT-09, APPS-17, APPS-20. As the SRS's layout mock: Messages with three,
    // then Mail with one, and an app with no icon in the grid named as the system names it.
    let fakes = Fakes::after_setup();
    let messages = App {
        accent: Rgb::hex(0x34A853),
        ..app("messages", "Messages")
    };
    *fakes.apps.apps.lock().unwrap() =
        vec![messages, app("mail", "Mail"), work_app("mail", "Mail")];
    let posted = |key: &str, package: &str, posted_at_ms| Notification {
        posted_at_ms,
        ..notification(key, package)
    };
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![
        posted("s1", "android", 10),
        posted("m1", "messages", 20),
        posted("e1", "mail", 30),
        posted("m2", "messages", 40),
        Notification {
            title: "Anna".into(),
            text: "See you at eight".into(),
            ..posted("m3", "messages", 50)
        },
        Notification {
            profile: WORK_PROFILE,
            ..posted("w1", "mail", 5)
        },
        Notification {
            app_label: "Android System".into(),
            ..posted("s2", "android", 15)
        },
    ];
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));

    assert_eq!(
        events_list(&mut launcher),
        (
            EventsListMode::List,
            vec![
                ("Messages".into(), 3),
                ("Mail".into(), 1),
                ("Android System".into(), 2),
                ("Mail".into(), 1),
            ]
        )
    );
    let rows = launcher.view().events().rows();
    assert_eq!(
        (rows[0].title.as_str(), rows[0].text.as_str()),
        ("Anna", "See you at eight")
    );
    // EVT-09: the accent as the theme draws it, and a neutral marker without one.
    let theme = tokens(launcher.shell().theme().state().theme);
    assert_eq!(rows[0].accent, theme.app_accent(Rgb::hex(0x34A853)));
    assert_eq!(rows[2].accent, theme.text_secondary);
    assert_eq!(rows[2].activity, None);
    assert_eq!(rows[1].activity.as_deref(), Some("mail.Main"));
    assert_eq!((rows[1].work, rows[3].work), (false, true));
}

#[test]
fn evt05_given_a_media_playback_notification_then_it_isnt_listed() {
    // Issue #19: EVT-05. Playback, navigation and running services are ongoing.
    let fakes = Fakes::after_setup();
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![
        Notification {
            ongoing: true,
            clearable: false,
            ..notification("p1", "player")
        },
        notification("m1", "mail"),
    ];
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));

    assert_eq!(
        events_list(&mut launcher),
        (EventsListMode::List, vec![("mail".into(), 1)])
    );

    // With only the player left, nothing is new.
    fakes
        .notifications
        .active
        .lock()
        .unwrap()
        .retain(|n| n.package == "player");
    launcher.dispatch(ShellIntent::NotificationsChanged);
    assert_eq!(
        events_list(&mut launcher),
        (EventsListMode::Empty, Vec::new())
    );
}

#[test]
fn evt10_given_events_showing_then_a_new_group_slides_in_and_a_dismissed_one_collapses() {
    // Issue #19: EVT-10, ANIM-20, EVT-11.
    let fakes = Fakes::after_setup();
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(events_list(&mut launcher).0, EventsListMode::Empty);

    fakes
        .notifications
        .active
        .lock()
        .unwrap()
        .push(notification("m1", "mail"));
    launcher.dispatch(ShellIntent::NotificationsChanged);
    let frame = launcher.frame();
    assert!(frame.animating);
    assert_eq!(frame.events_change, 0);
    assert_eq!(launcher.view().events().rows()[0].from, None, "it comes in");

    let started = clock;
    come_to_rest(&mut launcher, &mut clock);
    assert!(clock - started <= 200 + 16, "{} ms", clock - started);
    assert_eq!(launcher.frame().events_change, 10_000);

    fakes.notifications.active.lock().unwrap().clear();
    launcher.dispatch(ShellIntent::NotificationsChanged);
    let frame = launcher.frame();
    assert!(frame.animating, "the group collapses");
    assert_eq!(
        events_list(&mut launcher),
        (EventsListMode::Empty, Vec::new())
    );
}

/// Where a tapped Events row is on the phone's screen.
const ROW: Rect = Rect {
    left: 0,
    top: 900,
    right: 1080,
    bottom: 1100,
};

/// The Events rows after a frame, with their revision: each row's name, whether it is one
/// notification's, its count and its title.
fn events_rows(launcher: &mut Launcher) -> (u32, Vec<(String, bool, usize, String)>) {
    let list = launcher.frame().events_list.expect("Events works");
    let rows = launcher
        .view()
        .events()
        .rows()
        .iter()
        .map(|row| {
            (
                row.label.clone(),
                row.notification,
                row.count,
                row.title.clone(),
            )
        })
        .collect();
    (list.revision, rows)
}

fn sorted(keys: &std::sync::Mutex<Vec<String>>) -> Vec<String> {
    let mut keys = keys.lock().unwrap().clone();
    keys.sort();
    keys
}

#[test]
fn ac12_given_clearable_and_ongoing_notifications_when_clear_is_tapped_then_only_ongoing_remain() {
    // Issue #20: EVT-05, EVT-07. A paused player can be cleared in the system's shade, but it
    // is ongoing playback, so Events never showed it and Clear leaves it.
    let fakes = Fakes::after_setup();
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![
        notification("m1", "mail"),
        notification("m2", "mail"),
        notification("c1", "chat"),
        Notification {
            ongoing: true,
            ..notification("p1", "player")
        },
        Notification {
            ongoing: true,
            clearable: false,
            ..notification("n1", "maps")
        },
    ];
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    let list = launcher.frame().events_list.expect("Events works");
    assert!(list.clearable, "Clear is offered");

    launcher.view().events().clear_tapped();
    launcher.frame();
    assert_eq!(sorted(&fakes.notifications.dismissed), ["c1", "m1", "m2"]);
    // The system reports them gone.
    launcher.dispatch(ShellIntent::NotificationsChanged);

    let list = launcher.frame().events_list.expect("Events works");
    assert_eq!(list.mode, EventsListMode::Empty);
    assert!(!list.clearable);
    let left: Vec<String> = fakes
        .notifications
        .active
        .lock()
        .unwrap()
        .iter()
        .map(|n| n.key.clone())
        .collect();
    assert_eq!(left, ["p1", "n1"]);
}

#[test]
fn evt06_given_a_group_of_three_when_it_is_tapped_then_it_expands_and_tapping_one_item_opens_it() {
    // Issue #20's acceptance criterion: EVT-06, ANIM-08, ANIM-20. The opened notification is
    // meant to disappear when opened, so MeeGone removes it.
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("messages", "Messages")];
    let message = |key: &str, title: &str, posted_at_ms| Notification {
        title: title.into(),
        posted_at_ms,
        auto_cancel: true,
        ..notification(key, "messages")
    };
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![
        message("m1", "Anna", 10),
        message("m2", "Ben", 20),
        message("m3", "Cleo", 30),
    ];
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    come_to_rest(&mut launcher, &mut clock);
    let (revision, rows) = events_rows(&mut launcher);
    assert_eq!(rows, [("Messages".into(), false, 3, "Cleo".into())]);

    assert!(launcher.view().events().row_tapped(revision, 0, ROW));
    let frame = launcher.frame();

    assert!(frame.animating, "its notifications slide in");
    let (revision, rows) = events_rows(&mut launcher);
    assert_eq!(
        rows,
        [
            ("Messages".into(), false, 3, String::new()),
            ("Messages".into(), true, 1, "Cleo".into()),
            ("Messages".into(), true, 1, "Ben".into()),
            ("Messages".into(), true, 1, "Anna".into()),
        ]
    );
    assert!(fakes.notifications.opened.lock().unwrap().is_empty());

    assert!(launcher.view().events().row_tapped(revision, 2, ROW));
    launcher.frame();

    assert_eq!(*fakes.notifications.opened.lock().unwrap(), ["m2"]);
    assert_eq!(
        *fakes.notifications.opened_from.lock().unwrap(),
        [Some(ROW)]
    );
    assert_eq!(*fakes.notifications.dismissed.lock().unwrap(), ["m2"]);
    launcher.dispatch(ShellIntent::NotificationsChanged);
    let (_, rows) = events_rows(&mut launcher);
    assert_eq!(rows.len(), 3, "the group and its two left");
}

#[test]
fn evt06_given_a_group_of_one_when_it_is_tapped_then_its_notification_opens() {
    // Issue #20: EVT-06.
    let fakes = Fakes::after_setup();
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![notification("c1", "chat")];
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    let (revision, _) = events_rows(&mut launcher);

    assert!(launcher.view().events().row_tapped(revision, 0, ROW));
    launcher.frame();

    assert_eq!(*fakes.notifications.opened.lock().unwrap(), ["c1"]);
    assert!(
        fakes.notifications.dismissed.lock().unwrap().is_empty(),
        "it isn't meant to disappear when opened"
    );
}

#[test]
fn evt08_given_a_long_press_on_a_notification_then_dismiss_and_notification_settings_are_offered() {
    // Issue #20: EVT-08, ANIM-24.
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![
        Notification {
            posted_at_ms: 20,
            ..notification("m1", "mail")
        },
        Notification {
            clearable: false,
            ..notification("s1", "android")
        },
    ];
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    come_to_rest(&mut launcher, &mut clock);
    let (revision, _) = events_rows(&mut launcher);
    let hold = |launcher: &mut Launcher, row: usize, clock: u64| {
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Down,
            x: 500.0,
            y: 1000.0,
            at: Duration::from_millis(clock),
        }));
        let target = launcher
            .view()
            .events()
            .row_target(revision, row)
            .expect("the row shows");
        launcher.dispatch(ShellIntent::LongPressNotification(target));
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Up,
            x: 500.0,
            y: 1000.0,
            at: Duration::from_millis(clock + 700),
        }));
        launcher.frame()
    };

    // Mail can be cleared; the system's notification can't.
    let frame = hold(&mut launcher, 1, clock);
    assert_eq!(
        frame.events_menu,
        Some(EventsMenuFrame {
            dismiss: false,
            row: Some(1),
        })
    );
    launcher.dispatch(ShellIntent::CloseMenu);
    let frame = hold(&mut launcher, 0, clock + 1_000);
    assert_eq!(
        frame.events_menu,
        Some(EventsMenuFrame {
            dismiss: true,
            row: Some(0),
        })
    );
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 2);
    assert!(
        fakes.notifications.opened.lock().unwrap().is_empty(),
        "lifting the finger opened nothing"
    );

    launcher
        .view()
        .events()
        .menu_item_chosen(NotificationMenuItem::NotificationSettings);
    let frame = launcher.frame();
    assert_eq!(frame.events_menu, None);
    assert_eq!(
        *fakes.notifications.app_settings_opened.lock().unwrap(),
        [(ProfileId(0), "mail".to_owned())]
    );

    hold(&mut launcher, 0, clock + 2_000);
    launcher
        .view()
        .events()
        .menu_item_chosen(NotificationMenuItem::Dismiss);
    launcher.frame();
    assert_eq!(*fakes.notifications.dismissed.lock().unwrap(), ["m1"]);
    launcher.dispatch(ShellIntent::NotificationsChanged);
    let (_, rows) = events_rows(&mut launcher);
    assert_eq!(rows, [("android".into(), false, 1, "Title".into())]);
}

#[test]
fn a11y05_a_screen_reader_opens_a_notifications_menu_without_a_gesture() {
    // Issue #20: A11Y-05. Back closes it (HOME-08).
    let fakes = Fakes::after_setup();
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![notification("c1", "chat")];
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    let (revision, _) = events_rows(&mut launcher);
    let target = launcher
        .view()
        .events()
        .row_target(revision, 0)
        .expect("the row shows");

    launcher.dispatch(ShellIntent::OpenNotificationMenu(target));
    let frame = launcher.frame();

    assert!(frame.events_menu.is_some());
    assert!(!frame.menu_offered);
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 0);
    launcher.dispatch(ShellIntent::Back);
    assert_eq!(launcher.frame().events_menu, None);
}

/// Notification access granted, but reading the notifications panics: a bug in Events.
struct BrokenListener;

impl NotificationsAdapter for BrokenListener {
    fn access(&self) -> Access {
        Access::Granted
    }

    fn active(&self) -> Result<Vec<Notification>, DataError> {
        panic!("listener bug");
    }

    fn open(&self, _key: &str, _from: Option<Rect>) -> Result<(), DataError> {
        Ok(())
    }

    fn dismiss(&self, _keys: &[String]) -> Result<(), DataError> {
        Ok(())
    }

    fn open_access_settings(&self) -> Result<(), DataError> {
        Ok(())
    }

    fn open_app_settings(&self, _profile: ProfileId, _package: &str) -> Result<(), DataError> {
        Ok(())
    }
}

#[test]
fn rel01_given_events_fails_then_nothing_shows_below_its_header_and_applications_works() {
    // Issue #19: REL-01, ARCH-09.
    let fakes = Fakes::after_setup();
    fakes.apps.apps.lock().unwrap().push(app("mail", "Mail"));
    let mut platform = fakes.platform();
    platform.notifications = std::sync::Arc::new(BrokenListener);
    let mut launcher = Launcher::start(platform);
    launcher.dispatch(ShellIntent::Layout(phone()));
    launcher.dispatch(ShellIntent::NotificationsChanged);

    let frame = launcher.frame();
    assert_eq!(frame.events_list, None);
    assert_eq!(frame.events_header, None);

    launcher.view().apps().icon_tapped(app_id("mail"), None);
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
}

#[test]
fn evt13_given_notifications_showing_then_their_content_is_never_stored_or_printed() {
    // Issue #19: EVT-13, PRIV-03. Nothing on disk, and nothing a log could print: the state, the
    // frame and the rows leave it out of their debug output.
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("chat", "Chat")];
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![Notification {
        title: "Robin".into(),
        text: "The code is 482913".into(),
        ..notification("c1", "chat")
    }];
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    let frame = launcher.frame();
    assert_eq!(
        launcher.view().events().rows()[0].text,
        "The code is 482913"
    );
    // The layout and settings are saved, as they would be on any day.
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().settings().wrap_around_toggled(false);
    launcher.frame();
    assert!(!fakes.storage.writes.lock().unwrap().is_empty());

    let printed = format!(
        "{:?} {:?} {:?} {:?}",
        launcher.shell().events().state(),
        frame,
        launcher.view().events().rows(),
        launcher.shell().saved(),
    );
    let stored = fakes.storage.stored_text();
    for private in ["Robin", "482913"] {
        assert!(!printed.contains(private), "{private:?} in {printed}");
        assert!(!stored.contains(private), "{private:?} in {stored}");
    }
}

/// Sunday 15 November 2026, 00:00 UTC: the day of the SRS's layout mock.
const SUNDAY_MS: u64 = 1_794_700_800_000;
/// An hour, in ms.
const HOUR_MS: u64 = 60 * 60 * 1000;

/// The Events header as the host draws it after a frame: its lines, the alarm's last if it
/// shows, and its revision.
fn events_header(launcher: &mut Launcher) -> (Vec<String>, u32) {
    let revision = launcher.frame().events_header.expect("Events works");
    let header = launcher.view().events().header();
    let mut lines = vec![header.weekday.clone(), header.date.clone()];
    lines.extend(header.alarm.clone());
    (lines, revision)
}

#[test]
fn evt01_given_the_date_changes_at_midnight_or_the_time_zone_changes_then_the_header_updates() {
    // Issue #18: EVT-01. The host passes on the system's date, time and time-zone broadcasts.
    let fakes = Fakes::after_setup();
    *fakes.system.now_ms.lock().unwrap() = SUNDAY_MS + DAY_MS - 60_000;
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    let (lines, before) = events_header(&mut launcher);
    assert_eq!(lines, ["Sunday", "15 November"]);

    *fakes.system.now_ms.lock().unwrap() = SUNDAY_MS + DAY_MS;
    launcher.dispatch(ShellIntent::ClockChanged);
    let (lines, midnight) = events_header(&mut launcher);
    assert_eq!(lines, ["Monday", "16 November"]);
    assert_ne!(midnight, before, "the host reads the header again");

    // A time zone five hours west, where it is still Sunday evening.
    *fakes.system.utc_offset_ms.lock().unwrap() = -5 * HOUR_MS as i64;
    launcher.dispatch(ShellIntent::ClockChanged);
    let (lines, west) = events_header(&mut launcher);
    assert_eq!(lines, ["Sunday", "15 November"]);
    assert_ne!(west, midnight);

    // When nothing changed, the host has nothing to read again.
    launcher.dispatch(ShellIntent::ClockChanged);
    assert_eq!(events_header(&mut launcher).1, west);
}

#[test]
fn evt01_the_header_shows_the_date_without_notification_access() {
    // Issue #18: EVT-01, EVT-03.
    let fakes = Fakes::after_setup();
    *fakes.system.now_ms.lock().unwrap() = SUNDAY_MS + 9 * HOUR_MS;
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));

    assert_eq!(launcher.shell().events().state().access, Access::Missing);
    assert_eq!(events_header(&mut launcher).0, ["Sunday", "15 November"]);
}

#[test]
fn evt02_given_an_alarm_at_7_on_a_24_hour_device_then_the_header_shows_07_00_and_opens_its_app() {
    // Issue #18: EVT-02, L10N-03. The alarm is set on Sunday evening for Monday morning.
    let fakes = Fakes::after_setup();
    *fakes.system.hour_24.lock().unwrap() = true;
    *fakes.system.now_ms.lock().unwrap() = SUNDAY_MS + 22 * HOUR_MS;
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert_eq!(events_header(&mut launcher).0, ["Sunday", "15 November"]);

    *fakes.system.next_alarm_ms.lock().unwrap() = Some(SUNDAY_MS + DAY_MS + 7 * HOUR_MS);
    launcher.dispatch(ShellIntent::ClockChanged);
    assert_eq!(
        events_header(&mut launcher).0,
        ["Sunday", "15 November", "07:00"]
    );

    launcher.view().events().alarm_tapped();
    launcher.frame();
    assert_eq!(*fakes.system.alarm_opened.lock().unwrap(), 1);
    assert_eq!(launcher.shell().events().state().problem, None);
}

#[test]
fn evt02_without_a_next_alarm_the_alarm_row_is_hidden() {
    // Issue #18: EVT-02, SRS 8.1. The alarm rings at 07:00 on a 12-hour device, then is turned
    // off.
    let fakes = Fakes::after_setup();
    *fakes.system.next_alarm_ms.lock().unwrap() = Some(SUNDAY_MS + 7 * HOUR_MS);
    *fakes.system.now_ms.lock().unwrap() = SUNDAY_MS;
    let (mut launcher, _) = on_a_phone(&fakes);
    assert_eq!(
        events_header(&mut launcher).0,
        ["Sunday", "15 November", "7:00 AM"]
    );

    *fakes.system.next_alarm_ms.lock().unwrap() = None;
    launcher.dispatch(ShellIntent::ClockChanged);
    assert_eq!(events_header(&mut launcher).0, ["Sunday", "15 November"]);
    // A tap that raced the change opens nothing.
    launcher.view().events().alarm_tapped();
    launcher.frame();
    assert_eq!(*fakes.system.alarm_opened.lock().unwrap(), 0);
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    *fakes.usage.uses.lock().unwrap() = vec![used("mail", 9_940_000)];
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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

const FIXED: [AppMenuItem; 4] = [
    AppMenuItem::AppInfo,
    AppMenuItem::Uninstall,
    AppMenuItem::Hide,
    AppMenuItem::EditApplications,
];

#[test]
fn apps11_given_a_system_app_when_its_menu_opens_then_it_has_no_uninstall_item() {
    // Issue #12, APPS-13, ANIM-24
    let fakes = Fakes::after_setup();
    mail_and_settings(&fakes);
    let (mut launcher, mut clock) = on_a_phone(&fakes);

    assert!(long_press_cell(&mut launcher, &mut clock, 1, 400.0, 300.0));
    assert_eq!(
        app_menu(&mut launcher),
        [
            AppMenuItem::AppInfo,
            AppMenuItem::Hide,
            AppMenuItem::EditApplications
        ]
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
    let fakes = Fakes::after_setup();
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
    assert_eq!(items[..4], FIXED);
    assert_eq!(items[4..], [AppMenuItem::Shortcut(compose.clone())]);
    finger(&mut launcher, &mut clock, PointerKind::Up, 150.0, 300.0);
    choose(&mut launcher, 4);
    assert_eq!(*fakes.apps.shortcuts_launched.lock().unwrap(), [compose]);
    assert_eq!(launcher.frame().apps.menu, None);
}

#[test]
fn apps11_given_a_long_press_followed_by_a_drag_then_the_icon_lifts_and_edit_mode_starts() {
    // Issues #12 to #14. With the grid not laid out yet, the icon's centre is the finger, and
    // it takes no new place: dropped, it is back in its cell at once.
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
        let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
        let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    let fakes = Fakes::after_setup();
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
    // Issue #32: the whole screen cross-fades to Dark, done within 300 ms (ANIM-19).
    assert!(frame.theme_change < 10_000);
    let switched = clock;
    come_to_rest(&mut launcher, &mut clock);
    assert!(clock - switched <= 300, "faded in {} ms", clock - switched);
    let frame = launcher.frame();
    assert_eq!(frame.theme_change, 10_000);
    assert!(frame.folder.expect("still open").open);
    assert_eq!(frame.apps.scroll_px, 120);
}

#[test]
fn theme01_settings_offers_system_light_and_dark_and_the_screen_takes_their_colours() {
    // Issue #32: THEME-01, THEME-02, THEME-04, SET-03, AC-06.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::OpenSettings);
    let frame = launcher.frame();
    assert_eq!(frame.theme_choice, Some(ThemeChoice::System));
    assert_eq!(frame.background, tokens::LIGHT.background);

    launcher.view().settings().theme_chosen(ThemeChoice::Dark);
    let frame = launcher.frame();
    assert_eq!(frame.theme_choice, Some(ThemeChoice::Dark));
    assert!(
        frame.animating && frame.theme_change < 10_000,
        "it cross-fades"
    );
    // True black home views (THEME-04), and every colour from the Dark tokens (SRS 5.3).
    assert_eq!(frame.background, Rgb::hex(0x000000));
    let dark = tokens::DARK;
    assert_eq!(
        (
            frame.surface,
            frame.foreground,
            frame.dim,
            frame.divider,
            frame.accent
        ),
        (
            dark.surface,
            dark.text_primary,
            dark.text_secondary,
            dark.divider,
            dark.accent
        )
    );
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(launcher.frame().theme_change, 10_000);

    // Light stays light while the device is dark (AC-06)…
    *fakes.system.dark.lock().unwrap() = true;
    launcher.view().settings().theme_chosen(ThemeChoice::Light);
    assert_eq!(launcher.frame().background, tokens::LIGHT.background);
    launcher.dispatch(ShellIntent::SystemThemeChanged);
    assert_eq!(launcher.frame().background, tokens::LIGHT.background);
    // …and System follows the device again.
    launcher.view().settings().theme_chosen(ThemeChoice::System);
    assert_eq!(launcher.frame().background, tokens::DARK.background);
    assert!(
        launcher.shell().state().settings_open,
        "Settings stays open"
    );
}

#[test]
fn theme06_a_theme_chosen_in_settings_keeps_edit_mode_and_the_scroll_position_below_it() {
    // Issue #32: THEME-06, SET-03.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    launcher.view().apps().scrolled(Surface::Grid, 600.0);
    long_press(&mut launcher, &mut clock, 540.0, 1800.0);
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.frame();

    launcher.view().settings().theme_chosen(ThemeChoice::Dark);
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);
    assert!(launcher.shell().state().settings_open);

    launcher.dispatch(ShellIntent::Back);
    let frame = launcher.frame();
    assert_eq!(frame.current, HomeView::Applications);
    assert!(frame.edit.on);
    assert_eq!(frame.apps.scroll_px, 600);
}

#[test]
fn theme06_the_device_turning_dark_keeps_search_open_with_its_query_and_results() {
    // Issue #32: THEME-02, THEME-06.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    open_search(&mut launcher, &mut clock);
    type_in(&mut launcher, "ma");
    let found = results(&launcher);
    assert!(!found.is_empty());

    *fakes.system.dark.lock().unwrap() = true;
    launcher.dispatch(ShellIntent::SystemThemeChanged);
    come_to_rest(&mut launcher, &mut clock);

    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);
    let search = launcher.frame().search;
    assert!(search.open && search.results_shown);
    assert_eq!(launcher.view().apps().query(), Some("ma"));
    assert_eq!(results(&launcher), found);
}

#[test]
fn theme06_the_device_turning_dark_keeps_recent_showing_in_close_mode() {
    // Issue #32: THEME-02, THEME-06.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    assert!(recent_frame(&mut launcher).close.on);

    *fakes.system.dark.lock().unwrap() = true;
    launcher.dispatch(ShellIntent::SystemThemeChanged);
    come_to_rest(&mut launcher, &mut clock);

    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);
    assert_eq!(launcher.shell().state().current, HomeView::Recent);
    assert!(recent_frame(&mut launcher).close.on);
    assert_eq!(recent_cards(&mut launcher).1.len(), 6);
}

/// The search field's band on that phone: 176 px tall, below the 82 px status bar.
const SEARCH_BAND: Bounds = Bounds {
    left: 0.0,
    top: 82.0,
    right: WIDTH,
    bottom: 258.0,
};

/// Gmail, Mail, Maps and Café, on a phone with the grid and search's results laid out.
fn ready_to_search(fakes: &Fakes) -> (Launcher, u64) {
    *fakes.apps.apps.lock().unwrap() = vec![
        app("gmail", "Gmail"),
        app("mail", "Mail"),
        app("maps", "Maps"),
        app("cafe", "Café"),
    ];
    let (mut launcher, clock) = on_a_phone(fakes);
    grid_on_a_phone(&mut launcher);
    launcher.view().apps().search_laid_out(
        GridLayout {
            top_px: SEARCH_BAND.bottom,
            ..PHONE_GRID
        },
        SEARCH_BAND,
    );
    launcher.frame();
    (launcher, clock)
}

/// A finger drags down from `from` to `to` px over 320 ms in the middle of the screen, rests
/// and lifts; then the display draws frames until nothing moves. Returns whether the shell had
/// the gesture, so the grid got none of it.
fn pull(launcher: &mut Launcher, clock: &mut u64, from: f64, to: f64) -> bool {
    let mut taken = false;
    let mut touch = |launcher: &mut Launcher, kind, y, at| {
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x: 540.0,
            y,
            at: Duration::from_millis(at),
        }));
        taken |= launcher.frame().search.pulling;
    };
    touch(launcher, PointerKind::Down, from, *clock);
    for step in 1..=40u32 {
        let y = from + (to - from) * f64::from(step) / 40.0;
        touch(launcher, PointerKind::Move, y, *clock + u64::from(step) * 8);
    }
    *clock += 320 + 100;
    touch(launcher, PointerKind::Up, to, *clock);
    come_to_rest(launcher, clock);
    taken
}

/// Pulls the search field open, past its whole band.
fn open_search(launcher: &mut Launcher, clock: &mut u64) {
    pull(launcher, clock, 600.0, 600.0 + 22.0 + 176.0 + 60.0);
}

/// Types `query` one character at a time, as the host sends each change of the field.
fn type_in(launcher: &mut Launcher, query: &str) {
    let mut typed = String::new();
    for character in query.chars() {
        typed.push(character);
        launcher.view().apps().search_changed(typed.clone());
        launcher.frame();
    }
}

/// Search's results as the host draws them.
fn results(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .apps()
        .cells(Surface::Search)
        .map(|cell| cell.label.to_owned())
        .collect()
}

#[test]
fn srch01_pulling_down_at_the_top_of_applications_reveals_search() {
    // Issue #15: SRCH-01, ANIM-16. The field shows in step with the pull, and past its whole
    // band it springs open: the field takes the keyboard.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_search(&fakes);

    let taken = pull(&mut launcher, &mut clock, 600.0, 600.0 + 22.0 + 100.0);
    assert!(taken);
    assert!(!launcher.frame().search.open);
    assert_eq!(launcher.frame().search.reveal_px, 0);

    open_search(&mut launcher, &mut clock);
    let search = launcher.frame().search;
    assert!(search.open && !search.results_shown);
    assert_eq!(search.reveal_px, 176);
    assert_eq!(launcher.view().apps().query(), Some(""));
}

#[test]
fn srch01_a_pull_counts_only_at_the_top_of_the_grid() {
    // Issue #15: SRCH-01. Further down, the grid scrolls instead.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    launcher.view().apps().scrolled(Surface::Grid, 300.0);

    let taken = pull(&mut launcher, &mut clock, 600.0, 1200.0);

    assert!(!taken);
    assert!(!launcher.frame().search.open);
}

#[test]
fn srch02_given_mail_maps_and_gmail_when_the_user_types_ma_then_mail_and_maps_come_before_gmail() {
    // Issue #15: SRCH-02, the issue's first acceptance criterion. Matches at a word's start
    // come first, each group alphabetically.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    open_search(&mut launcher, &mut clock);

    type_in(&mut launcher, "m");
    assert_eq!(results(&launcher), ["Mail", "Maps", "Gmail"]);
    type_in(&mut launcher, "ma");

    assert_eq!(results(&launcher), ["Mail", "Maps", "Gmail"]);
    let frame = launcher.frame();
    assert!(frame.search.results_shown && !frame.search.nothing_found);
    assert_eq!(frame.apps.results.rows, 1);
}

#[test]
fn srch02_given_an_app_labelled_cafe_when_the_user_types_cafe_then_it_matches() {
    // Issue #15: SRCH-02, L10N-04, the issue's second acceptance criterion: case and accents
    // are folded by the platform, here the fake's stand-in.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    open_search(&mut launcher, &mut clock);

    type_in(&mut launcher, "cafe");
    assert_eq!(results(&launcher), ["Café"]);
    type_in(&mut launcher, "CAFÉ");
    assert_eq!(results(&launcher), ["Café"]);
}

#[test]
fn srch03_given_results_when_the_user_presses_the_action_key_then_the_first_result_launches() {
    // Issue #15: SRCH-03, the issue's third acceptance criterion, from the result's icon
    // (ANIM-08). Coming back shows the grid, not the old query (SRCH-04).
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    open_search(&mut launcher, &mut clock);
    type_in(&mut launcher, "ma");
    let icon = Rect {
        left: 44,
        top: 280,
        right: 198,
        bottom: 434,
    };

    launcher.view().apps().search_submitted(Some(icon));
    launcher.frame();

    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
    assert_eq!(*fakes.apps.launched_from.lock().unwrap(), [Some(icon)]);
    assert!(!launcher.frame().search.open);
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(launcher.frame().search.reveal_px, 0);
    assert_eq!(launcher.view().apps().query(), None);
}

#[test]
fn srch04_back_or_home_close_search_and_restore_the_grid_where_it_was() {
    // Issue #15: SRCH-04, HOME-07, HOME-08. A screen reader opens search anywhere in the grid
    // (A11Y-05); closing it leaves the grid where it was.
    for close in [ShellIntent::Back, ShellIntent::Home] {
        let fakes = Fakes::after_setup();
        let (mut launcher, mut clock) = ready_to_search(&fakes);
        launcher.view().apps().scrolled(Surface::Grid, 300.0);
        assert!(launcher.frame().search.offered);
        launcher.view().apps().open_search();
        type_in(&mut launcher, "ma");
        assert!(launcher.frame().search.results_shown);

        launcher.dispatch(close);

        let frame = launcher.frame();
        assert!(!frame.search.open && !frame.search.results_shown);
        assert_eq!(frame.apps.scroll_px, 300);
        assert_eq!(frame.current, HomeView::Applications);
        come_to_rest(&mut launcher, &mut clock);
        assert_eq!(launcher.frame().search.reveal_px, 0);
    }
}

#[test]
fn srch04_pushing_the_empty_field_away_closes_search() {
    // Issue #15: SRCH-04, "scrolling down past the field".
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    open_search(&mut launcher, &mut clock);

    let taken = pull(
        &mut launcher,
        &mut clock,
        1200.0,
        1200.0 - 22.0 - 176.0 - 40.0,
    );

    assert!(taken);
    assert!(!launcher.frame().search.open);
    assert_eq!(launcher.frame().search.reveal_px, 0);
}

#[test]
fn srch05_given_no_match_then_a_web_search_is_offered_and_runs() {
    // Issue #15: SRCH-05, with the device's default search handler.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    open_search(&mut launcher, &mut clock);
    type_in(&mut launcher, "zebra");

    let search = launcher.frame().search;
    assert!(search.nothing_found && search.web_offered);
    launcher.view().apps().search_web();
    launcher.frame();

    assert_eq!(*fakes.system.web_searches.lock().unwrap(), ["zebra"]);
    assert!(!launcher.frame().search.open);
}

#[test]
fn srch05_without_a_browser_nothing_is_offered() {
    // Issue #15: SRCH-05, SRS 8.1: "has_browser is false: link hidden".
    let fakes = Fakes::after_setup();
    *fakes.system.browser.lock().unwrap() = false;
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    open_search(&mut launcher, &mut clock);
    type_in(&mut launcher, "zebra");

    let search = launcher.frame().search;
    assert!(search.nothing_found && !search.web_offered);
    launcher.view().apps().search_submitted(None);
    launcher.frame();
    assert!(fakes.system.web_searches.lock().unwrap().is_empty());
}

/// Gmail, Mail, Maps and Café ready to search, with usage access: Maps was used, then Mail.
fn ready_to_hide(fakes: &Fakes) -> (Launcher, u64) {
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.system.now_ms.lock().unwrap() = 1_000_000;
    *fakes.usage.uses.lock().unwrap() = [("maps", 900_000), ("mail", 999_000)]
        .iter()
        .map(|(package, at)| used(package, *at))
        .collect();
    ready_to_search(fakes)
}

/// Recent's cards, newest first.
fn recent(launcher: &Launcher) -> Vec<String> {
    let state = launcher.shell().recent().state();
    state
        .cards
        .iter()
        .map(|card| card.app.label.clone())
        .collect()
}

/// `package`'s menu opens, as a screen reader opens it, and Hide is tapped.
fn hide(launcher: &mut Launcher, package: &str) {
    launcher.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app_id(package))));
    let items = app_menu(launcher);
    let hide = items.iter().position(|item| *item == AppMenuItem::Hide);
    choose(launcher, hide.expect("the menu offers Hide"));
}

/// Settings → Applications → Hidden apps, as the host lists it.
fn hidden_apps(launcher: &Launcher) -> Vec<String> {
    let section = launcher.view().hidden_apps();
    section.apps().iter().map(|app| app.label.clone()).collect()
}

#[test]
fn apps19_given_an_app_is_hidden_then_it_doesnt_appear_in_the_grid_search_or_recent() {
    // Issue #16: APPS-19, REC-03. Mail is hidden from its long-press menu.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = ready_to_hide(&fakes);
    assert_eq!(grid(&launcher), ["Café", "Gmail", "Mail", "Maps"]);
    assert_eq!(recent(&launcher), ["Mail", "Maps"]);

    hide(&mut launcher, "mail");

    assert_eq!(launcher.frame().apps.menu, None);
    assert_eq!(grid(&launcher), ["Café", "Gmail", "Maps"]);
    // Recent leaves it out at once, before the user even swipes to it.
    assert_eq!(recent(&launcher), ["Maps"]);
    open_search(&mut launcher, &mut clock);
    type_in(&mut launcher, "ma");
    assert_eq!(results(&launcher), ["Maps", "Gmail"]);
}

#[test]
fn apps19_given_a_hidden_app_is_unhidden_in_settings_then_it_returns_to_the_grid() {
    // Issue #16: APPS-19, SET-02, SET-03. It comes back in its old place, at once.
    let fakes = Fakes::after_setup();
    let (mut launcher, _) = ready_to_hide(&fakes);
    hide(&mut launcher, "mail");

    launcher.dispatch(ShellIntent::OpenSettings);
    let revision = launcher.frame().hidden_apps.expect("Settings offers it");
    assert_eq!(hidden_apps(&launcher), ["Mail"]);
    assert!(launcher.view().hidden_apps().unhide_tapped(revision, 0));
    let frame = launcher.frame();

    assert!(frame.settings_open);
    assert_ne!(frame.hidden_apps, Some(revision));
    assert!(hidden_apps(&launcher).is_empty());
    assert_eq!(grid(&launcher), ["Café", "Gmail", "Mail", "Maps"]);
    assert_eq!(recent(&launcher), ["Mail", "Maps"]);
    // A tap on the list as it was does nothing.
    assert!(!launcher.view().hidden_apps().unhide_tapped(revision, 0));
}

/// A day, in ms.
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// The backup of a user's old device (APPS-21): Phone, a Games folder with Chess and Go, Maps,
/// Bank, which they hid, and Mail, in the dark theme. MeeGone was installed there at 1 000 ms.
fn backed_up_home() -> BTreeMap<StoredFile, Vec<u8>> {
    let old = Fakes::after_setup();
    *old.system.installed_ms.lock().unwrap() = 1_000;
    *old.apps.apps.lock().unwrap() = vec![
        app("phone", "Phone"),
        app("chess", "Chess"),
        app("go", "Go"),
        app("maps", "Maps"),
        app("bank", "Bank"),
        app("mail", "Mail"),
    ];
    saved_layout(
        &old,
        "app 0 phone phone.Main\n\
         folder 1 Games 2 0 chess chess.Main 0 go go.Main\n\
         app 0 maps maps.Main\n\
         app 0 bank bank.Main\n\
         app 0 mail mail.Main\n",
    );
    let (mut launcher, _) = on_a_phone(&old);
    hide(&mut launcher, "bank");
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().settings().theme_chosen(ThemeChoice::Dark);
    launcher.frame();
    assert_eq!(grid(&launcher), ["Phone", "Games", "Maps", "Mail"]);
    backup::back_up(&old.storage)
}

/// A new device with `installed` apps, where the system restores `backup` as it installs
/// MeeGone, at 5 000 ms, before MeeGone first runs (APPS-21).
fn new_device(backup: &BTreeMap<StoredFile, Vec<u8>>, installed: Vec<App>) -> Fakes {
    let new = Fakes::after_setup();
    *new.system.installed_ms.lock().unwrap() = 5_000;
    *new.system.now_ms.lock().unwrap() = 6_000;
    *new.apps.apps.lock().unwrap() = installed;
    backup::restore(&new.storage, backup);
    new
}

#[test]
fn apps21_given_a_backup_restored_on_another_device_then_the_home_matches_minus_apps_not_installed()
{
    // Issue #17: APPS-21. Chess and Maps aren't installed yet: they are skipped. Notes, which the
    // backup doesn't hold, joins the end, without a dot.
    let backup = backed_up_home();
    let new = new_device(
        &backup,
        vec![
            app("phone", "Phone"),
            app("go", "Go"),
            app("bank", "Bank"),
            app("mail", "Mail"),
            app("notes", "Notes"),
        ],
    );

    let (mut launcher, _) = on_a_phone(&new);

    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);
    assert_eq!(grid(&launcher), ["Phone", "Games", "Mail", "Notes"]);
    assert_eq!(dots(&mut launcher), [false; 4]);
    open_folder(&mut launcher, 1);
    assert_eq!(in_folder(&launcher), ["Go"]);
    launcher.dispatch(ShellIntent::Back);
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.frame();
    assert_eq!(hidden_apps(&launcher), ["Bank"]);
    launcher.dispatch(ShellIntent::Back);

    // As the system installs the rest, each takes its old place, in its folder too.
    new.apps
        .apps
        .lock()
        .unwrap()
        .extend([app("maps", "Maps"), app("chess", "Chess")]);
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_eq!(grid(&launcher), ["Phone", "Games", "Maps", "Mail", "Notes"]);
    assert_eq!(dots(&mut launcher), [false; 5]);
    open_folder(&mut launcher, 1);
    assert_eq!(in_folder(&launcher), ["Chess", "Go"]);
}

#[test]
fn apps21_given_a_backed_up_app_comes_after_a_week_then_it_is_new_at_the_end() {
    // Issue #17: APPS-21, APPS-06. A week after the restore, MeeGone stops keeping Maps' place.
    let backup = backed_up_home();
    let new = new_device(
        &backup,
        vec![
            app("phone", "Phone"),
            app("chess", "Chess"),
            app("go", "Go"),
            app("bank", "Bank"),
            app("mail", "Mail"),
        ],
    );
    let (mut launcher, _) = on_a_phone(&new);
    assert_eq!(grid(&launcher), ["Phone", "Games", "Mail"]);

    *new.system.now_ms.lock().unwrap() += 7 * DAY_MS;
    launcher.dispatch(ShellIntent::AppsChanged);
    new.apps.apps.lock().unwrap().push(app("maps", "Maps"));
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_eq!(grid(&launcher), ["Phone", "Games", "Mail", "Maps"]);
    assert_eq!(dots(&mut launcher), [false, false, false, true]);

    // After a restart, the layout is this device's own: an app uninstalled meanwhile is gone,
    // and its folder turns back into Chess (APPS-08, FOLD-06).
    drop(launcher);
    new.apps
        .apps
        .lock()
        .unwrap()
        .retain(|app| app.id != app_id("go"));
    let restarted = Launcher::start(new.platform());
    assert_eq!(grid(&restarted), ["Phone", "Chess", "Mail", "Maps"]);
}

/// The image cache holding today's image, Puffins on a cliff, in its first file, as the daily
/// image's updater leaves it (WALL-09, ADR 0022).
fn image_cached(fakes: &Fakes) {
    image_cached_with_link(fakes, None);
}

/// [`image_cached`], with `info_link` as the image's link to more about it (WALL-15).
fn image_cached_with_link(fakes: &Fakes, info_link: Option<&str>) {
    let link = info_link.unwrap_or("-");
    let mut files = fakes.storage.files.lock().unwrap();
    files.insert(
        StoredFile::ImageDetails,
        seal(
            "image-cache",
            2,
            &format!(
                "current 20261116 en-GB Puffins\\son\\sa\\scliff,\\sSkomer \
                 ©\\sA\\sPhotographer\\s/\\sAgency {link} 1763272800000 a\nprevious -\n"
            ),
        ),
    );
    files.insert(StoredFile::ImageA, b"\xff\xd8 a JPEG".to_vec());
}

/// MeeGone with today's image cached, its daily image turned on in Settings, showing Events.
fn with_the_daily_image(fakes: &Fakes) -> Launcher {
    image_cached(fakes);
    daily_image_on(fakes)
}

/// MeeGone with its daily image turned on in Settings, showing Events.
fn daily_image_on(fakes: &Fakes) -> Launcher {
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().image_settings().daily_image_toggled(true);
    // The switch takes effect with the frame, as on the device.
    launcher.frame();
    launcher.dispatch(ShellIntent::Back);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    launcher
}

/// What the image-of-the-day card shows after a frame, or `None` while it doesn't.
fn image_card(launcher: &mut Launcher) -> Option<CardContent> {
    launcher.frame().image_card?;
    launcher.view().image_card().content()
}

/// Taps the image-of-the-day card, which opens the image details screen (EVT-12).
fn tap_the_image_card(launcher: &mut Launcher) {
    let revision = launcher.frame().image_card.expect("the card shows");
    assert!(launcher.view().image_card().tapped(revision));
}

/// What the image details screen shows after a frame, or `None` while it doesn't (WALL-15).
fn image_details(launcher: &mut Launcher) -> Option<DetailsContent> {
    launcher.frame().image_details?;
    launcher.view().image_details().content()
}

#[test]
fn evt12_given_an_image_is_cached_then_events_shows_its_card_offline_and_without_access() {
    // Issue #21: EVT-12, LEGAL-03, EVT-03, REL-06. The network is offline, and MeeGone has no
    // notification access.
    let fakes = Fakes::after_setup();
    let mut launcher = with_the_daily_image(&fakes);

    assert_eq!(
        image_card(&mut launcher),
        Some(CardContent {
            title: "Puffins on a cliff, Skomer".into(),
            attribution: "© A Photographer / Agency".into(),
            image_path: "images/a",
            image_key: "20261116 en-GB images/a".into(),
        })
    );
    // Below the notification-access card, which still asks (EVT-03).
    assert_eq!(
        events_list(&mut launcher),
        (EventsListMode::Explainer, Vec::new())
    );
    // The thumbnail is the cached file's; nothing was fetched.
    assert!(fakes.network.requests.lock().unwrap().is_empty());
    assert_eq!(
        fakes.storage.files.lock().unwrap()[&StoredFile::ImageA],
        b"\xff\xd8 a JPEG"
    );
}

#[test]
fn evt12_given_show_card_in_events_is_off_then_the_card_isnt_shown() {
    // Issue #21: EVT-12, SET-03. The choice takes effect at once, and lasts.
    let fakes = Fakes::after_setup();
    let mut launcher = with_the_daily_image(&fakes);
    assert!(image_card(&mut launcher).is_some());

    launcher.dispatch(ShellIntent::OpenSettings);
    assert_eq!(launcher.frame().show_image_card, Some(true));
    launcher.view().image_settings().show_card_toggled(false);
    assert_eq!(launcher.frame().show_image_card, Some(false));
    launcher.dispatch(ShellIntent::Back);

    assert_eq!(image_card(&mut launcher), None);
    drop(launcher);
    let mut restarted = Launcher::start(fakes.platform());
    restarted.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert_eq!(image_card(&mut restarted), None);

    restarted.view().image_settings().show_card_toggled(true);
    assert!(image_card(&mut restarted).is_some());
}

#[test]
fn evt12_given_the_daily_image_is_off_then_there_is_no_card() {
    // Issue #21: EVT-12. An image left in the cache shows nothing while the feature is off.
    let fakes = Fakes::after_setup();
    image_cached(&fakes);
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));

    assert_eq!(image_card(&mut launcher), None);
    // Its setting is still offered.
    assert_eq!(launcher.frame().show_image_card, Some(true));
}

#[test]
fn evt12_given_the_card_is_tapped_then_the_image_details_open_and_back_closes_them() {
    // Issue #21: EVT-12, WALL-15, HOME-08.
    let fakes = Fakes::after_setup();
    let mut launcher = with_the_daily_image(&fakes);
    let revision = launcher.frame().image_card.expect("the card shows");

    assert!(launcher.view().image_card().tapped(revision));
    let frame = launcher.frame();
    assert!(frame.image_details_open);
    assert!(!frame.menu_offered);

    launcher.dispatch(ShellIntent::Back);
    let frame = launcher.frame();
    assert!(!frame.image_details_open);
    assert_eq!(frame.current, HomeView::Events);
}

#[test]
fn evt12_given_a_new_image_arrives_then_the_card_shows_it_and_its_thumbnail_cross_fades() {
    // Issue #21: EVT-12, ANIM-22. The daily job cached a new image while MeeGone was away; the
    // host decodes it and fades it in over 300 ms.
    let fakes = Fakes::after_setup();
    let mut launcher = with_the_daily_image(&fakes);
    let first = launcher.frame().image_card.expect("the card shows");
    fakes.storage.files.lock().unwrap().insert(
        StoredFile::ImageDetails,
        seal(
            "image-cache",
            2,
            "current 20261117 en-GB Tomorrow ©\\sSomeone - - b\nprevious -\n",
        ),
    );
    fakes
        .storage
        .files
        .lock()
        .unwrap()
        .insert(StoredFile::ImageB, b"\xff\xd8 another JPEG".to_vec());

    launcher.dispatch(ShellIntent::Resumed);
    let frame = launcher.frame();
    let second = frame.image_card.expect("the card still shows");
    assert_ne!(second, first);
    let content = launcher.view().image_card().content().unwrap();
    assert_eq!(content.title, "Tomorrow");
    assert_eq!(content.image_path, "images/b");
    assert_eq!(frame.image_change, 10_000);

    launcher.dispatch(ShellIntent::NewImageShown);
    let mut clock = 9_000;
    launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
    assert_eq!(launcher.frame().image_change, 0);
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(clock, 9_304);
    assert_eq!(launcher.frame().image_change, 10_000);
}

/// 19 November 2026, 00:00 UTC, the daily check's first day.
const NOV_19_MS: u64 = 1_795_046_400_000;
/// Where Bing's feed gives today's image in the device's region, United States English.
const FEED_URL: &str = "https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=en-US";
/// The feed's largest version of that image, the one picked for this phone's lock screen.
const UHD_URL: &str = "https://www.bing.com/th?id=OHR.BarnOwlsUnite_EN-US9661992435_UHD.jpg";

/// MeeGone, on its first day, with its daily image turned on in Settings, and the job its
/// updater shares the stores with (ADR 0029). Its own clock starts at local midnight in UTC.
fn with_the_daily_check(fakes: &Fakes) -> (Shared, Launcher) {
    *fakes.system.now_ms.lock().unwrap() = NOV_19_MS;
    let shared = Shared::new(fakes.platform());
    let mut launcher = Launcher::start_shared(&shared);
    launcher.view().image_settings().daily_image_toggled(true);
    launcher.frame();
    (shared, launcher)
}

/// Plays the system's job scheduler until `until_ms`: whenever MeeGone's one job is due, the
/// clock moves to it and the job runs, then the home hears it checked (ARCH-07). It runs the job
/// even where the system would wait for its network, as MeeGone's start may. Returns how many
/// times it ran.
fn run_the_daily_job(
    fakes: &Fakes,
    shared: &Shared,
    launcher: &mut Launcher,
    until_ms: u64,
) -> u32 {
    let mut runs = 0;
    loop {
        let now = *fakes.system.now_ms.lock().unwrap();
        let Some(job) = *fakes.scheduler.job.lock().unwrap() else {
            return runs;
        };
        let at = job.not_before_ms.max(now);
        if at >= until_ms {
            return runs;
        }
        *fakes.system.now_ms.lock().unwrap() = at;
        if shared.check_image() {
            launcher.dispatch(ShellIntent::ImageChecked);
        }
        runs += 1;
    }
}

#[test]
fn ac16_given_no_connection_the_daily_check_keeps_the_wallpaper_says_it_will_try_again_and_makes_at_most_6_attempts()
 {
    // AC-16, issue #28: WALL-08, WALL-16, REL-04. Today's image is already cached; then the
    // connection goes for the whole day.
    let fakes = Fakes::after_setup();
    image_cached(&fakes);
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    let cached = fakes.storage.files.lock().unwrap()[&StoredFile::ImageA].clone();

    let runs = run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 24 * HOUR_MS);

    // Six attempts, each one request to the feed, backing off 15, 30, 60, 120 and 240 minutes.
    assert_eq!(runs, 6);
    assert_eq!(*fakes.network.requests.lock().unwrap(), [FEED_URL; 6]);
    // The lock-screen image and its wallpaper are as they were (WALL-08).
    assert_eq!(
        fakes.storage.files.lock().unwrap()[&StoredFile::ImageA],
        cached
    );
    assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 0);
    // Settings says why, and that it will try again (WALL-16).
    let settings = launcher.view().image_settings();
    assert_eq!(
        settings.checks(),
        CheckStatus::Retrying("No connection".into())
    );
    assert!(!settings.daily_image_unavailable());
    // The next attempt is the next day's check, after local midnight (WALL-03).
    assert_eq!(
        fakes
            .scheduler
            .job
            .lock()
            .unwrap()
            .map(|job| job.not_before_ms),
        Some(NOV_19_MS + 24 * HOUR_MS)
    );
}

#[test]
fn srs7_6_after_3_days_of_failures_settings_says_daily_image_unavailable_with_try_again() {
    // Issue #28, SRS 7.6: the connection is gone for three days, then comes back.
    let fakes = Fakes::after_setup();
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    let three_days = NOV_19_MS + 72 * HOUR_MS;

    run_the_daily_job(&fakes, &shared, &mut launcher, three_days + HOUR_MS);

    // Six attempts a day, and three in the fourth day's first hour, backing off (REL-04).
    assert_eq!(fakes.network.requests.lock().unwrap().len(), 3 * 6 + 3);
    assert!(launcher.frame().image_unavailable);
    assert!(launcher.view().image_settings().daily_image_unavailable());

    // Try again: the message goes, and the job runs as soon as it may.
    serve_todays_image(&fakes);
    launcher.view().image_settings().try_again();
    assert!(!launcher.frame().image_unavailable);
    run_the_daily_job(&fakes, &shared, &mut launcher, three_days + 24 * HOUR_MS);

    assert_eq!(launcher.view().image_settings().checks(), CheckStatus::Fine);
    assert_eq!(
        fakes
            .network
            .requests
            .lock()
            .unwrap()
            .last()
            .map(String::as_str),
        Some(UHD_URL)
    );
}

/// The network serves Bing's feed with today's image, Barn owls, and that image's largest
/// version, on Wi-Fi.
fn serve_todays_image(fakes: &Fakes) {
    *fakes.network.state.lock().unwrap() = NetworkState::Unmetered;
    let mut responses = fakes.network.responses.lock().unwrap();
    responses.insert(
        FEED_URL.into(),
        include_bytes!("../crates/lockimage/data/fixtures/bing-en-US.json").to_vec(),
    );
    responses.insert(
        UHD_URL.into(),
        fakes::jpeg(Size {
            width: 3840,
            height: 2160,
        }),
    );
}

#[test]
fn wall04_given_mobile_data_only_and_use_mobile_data_off_no_automatic_download_happens() {
    // Issue #28: WALL-04, PWR-04. Only mobile data all day, with "Use mobile data" off.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    *fakes.network.state.lock().unwrap() = NetworkState::Metered;
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    assert_eq!(launcher.frame().use_mobile_data, Some(false));

    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 24 * HOUR_MS);

    // Nothing was asked of the network, and the job waits for an unmetered one.
    assert!(fakes.network.requests.lock().unwrap().is_empty());
    assert_eq!(launcher.view().image_settings().checks(), CheckStatus::Fine);
    assert_eq!(
        fakes.scheduler.job.lock().unwrap().map(|job| job.network),
        Some(JobNetwork::Unmetered)
    );

    // With Data Saver on, turning "Use mobile data" on still downloads nothing (PWR-04).
    *fakes.network.saves_data.lock().unwrap() = true;
    launcher
        .view()
        .image_settings()
        .use_mobile_data_toggled(true);
    assert_eq!(launcher.frame().use_mobile_data, Some(true));
    assert_eq!(
        fakes.scheduler.job.lock().unwrap().map(|job| job.network),
        Some(JobNetwork::Any)
    );
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 25 * HOUR_MS);
    assert!(fakes.network.requests.lock().unwrap().is_empty());

    // Without it, today's image downloads over mobile data, and the lock screen shows it.
    *fakes.network.saves_data.lock().unwrap() = false;
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 26 * HOUR_MS);
    assert_eq!(*fakes.network.requests.lock().unwrap(), [FEED_URL, UHD_URL]);
    assert_eq!(lock_screen(&fakes), cached_image(&fakes));
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert_eq!(
        image_card(&mut launcher).map(|card| card.title),
        Some("Hunting from dusk till dawn".into())
    );
}

#[test]
fn wall03_a_new_image_is_fetched_once_a_day_after_local_midnight() {
    // Issue #28: WALL-03, PWR-03. On Wi-Fi in Bangkok, the image is fetched just after local
    // midnight, and the next day's checks find the same image and download nothing: one image
    // a day, about 4 MB from the feed, and a few small details responses.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    *fakes.system.utc_offset_ms.lock().unwrap() = 7 * HOUR_MS as i64;
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    let bangkok_midnight = NOV_19_MS + 17 * HOUR_MS;

    run_the_daily_job(&fakes, &shared, &mut launcher, bangkok_midnight);
    assert_eq!(*fakes.network.requests.lock().unwrap(), [FEED_URL, UHD_URL]);

    // The next local day: four checks two hours apart find the same image.
    run_the_daily_job(
        &fakes,
        &shared,
        &mut launcher,
        bangkok_midnight + 24 * HOUR_MS,
    );
    let requests = fakes.network.requests.lock().unwrap().clone();
    assert_eq!(requests[2..], [FEED_URL; 4]);
    // Set as the lock-screen wallpaper once, the day it came (ADR 0030).
    assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 1);
    assert_eq!(
        fakes
            .scheduler
            .job
            .lock()
            .unwrap()
            .map(|job| job.not_before_ms),
        Some(bangkok_midnight + 24 * HOUR_MS)
    );
}

/// The lock screen's own wallpaper, or `None` while it shows the home wallpaper.
fn lock_screen(fakes: &Fakes) -> Option<Vec<u8>> {
    fakes.wallpaper.lock_screen.lock().unwrap().clone()
}

/// The image the updater cached first, in the cache's first file.
fn cached_image(fakes: &Fakes) -> Option<Vec<u8>> {
    fakes
        .storage
        .files
        .lock()
        .unwrap()
        .get(&StoredFile::ImageA)
        .cloned()
}

/// Runs what the user asked of the job's thread, such as "Update now", as that thread does,
/// then tells the home (WALL-11, ADR 0030, ADR 0032).
fn run_update_now(shared: &Shared, launcher: &mut Launcher) {
    shared.run_image_tasks();
    launcher.dispatch(ShellIntent::ImageChecked);
}

#[test]
fn ac15_given_the_daily_image_is_on_and_wifi_when_a_new_image_is_published_then_the_lock_screen_shows_it_and_the_details_its_title_and_attribution()
 {
    // AC-15, issue #29: WALL-01, WALL-03, WALL-15.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    assert_eq!(lock_screen(&fakes), None);

    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 24 * HOUR_MS);

    // The lock screen shows the image, cropped for it, and only the lock screen's wallpaper
    // was set (WALL-01): the platform is asked for nothing else.
    assert!(cached_image(&fakes).is_some());
    assert_eq!(lock_screen(&fakes), cached_image(&fakes));
    assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 1);
    let current = launcher
        .shell()
        .lockimage()
        .state()
        .current
        .clone()
        .unwrap();
    assert_eq!(current.applied_at_ms, Some(NOV_19_MS));
    // The details screen opens from the card, and both show its title and attribution
    // (LEGAL-03).
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    let card = image_card(&mut launcher).expect("the card shows");
    assert_eq!(card.title, "Hunting from dusk till dawn");
    assert_eq!(
        card.attribution,
        "Pair of barn owls (© Zahoor Salmi/Getty Images)"
    );
    tap_the_image_card(&mut launcher);
    assert!(launcher.frame().image_details_open);
    let details = image_details(&mut launcher).expect("the details show");
    assert_eq!(details.title, card.title);
    assert_eq!(details.attribution, card.attribution);
    assert_eq!(details.date, "18 November");
    assert_eq!(details.region, "en-US");
    assert!(details.more_info);
    assert_eq!(current.title, card.title);
    assert_eq!(current.attribution, card.attribution);
    // And when it went on the lock screen (WALL-16).
    assert_eq!(
        launcher.view().image_settings().update_status(),
        Some(UpdateStatus {
            updated: Some("19 November, 12:00 AM".into()),
            checks: CheckStatus::Fine,
            pinned: false,
        })
    );
}

#[test]
fn wall15_given_a_cached_image_with_an_info_link_then_more_info_opens_it_in_the_browser_and_without_one_it_is_hidden()
 {
    // Issue #30: WALL-15, SEC-02.
    let link = "https://www.bing.com/search?q=Atlantic+puffin&form=hpcapt";
    let fakes = Fakes::after_setup();
    image_cached_with_link(&fakes, Some(link));
    let mut launcher = daily_image_on(&fakes);
    tap_the_image_card(&mut launcher);

    let details = image_details(&mut launcher).expect("the details show");
    assert!(details.more_info);
    assert!(launcher.view().image_details().more_info());
    let frame = launcher.frame();

    // It opens in the browser; MeeGone stays where it was.
    assert_eq!(*fakes.system.opened_urls.lock().unwrap(), [link]);
    assert!(frame.image_details_open);

    // Without a link, More info is hidden, and nothing opens.
    let fakes = Fakes::after_setup();
    image_cached(&fakes);
    let mut launcher = daily_image_on(&fakes);
    tap_the_image_card(&mut launcher);

    assert!(!image_details(&mut launcher).unwrap().more_info);
    assert!(!launcher.view().image_details().more_info());
    launcher.frame();
    assert!(fakes.system.opened_urls.lock().unwrap().is_empty());

    // Nor without a browser to open it (SRS 8.1).
    let fakes = Fakes::after_setup();
    image_cached_with_link(&fakes, Some(link));
    *fakes.system.browser.lock().unwrap() = false;
    let mut launcher = daily_image_on(&fakes);
    tap_the_image_card(&mut launcher);
    assert!(!image_details(&mut launcher).unwrap().more_info);
}

#[test]
fn wall16_given_the_last_attempt_failed_for_lack_of_connection_then_settings_says_no_connection_will_try_again_later()
 {
    // Issue #30: WALL-16. Today's image went on the lock screen; then the next check found no
    // connection. The host words it: "Last updated 16 November, 6:00 AM" and "No connection.
    // Will try again later."
    let fakes = Fakes::after_setup();
    image_cached(&fakes);
    let (shared, mut launcher) = with_the_daily_check(&fakes);

    assert_eq!(
        run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 1),
        1
    );

    let frame = launcher.frame();
    assert!(frame.image_status.is_some());
    let status = UpdateStatus {
        updated: Some("16 November, 6:00 AM".into()),
        checks: CheckStatus::Retrying("No connection".into()),
        pinned: false,
    };
    assert_eq!(
        launcher.view().image_settings().update_status(),
        Some(status)
    );

    // The details screen says the same.
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    tap_the_image_card(&mut launcher);
    let frame = launcher.frame();
    assert!(frame.image_details.is_some());
    assert!(frame.image_status.is_some());

    // Once the daily image is off, there is no status to show.
    launcher.view().image_settings().daily_image_toggled(false);
    assert_eq!(launcher.frame().image_status, None);
}

#[test]
fn wall15_given_an_image_then_the_home_menus_lock_screen_image_opens_its_details_and_settings_open_over_them()
 {
    // Issue #30: HOME-10, WALL-15, HOME-08. Before the first image, the item opens Settings
    // (home10_in_events_the_home_menu_has_no_edit_applications).
    let fakes = Fakes::after_setup();
    let mut launcher = with_the_daily_image(&fakes);
    launcher.dispatch(ShellIntent::OpenMenu);

    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::LockScreenImage));

    let frame = launcher.frame();
    assert!(frame.image_details_open);
    assert!(!frame.settings_open);
    assert_eq!(
        image_details(&mut launcher),
        Some(DetailsContent {
            title: "Puffins on a cliff, Skomer".into(),
            attribution: "© A Photographer / Agency".into(),
            date: "16 November".into(),
            region: "en-GB".into(),
            image_path: "images/a",
            image_key: "20261116 en-GB images/a".into(),
            more_info: false,
            pinned: false,
        })
    );

    // Settings opens over the details at its Lock-screen image group; Back comes back to them.
    assert!(launcher.view().image_details().open_settings());
    let frame = launcher.frame();
    assert!(frame.settings_open);
    assert_eq!(frame.settings_group, Some(SettingsGroup::LockScreenImage));
    assert!(frame.image_details_open);
    launcher.dispatch(ShellIntent::Back);
    let frame = launcher.frame();
    assert!(!frame.settings_open);
    assert!(frame.image_details_open);

    // Turned off in Settings over them, the details close: Back goes home, not to an empty
    // screen.
    assert!(launcher.view().image_details().open_settings());
    launcher.frame();
    launcher.view().image_settings().daily_image_toggled(false);
    launcher.dispatch(ShellIntent::Back);
    let frame = launcher.frame();
    assert!(!frame.settings_open);
    assert!(!frame.image_details_open);
    assert_eq!(frame.current, HomeView::Events);
}

#[test]
fn ac17_given_the_feature_is_on_when_it_is_turned_off_and_the_lock_screen_reset_then_no_more_downloads_happen_the_images_are_deleted_and_the_lock_screen_shows_the_home_wallpaper()
 {
    // AC-17, issue #29: WALL-12.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + HOUR_MS);
    assert!(lock_screen(&fakes).is_some());
    launcher.dispatch(ShellIntent::OpenSettings);
    assert!(!launcher.frame().image_off_actions);

    launcher.view().image_settings().daily_image_toggled(false);
    // The wallpaper stays until the user resets it; Settings offers the reset and the
    // system's wallpaper settings.
    let frame = launcher.frame();
    assert_eq!(frame.daily_image, Some(DailyImage::Off));
    assert!(frame.image_off_actions);
    assert_eq!(frame.update_now, None);
    assert!(lock_screen(&fakes).is_some());
    launcher.view().image_settings().reset_lock_screen();
    launcher.frame();

    assert_eq!(lock_screen(&fakes), None);
    assert_eq!(*fakes.wallpaper.resets.lock().unwrap(), 1);
    // The cached images and their details are gone (WALL-12).
    assert!(
        fakes
            .storage
            .files
            .lock()
            .unwrap()
            .keys()
            .all(|file| !matches!(
                file,
                StoredFile::ImageDetails | StoredFile::ImageA | StoredFile::ImageB
            ))
    );
    // No more downloads: no job waits, and a check or "Update now" does nothing.
    assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
    let requests = fakes.network.requests.lock().unwrap().len();
    *fakes.system.now_ms.lock().unwrap() = NOV_19_MS + 48 * HOUR_MS;
    assert!(!shared.check_image());
    launcher.view().image_settings().update_now();
    launcher.frame();
    shared.run_image_tasks();
    assert_eq!(fakes.network.requests.lock().unwrap().len(), requests);
    assert_eq!(lock_screen(&fakes), None);

    // The shortcut opens the system's wallpaper settings.
    launcher.view().image_settings().open_wallpaper_settings();
    launcher.frame();
    assert_eq!(*fakes.wallpaper.settings_opened.lock().unwrap(), 1);
}

#[test]
fn wall02_given_the_daily_image_was_never_confirmed_then_the_lock_screen_is_unchanged() {
    // Issue #29: WALL-02. As when setup is left before Continue on the image screen: the
    // daily image was never turned on, and MeeGone's start checks as usual.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    *fakes.system.now_ms.lock().unwrap() = NOV_19_MS;
    let shared = Shared::new(fakes.platform());
    let mut launcher = Launcher::start_shared(&shared);

    assert!(!shared.check_image());
    launcher.view().image_settings().update_now();
    launcher.frame();
    shared.run_image_tasks();
    launcher.dispatch(ShellIntent::Resumed);

    assert_eq!(launcher.frame().daily_image, Some(DailyImage::Off));
    assert!(fakes.network.requests.lock().unwrap().is_empty());
    assert_eq!(lock_screen(&fakes), None);
    assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 0);
    assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
}

#[test]
fn wall10_given_no_image_was_ever_downloaded_then_the_lock_screen_is_left_as_it_is() {
    // Issue #29: WALL-10, WALL-08. On, but offline all day: no placeholder is applied.
    let fakes = Fakes::after_setup();
    let (shared, mut launcher) = with_the_daily_check(&fakes);

    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 24 * HOUR_MS);
    launcher.view().image_settings().update_now();
    launcher.frame();
    run_update_now(&shared, &mut launcher);

    assert_eq!(fakes.network.requests.lock().unwrap().len(), 7);
    assert_eq!(lock_screen(&fakes), None);
    assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 0);
}

#[test]
fn wall11_update_now_fetches_the_image_at_once_over_mobile_data() {
    // Issue #29: WALL-11. Only mobile data, "Use mobile data" off: the daily check waits, but
    // "Update now" goes ahead.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    *fakes.network.state.lock().unwrap() = NetworkState::Metered;
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + HOUR_MS);
    assert!(fakes.network.requests.lock().unwrap().is_empty());
    launcher.dispatch(ShellIntent::OpenSettings);
    assert_eq!(launcher.frame().update_now, Some(false));

    launcher.view().image_settings().update_now();
    // It shows as on its way, and a second tap asks for nothing more.
    assert_eq!(launcher.frame().update_now, Some(true));
    launcher.view().image_settings().update_now();
    launcher.frame();
    assert_eq!(*fakes.scheduler.task_runs.lock().unwrap(), 1);
    run_update_now(&shared, &mut launcher);

    assert_eq!(launcher.frame().update_now, Some(false));
    assert_eq!(*fakes.network.requests.lock().unwrap(), [FEED_URL, UHD_URL]);
    assert_eq!(lock_screen(&fakes), cached_image(&fakes));
    assert_eq!(launcher.view().image_settings().checks(), CheckStatus::Fine);
}

#[test]
fn wall13_given_the_device_cant_set_wallpapers_then_the_feature_is_unavailable_with_the_reason() {
    // Issue #29: WALL-13.
    let fakes = Fakes::after_setup();
    *fakes.wallpaper.unsupported.lock().unwrap() =
        Some("Your organisation doesn't allow changing the wallpaper".into());
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::OpenSettings);

    launcher.view().image_settings().daily_image_toggled(true);
    let frame = launcher.frame();

    assert_eq!(frame.daily_image, Some(DailyImage::Unavailable));
    assert_eq!(
        launcher
            .view()
            .image_settings()
            .unavailable_reason()
            .as_deref(),
        Some("Your organisation doesn't allow changing the wallpaper")
    );
    assert_eq!(frame.update_now, None);
    assert!(!frame.image_off_actions);
    assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
}

#[test]
fn wall14_given_someone_else_changes_the_lock_screen_then_daily_updates_pause_until_resumed() {
    // Issue #29: WALL-14.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + HOUR_MS);
    let ours = lock_screen(&fakes);
    assert!(ours.is_some());

    fakes.wallpaper.changed_by_someone_else();
    launcher.dispatch(ShellIntent::Resumed);
    launcher.dispatch(ShellIntent::OpenSettings);
    assert!(launcher.frame().image_paused);

    // The next day's check asks for nothing and changes nothing, and no job waits after it.
    let requests = fakes.network.requests.lock().unwrap().len();
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 48 * HOUR_MS);
    assert_eq!(fakes.network.requests.lock().unwrap().len(), requests);
    assert_eq!(lock_screen(&fakes).as_deref(), Some(&b"someone else's"[..]));
    assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);

    // Resume puts the image back, and the daily checks go on.
    launcher.view().image_settings().update_now();
    launcher.frame();
    run_update_now(&shared, &mut launcher);

    assert!(!launcher.frame().image_paused);
    assert_eq!(lock_screen(&fakes), ours);
    assert!(fakes.scheduler.job.lock().unwrap().is_some());
}

/// A card's on-screen rect, which its app grows from (ANIM-08).
const CARD: Rect = Rect {
    left: 48,
    top: 400,
    right: 368,
    bottom: 1100,
};

/// What Recent shows after a frame, and each card's label and age.
fn recent_cards(launcher: &mut Launcher) -> (RecentMode, Vec<(String, Age)>) {
    let recent = launcher.frame().recent.expect("Recent works");
    let cards = launcher.view().recent().cards();
    let cards = cards
        .iter()
        .map(|card| (card.label.clone(), card.age))
        .collect();
    (recent.mode, cards)
}

fn labels(cards: &[(String, Age)]) -> Vec<&str> {
    cards.iter().map(|(label, _)| label.as_str()).collect()
}

/// Recent showing, with usage access: Maps was used an hour ago, then Mail five minutes ago.
fn on_recent(fakes: &Fakes) -> Launcher {
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("maps", "Maps")];
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.system.now_ms.lock().unwrap() = 10_000_000;
    *fakes.usage.uses.lock().unwrap() = vec![used("maps", 6_400_000), used("mail", 9_700_000)];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    assert_eq!(launcher.shell().state().current, HomeView::Recent);
    launcher
}

#[test]
fn rec02_given_usage_access_is_revoked_while_recent_shows_then_the_explainer_appears() {
    // Issue #22: REC-02, REL-01. The system reports the change while Recent is on screen.
    let fakes = Fakes::after_setup();
    let mut launcher = on_recent(&fakes);
    assert_eq!(recent_cards(&mut launcher).0, RecentMode::Cards);

    *fakes.usage.access.lock().unwrap() = Access::Missing;
    launcher.dispatch(ShellIntent::UsageAccessChanged);

    assert_eq!(
        recent_cards(&mut launcher),
        (RecentMode::Explainer, Vec::new())
    );
    let recent = launcher.shell().recent();
    assert_eq!(*recent.health(), Health::Ready);
    assert_eq!(recent.state().problem, None);
    assert_eq!(launcher.shell().state().current, HomeView::Recent);
    // A tap on a card as it was drawn opens nothing.
    assert!(!launcher.view().recent().card_tapped(1, 0, CARD));
    assert!(fakes.apps.launched.lock().unwrap().is_empty());
}

#[test]
fn rec02_given_access_is_revoked_as_recent_reads_then_the_explainer_appears_without_a_problem() {
    // Issue #22: REC-02. Access goes between asking whether it is granted and reading the uses.
    let fakes = Fakes::after_setup();
    let mut launcher = on_recent(&fakes);

    *fakes.usage.error.lock().unwrap() = Some(DataError::AccessDenied);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));

    assert_eq!(
        recent_cards(&mut launcher),
        (RecentMode::Explainer, Vec::new())
    );
    assert_eq!(launcher.shell().recent().state().problem, None);
}

#[test]
fn rec02_without_usage_access_recent_explains_and_allow_opens_the_access_screen() {
    // Issue #22: REC-02, ONB-02. No placeholder cards; access is granted in system settings.
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
    *fakes.usage.uses.lock().unwrap() = vec![used("mail", 1)];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    assert_eq!(
        recent_cards(&mut launcher),
        (RecentMode::Explainer, Vec::new())
    );

    launcher.view().recent().allow_access_tapped();
    launcher.frame();
    assert_eq!(*fakes.usage.access_screen_opened.lock().unwrap(), 1);
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    launcher.dispatch(ShellIntent::Resumed);
    launcher.dispatch(ShellIntent::Returned { home: false });

    let (mode, cards) = recent_cards(&mut launcher);
    assert_eq!(mode, RecentMode::Cards);
    assert_eq!(labels(&cards), ["Mail"]);
}

#[test]
fn rec03_given_twenty_apps_used_then_the_eighteen_newest_show_newest_first() {
    // Issue #22: REC-03. Three columns, filling rows left to right from the top left.
    let fakes = Fakes::after_setup();
    let packages: Vec<String> = (0..20).map(|i| format!("app{i:02}")).collect();
    *fakes.apps.apps.lock().unwrap() = packages.iter().map(|p| app(p, p)).collect();
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.system.now_ms.lock().unwrap() = 10_000_000;
    *fakes.usage.uses.lock().unwrap() = packages
        .iter()
        .zip(0..)
        .map(|(package, i)| used(package, 10_000_000 - i * 60_000))
        .collect();
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));

    let (_, cards) = recent_cards(&mut launcher);
    assert_eq!(launcher.frame().recent.unwrap().columns, 3);
    assert_eq!(cards.len(), 18);
    assert_eq!(cards[0], ("app00".to_owned(), Age::JustNow));
    assert_eq!(cards[17], ("app17".to_owned(), Age::Minutes(17)));
}

#[test]
fn rec04_a_card_is_filled_with_its_apps_accent_and_says_when_it_was_used() {
    // Issue #22: REC-04, APPS-17, THEME-07.
    let fakes = Fakes::after_setup();
    let mut launcher = mail_in_recent(&fakes, &icon_of(0x1877F2));

    let (_, cards) = recent_cards(&mut launcher);
    let card = &launcher.view().recent().cards()[0];
    let (accent, background) = drawn_accent(&launcher);

    assert_eq!(cards, [("Mail".to_owned(), Age::Minutes(1))]);
    assert_eq!(card.fill, accent);
    assert!(card.fill.contrast_ratio(background) >= 3.0);
    assert!(card.available);
}

#[test]
fn rec09_recent_refreshes_when_it_becomes_visible() {
    // Issue #22: REC-09. Nothing polls: the cards change only when Recent comes into view.
    let fakes = Fakes::after_setup();
    let mut launcher = on_recent(&fakes);
    assert_eq!(labels(&recent_cards(&mut launcher).1), ["Mail", "Maps"]);

    *fakes.system.now_ms.lock().unwrap() = 10_100_000;
    fakes.usage.uses.lock().unwrap()[0].last_used_ms = 10_100_000;
    launcher.frame();
    assert_eq!(labels(&recent_cards(&mut launcher).1), ["Mail", "Maps"]);

    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    assert_eq!(labels(&recent_cards(&mut launcher).1), ["Maps", "Mail"]);
}

#[test]
fn rec10_given_no_app_was_used_then_recent_says_no_recent_apps() {
    // Issue #22: REC-10.
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));

    assert_eq!(recent_cards(&mut launcher), (RecentMode::Empty, Vec::new()));
}

#[test]
fn rec11_given_an_app_used_30_hours_ago_then_it_shows_with_3_days_and_not_with_24_hours() {
    // Issue #25: REC-11, SET-03. The window is chosen in Settings, takes effect at once, and
    // lasts.
    const HOUR_MS: u64 = 60 * 60 * 1000;
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("maps", "Maps")];
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.system.now_ms.lock().unwrap() = 100 * HOUR_MS;
    *fakes.usage.uses.lock().unwrap() = vec![
        used("maps", 70 * HOUR_MS),
        used("mail", 100 * HOUR_MS - 5 * 60_000),
    ];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    assert_eq!(labels(&recent_cards(&mut launcher).1), ["Mail"]);

    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    launcher.dispatch(ShellIntent::OpenSettings);
    assert_eq!(
        launcher.frame().recent_window,
        Some(RecentWindow::TwentyFourHours)
    );
    launcher
        .view()
        .recent_settings()
        .window_chosen(RecentWindow::ThreeDays);
    assert_eq!(
        launcher.frame().recent_window,
        Some(RecentWindow::ThreeDays)
    );
    assert_eq!(recent(&launcher), ["Mail", "Maps"], "at once");
    launcher.dispatch(ShellIntent::Back);
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));

    assert_eq!(
        recent_cards(&mut launcher),
        (
            RecentMode::Cards,
            vec![
                ("Mail".to_owned(), Age::Minutes(5)),
                ("Maps".to_owned(), Age::Days(1)),
            ]
        )
    );

    drop(launcher);
    let mut restarted = Launcher::start(fakes.platform());
    assert_eq!(recent(&restarted), ["Mail", "Maps"]);
    restarted.dispatch(ShellIntent::OpenSettings);
    assert_eq!(
        restarted.frame().recent_window,
        Some(RecentWindow::ThreeDays)
    );
    restarted
        .view()
        .recent_settings()
        .window_chosen(RecentWindow::TwentyFourHours);
    restarted.frame();
    restarted.dispatch(ShellIntent::Back);
    restarted.dispatch(ShellIntent::Swipe(Direction::Next));
    assert_eq!(labels(&recent_cards(&mut restarted).1), ["Mail"]);
}

/// Recent on a phone with six apps used, newest first, laid out as the host does: the cards
/// area is the screen's width less its 16 dp margins.
fn six_on_recent(fakes: &Fakes) -> (Launcher, u64) {
    let packages = ["mail", "maps", "music", "camera", "clock", "notes"];
    *fakes.apps.apps.lock().unwrap() = packages.iter().map(|p| app(p, p)).collect();
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.system.now_ms.lock().unwrap() = 10_000_000;
    *fakes.usage.uses.lock().unwrap() = (0..)
        .zip(packages)
        .map(|(i, package)| used(package, 9_000_000 - i * 60_000))
        .collect();
    let (mut launcher, mut clock) = on_a_phone(fakes);
    swipe(&mut launcher, &mut clock, 900.0, 300.0, 400, 100);
    assert_eq!(launcher.shell().state().current, HomeView::Recent);
    launcher
        .view()
        .recent()
        .laid_out(WIDTH - 2.0 * 16.0 * 2.75, 2.75);
    launcher.frame();
    (launcher, clock)
}

/// Two fingers at the middle of the screen, `from` px apart, move to `to` px apart over 160 ms
/// and lift, the second one first. Returns the cards' scale as drawn just before they lift.
fn pinch(launcher: &mut Launcher, clock: &mut u64, from: f64, to: f64) -> u16 {
    let first = |launcher: &mut Launcher, kind, span: f64, at: u64| {
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x: 540.0 - span / 2.0,
            y: 1200.0,
            at: Duration::from_millis(at),
        }));
    };
    let both = |launcher: &mut Launcher, kind, span_px, at: u64| {
        launcher.dispatch(ShellIntent::Pinch(PinchEvent {
            kind,
            span_px,
            x: 540.0,
            y: 1200.0,
            at: Duration::from_millis(at),
        }));
        launcher.frame()
    };
    first(launcher, PointerKind::Down, from, *clock);
    both(launcher, PinchKind::Start, from, *clock + 8);
    let mut scale = 10_000;
    for step in 1..=20_u32 {
        let span = from + (to - from) * f64::from(step) / 20.0;
        let at = *clock + 8 + 8 * u64::from(step);
        first(launcher, PointerKind::Move, span, at);
        scale = both(launcher, PinchKind::Move, span, at)
            .recent
            .expect("Recent works")
            .scale;
    }
    *clock += 180;
    both(launcher, PinchKind::End, to, *clock);
    first(launcher, PointerKind::Up, to, *clock + 8);
    *clock += 8;
    scale
}

#[test]
fn ac13_given_recent_with_six_apps_pinching_in_shows_two_columns_after_a_restart() {
    // Issue #23: REC-05. Spreading the fingers zooms in, to bigger cards.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    let recent = launcher.frame().recent.expect("Recent works");
    assert_eq!((recent.columns, recent.rows_per_screen), (3, 3));
    assert_eq!(recent_cards(&mut launcher).1.len(), 6);

    pinch(&mut launcher, &mut clock, 300.0, 600.0);
    come_to_rest(&mut launcher, &mut clock);

    let recent = launcher.frame().recent.expect("Recent works");
    assert_eq!((recent.columns, recent.rows_per_screen), (2, 2));
    assert_eq!(launcher.shell().state().current, HomeView::Recent);

    let mut restarted = Launcher::start(fakes.platform());
    restarted.dispatch(ShellIntent::Layout(phone()));
    restarted.dispatch(ShellIntent::Swipe(Direction::Next));
    let recent = restarted.frame().recent.expect("Recent works");
    assert_eq!((recent.columns, recent.rows_per_screen), (2, 2));
}

#[test]
fn rec05_pinching_the_other_way_shows_more_cards_again() {
    // Issue #23: REC-05.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    pinch(&mut launcher, &mut clock, 300.0, 600.0);
    come_to_rest(&mut launcher, &mut clock);

    pinch(&mut launcher, &mut clock, 600.0, 300.0);
    come_to_rest(&mut launcher, &mut clock);

    assert_eq!(launcher.frame().recent.expect("Recent works").columns, 3);
    assert_eq!(
        launcher.shell().recent().state().density,
        RecentDensity::ThreeColumns
    );
}

#[test]
fn rec05_a_tablet_adds_columns_to_both_densities() {
    // Issue #23: REC-05. A tablet 1 280 dp wide keeps cards about a phone's size.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    launcher.view().recent().laid_out(1_248.0 * 2.0, 2.0);
    assert_eq!(launcher.frame().recent.expect("Recent works").columns, 8);

    pinch(&mut launcher, &mut clock, 300.0, 600.0);
    come_to_rest(&mut launcher, &mut clock);

    assert_eq!(launcher.frame().recent.expect("Recent works").columns, 5);
}

#[test]
fn anim17_the_cards_scale_with_the_fingers_then_reflow_in_about_300_ms() {
    // Issue #23: ANIM-17, ANIM-01. The drawn scale is the fingers' as of the latest event.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);

    let scale = pinch(&mut launcher, &mut clock, 300.0, 420.0);
    assert_eq!(scale, 14_000);
    assert_eq!(launcher.frame().pages[2], 0, "Recent stays put");

    let recent = launcher.frame().recent.expect("Recent works");
    assert_eq!(recent.scale, 10_000);
    assert!(recent.reflow_progress < 10_000);
    let start = clock;
    come_to_rest(&mut launcher, &mut clock);
    assert!(
        (280..=330).contains(&(clock - start)),
        "{} ms",
        clock - start
    );
    let recent = launcher.frame().recent.expect("Recent works");
    assert_eq!((recent.columns, recent.reflow_progress), (2, 10_000));
}

#[test]
fn a11y05_a_screen_reader_switches_recent_between_more_and_bigger_cards() {
    // Issue #23: A11Y-05. "Bigger cards" and "More cards" are offered while the cards show.
    let fakes = Fakes::after_setup();
    let (mut launcher, _) = six_on_recent(&fakes);
    let recent = launcher.frame().recent.expect("Recent works");
    assert!(recent.density_offered);

    launcher
        .view()
        .recent()
        .density_chosen(RecentDensity::TwoColumns);
    let recent = launcher.frame().recent.expect("Recent works");
    assert_eq!((recent.columns, recent.rows_per_screen), (2, 2));
    assert!(launcher.frame().animating, "the cards reflow");

    launcher
        .view()
        .recent()
        .density_chosen(RecentDensity::ThreeColumns);
    assert_eq!(launcher.frame().recent.expect("Recent works").columns, 3);

    // Not on another view.
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert!(
        !launcher
            .frame()
            .recent
            .expect("Recent works")
            .density_offered
    );
}

/// Recent as the host draws it now.
fn recent_frame(launcher: &mut Launcher) -> RecentFrame {
    launcher.frame().recent.expect("Recent works")
}

/// Where each of Recent's cards was in the cards the host read before, as the host reads them.
fn recent_froms(launcher: &mut Launcher) -> Vec<Option<usize>> {
    launcher.frame();
    let cards = launcher.view().recent().cards();
    cards.iter().map(|card| card.from).collect()
}

#[test]
fn ac14_given_recent_a_long_press_and_clear_all_remove_every_card_and_stop_no_app() {
    // Issue #24: REC-07, REC-08, PRIV-03.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    fakes.storage.writes.lock().unwrap().clear();

    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    assert!(recent_frame(&mut launcher).close.on);
    launcher.view().recent().clear_all_tapped();
    come_to_rest(&mut launcher, &mut clock);

    assert_eq!(recent_cards(&mut launcher), (RecentMode::Empty, Vec::new()));
    let recent = recent_frame(&mut launcher);
    assert!(!recent.close.on, "nothing is left to remove");
    assert_eq!(recent.removals, 1);
    // No app was stopped, opened or asked about: only the list of removals was saved.
    assert!(fakes.apps.launched.lock().unwrap().is_empty());
    assert!(fakes.apps.uninstall_requested.lock().unwrap().is_empty());
    assert!(fakes.apps.app_info_opened.lock().unwrap().is_empty());
    assert_eq!(
        *fakes.storage.writes.lock().unwrap(),
        [StoredFile::Settings]
    );
    let stored = fakes.storage.stored_text();
    let removals = stored.lines().filter(|line| line.starts_with("removed "));
    assert_eq!(removals.count(), 6, "{stored}");

    // Each app returns to Recent when it is next used.
    *fakes.system.now_ms.lock().unwrap() = 10_100_000;
    fakes.usage.uses.lock().unwrap()[2].last_used_ms = 10_050_000;
    launcher.dispatch(ShellIntent::Returned { home: false });
    assert_eq!(labels(&recent_cards(&mut launcher).1), ["music"]);
}

#[test]
fn rec07_a_tap_on_empty_space_back_home_or_another_view_leaves_close_mode() {
    // Issue #24: REC-07, ANIM-24.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    let close_on = |launcher: &mut Launcher| recent_frame(launcher).close.on;

    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    assert!(close_on(&mut launcher));
    launcher.view().recent().empty_tapped();
    assert!(!close_on(&mut launcher));

    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    launcher.dispatch(ShellIntent::Back);
    assert!(!close_on(&mut launcher));

    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    launcher.dispatch(ShellIntent::Home);
    assert!(!close_on(&mut launcher));
    assert_eq!(launcher.shell().state().current, HomeView::Recent);

    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    swipe(&mut launcher, &mut clock, 300.0, 900.0, 400, 100);
    assert_eq!(launcher.shell().state().current, HomeView::Applications);
    assert!(!launcher.shell().recent().state().close_mode);
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 4);

    // A card tapped in close mode opens nothing.
    swipe(&mut launcher, &mut clock, 900.0, 300.0, 400, 100);
    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    let revision = recent_frame(&mut launcher).revision;
    assert!(launcher.view().recent().card_tapped(revision, 0, CARD));
    launcher.frame();
    assert!(fakes.apps.launched.lock().unwrap().is_empty());
}

#[test]
fn rec08_a_badge_removes_its_card_and_the_others_close_the_gap() {
    // Issue #24: REC-08, ANIM-12, ANIM-18.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    come_to_rest(&mut launcher, &mut clock);

    // ANIM-12: the cards at 92 %, and every row's ✕ at full size.
    let recent = recent_frame(&mut launcher);
    assert_eq!(recent.close.scale, 9_200);
    assert_eq!(recent.close.progress, 10_000);
    assert_eq!(recent.close.badges, [10_000; RECENT_ROWS]);
    recent_froms(&mut launcher);

    assert!(launcher.view().recent().badge_tapped(recent.revision, 1));
    let start = clock;
    let recent = recent_frame(&mut launcher);
    assert_eq!(recent.removals, 1);
    assert_eq!(recent.leave[0], 0, "Maps shrinks away in the first row");
    assert_eq!(recent.leave[1], 10_000);
    assert!(recent.close.on, "the others can still be removed");
    assert_eq!(
        recent_froms(&mut launcher),
        [Some(0), Some(2), Some(3), Some(4), Some(5)]
    );
    come_to_rest(&mut launcher, &mut clock);
    // ANIM-18: 180 ms.
    assert!(
        (180..=210).contains(&(clock - start)),
        "{} ms",
        clock - start
    );
    let (_, cards) = recent_cards(&mut launcher);
    assert_eq!(
        labels(&cards),
        ["mail", "music", "camera", "clock", "notes"]
    );
}

#[test]
fn anim18_clear_all_collapses_the_cards_within_400_ms() {
    // Issue #24: ANIM-18. Six cards in two rows: the second starts 220 ms after the first.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    come_to_rest(&mut launcher, &mut clock);
    recent_froms(&mut launcher);

    launcher.view().recent().clear_all_tapped();
    let start = clock;
    let recent = recent_frame(&mut launcher);
    assert_eq!(recent.leave[..3], [0, 0, 10_000]);
    wait(&mut launcher, &mut clock, 160);
    let recent = recent_frame(&mut launcher);
    assert!(
        recent.leave[0] > 5_000 && recent.leave[1] == 0,
        "{:?}",
        recent.leave
    );
    come_to_rest(&mut launcher, &mut clock);

    assert!(
        (400..=420).contains(&(clock - start)),
        "{} ms",
        clock - start
    );
    assert_eq!(recent_frame(&mut launcher).leave, [10_000; RECENT_ROWS]);
}

#[test]
fn ac18_with_animations_removed_close_mode_and_removals_are_instant() {
    // Issue #24: ANIM-05, AC-18.
    let fakes = Fakes::after_setup();
    *fakes.system.animation_scale.lock().unwrap() = 0.0;
    let (mut launcher, mut clock) = six_on_recent(&fakes);

    long_press(&mut launcher, &mut clock, 540.0, 1200.0);
    let frame = launcher.frame();
    assert!(!frame.animating);
    let recent = frame.recent.expect("Recent works");
    assert_eq!(recent.close.scale, 9_200);
    assert_eq!(recent.close.badges, [10_000; RECENT_ROWS]);

    assert!(launcher.view().recent().badge_tapped(recent.revision, 0));
    let frame = launcher.frame();
    assert!(!frame.animating);
    assert_eq!(
        frame.recent.expect("Recent works").leave,
        [10_000; RECENT_ROWS]
    );

    launcher.view().recent().clear_all_tapped();
    assert!(!launcher.frame().animating);
    assert_eq!(recent_cards(&mut launcher).0, RecentMode::Empty);
}

/// Fakes whose system has animations removed (ANIM-05).
fn animations_removed() -> Fakes {
    let fakes = Fakes::after_setup();
    *fakes.system.animation_scale.lock().unwrap() = 0.0;
    fakes
}

/// Nothing moves in the frame the host draws next, so it asks for no more.
fn at_rest(launcher: &mut Launcher) -> bool {
    !launcher.frame().animating
}

#[test]
fn ac18_with_animations_removed_the_views_the_return_home_and_the_theme_change_at_once() {
    // Issue #33: ANIM-05, A11Y-07, AC-18. Each ends in the same frame where its motion would
    // have, and nothing is missing.
    let fakes = animations_removed();
    let (mut launcher, mut clock) = on_a_phone(&fakes);

    // A finger moves the views: they settle at once. The indicator, which says where the views
    // are, still stays its while, a wait rather than motion, then goes at once (ANIM-11).
    for (kind, x) in [
        (PointerKind::Down, 900.0),
        (PointerKind::Move, 700.0),
        (PointerKind::Move, 300.0),
        (PointerKind::Up, 300.0),
    ] {
        finger(&mut launcher, &mut clock, kind, x, 1200.0);
    }
    let frame = launcher.frame();
    assert_eq!((frame.current, frame.pages[2]), (HomeView::Recent, 0));
    assert_eq!(frame.indicator_alpha, 255);
    wait(&mut launcher, &mut clock, 608);
    assert_eq!(launcher.frame().indicator_alpha, 0);
    assert!(at_rest(&mut launcher));
    // A screen reader's swipe.
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert_eq!(launcher.frame().pages[1], 0);
    assert!(at_rest(&mut launcher));

    // Coming back from an app, where the system doesn't animate the return itself.
    launcher.dispatch(ShellIntent::Resumed);
    launcher.dispatch(ShellIntent::Returned { home: false });
    let frame = launcher.frame();
    assert_eq!((frame.content_scale, frame.content_alpha), (10_000, 255));
    assert!(at_rest(&mut launcher));

    // The device turning dark.
    *fakes.system.dark.lock().unwrap() = true;
    launcher.dispatch(ShellIntent::SystemThemeChanged);
    let frame = launcher.frame();
    assert_eq!(frame.theme_change, 10_000);
    assert_eq!(frame.background, tokens(Theme::Dark).background);
    assert!(at_rest(&mut launcher));
}

#[test]
fn ac18_with_animations_removed_edit_mode_dragging_folders_and_search_are_instant() {
    // Issue #33: ANIM-05, A11Y-07, AC-18.
    let fakes = animations_removed();
    *fakes.apps.apps.lock().unwrap() = vec![
        app("camera", "Camera"),
        app("mail", "Mail"),
        app("maps", "Maps"),
        app("phone", "Phone"),
    ];
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    grid_on_a_phone(&mut launcher);

    // Edit mode: the cells are at 92 % and every ✕ shows at once (ANIM-12).
    long_press(&mut launcher, &mut clock, 500.0, 1500.0);
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
    let frame = launcher.frame();
    assert!(frame.edit.on);
    assert_eq!(frame.edit.scale, 9_200);
    assert_eq!(frame.edit.badges, [10_000; BADGE_ROWS]);
    assert!(at_rest(&mut launcher));

    // Dragging Maps first: the others are in their new places at once, and the dropped icon is
    // in its cell as the finger lifts (ANIM-13, ANIM-21). The wait before it takes the place is
    // the same as ever.
    assert!(long_press_cell(&mut launcher, &mut clock, 2, 620.0, 300.0));
    finger(&mut launcher, &mut clock, PointerKind::Move, 124.0, 451.0);
    wait(&mut launcher, &mut clock, 192);
    assert_eq!(grid(&launcher), ["Maps", "Camera", "Mail", "Phone"]);
    assert_eq!(launcher.frame().grid_change.shift, 10_000);
    finger(&mut launcher, &mut clock, PointerKind::Up, 124.0, 451.0);
    assert_eq!(launcher.frame().lifted, None);
    assert!(at_rest(&mut launcher));
    launcher.dispatch(ShellIntent::Back);
    assert_eq!(launcher.frame().edit.scale, 10_000);
    assert!(at_rest(&mut launcher));

    // A folder opens and closes at once (ANIM-15).
    let fakes = animations_removed();
    let (mut launcher, _) = games_and_maps(&fakes);
    open_folder(&mut launcher, 0);
    let folder = launcher.frame().folder.expect("open");
    assert_eq!((folder.open, folder.progress), (true, 10_000));
    assert!(at_rest(&mut launcher));
    launcher.dispatch(ShellIntent::Back);
    assert_eq!(launcher.frame().folder, None);
    assert!(at_rest(&mut launcher));

    // Pulled past its band, search snaps open at once (ANIM-16).
    let fakes = animations_removed();
    let (mut launcher, mut clock) = ready_to_search(&fakes);
    open_search(&mut launcher, &mut clock);
    let search = launcher.frame().search;
    assert!(search.open);
    assert_eq!(
        search.reveal_px,
        SEARCH_BAND.bottom as i32 - SEARCH_BAND.top as i32
    );
    assert!(at_rest(&mut launcher));
}

#[test]
fn ac18_with_animations_removed_lists_change_and_recent_reflows_at_once() {
    // Issue #33: ANIM-05, A11Y-07, AC-18.
    let fakes = animations_removed();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    let (mut launcher, _) = on_a_phone(&fakes);

    // An app installed joins the grid at once (ANIM-21).
    fakes.apps.apps.lock().unwrap().push(app("atlas", "Atlas"));
    launcher.dispatch(ShellIntent::AppsChanged);
    let change = launcher.frame().grid_change;
    assert_eq!((change.appear_scale, change.appear_alpha), (10_000, 10_000));
    assert!(at_rest(&mut launcher));

    // A notification comes into Events at once (ANIM-20).
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    fakes
        .notifications
        .active
        .lock()
        .unwrap()
        .push(notification("m1", "mail"));
    launcher.dispatch(ShellIntent::NotificationsChanged);
    assert_eq!(launcher.frame().events_change, 10_000);
    assert_eq!(events_list(&mut launcher).1.len(), 1);
    assert!(at_rest(&mut launcher));

    // Recent's cards reflow to bigger ones at once after a pinch (ANIM-17).
    let fakes = animations_removed();
    let (mut launcher, mut clock) = six_on_recent(&fakes);
    pinch(&mut launcher, &mut clock, 300.0, 420.0);
    let recent = launcher.frame().recent.expect("Recent works");
    assert_eq!((recent.columns, recent.reflow_progress), (2, 10_000));
    assert!(at_rest(&mut launcher));
}

#[test]
fn anim05_a_new_animation_speed_is_taken_on_at_once() {
    // Issue #33: ANIM-05, A11Y-07. Animations removed while the views move, such as from the
    // accessibility shortcut over the home, put them where they were going; a slower speed
    // makes what starts next take longer.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = games_and_maps(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    clock += 16;
    launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
    clock += 16;
    launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
    assert!(launcher.frame().pages[2] != 0, "Recent is coming in");

    *fakes.system.animation_scale.lock().unwrap() = 0.0;
    launcher.dispatch(ShellIntent::AnimationScaleChanged);
    let frame = launcher.frame();
    assert_eq!((frame.current, frame.pages[2]), (HomeView::Recent, 0));
    assert!(at_rest(&mut launcher));

    *fakes.system.animation_scale.lock().unwrap() = 2.0;
    launcher.dispatch(ShellIntent::AnimationScaleChanged);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    come_to_rest(&mut launcher, &mut clock);
    open_folder(&mut launcher, 0);
    let start = clock;
    come_to_rest(&mut launcher, &mut clock);
    // ANIM-15's 250 ms, at half speed.
    assert!(
        (490..=520).contains(&(clock - start)),
        "{} ms",
        clock - start
    );
}

#[test]
fn anim03_a_touch_catches_the_moving_row_and_a_settling_icon_where_they_are() {
    // Issue #33: ANIM-03. The row is caught as it settles and follows the finger; so is an icon
    // settling into its cell after a drop, which the finger picks up again.
    let fakes = Fakes::after_setup();
    let (mut launcher, mut clock) = editing_four(&fakes);
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    wait(&mut launcher, &mut clock, 48);
    let caught = launcher.frame().pages[2];
    assert!(caught > 100, "{caught} px from where it fills the screen");
    finger(&mut launcher, &mut clock, PointerKind::Down, 540.0, 1200.0);
    assert_eq!(launcher.frame().pages[2], caught);
    assert!(launcher.shell().state().carousel.dragging);
    finger(&mut launcher, &mut clock, PointerKind::Move, 640.0, 1200.0);
    assert_eq!(launcher.frame().pages[2], caught + 100);
    finger(&mut launcher, &mut clock, PointerKind::Up, 640.0, 1200.0);
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(launcher.frame().current, HomeView::Applications);

    // Maps, dropped far from its cell, springs towards it…
    assert!(long_press_cell(&mut launcher, &mut clock, 2, 620.0, 300.0));
    finger(&mut launcher, &mut clock, PointerKind::Move, 600.0, 1500.0);
    finger(&mut launcher, &mut clock, PointerKind::Up, 600.0, 1500.0);
    wait(&mut launcher, &mut clock, 48);
    let settling = launcher.frame().lifted.expect("settling");
    assert!(settling.settling);
    // …and a finger on it catches it there, and drags it on.
    let (x, y) = (f64::from(settling.x), f64::from(settling.y));
    finger(&mut launcher, &mut clock, PointerKind::Down, x, y);
    let caught = launcher.frame().lifted.expect("caught");
    assert!(!caught.settling);
    assert_eq!((caught.x, caught.y), (settling.x, settling.y));
    finger(&mut launcher, &mut clock, PointerKind::Move, x + 300.0, y);
    assert_eq!(
        launcher.frame().lifted.expect("dragged").x,
        settling.x + 300
    );
    assert_eq!(launcher.frame().pages[1], 0, "the views stay put");
    finger(&mut launcher, &mut clock, PointerKind::Up, x + 300.0, y);
    come_to_rest(&mut launcher, &mut clock);
    assert_eq!(launcher.frame().lifted, None);
    assert!(launcher.shell().apps().state().edit_mode);
}

#[test]
fn anim03_a_touch_on_the_last_sliver_of_a_settle_is_a_tap_on_what_is_under_it() {
    // Issue #33: ANIM-03. Within a third of the touch slop of rest, the views are as good as
    // there: a touch lands them and is the content's, so a tap on an icon launches it.
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    grid_on_a_phone(&mut launcher);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    come_to_rest(&mut launcher, &mut clock);
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    // A third of the phone's 22 px slop.
    let away = |launcher: &Launcher| launcher.shell().state().carousel.offset.abs() * WIDTH;
    while away(&launcher) >= 22.0 / 3.0 {
        clock += 8;
        launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
    }
    assert!(away(&launcher) > 0.0, "still settling");

    finger(&mut launcher, &mut clock, PointerKind::Down, 168.0, 181.0);
    let state = launcher.shell().state();
    assert_eq!(state.carousel.offset, 0.0);
    assert!(!state.carousel.dragging && !state.carousel.held);
    finger(&mut launcher, &mut clock, PointerKind::Up, 168.0, 181.0);
    let revision = launcher.frame().apps.revision;
    assert!(
        launcher
            .view()
            .apps()
            .cell_tapped(Surface::Grid, revision, 0, Rect::default())
    );
    launcher.frame();
    assert_eq!(fakes.apps.launched.lock().unwrap().len(), 1);
}

#[test]
fn a11y05_a_screen_reader_clears_recent_without_gestures() {
    // Issue #24: A11Y-05, AC-19. "Edit Recent" enters close mode, with a tick as edit mode's
    // (issue #33, ANIM-24), and the cards' "Remove ‹app› from Recent" and Clear all work as the
    // ✕ and the button do.
    let fakes = Fakes::after_setup();
    let (mut launcher, _) = six_on_recent(&fakes);
    assert!(recent_frame(&mut launcher).close_offered);

    launcher.view().recent().close_mode_requested();
    let recent = recent_frame(&mut launcher);
    assert!(recent.close.on);
    assert!(!recent.close_offered);
    assert!(!recent.density_offered, "no pinch while removing cards");
    assert_eq!(*fakes.system.haptic_ticks.lock().unwrap(), 1);

    recent_froms(&mut launcher);
    assert!(launcher.view().recent().badge_tapped(recent.revision, 0));
    assert_eq!(recent_cards(&mut launcher).1.len(), 5);
    launcher.view().recent().clear_all_tapped();
    assert_eq!(recent_cards(&mut launcher), (RecentMode::Empty, Vec::new()));
    assert!(
        !recent_frame(&mut launcher).close_offered,
        "nothing to remove"
    );
}

/// Where Bing's feed gives the past week's images in the device's region (WALL-17).
const WEEK_URL: &str = "https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=7&mkt=en-US";
/// The images of the week recorded on 19 November 2026, newest first. The first, the
/// sculpture, isn't offered as a wallpaper; the second is today's image in `FEED_URL`.
const WEEK: [&str; 7] = [
    "OHR.KnowledgeSculpture_EN-US9746703110",
    "OHR.BarnOwlsUnite_EN-US9661992435",
    "OHR.TrailCedars_EN-US2240500417",
    "OHR.GilaCliff_EN-US2139490645",
    "OHR.FirstFrost_EN-US8881008245",
    "OHR.BlueHK_EN-US7870996687",
    "OHR.AbrahamCanada_EN-US3455540217",
];
/// The cedars' largest version, which pinning them downloads on this phone.
const CEDARS_UHD_URL: &str = "https://www.bing.com/th?id=OHR.TrailCedars_EN-US2240500417_UHD.jpg";

fn preview_url(id: &str) -> String {
    format!("https://www.bing.com/th?id={id}_800x480.jpg")
}

/// A JPEG of `size` that is `bytes` long, padded with comments, as a real one of that size
/// would download.
fn jpeg_of(size: Size, bytes: usize) -> Vec<u8> {
    let header = fakes::jpeg(size);
    let mut jpeg = header[..2].to_vec();
    while jpeg.len() + header.len() - 2 < bytes {
        let room = bytes - (jpeg.len() + header.len() - 2);
        let length = room.clamp(4, 0xFFFF);
        jpeg.extend([0xFF, 0xFE]);
        jpeg.extend(u16::try_from(length - 2).unwrap().to_be_bytes());
        jpeg.resize(jpeg.len() + length - 4, b' ');
    }
    jpeg.extend(&header[2..]);
    jpeg
}

/// The network serves the past week, a preview of `preview_bytes` for each image, and the
/// cedars' largest version at `uhd_bytes` (WALL-17).
fn serve_the_past_week(fakes: &Fakes, preview_bytes: usize, uhd_bytes: usize) {
    let mut responses = fakes.network.responses.lock().unwrap();
    responses.insert(
        WEEK_URL.into(),
        include_bytes!("../crates/lockimage/data/fixtures/bing-en-US-week.json").to_vec(),
    );
    for id in WEEK {
        let preview = Size {
            width: 800,
            height: 480,
        };
        responses.insert(preview_url(id), jpeg_of(preview, preview_bytes));
    }
    let uhd = Size {
        width: 3840,
        height: 2160,
    };
    responses.insert(CEDARS_UHD_URL.into(), jpeg_of(uhd, uhd_bytes));
}

/// What the past week's screen shows after a frame, or `None` while it doesn't (WALL-17).
fn past_images(launcher: &mut Launcher) -> Option<(u32, PastContent)> {
    let revision = launcher.frame().past_images?;
    Some((revision, launcher.view().past_images().content()?))
}

#[test]
fn wall17_given_a_pinned_image_the_next_days_check_leaves_the_lock_screen_and_unpinning_resumes_daily_updates()
 {
    // Issue #31: WALL-17, LEGAL-03, LEGAL-04. Today's barn owls are on the lock screen; the user
    // browses the past week from the details screen and pins the cedars.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    serve_the_past_week(&fakes, 1_000, 1_000);
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + HOUR_MS);
    assert_eq!(current_image(&fakes), Some((WEEK[1].to_owned(), false)));
    let owls = lock_screen(&fakes);
    assert!(owls.is_some());

    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    tap_the_image_card(&mut launcher);
    launcher.frame();
    assert!(launcher.view().image_details().past_images());
    assert!(launcher.frame().past_images_open);
    // The week is read on the job's thread.
    assert!(past_images(&mut launcher).unwrap().1.loading);
    run_update_now(&shared, &mut launcher);

    let (revision, week) = past_images(&mut launcher).unwrap();
    assert!(!week.loading);
    assert_eq!(
        week.images
            .iter()
            .map(|image| image.id.as_str())
            .collect::<Vec<_>>(),
        WEEK
    );
    assert_eq!(week.images[2].title, "Step into the great outdoors");
    assert_eq!(
        week.images[2].attribution,
        "Trail of the Cedars, Glacier National Park, Montana (© DBushue Photography/Getty Images)"
    );
    assert_eq!(week.images[2].date, "17 November");
    assert!(week.images[1].current);
    // The sculpture can't be pinned, and has no preview (LEGAL-04); every other has one.
    assert_eq!(week.images[0].action, PastAction::NotOffered);
    assert!(week.images[0].preview.is_none());
    assert!(
        week.images[1..]
            .iter()
            .all(|image| image.action == PastAction::Pin && image.preview.is_some())
    );

    assert!(launcher.view().past_images().pin(revision, 2));
    assert_eq!(
        past_images(&mut launcher).unwrap().1.images[2].action,
        PastAction::Pinning
    );
    run_update_now(&shared, &mut launcher);

    // The cedars are the lock screen's wallpaper, pinned.
    assert_eq!(current_image(&fakes), Some((WEEK[2].to_owned(), true)));
    assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 2);
    assert_eq!(lock_screen(&fakes), current_image_bytes(&fakes));
    let week = past_images(&mut launcher).unwrap().1;
    assert!(week.images[2].current);
    assert_eq!(week.images[2].action, PastAction::Unpin);
    assert!(
        launcher
            .view()
            .image_settings()
            .update_status()
            .unwrap()
            .pinned
    );
    // One request for the week, one preview for each image offered, and the cedars.
    let requests = fakes.network.requests.lock().unwrap().clone();
    assert_eq!(requests[..2], [FEED_URL, UHD_URL]);
    assert_eq!(requests[2], WEEK_URL);
    assert_eq!(
        requests[3..9],
        WEEK[1..]
            .iter()
            .map(|id| preview_url(id))
            .collect::<Vec<_>>()
    );
    assert_eq!(requests[9..], [CEDARS_UHD_URL]);
    // Daily updates wait: no job.
    assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);

    // The next day: neither the system's job nor MeeGone's start asks for anything, and the lock
    // screen keeps the cedars.
    let asked = requests.len();
    assert_eq!(
        run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 48 * HOUR_MS),
        0
    );
    *fakes.system.now_ms.lock().unwrap() = NOV_19_MS + 25 * HOUR_MS;
    assert!(!shared.check_image());
    launcher.dispatch(ShellIntent::Resumed);
    assert_eq!(fakes.network.requests.lock().unwrap().len(), asked);
    assert_eq!(current_image(&fakes), Some((WEEK[2].to_owned(), true)));
    assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 2);

    // Unpinned: today's image comes back from the cache, and the daily checks resume.
    assert!(launcher.view().past_images().unpin());
    launcher.frame();
    run_update_now(&shared, &mut launcher);

    assert_eq!(current_image(&fakes), Some((WEEK[1].to_owned(), false)));
    assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 3);
    assert_eq!(lock_screen(&fakes), owls);
    assert!(
        !launcher
            .view()
            .image_settings()
            .update_status()
            .unwrap()
            .pinned
    );
    assert_eq!(fakes.network.requests.lock().unwrap()[asked..], [FEED_URL]);
    let job = fakes.scheduler.job.lock().unwrap().expect("the daily job");
    assert_eq!(job.not_before_ms, NOV_19_MS + 48 * HOUR_MS);
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 49 * HOUR_MS);
    assert_eq!(
        fakes.network.requests.lock().unwrap()[asked + 1..],
        [FEED_URL]
    );
}

/// The fields of the current image's saved record: `current`, then its date, region, title,
/// attribution, link, time set, file, id, marker and pin.
fn current_record(fakes: &Fakes) -> Option<Vec<String>> {
    let files = fakes.storage.files.lock().unwrap();
    let details = String::from_utf8_lossy(files.get(&StoredFile::ImageDetails)?).into_owned();
    let current = details.lines().find(|line| line.starts_with("current "))?;
    Some(current.split(' ').map(str::to_owned).collect())
}

/// The current image's feed id, and whether it is pinned, as saved.
fn current_image(fakes: &Fakes) -> Option<(String, bool)> {
    let record = current_record(fakes)?;
    Some((record.get(8)?.clone(), record.get(10)? == "true"))
}

/// The current image's bytes, which the lock screen shows once it is set.
fn current_image_bytes(fakes: &Fakes) -> Option<Vec<u8>> {
    let file = match current_record(fakes)?.get(7)?.as_str() {
        "b" => StoredFile::ImageB,
        _ => StoredFile::ImageA,
    };
    fakes.storage.files.lock().unwrap().get(&file).cloned()
}

#[test]
fn wall17_the_past_week_opens_from_settings_before_any_image_is_cached() {
    // Issue #31: WALL-17, WALL-10. Today's image isn't offered as a wallpaper, so nothing is
    // cached yet; the user picks a recent one instead.
    let fakes = Fakes::after_setup();
    serve_the_past_week(&fakes, 1_000, 1_000);
    *fakes.network.state.lock().unwrap() = NetworkState::Unmetered;
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    launcher.dispatch(ShellIntent::OpenSettings);
    assert!(launcher.frame().image_details.is_none());

    assert!(launcher.view().image_settings().past_images());
    launcher.frame();
    run_update_now(&shared, &mut launcher);
    let (revision, week) = past_images(&mut launcher).unwrap();
    assert_eq!(week.images.len(), 7);
    assert!(week.images.iter().all(|image| !image.current));
    assert!(launcher.view().past_images().pin(revision, 2));
    launcher.frame();
    run_update_now(&shared, &mut launcher);

    assert!(lock_screen(&fakes).is_some());
    assert_eq!(current_image(&fakes), Some((WEEK[2].to_owned(), true)));
    assert!(launcher.view().image_settings().pinned());
    // Back goes to Settings, then home.
    launcher.dispatch(ShellIntent::Back);
    let frame = launcher.frame();
    assert!(!frame.past_images_open && frame.settings_open);
}

#[test]
fn pwr03_browsing_and_pinning_keep_a_day_within_10_mb() {
    // Issue #31: PWR-03, WALL-17. The feed's real sizes: today's image about 3.7 MB, the
    // week's previews about 35 KB each. A day with a check, two browses, a pin and an unpin.
    let fakes = Fakes::after_setup();
    serve_todays_image(&fakes);
    let uhd = Size {
        width: 3840,
        height: 2160,
    };
    fakes
        .network
        .responses
        .lock()
        .unwrap()
        .insert(UHD_URL.into(), jpeg_of(uhd, 3_700_000));
    serve_the_past_week(&fakes, 35_000, 3_700_000);
    let (shared, mut launcher) = with_the_daily_check(&fakes);
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + HOUR_MS);

    launcher.dispatch(ShellIntent::OpenSettings);
    for _ in 0..2 {
        assert!(launcher.view().image_settings().past_images());
        launcher.frame();
        run_update_now(&shared, &mut launcher);
        launcher.dispatch(ShellIntent::Back);
    }
    launcher.view().image_settings().past_images();
    let (revision, _) = past_images(&mut launcher).unwrap();
    assert!(launcher.view().past_images().pin(revision, 2));
    launcher.frame();
    run_update_now(&shared, &mut launcher);
    assert_eq!(current_image(&fakes), Some((WEEK[2].to_owned(), true)));
    assert!(launcher.view().past_images().unpin());
    launcher.frame();
    run_update_now(&shared, &mut launcher);
    assert_eq!(current_image(&fakes), Some((WEEK[1].to_owned(), false)));
    run_the_daily_job(&fakes, &shared, &mut launcher, NOV_19_MS + 24 * HOUR_MS);

    let responses = fakes.network.responses.lock().unwrap();
    let requests = fakes.network.requests.lock().unwrap();
    let bytes: usize = requests.iter().map(|url| responses[url].len()).sum();
    // The week is read once that day, and the unpin downloads nothing.
    assert_eq!(requests.iter().filter(|url| *url == WEEK_URL).count(), 1);
    assert_eq!(requests.iter().filter(|url| *url == UHD_URL).count(), 1);
    assert!(bytes < 10_000_000, "{bytes} bytes");
    assert!(bytes > 7_400_000, "{bytes} bytes");
}

#[test]
fn set01_settings_opens_from_the_home_menu_and_from_the_systems_settings_entry() {
    // Issue #34: SET-01, AC-19. The menu opens without a gesture, as a screen reader opens it.
    let (mut launcher, _) = on_a_phone(&Fakes::after_setup());
    launcher.dispatch(ShellIntent::OpenMenu);
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::LauncherSettings));
    let frame = launcher.frame();
    assert!(frame.settings_open);
    assert_eq!(frame.settings_group, None);
    launcher.dispatch(ShellIntent::Back);
    assert!(!launcher.frame().settings_open);

    // The system's entry, such as the gear beside MeeGone in Default apps → Home app, reaches
    // the host, which opens Settings at its top, over whatever was open.
    launcher.dispatch(ShellIntent::OpenMenu);
    launcher.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
    launcher.dispatch(ShellIntent::OpenSettings);
    let frame = launcher.frame();
    assert!(frame.settings_open);
    assert_eq!(frame.settings_group, None);
    assert_eq!(frame.menu, None);
}

#[test]
fn set02_settings_offers_every_group_with_the_status_of_each_access() {
    // Issue #34: SET-02. Appearance, Home, Applications, Recent, the lock-screen image, Access
    // and About, with Reset launcher (SET-06).
    let fakes = Fakes::after_setup();
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.system.default_home.lock().unwrap() = true;
    let (mut launcher, _) = on_a_phone(&fakes);

    launcher.dispatch(ShellIntent::OpenSettings);
    let frame = launcher.frame();

    assert_eq!(frame.theme_choice, Some(ThemeChoice::System));
    assert_eq!(frame.home_target, Some(HomeTarget::LastView));
    assert_eq!(frame.wrap_around, Some(true));
    assert!(frame.hidden_apps.is_some());
    assert_eq!(frame.recent_window, Some(RecentWindow::TwentyFourHours));
    assert_eq!(frame.daily_image, Some(DailyImage::Off));
    assert_eq!(frame.region, Some(0), "Automatic");
    assert_eq!(frame.use_mobile_data, Some(false));
    assert_eq!(frame.show_image_card, Some(true));
    assert_eq!(
        frame.access,
        Some(AccessStatus {
            default_home: true,
            notifications: true,
            usage: false,
            instructions: None,
        })
    );
    assert!(frame.reset_offered);
}

#[test]
fn set02_an_access_shortcut_opens_its_system_screen_or_shows_written_instructions() {
    // Issue #34: SET-02, SRS 8.1. Usage access has no screen on this device.
    let fakes = Fakes::after_setup();
    let instructions = "Open Settings, then Apps, Special app access, Usage access";
    *fakes.usage.error.lock().unwrap() = Some(DataError::Unsupported(instructions.into()));
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::OpenSettings);

    launcher.view().settings().notification_access_tapped();
    launcher.view().settings().default_home_tapped();
    launcher.view().settings().usage_access_tapped();
    let frame = launcher.frame();

    assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
    assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 1);
    let access = frame.access.expect("Settings works");
    assert_eq!(access.instructions, Some(AccessItem::Usage));
    let state = launcher.shell().settings().state();
    assert_eq!(state.instructions.as_ref().unwrap().1, instructions);
    assert_eq!(state.problem, None);

    // Back from the system's screens, the status is read again and the instructions go.
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    launcher.dispatch(ShellIntent::Resumed);
    let access = launcher.frame().access.expect("Settings works");
    assert!(access.usage);
    assert_eq!(access.instructions, None);
}

#[test]
fn set02_the_region_is_automatic_or_one_the_feed_offers() {
    // Issue #34: SET-02, WALL-05. Settings lists Automatic first, then the feed's regions.
    let fakes = Fakes::after_setup();
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::OpenSettings);
    let section = launcher.view().image_settings();
    let regions = section.regions();
    let place = regions.iter().position(|region| region == "de-DE");
    let place = place.expect("the feed offers de-DE");

    section.region_chosen(Some("de-DE".into()));
    assert_eq!(launcher.frame().region, Some(place + 1));

    launcher.view().image_settings().region_chosen(None);
    assert_eq!(launcher.frame().region, Some(0));
}

#[test]
fn set03_a_change_in_settings_shows_on_returning_home_without_a_restart() {
    // Issue #34: SET-03. No Save button: each change is saved and applied as it is made.
    let fakes = Fakes::after_setup();
    let (mut launcher, _) = on_a_phone(&fakes);
    launcher.dispatch(ShellIntent::OpenSettings);
    let settings = launcher.view().settings();
    settings.theme_chosen(ThemeChoice::Dark);
    settings.wrap_around_toggled(false);
    settings.home_target_chosen(HomeTarget::Applications);
    let frame = launcher.frame();
    assert_eq!(frame.wrap_around, Some(false));
    assert_eq!(frame.home_target, Some(HomeTarget::Applications));

    launcher.dispatch(ShellIntent::Back);
    let frame = launcher.frame();
    assert!(!frame.settings_open);
    assert_eq!(frame.background, tokens::DARK.background);
    // Recent is the last view: without wrap-around, it stays put.
    for _ in 0..2 {
        launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    }
    assert_eq!(launcher.frame().current, HomeView::Recent);
    // From an app, Home now shows Applications.
    launcher.dispatch(ShellIntent::Resumed);
    launcher.dispatch(ShellIntent::Returned { home: true });
    assert_eq!(launcher.frame().current, HomeView::Applications);

    let saved = PersistentSettingsStore::new(fakes.storage.clone()).get();
    assert_eq!(
        (saved.theme, saved.wrap_around, saved.home_target),
        (ThemeChoice::Dark, false, HomeTarget::Applications)
    );
}

#[test]
fn set06_reset_launcher_restores_the_default_layout_and_settings() {
    // Issue #34: SET-06, after the host's confirmation. Access, granted in system settings,
    // stays as it is.
    let fakes = Fakes::after_setup();
    let (mut launcher, _) = ready_to_hide(&fakes);
    *fakes.system.default_home.lock().unwrap() = true;
    fakes.apps.apps.lock().unwrap().push(app("atlas", "Atlas"));
    launcher.dispatch(ShellIntent::AppsChanged);
    hide(&mut launcher, "maps");
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().settings().theme_chosen(ThemeChoice::Dark);
    launcher.view().settings().wrap_around_toggled(false);
    launcher
        .view()
        .recent_settings()
        .window_chosen(RecentWindow::ThreeDays);
    let image = launcher.view().image_settings();
    image.daily_image_toggled(true);
    image.use_mobile_data_toggled(true);
    image.show_card_toggled(false);
    launcher.frame();
    assert_eq!(grid(&launcher), ["Café", "Gmail", "Mail", "Atlas"]);
    assert_eq!(dots(&mut launcher), [false, false, false, true]);

    assert!(launcher.frame().reset_offered);
    launcher.view().settings().reset_launcher_confirmed();
    let frame = launcher.frame();

    // Settings closes on Applications, at its top, fading to the default theme.
    assert!(!frame.settings_open);
    assert_eq!(frame.current, HomeView::Applications);
    assert!(frame.theme_change < 10_000);
    assert_eq!(frame.background, tokens::LIGHT.background);
    // A first start's grid: alphabetical, nothing new, nothing hidden.
    let first_grid = ["Atlas", "Café", "Gmail", "Mail", "Maps"];
    assert_eq!(grid(&launcher), first_grid);
    assert_eq!(dots(&mut launcher), [false; 5]);
    assert!(hidden_apps(&launcher).is_empty());
    assert_eq!(launcher.shell().apps().state().scroll_px, 0.0);
    // Every option at its default; the daily image is off, with nothing scheduled.
    let frame = launcher.frame();
    assert_eq!(frame.wrap_around, Some(true));
    assert_eq!(frame.recent_window, Some(RecentWindow::TwentyFourHours));
    assert_eq!(frame.daily_image, Some(DailyImage::Off));
    assert_eq!(frame.use_mobile_data, Some(false));
    assert_eq!(frame.show_image_card, Some(true));
    assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
    assert!(frame.access.is_some_and(|access| access.default_home));

    // And so after a restart.
    drop(launcher);
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Layout(phone()));
    assert_eq!(grid(&launcher), first_grid);
    assert_eq!(launcher.frame().theme_choice, Some(ThemeChoice::System));
}

/// The preview of today's image in the feed, Barn owls (ONB-01).
const PREVIEW_URL: &str =
    "https://www.bing.com/th?id=OHR.BarnOwlsUnite_EN-US9661992435_800x480.jpg";

/// MeeGone just installed, at 08:00 on 19 November, on Wi-Fi: no file of its own yet, a few
/// apps, and Bing's feed serving today's image, its preview and its largest version. The home
/// shares its stores with the daily image's job (ADR 0029), and has just come to the
/// foreground.
fn fresh_install() -> (Fakes, Shared, Launcher) {
    let fakes = Fakes::default();
    *fakes.system.installed_ms.lock().unwrap() = NOV_19_MS + 7 * HOUR_MS;
    *fakes.system.now_ms.lock().unwrap() = NOV_19_MS + 8 * HOUR_MS;
    *fakes.apps.apps.lock().unwrap() = vec![
        app("maps", "Maps"),
        app("calendar", "Calendar"),
        app("bank", "Bank"),
        app("mail", "Mail"),
    ];
    serve_todays_image(&fakes);
    fakes.network.responses.lock().unwrap().insert(
        PREVIEW_URL.into(),
        fakes::jpeg(Size {
            width: 800,
            height: 480,
        }),
    );
    let shared = Shared::new(fakes.platform());
    let mut launcher = Launcher::start_shared(&shared);
    launcher.dispatch(ShellIntent::Layout(phone()));
    launcher.dispatch(ShellIntent::Resumed);
    (fakes, shared, launcher)
}

fn setup_step(launcher: &mut Launcher) -> Option<SetupStep> {
    launcher.frame().setup.map(|setup| setup.step)
}

/// The job's thread runs what the home asked of it, such as reading today's image for setup,
/// then tells the home (ADR 0032, ADR 0036).
fn run_image_tasks(shared: &Shared, launcher: &mut Launcher) {
    run_update_now(shared, launcher);
}

#[test]
fn ac01_given_a_fresh_install_on_wifi_when_every_setup_default_is_accepted_then_meegone_is_home_in_system_theme_on_applications_with_todays_image_on_the_lock_screen()
 {
    // AC-01, issue #35: ONB-01, ONB-03, ONB-05, THEME-01, WALL-01, HOME-05, APPS-04. No time
    // passes on the device's clock: the lock screen has today's image long before 60 s.
    let (fakes, shared, mut launcher) = fresh_install();
    let started = *fakes.system.now_ms.lock().unwrap();
    assert_eq!(setup_step(&mut launcher), Some(SetupStep::Welcome));

    launcher.view().setup().continue_tapped();
    assert_eq!(setup_step(&mut launcher), Some(SetupStep::DefaultHome));
    launcher
        .view()
        .setup()
        .allow_tapped(AccessItem::DefaultHome);
    launcher.frame();
    assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 1);
    // MeeGone is chosen in the system's dialog, which closes: MeeGone resumes.
    *fakes.system.default_home.lock().unwrap() = true;
    launcher.dispatch(ShellIntent::Resumed);

    assert_eq!(setup_step(&mut launcher), Some(SetupStep::LockScreenImage));
    run_image_tasks(&shared, &mut launcher);
    let frame = launcher.frame();
    assert_eq!(frame.setup_image_switch, Some(DailyImage::On));
    assert_eq!(frame.region, Some(0), "Automatic");
    assert!(
        launcher
            .view()
            .image_setup()
            .preview(frame.setup_image.unwrap())
            .is_some()
    );
    assert_eq!(
        lock_screen(&fakes),
        None,
        "nothing before Continue (WALL-02)"
    );
    launcher.view().setup().continue_tapped();
    assert_eq!(setup_step(&mut launcher), Some(SetupStep::Access));
    // The first check runs at once on the job's thread, without waiting for the scheduler.
    run_image_tasks(&shared, &mut launcher);
    assert!(lock_screen(&fakes).is_some());

    for item in [AccessItem::Notifications, AccessItem::Usage] {
        launcher.view().setup().allow_tapped(item);
        launcher.frame();
        match item {
            AccessItem::Notifications => {
                *fakes.notifications.access.lock().unwrap() = Access::Granted;
            }
            _ => *fakes.usage.access.lock().unwrap() = Access::Granted,
        }
        launcher.dispatch(ShellIntent::Resumed);
    }

    let frame = launcher.frame();
    assert_eq!(frame.setup, None);
    assert_eq!(frame.current, HomeView::Applications);
    assert_eq!(frame.theme_choice, Some(ThemeChoice::System));
    assert!(launcher.shell().settings().state().default_home);
    assert_eq!(grid(&launcher), ["Bank", "Calendar", "Mail", "Maps"]);
    let current = launcher
        .shell()
        .lockimage()
        .state()
        .current
        .clone()
        .unwrap();
    assert_eq!(current.title, "Hunting from dusk till dawn");
    assert_eq!(*fakes.system.now_ms.lock().unwrap(), started);
}

#[test]
fn onb01_setup_is_welcome_the_default_home_app_the_lock_screen_image_and_access_without_a_theme_question()
 {
    // Issue #35: ONB-01, ONB-05. At most four screens; System applies by default.
    let (fakes, shared, mut launcher) = fresh_install();
    let mut steps = vec![setup_step(&mut launcher).unwrap()];

    launcher.view().setup().continue_tapped();
    steps.push(setup_step(&mut launcher).unwrap());
    launcher
        .view()
        .setup()
        .not_now_tapped(AccessItem::DefaultHome);
    steps.push(setup_step(&mut launcher).unwrap());
    run_image_tasks(&shared, &mut launcher);
    let frame = launcher.frame();
    let content = launcher.view().image_setup().content().unwrap();
    let today = content.today.unwrap();
    assert_eq!(today.title, "Hunting from dusk till dawn");
    assert_eq!(
        today.attribution,
        "Pair of barn owls (© Zahoor Salmi/Getty Images)"
    );
    assert!(!content.loading);
    assert_eq!(frame.setup_image_switch, Some(DailyImage::On));
    assert_eq!(frame.region, Some(0));
    launcher.view().setup().continue_tapped();
    steps.push(setup_step(&mut launcher).unwrap());

    assert_eq!(
        steps,
        [
            SetupStep::Welcome,
            SetupStep::DefaultHome,
            SetupStep::LockScreenImage,
            SetupStep::Access
        ]
    );
    assert_eq!(frame.theme_choice, Some(ThemeChoice::System));
    assert_eq!(launcher.shell().theme().state().theme, Theme::Light);
    // Only the feed and today's preview were asked for before Continue.
    assert_eq!(
        fakes.network.requests.lock().unwrap()[..2],
        [FEED_URL, PREVIEW_URL]
    );
}

#[test]
fn onb02_given_not_now_on_both_access_requests_then_setup_completes_and_apps_launch_normally() {
    // Issue #35: ONB-02. Each request says what is read and why; declining never blocks.
    let (fakes, _, mut launcher) = fresh_install();
    launcher.view().setup().continue_tapped();
    launcher
        .view()
        .setup()
        .not_now_tapped(AccessItem::DefaultHome);
    launcher.view().setup().continue_tapped();
    assert_eq!(setup_step(&mut launcher), Some(SetupStep::Access));

    launcher
        .view()
        .setup()
        .not_now_tapped(AccessItem::Notifications);
    let frame = launcher.frame();
    assert!(frame.setup.unwrap().notifications_put_off);
    launcher.view().setup().not_now_tapped(AccessItem::Usage);
    let frame = launcher.frame();
    assert_eq!(frame.setup, None);
    assert_eq!(frame.current, HomeView::Applications);

    launcher.view().apps().icon_tapped(app_id("mail"), None);
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
    assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 0);
    assert_eq!(*fakes.usage.access_screen_opened.lock().unwrap(), 0);

    // Done on this install: a restart goes straight to the home.
    let mut restarted = Launcher::start(fakes.platform());
    assert_eq!(restarted.frame().setup, None);
}

#[test]
fn wall02_given_setup_is_left_before_continue_on_the_image_screen_then_the_lock_screen_is_unchanged()
 {
    // Issue #35: WALL-02. The switch starts on, today's image shows, then the user goes back,
    // and MeeGone is restarted: nothing was set, cached or scheduled, and setup starts again.
    let (fakes, shared, mut launcher) = fresh_install();
    launcher.view().setup().continue_tapped();
    launcher
        .view()
        .setup()
        .not_now_tapped(AccessItem::DefaultHome);
    assert_eq!(setup_step(&mut launcher), Some(SetupStep::LockScreenImage));
    run_image_tasks(&shared, &mut launcher);
    launcher.frame();
    assert!(
        launcher
            .view()
            .image_setup()
            .content()
            .unwrap()
            .today
            .is_some()
    );

    launcher.dispatch(ShellIntent::Back);
    assert_eq!(setup_step(&mut launcher), Some(SetupStep::DefaultHome));
    let shared = Shared::new(fakes.platform());
    let mut launcher = Launcher::start_shared(&shared);
    assert!(!shared.check_image());

    assert_eq!(setup_step(&mut launcher), Some(SetupStep::Welcome));
    assert_eq!(launcher.frame().daily_image, Some(DailyImage::Off));
    assert_eq!(lock_screen(&fakes), None);
    assert_eq!(cached_image(&fakes), None);
    assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
    assert_eq!(
        *fakes.network.requests.lock().unwrap(),
        [FEED_URL, PREVIEW_URL]
    );
}

#[test]
fn adr0036_setup_runs_once_per_install_and_again_after_a_restore_on_another_device() {
    // Issue #35, ADR 0018: settings from another install's backup run setup again; settings
    // saved before setup existed don't.
    let restored = Fakes::default();
    *restored.system.installed_ms.lock().unwrap() = 5_000;
    PersistentSettingsStore::new(restored.storage.clone())
        .update(&mut |s| {
            s.theme = ThemeChoice::Dark;
            s.setup_install_ms = Some(1_000);
        })
        .unwrap();
    let mut launcher = Launcher::start(restored.platform());
    assert_eq!(setup_step(&mut launcher), Some(SetupStep::Welcome));
    assert_eq!(launcher.frame().theme_choice, Some(ThemeChoice::Dark));

    let upgraded = Fakes::default();
    *upgraded.system.installed_ms.lock().unwrap() = 5_000;
    upgraded.storage.files.lock().unwrap().insert(
        StoredFile::Settings,
        include_bytes!("../crates/core/data/fixtures/settings-v3.txt").to_vec(),
    );
    let mut launcher = Launcher::start(upgraded.platform());
    assert_eq!(setup_step(&mut launcher), None);
}
