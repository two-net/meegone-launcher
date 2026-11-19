use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use meegone_core_ui::{Dispatch, Latest};
use meegone_lockimage_state::{
    CheckStatus, ImageFile, ImageRecord, ImageStatus, LockImageIntent, LockImageState, PastEntry,
};

use super::*;

fn record(date: &str, image: Option<ImageFile>) -> ImageRecord {
    ImageRecord {
        date: date.into(),
        region: "en-GB".into(),
        title: "Puffins on a cliff, Skomer".into(),
        attribution: "© A Photographer / Agency".into(),
        info_link: None,
        applied_at_ms: None,
        image,
        id: None,
        marker: None,
        pinned: false,
    }
}

/// The daily image on, showing its card, with today's image in file A.
fn on() -> LockImageState {
    LockImageState {
        status: ImageStatus::On,
        current: Some(record("20261116", Some(ImageFile::A))),
        show_card: true,
        ..LockImageState::default()
    }
}

type Sent = Rc<RefCell<Vec<LockImageIntent>>>;

fn card(state: LockImageState) -> (Latest<LockImageState>, ImageCard, Sent) {
    let latest = Latest::new(Arc::new(state));
    let sent: Sent = Rc::default();
    let dispatch = Dispatch::new({
        let sent = sent.clone();
        move |intent| sent.borrow_mut().push(intent)
    });
    (latest.clone(), ImageCard::new(latest, dispatch), sent)
}

#[test]
fn the_card_shows_today_s_thumbnail_title_and_attribution() {
    // EVT-12, LEGAL-03
    let (_, card, _) = card(on());

    assert!(card.is_visible());
    card.revision();
    assert_eq!(
        card.content(),
        Some(CardContent {
            title: "Puffins on a cliff, Skomer".into(),
            attribution: "© A Photographer / Agency".into(),
            image_path: "images/a",
            image_key: "20261116 en-GB images/a".into(),
        })
    );
}

#[test]
fn the_card_shows_only_while_the_daily_image_is_on_with_an_image_and_not_hidden() {
    // EVT-12
    let hidden = [
        LockImageState {
            status: ImageStatus::Off,
            ..on()
        },
        LockImageState {
            status: ImageStatus::Unavailable("Blocked by device policy".into()),
            ..on()
        },
        LockImageState {
            show_card: false,
            ..on()
        },
        LockImageState {
            current: None,
            ..on()
        },
        LockImageState {
            current: Some(record("20261116", None)),
            ..on()
        },
    ];

    for state in hidden {
        let (_, card, _) = card(state.clone());
        assert!(!card.is_visible(), "{state:?}");
        assert_eq!(card.revision(), 0);
        assert_eq!(card.content(), None);
    }
}

#[test]
fn the_revision_changes_only_when_what_the_card_shows_does() {
    // ANIM-22: a new image is a new revision, and the same image never is.
    let (latest, card, _) = card(on());
    let first = card.revision();
    latest.set(Arc::new(LockImageState {
        problem: Some("No connection".into()),
        ..on()
    }));
    assert_eq!(card.revision(), first);

    latest.set(Arc::new(LockImageState {
        current: Some(record("20261117", Some(ImageFile::B))),
        ..on()
    }));
    let second = card.revision();

    assert_ne!(second, first);
    assert_eq!(
        card.content().map(|content| content.image_key),
        Some("20261117 en-GB images/b".into())
    );
}

#[test]
fn a_hidden_card_keeps_what_it_last_showed() {
    // EVT-12: hiding and showing the same image again needs no new thumbnail.
    let (latest, card, _) = card(on());
    let shown = card.revision();

    latest.set(Arc::new(LockImageState {
        show_card: false,
        ..on()
    }));
    assert_eq!(card.revision(), shown);
    latest.set(Arc::new(on()));

    assert_eq!(card.revision(), shown);
}

