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

use meegone_core_data::fakes::{Fakes, app, app_id};
use meegone_core_data::{HomeTarget, Notification, RecentUse, ThemeChoice};
use meegone_core_state::{Access, Health, Theme};
use meegone_launcher::{
    Direction, HomeView, Launcher, Layout, PointerEvent, PointerKind, ShellIntent,
};

/// A phone 1080 px wide at 2.75 px per dp.
const WIDTH: f64 = 1080.0;

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

/// A launcher laid out on a phone, with the time at 0 ms.
fn on_a_phone(fakes: &Fakes) -> (Launcher, u64) {
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Layout(Layout {
        width_px: WIDTH,
        density: 2.75,
        touch_slop_px: 22.0,
        rtl: false,
    }));
    (launcher, 0)
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
    while launcher.frame().animating {
        clock += 8;
        launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
    }
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
    launcher.view().apps().icon_tapped(app_id("mail"));
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
}
