use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use meegone_core_ui::{Dispatch, Latest};
use meegone_lockimage_state::{
    CheckStatus, ImageFile, ImageRecord, ImageStatus, LockImageIntent, LockImageState,
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
        })
    );

    // Before the first image, it says so; while the daily image is off, there is none.
    latest.set(Arc::new(on()));
    assert_eq!(
        section.update_status(),
        Some(UpdateStatus {
            updated: None,
            checks: CheckStatus::Fine,
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