#[test]
fn a_tap_on_the_card_opens_the_image_details() {
    // EVT-12, WALL-15
    let (_, card, sent) = card(on());
    let revision = card.revision();

    assert!(card.tapped(revision));

    assert!(matches!(sent.borrow()[..], [LockImageIntent::OpenDetails]));
}

#[test]
fn a_tap_on_a_card_that_changed_or_went_does_nothing() {
    let (latest, card, sent) = card(on());
    let old = card.revision();
    latest.set(Arc::new(LockImageState {
        current: Some(record("20261117", Some(ImageFile::B))),
        ..on()
    }));
    let new = card.revision();

    assert!(!card.tapped(old));
    latest.set(Arc::new(LockImageState {
        show_card: false,
        ..on()
    }));
    assert!(!card.tapped(new));

    assert!(sent.borrow().is_empty());
}

#[test]
fn clones_share_what_the_host_was_told() {
    let (_, card, _) = card(on());
    let other = card.clone();

    let revision = card.revision();

    assert_eq!(other.revision(), revision);
    assert!(other.content().is_some());
}

#[test]
fn settings_show_and_switch_the_card() {
    // EVT-12, SET-02
    let latest = Latest::new(Arc::new(LockImageState {
        show_card: false,
        ..on()
    }));
    let sent: Sent = Rc::default();
    let section = ImageSettingsSection::new(
        latest,
        Dispatch::new({
            let sent = sent.clone();
            move |intent| sent.borrow_mut().push(intent)
        }),
    );

    assert!(!section.show_card());
    section.show_card_toggled(true);

    assert!(matches!(
        sent.borrow()[..],
        [LockImageIntent::SetShowCard(true)]
    ));
}

#[test]
fn settings_show_and_choose_the_region() {
    // WALL-05, SET-02
    let latest = Latest::new(Arc::new(LockImageState {
        regions: vec!["en-US".into(), "de-DE".into(), "ja-JP".into()],
        region: Some("ja-JP".into()),
        automatic_region: "de-DE".into(),
        ..on()
    }));
    let sent: Sent = Rc::default();
    let section = ImageSettingsSection::new(
        latest,
        Dispatch::new({
            let sent = sent.clone();
            move |intent| sent.borrow_mut().push(intent)
        }),
    );

    assert_eq!(section.regions(), ["en-US", "de-DE", "ja-JP"]);
    assert_eq!(section.region().as_deref(), Some("ja-JP"));
    assert_eq!(section.automatic_region(), "de-DE");
    assert_eq!(section.region_choice(), 3);
    section.region_chosen(None);
    section.region_chosen(Some("en-US".into()));

    assert!(matches!(
        &sent.borrow()[..],
        [
            LockImageIntent::SetRegion(None),
            LockImageIntent::SetRegion(Some(region)),
        ] if region == "en-US"
    ));
}

#[test]
fn automatic_is_the_first_region_choice() {
    // WALL-05: and so is a region the feed no longer offers.
    for region in [None, Some("fi-FI".to_owned())] {
        let latest = Latest::new(Arc::new(LockImageState {
            regions: vec!["en-US".into()],
            region,
            ..on()
        }));
        let section = ImageSettingsSection::new(latest, Dispatch::new(|_| {}));

        assert_eq!(section.region_choice(), 0);
    }
}

fn section(state: LockImageState) -> (Latest<LockImageState>, ImageSettingsSection, Sent) {
    let latest = Latest::new(Arc::new(state));
    let sent: Sent = Rc::default();
    let dispatch = Dispatch::new({
        let sent = sent.clone();
        move |intent| sent.borrow_mut().push(intent)
    });
    (
        latest.clone(),
        ImageSettingsSection::new(latest, dispatch),
        sent,
    )
}

#[test]
fn settings_show_and_switch_use_mobile_data() {
    // WALL-04, SET-02
    let (_, section, sent) = section(LockImageState {
        use_mobile_data: true,
        ..on()
    });

    assert!(section.use_mobile_data());
    section.use_mobile_data_toggled(false);

    assert!(matches!(
        sent.borrow()[..],
        [LockImageIntent::SetUseMobileData(false)]
    ));
}

