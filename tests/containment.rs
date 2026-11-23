//! A bug in one feature stops that feature alone, wherever its code runs, and launching apps
//! keeps working (REL-01, ARCH-09, ADR 0043). Each test arms a fault: the feature's next
//! contained step panics, as a bug would.

use meegone_core_data::fakes::{Fakes, app, app_id, notification, used};
use meegone_core_data::storage::format::seal;
use meegone_core_data::{Access, StoredFile};
use meegone_core_state::{Health, faults};
use meegone_launcher::{
    Direction, Feature, HomeView, Launcher, PinchEvent, PinchKind, SetupStep, ShellIntent,
    StoppedCards,
};

/// MeeGone after setup with Mail and Maps, a notification from Mail, and recent use of Maps.
fn with_everything() -> (Fakes, Launcher) {
    let fakes = Fakes::after_setup();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("maps", "Maps")];
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![notification("m1", "mail")];
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.usage.uses.lock().unwrap() = vec![used("maps", 1)];
    let launcher = Launcher::start(fakes.platform());
    (fakes, launcher)
}

/// Goes to Applications and taps `id`'s icon: it opens (REL-01).
fn applications_launch(fakes: &Fakes, launcher: &mut Launcher, id: &str) {
    launcher.dispatch(ShellIntent::Home);
    launcher.frame();
    launcher.view().apps().icon_tapped(app_id(id), None);
    launcher.frame();
    assert_eq!(
        fakes.apps.launched.lock().unwrap().last(),
        Some(&app_id(id))
    );
}

/// The image cache holding today's image, as the daily image's updater leaves it.
fn image_cached(fakes: &Fakes) {
    let mut files = fakes.storage.files.lock().unwrap();
    files.insert(
        StoredFile::ImageDetails,
        seal(
            "image-cache",
            2,
            "current 20261116 en-GB Puffins\\son\\sa\\scliff,\\sSkomer \
             ©\\sA\\sPhotographer\\s/\\sAgency - 1763272800000 a\nprevious -\n",
        ),
    );
    files.insert(StoredFile::ImageA, b"\xff\xd8 a JPEG".to_vec());
}

#[test]
fn rel01_given_events_panics_while_drawn_then_it_shows_only_its_card_in_that_frame_and_applications_launches()
 {
    let (fakes, mut launcher) = with_everything();
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert!(launcher.frame().events_list.is_some());

    faults::arm("EventsHolder");
    let frame = launcher.frame();

    assert_eq!(frame.events_header, None);
    assert_eq!(frame.events_list, None);
    assert!(frame.failed.contains(Feature::Events));
    assert_eq!(
        frame.stopped,
        StoppedCards {
            events: true,
            ..StoppedCards::default()
        }
    );
    assert!(!launcher.needs_redraw());
    assert_eq!(launcher.shell().recent().health(), Health::Ready);
    applications_launch(&fakes, &mut launcher, "mail");
}

#[test]
fn rel01_given_recent_panics_while_drawn_then_it_shows_only_its_card_and_applications_launches() {
    let (fakes, mut launcher) = with_everything();
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    assert!(launcher.frame().recent.is_some());

    faults::arm("RecentHolder");
    let frame = launcher.frame();

    assert_eq!(frame.recent, None);
    assert_eq!(frame.recent_window, None);
    assert!(frame.stopped.recent);
    assert!(!frame.stopped.events);
    // Its Settings section goes, and so does Reset launcher, which would reset it.
    assert!(!frame.reset_offered);
    applications_launch(&fakes, &mut launcher, "maps");
}

#[test]
fn rel01_given_recent_panics_as_a_pinch_starts_then_recent_stops_and_applications_launches() {
    let (fakes, mut launcher) = with_everything();
    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    launcher.frame();

    faults::arm("RecentHolder");
    launcher.dispatch(ShellIntent::Pinch(PinchEvent {
        kind: PinchKind::Start,
        span_px: 300.0,
        x: 500.0,
        y: 900.0,
        at: std::time::Duration::ZERO,
    }));

    assert_eq!(launcher.shell().recent().health(), Health::Failed);
    assert!(!launcher.shell().state().pinch.holding);
    assert!(launcher.frame().stopped.recent);
    applications_launch(&fakes, &mut launcher, "mail");
}

