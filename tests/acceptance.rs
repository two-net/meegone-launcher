//! The acceptance scenarios of SRS section 9, run on the whole app with fake platform adapters
//! (MAINT-03). Each test carries its scenario's id; each feature adds its own here.
//!
//! Not automated yet, because the behaviour isn't built:
//! - AC-01: first-run setup (#35)
//! - AC-05's open folder, AC-08: folders (#14)
//! - AC-07, AC-09: edit mode and a persisted layout (#13, #11)
//! - AC-12: Clear in Events (#20)
//! - AC-13, AC-14: Recent density and close mode (#23, #24)
//! - AC-15 to AC-17: the daily image (#27 to #30)
//!
//! AC-18 to AC-20 (animation scale, screen reader, timing) need a device; they belong to the
//! device matrix (#43). Their core logic is tested without one: the carousel with animations
//! removed and its screen-reader actions in `meegone-shell-state`.

use std::time::Duration;

use meegone_core_data::fakes::{Fakes, WORK_PROFILE, app, app_id, work_app};
use meegone_core_data::{
    App, DataError, HomeTarget, Notification, Profile, ProfileId, RecentUse, Rect, Rgb,
    ThemeChoice, accent,
};
use meegone_core_state::{Access, Health, Theme};
use meegone_core_ui::tokens;
use meegone_launcher::{
    Direction, HomeView, Launcher, Layout, MenuItem, PointerEvent, PointerKind, SettingsGroup,
    ShellIntent,
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
    launcher.view().apps().grid_scrolled(600.0);
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
    launcher.view().apps().grid_scrolled(bottom);
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
    launcher.view().apps().grid_scrolled(600.0);
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
        launcher.view().apps().grid_scrolled(600.0);
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
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("phone", "Phone")];
    let mut launcher = Launcher::start(fakes.platform());
    assert_eq!(grid(&launcher), ["Mail", "Phone"]);

    fakes.apps.apps.lock().unwrap().push(app("atlas", "Atlas"));
    launcher.dispatch(ShellIntent::AppsChanged);

    assert_eq!(grid(&launcher), ["Mail", "Phone", "Atlas"]);
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
        .cells()
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
        },
        Profile {
            id: WORK_PROFILE,
            work: true,
            paused,
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
        launcher
            .view()
            .apps()
            .grid_laid_out(grid_dp * density, 112.0 * density, density);
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
    assert!(launcher.view().apps().cell_tapped(revision, 0, icon));
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [paused.id.clone()]);
    assert_eq!(*fakes.apps.launched_from.lock().unwrap(), [Some(icon)]);
    assert_eq!(launcher.shell().apps().state().problem, None);

    *fakes.apps.error.lock().unwrap() = Some(DataError::Unsupported("App isn't available".into()));
    assert!(launcher.view().apps().cell_tapped(revision, 0, icon));
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