#[test]
fn settings_say_the_daily_image_is_unavailable_after_3_days_of_failures() {
    // SRS 7.6
    let unavailable = CheckStatus::Unavailable("No connection".into());
    let (latest, section, sent) = section(LockImageState {
        checks: unavailable.clone(),
        ..on()
    });

    assert!(section.daily_image_unavailable());
    assert_eq!(section.checks(), unavailable);
    section.try_again();
    assert!(matches!(sent.borrow()[..], [LockImageIntent::TryAgain]));

    // Not while it is only retrying, or while the daily image is off.
    latest.set(Arc::new(LockImageState {
        checks: CheckStatus::Retrying("No connection".into()),
        ..on()
    }));
    assert!(!section.daily_image_unavailable());
    latest.set(Arc::new(LockImageState {
        status: ImageStatus::Off,
        checks: unavailable,
        ..on()
    }));
    assert!(!section.daily_image_unavailable());
}

#[test]
fn settings_switch_the_daily_image_and_say_why_it_cant_be_used() {
    // WALL-02, WALL-13
    let (latest, section, sent) = section(LockImageState::default());
    assert_eq!(section.daily_image(), DailyImage::Off);
    assert_eq!(section.unavailable_reason(), None);

    section.daily_image_toggled(true);
    assert!(matches!(
        sent.borrow()[..],
        [LockImageIntent::SetEnabled(true)]
    ));

    latest.set(Arc::new(on()));
    assert_eq!(section.daily_image(), DailyImage::On);

    latest.set(Arc::new(LockImageState {
        status: ImageStatus::Unavailable("Blocked by device policy".into()),
        ..LockImageState::default()
    }));
    assert_eq!(section.daily_image(), DailyImage::Unavailable);
    assert_eq!(
        section.unavailable_reason().as_deref(),
        Some("Blocked by device policy")
    );
    assert!(!section.is_switch_enabled());
}

#[test]
fn settings_offer_update_now_while_the_daily_image_is_on() {
    // WALL-11
    let (latest, section, sent) = section(on());
    assert!(section.update_now_offered());
    assert!(!section.updating());

    section.update_now();
    assert!(matches!(sent.borrow()[..], [LockImageIntent::UpdateNow]));

    latest.set(Arc::new(LockImageState {
        updating: true,
        ..on()
    }));
    assert!(section.updating());

    latest.set(Arc::new(LockImageState::default()));
    assert!(!section.update_now_offered());
}

#[test]
fn settings_say_when_updates_are_paused_or_the_image_may_not_show() {
    // WALL-14, SRS C5
    let (latest, section, _) = section(LockImageState {
        paused: true,
        may_not_show: true,
        ..on()
    });
    assert!(section.paused());
    assert!(section.may_not_show());

    // Not once the daily image is off.
    latest.set(Arc::new(LockImageState {
        status: ImageStatus::Off,
        paused: true,
        may_not_show: true,
        ..on()
    }));
    assert!(!section.paused());
    assert!(!section.may_not_show());
}

#[test]
fn once_off_settings_offer_the_reset_and_the_wallpaper_settings() {
    // WALL-12
    let (latest, section, sent) = section(LockImageState::default());
    assert!(section.off_actions_offered());

    section.reset_lock_screen();
    section.open_wallpaper_settings();
    assert!(matches!(
        sent.borrow()[..],
        [
            LockImageIntent::ResetLockScreen,
            LockImageIntent::OpenWallpaperSettings
        ]
    ));

    latest.set(Arc::new(on()));
    assert!(!section.off_actions_offered());
    latest.set(Arc::new(LockImageState {
        status: ImageStatus::Unavailable("No wallpapers".into()),
        ..LockImageState::default()
    }));
    assert!(!section.off_actions_offered());
}