#[test]
fn rel01_given_the_lock_screen_image_panics_while_its_details_show_then_they_close_in_that_frame_and_applications_launches()
 {
    let (fakes, mut launcher) = with_everything();
    image_cached(&fakes);
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().image_settings().daily_image_toggled(true);
    launcher.frame();
    launcher.dispatch(ShellIntent::Back);
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    let revision = launcher.frame().image_card.expect("the card shows");
    assert!(launcher.view().image_card().tapped(revision));
    assert!(launcher.frame().image_details_open);

    faults::arm("LockImageHolder");
    let frame = launcher.frame();

    assert!(!frame.image_details_open);
    assert_eq!(frame.image_details, None);
    assert_eq!(frame.image_card, None);
    assert_eq!(frame.daily_image, None);
    // Its card in Events, which showed, gives way to the stopped card, and so does its group
    // in Settings.
    assert!(frame.stopped.image_in_events);
    assert!(frame.stopped.image_in_settings);
    assert!(frame.events_list.is_some());
    applications_launch(&fakes, &mut launcher, "mail");
}

#[test]
fn arch09_the_lock_screen_images_card_replaces_its_events_card_only_if_that_was_showing() {
    let (fakes, mut launcher) = with_everything();

    faults::arm("LockImageHolder");
    let frame = launcher.frame();

    // Its daily image was off, so it had no card in Events.
    assert!(!frame.stopped.image_in_events);
    assert!(frame.stopped.image_in_settings);
    applications_launch(&fakes, &mut launcher, "mail");
}

#[test]
fn arch09_given_settings_panics_while_setup_shows_then_setup_ends_and_applications_launches() {
    let fakes = Fakes::default();
    fakes.apps.apps.lock().unwrap().push(app("mail", "Mail"));
    let mut launcher = Launcher::start(fakes.platform());
    assert_eq!(
        launcher.frame().setup.map(|setup| setup.step),
        Some(SetupStep::Welcome)
    );

    faults::arm("SettingsHolder");
    let frame = launcher.frame();

    assert_eq!(frame.setup, None);
    assert_eq!(frame.theme_choice, None);
    assert!(frame.stopped.settings);
    // The banner offering to make MeeGone the default home app is Settings' too.
    assert!(!frame.home_banner);
    assert_eq!(frame.current, HomeView::Applications);
    applications_launch(&fakes, &mut launcher, "mail");
}

#[test]
fn arch09_given_the_host_reports_events_failed_then_the_frame_says_so() {
    // The host found a bug in Events' own views (ADR 0043).
    let (fakes, mut launcher) = with_everything();
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    launcher.frame();

    launcher.dispatch(ShellIntent::Failed(Feature::Events));
    let frame = launcher.frame();

    assert_eq!(launcher.shell().events().health(), Health::Failed);
    assert!(frame.failed.contains(Feature::Events));
    assert!(frame.stopped.events);
    assert_eq!(frame.events_list, None);
    applications_launch(&fakes, &mut launcher, "mail");
}

#[test]
fn rel01_given_applications_fails_then_a_tap_in_its_grid_still_launches_and_a_card_offers_a_restart()
 {
    // ADR 0043: launching apps works whatever else fails.
    let (fakes, mut launcher) = with_everything();
    launcher.frame();
    assert!(launcher.frame().home_banner);

    faults::arm("AppsHolder");
    launcher.dispatch(ShellIntent::AppsChanged);
    let frame = launcher.frame();

    assert_eq!(launcher.shell().apps().health(), Health::Failed);
    assert!(frame.failed.contains(Feature::Applications));
    assert!(frame.stopped.applications);
    // In the banner's place.
    assert!(!frame.home_banner);
    applications_launch(&fakes, &mut launcher, "maps");
    applications_launch(&fakes, &mut launcher, "mail");
    // Elsewhere, or with something over it, its card doesn't show.
    launcher.dispatch(ShellIntent::Swipe(Direction::Previous));
    assert!(!launcher.frame().stopped.applications);
}

#[test]
fn arch09_one_failure_leaves_every_other_feature_working() {
    let (fakes, mut launcher) = with_everything();
    faults::arm("EventsHolder");
    launcher.dispatch(ShellIntent::NotificationsChanged);

    launcher.dispatch(ShellIntent::Swipe(Direction::Next));
    let frame = launcher.frame();

    assert_eq!(
        launcher
            .shell()
            .failures()
            .since(Default::default())
            .collect::<Vec<_>>(),
        [Feature::Events]
    );
    assert!(frame.recent.is_some());
    assert!(frame.theme_choice.is_some());
    assert!(frame.daily_image.is_some());
    applications_launch(&fakes, &mut launcher, "mail");
}
