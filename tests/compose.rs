//! The whole app, composed with fake platform adapters.

use meegone_core_data::ThemeChoice;
use meegone_core_data::fakes::{Fakes, app, app_id};
use meegone_core_state::Theme;
use meegone_launcher::{Launcher, ShellIntent};

#[test]
fn a_tap_in_a_view_reaches_the_platform_through_the_state_layer() {
    // UI → state → data (ARCH-02, ARCH-05).
    let fakes = Fakes::default();
    fakes.apps.apps.lock().unwrap().push(app("mail", "Mail"));
    let mut launcher = Launcher::start(fakes.platform());

    launcher.view().apps().icon_tapped(app_id("mail"));
    assert!(fakes.apps.launched.lock().unwrap().is_empty());
    launcher.frame();

    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
}

#[test]
fn a_theme_chosen_in_settings_applies_to_the_whole_app() {
    // THEME-01, THEME-03
    let fakes = Fakes::default();
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::OpenSettings);

    launcher.view().settings().theme_chosen(ThemeChoice::Dark);
    launcher.frame();

    assert_eq!(launcher.shell().theme().state().theme, Theme::Dark);
}