fn details(state: LockImageState) -> (Latest<LockImageState>, ImageDetailsScreen, Sent) {
    let latest = Latest::new(Arc::new(state));
    let sent: Sent = Rc::default();
    let dispatch = Dispatch::new({
        let sent = sent.clone();
        move |intent| sent.borrow_mut().push(intent)
    });
    (
        latest.clone(),
        ImageDetailsScreen::new(latest, dispatch),
        sent,
    )
}

/// [`on`], with a date written and an image with an info link, and a browser to open it.
fn on_with_link() -> LockImageState {
    let mut state = on();
    let current = state.current.as_mut().unwrap();
    current.info_link = Some("https://example.com/puffins".into());
    state.date = "16 November".into();
    state.has_browser = true;
    state
}

#[test]
fn the_details_show_the_image_its_title_attribution_date_and_region() {
    // WALL-15, LEGAL-03
    let (_, details, _) = details(on_with_link());

    assert!(details.is_offered());
    details.revision();
    assert_eq!(
        details.content(),
        Some(DetailsContent {
            title: "Puffins on a cliff, Skomer".into(),
            attribution: "© A Photographer / Agency".into(),
            date: "16 November".into(),
            region: "en-GB".into(),
            image_path: "images/a",
            image_key: "20261116 en-GB images/a".into(),
            more_info: true,
            pinned: false,
        })
    );
}

#[test]
fn the_details_are_offered_only_while_the_daily_image_is_on_and_has_an_image() {
    // WALL-15, ARCH-09: the hidden card doesn't hide them.
    let shown = |state: LockImageState| details(state).1.is_offered();
    assert!(shown(LockImageState {
        show_card: false,
        ..on()
    }));

    assert!(!shown(LockImageState {
        status: ImageStatus::Off,
        ..on()
    }));
    assert!(!shown(LockImageState {
        status: ImageStatus::Unavailable("Blocked".into()),
        ..on()
    }));
    assert!(!shown(LockImageState {
        current: None,
        ..on()
    }));
    assert!(!shown(LockImageState {
        current: Some(record("20261116", None)),
        ..on()
    }));
}

#[test]
fn more_info_opens_the_link_and_is_hidden_without_one_or_a_browser() {
    // WALL-15, SEC-02, SRS 8.1
    let (latest, details, sent) = details(on_with_link());
    assert!(details.more_info());
    assert!(matches!(sent.borrow()[..], [LockImageIntent::OpenInfoLink]));

    for state in [
        on(),
        LockImageState {
            has_browser: false,
            ..on_with_link()
        },
        {
            let mut state = on_with_link();
            state.current.as_mut().unwrap().info_link = Some("http://example.com".into());
            state
        },
    ] {
        latest.set(Arc::new(state));
        details.revision();
        assert!(!details.content().unwrap().more_info);
        assert!(!details.more_info());
    }
    assert_eq!(sent.borrow().len(), 1);
}

#[test]
fn settings_on_the_details_opens_settings_while_they_are_offered() {
    // WALL-15
    let (latest, details, sent) = details(on());
    assert!(details.open_settings());
    assert!(matches!(sent.borrow()[..], [LockImageIntent::OpenSettings]));

    latest.set(Arc::new(LockImageState {
        status: ImageStatus::Off,
        ..on()
    }));
    assert!(!details.open_settings());
    assert_eq!(sent.borrow().len(), 1);
}

#[test]
fn the_details_revision_changes_only_when_what_they_show_does() {
    // ADR 0031
    let (latest, details, _) = details(on());
    let first = details.revision();
    assert_eq!(details.revision(), first);

    latest.set(Arc::new(LockImageState {
        updating: true,
        ..on()
    }));
    assert_eq!(details.revision(), first);

    latest.set(Arc::new(on_with_link()));
    let second = details.revision();
    assert_ne!(second, first);
    assert!(details.content().unwrap().more_info);

    // With nothing to show, it keeps what it last showed.
    latest.set(Arc::new(LockImageState {
        status: ImageStatus::Off,
        ..on()
    }));
    assert_eq!(details.revision(), second);
    assert!(details.content().is_some());
}

