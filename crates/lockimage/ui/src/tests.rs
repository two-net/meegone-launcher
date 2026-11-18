use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use meegone_core_ui::{Dispatch, Latest};
use meegone_lockimage_state::{
    ImageFile, ImageRecord, ImageStatus, LockImageIntent, LockImageState,
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
