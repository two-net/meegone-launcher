//! Becoming the default home app (issue #4: HOME-01, ONB-04), on the whole app with fake
//! platform adapters. One test per acceptance criterion of the issue, except "when Home is
//! pressed from any app, then MeeGone shows", which needs a device (ADR 0005).

use meegone_core_data::fakes::{Fakes, app, app_id};
use meegone_launcher::{Launcher, ShellIntent};

const HOUR_MS: u64 = 60 * 60 * 1000;
/// Midnight UTC on some day. The fake device is on UTC.
const MIDNIGHT: u64 = 20_000 * 24 * HOUR_MS;

fn banner_shown(launcher: &Launcher) -> bool {
    launcher.view().home_banner().is_visible()
}

/// MeeGone comes to the foreground at `now_ms`.
fn resume_at(fakes: &Fakes, launcher: &mut Launcher, now_ms: u64) {
    *fakes.system.now_ms.lock().unwrap() = now_ms;
    launcher.dispatch(ShellIntent::Resumed);
}

#[test]
fn the_banner_shows_while_not_the_default_and_once_dismissed_stays_away_that_day() {
    let fakes = Fakes::default();
    let mut launcher = Launcher::start(fakes.platform());
    resume_at(&fakes, &mut launcher, MIDNIGHT + 9 * HOUR_MS);
    assert!(banner_shown(&launcher));

    launcher.view().home_banner().dismissed();
    launcher.frame();
    assert!(!banner_shown(&launcher));

    // Later that day, even after MeeGone was restarted.
    let mut launcher = Launcher::start(fakes.platform());
    resume_at(&fakes, &mut launcher, MIDNIGHT + 23 * HOUR_MS);
    assert!(!banner_shown(&launcher));

    resume_at(&fakes, &mut launcher, MIDNIGHT + 33 * HOUR_MS);
    assert!(banner_shown(&launcher));
}

#[test]
fn picking_meegone_in_the_system_dialog_removes_the_banner() {
    let fakes = Fakes::default();
    let mut launcher = Launcher::start(fakes.platform());
    resume_at(&fakes, &mut launcher, MIDNIGHT + 9 * HOUR_MS);
    assert!(banner_shown(&launcher));

    launcher.view().home_banner().make_default_tapped();
    launcher.frame();
    assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 1);

    // The user picks MeeGone; the dialog closes and MeeGone is in the foreground again.
    *fakes.system.default_home.lock().unwrap() = true;
    resume_at(&fakes, &mut launcher, MIDNIGHT + 9 * HOUR_MS + 1);
    assert!(launcher.shell().settings().state().default_home);
    assert!(!banner_shown(&launcher));

    // It stays away on later days too, because MeeGone is now the default.
    resume_at(&fakes, &mut launcher, MIDNIGHT + 33 * HOUR_MS);
    assert!(!banner_shown(&launcher));
}

#[test]
fn settings_access_opens_the_system_dialog() {
    // SET-02: the Access group's default-home-app item.
    let fakes = Fakes::default();
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::OpenSettings);

    launcher.view().settings().default_home_tapped();
    launcher.frame();

    assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 1);
}

#[test]
fn without_the_role_meegone_still_runs_as_a_normal_app() {
    // HOME-01: shortcuts are the only thing missing; the platform offers them only to the
    // default home app, and the long-press menu (#12) shows none.
    let fakes = Fakes::default();
    fakes.apps.apps.lock().unwrap().push(app("mail", "Mail"));
    let mut launcher = Launcher::start(fakes.platform());
    assert!(!launcher.shell().settings().state().default_home);

    launcher.view().apps().icon_tapped(app_id("mail"), None);
    launcher.frame();

    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
}