#[test]
fn the_status_line_says_when_the_image_last_updated_and_the_last_problem() {
    // WALL-16, SRS 7.6
    let retrying = CheckStatus::Retrying("No connection".into());
    let (latest, section, _) = section(LockImageState {
        updated: Some("19 November, 07:00".into()),
        checks: retrying.clone(),
        ..on()
    });

    assert_eq!(
        section.update_status(),
        Some(UpdateStatus {
            updated: Some("19 November, 07:00".into()),
            checks: retrying,
            pinned: false,
        })
    );

    // Before the first image, it says so; while the daily image is off, there is none.
    latest.set(Arc::new(on()));
    assert_eq!(
        section.update_status(),
        Some(UpdateStatus {
            updated: None,
            checks: CheckStatus::Fine,
            pinned: false,
        })
    );
    latest.set(Arc::new(LockImageState {
        status: ImageStatus::Off,
        updated: Some("19 November, 07:00".into()),
        ..on()
    }));
    assert_eq!(section.update_status(), None);
}

#[test]
fn the_status_line_revision_changes_only_when_it_does() {
    // WALL-16, ADR 0031
    let (latest, section, _) = section(on());
    let first = section.status_revision();
    latest.set(Arc::new(LockImageState {
        show_card: false,
        ..on()
    }));
    assert_eq!(section.status_revision(), first);

    latest.set(Arc::new(LockImageState {
        checks: CheckStatus::Retrying("No connection".into()),
        ..on()
    }));
    assert_ne!(section.status_revision(), first);
}

/// The past week as browsed: today's image, current; barn owls; a sculpture not offered as a
/// wallpaper.
fn browsed() -> LockImageState {
    let entry = |id: &str, date: &str, offered, current| PastEntry {
        id: id.into(),
        date: date.into(),
        title: format!("Image {id}"),
        attribution: "© Someone".into(),
        offered,
        current,
        preview: None,
    };
    LockImageState {
        past: vec![
            entry("puffins", "19 November", true, true),
            entry("owls", "18 November", true, false),
            entry("sculpture", "17 November", false, false),
        ],
        ..on()
    }
}

fn past(state: LockImageState) -> (Latest<LockImageState>, PastImagesScreen, Sent) {
    let latest = Latest::new(Arc::new(state));
    let sent: Sent = Rc::default();
    let into = sent.clone();
    let screen = PastImagesScreen::new(
        latest.clone(),
        Dispatch::new(move |intent| into.borrow_mut().push(intent)),
    );
    (latest, screen, sent)
}

fn actions(screen: &PastImagesScreen) -> Vec<PastAction> {
    screen.revision();
    screen
        .content()
        .unwrap()
        .images
        .iter()
        .map(|image| image.action)
        .collect()
}

#[test]
fn the_past_week_shows_each_image_with_what_it_offers() {
    // WALL-17, LEGAL-03, LEGAL-04
    let (_, screen, _) = past(browsed());

    assert!(screen.is_offered());
    screen.revision();
    let content = screen.content().unwrap();
    assert!(!content.loading);
    assert_eq!(content.problem, None);
    assert_eq!(content.images[1].title, "Image owls");
    assert_eq!(content.images[1].attribution, "© Someone");
    assert_eq!(content.images[1].date, "18 November");
    assert!(content.images[0].current);
    assert_eq!(
        actions(&screen),
        [PastAction::Pin, PastAction::Pin, PastAction::NotOffered]
    );
}

#[test]
fn a_pinned_image_offers_unpin_and_one_on_its_way_says_so() {
    // WALL-17
    let (latest, screen, _) = past(LockImageState {
        pinned: true,
        ..browsed()
    });
    assert_eq!(
        actions(&screen),
        [PastAction::Unpin, PastAction::Pin, PastAction::NotOffered]
    );

    latest.set(Arc::new(LockImageState {
        pinning: Some("owls".into()),
        ..browsed()
    }));
    assert_eq!(
        actions(&screen),
        [
            PastAction::PinLater,
            PastAction::Pinning,
            PastAction::NotOffered
        ]
    );

    latest.set(Arc::new(LockImageState {
        pinned: true,
        updating: true,
        ..browsed()
    }));
    assert_eq!(
        actions(&screen),
        [
            PastAction::Unpinning,
            PastAction::PinLater,
            PastAction::NotOffered
        ]
    );
}

#[test]
fn pin_asks_only_for_the_image_shown_while_it_may_be_pinned() {
    // WALL-17, LEGAL-04
    let (latest, screen, sent) = past(browsed());
    let revision = screen.revision();

    assert!(!screen.pin(revision, 2));
    assert!(!screen.pin(revision, 9));
    assert!(!screen.pin(revision.wrapping_add(1), 1));
    assert!(screen.pin(revision, 1));
    assert!(matches!(&sent.borrow()[..], [LockImageIntent::Pin(id)] if id == "owls"));

    // Once the week changed, the old revision pins nothing.
    latest.set(Arc::new(LockImageState {
        past: browsed().past[1..].to_vec(),
        ..browsed()
    }));
    let newer = screen.revision();
    assert_ne!(newer, revision);
    assert!(!screen.pin(revision, 1));
    assert_eq!(sent.borrow().len(), 1);
}

#[test]
fn unpin_is_update_now_and_try_again_browses() {
    // WALL-11, WALL-17
    let (latest, screen, sent) = past(browsed());
    assert!(!screen.unpin());

    latest.set(Arc::new(LockImageState {
        pinned: true,
        ..browsed()
    }));
    assert!(screen.unpin());
    assert!(screen.try_again());
    assert!(matches!(
        sent.borrow()[..],
        [LockImageIntent::UpdateNow, LockImageIntent::Browse]
    ));

    latest.set(Arc::new(LockImageState {
        browsing: true,
        ..browsed()
    }));
    assert!(!screen.try_again());
    screen.revision();
    assert!(screen.content().unwrap().loading);
}

#[test]
fn the_past_week_is_offered_only_while_the_daily_image_is_on() {
    // WALL-17: before the first image too.
    let (latest, screen, _) = past(LockImageState {
        current: None,
        ..browsed()
    });
    assert!(screen.is_offered());

    latest.set(Arc::new(LockImageState {
        status: ImageStatus::Off,
        ..browsed()
    }));
    assert!(!screen.is_offered());
    assert!(!screen.try_again());
}

#[test]
fn the_details_screen_and_settings_open_the_past_week_and_say_what_is_pinned() {
    // WALL-17
    let pinned = LockImageState {
        pinned: true,
        checks: CheckStatus::Unavailable("No connection".into()),
        ..browsed()
    };
    let (_, details, sent) = details(pinned.clone());
    details.revision();
    assert!(details.content().unwrap().pinned);
    assert!(details.past_images());
    assert!(matches!(
        sent.borrow()[..],
        [LockImageIntent::OpenPastImages]
    ));

    let (_, section, sent) = settings(pinned);
    assert!(section.pinned());
    assert!(section.past_images_offered());
    assert!(section.past_images());
    assert!(matches!(
        sent.borrow()[..],
        [LockImageIntent::OpenPastImages]
    ));
    assert!(section.update_status().unwrap().pinned);
    // While pinned, the checks wait, so they aren't "unavailable".
    assert!(!section.daily_image_unavailable());

    let (_, section, sent) = settings(LockImageState {
        status: ImageStatus::Off,
        ..browsed()
    });
    assert!(!section.past_images());
    assert!(sent.borrow().is_empty());
}

fn settings(state: LockImageState) -> (Latest<LockImageState>, ImageSettingsSection, Sent) {
    let latest = Latest::new(Arc::new(state));
    let sent: Sent = Rc::default();
    let into = sent.clone();
    let section = ImageSettingsSection::new(
        latest.clone(),
        Dispatch::new(move |intent| into.borrow_mut().push(intent)),
    );
    (latest, section, sent)
}
