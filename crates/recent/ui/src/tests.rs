use std::cell::RefCell;
use std::rc::Rc;

use meegone_core_state::{App, ProfileId};
use meegone_core_ui::tokens::{DARK, LIGHT};

use super::*;

fn app(package: &str, label: &str) -> App {
    App {
        id: AppId {
            profile: ProfileId(0),
            package: package.into(),
            activity: format!("{package}.Main"),
        },
        label: label.into(),
        sort_key: label.to_lowercase().into_bytes(),
        search_key: label.to_lowercase(),
        // Too light to read on the light background as it is (THEME-07).
        accent: Rgb::hex(0xFFE082),
        available: true,
        work: false,
        can_uninstall: true,
        installed_at_ms: 0,
    }
}

fn card(package: &str, label: &str, age: Age) -> RecentCard {
    RecentCard {
        app: app(package, label),
        last_used_ms: 0,
        age,
    }
}

fn state(access: Access, cards: Vec<RecentCard>) -> RecentState {
    RecentState {
        access,
        cards,
        ..RecentState::default()
    }
}

fn mail_and_maps() -> RecentState {
    state(
        Access::Granted,
        vec![
            card("mail", "Mail", Age::Minutes(5)),
            card("maps", "Maps", Age::Hours(2)),
        ],
    )
}

fn view() -> (RecentView, Rc<RefCell<Vec<RecentIntent>>>) {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let view = RecentView::new(Dispatch::new({
        let sent = Rc::clone(&sent);
        move |intent| sent.borrow_mut().push(intent)
    }));
    (view, sent)
}

fn labels(view: &RecentView) -> Vec<&str> {
    view.cards()
        .iter()
        .map(|card| card.label.as_str())
        .collect()
}

const CARD: Rect = Rect {
    left: 48,
    top: 300,
    right: 368,
    bottom: 900,
};

#[test]
fn without_access_recent_explains_and_shows_no_cards() {
    // REC-02: no placeholder data.
    let (mut view, _) = view();
    let mut missing = mail_and_maps();
    missing.access = Access::Missing;

    view.show_cards(&missing, &LIGHT);

    assert_eq!(view.mode(), RecentMode::Explainer);
    assert!(view.cards().is_empty());
}

#[test]
fn with_access_and_nothing_used_recent_says_so() {
    // REC-10
    let (mut view, _) = view();

    view.show_cards(&state(Access::Granted, Vec::new()), &LIGHT);

    assert_eq!(view.mode(), RecentMode::Empty);
    assert!(view.cards().is_empty());
}

#[test]
fn the_cards_show_in_order_with_their_label_and_age() {
    // REC-03, REC-04
    let (mut view, _) = view();
    let mut recent = mail_and_maps();
    recent.cards[1].app.work = true;
    recent.cards[1].app.available = false;

    view.show_cards(&recent, &LIGHT);

    assert_eq!(view.mode(), RecentMode::Cards);
    assert_eq!(
        view.cards(),
        [
            ShownCard {
                app: recent.cards[0].app.id.clone(),
                label: "Mail".into(),
                work: false,
                available: true,
                fill: LIGHT.app_accent(Rgb::hex(0xFFE082)),
                age: Age::Minutes(5),
            },
            ShownCard {
                app: recent.cards[1].app.id.clone(),
                label: "Maps".into(),
                work: true,
                available: false,
                fill: LIGHT.app_accent(Rgb::hex(0xFFE082)),
                age: Age::Hours(2),
            },
        ]
    );
}

#[test]
fn a_cards_fill_is_its_apps_accent_readable_in_each_theme() {
    // REC-04, APPS-17, THEME-07
    let (mut view, _) = view();
    let recent = mail_and_maps();

    view.show_cards(&recent, &LIGHT);
    let light = view.cards()[0].fill;
    view.show_cards(&recent, &DARK);
    let dark = view.cards()[0].fill;

    assert_eq!(light, LIGHT.app_accent(Rgb::hex(0xFFE082)));
    assert_eq!(dark, DARK.app_accent(Rgb::hex(0xFFE082)));
    assert_ne!(light, dark);
    assert!(light.contrast_ratio(LIGHT.background) >= 3.0);
}

#[test]
fn the_revision_changes_only_when_what_shows_does() {
    let (mut view, _) = view();
    let recent = mail_and_maps();

    let first = view.show_cards(&recent, &LIGHT);
    assert_eq!(view.show_cards(&recent, &LIGHT), first);

    // A problem or close mode shows nothing new here.
    let mut troubled = recent.clone();
    troubled.problem = Some("Mail can't open".into());
    troubled.close_mode = true;
    assert_eq!(view.show_cards(&troubled, &LIGHT), first);

    let mut older = recent.clone();
    older.cards[0].age = Age::Minutes(6);
    let second = view.show_cards(&older, &LIGHT);
    assert_ne!(second, first);

    let third = view.show_cards(&older, &DARK);
    assert_ne!(third, second);

    let mut swapped = older.clone();
    swapped.cards.reverse();
    let fourth = view.show_cards(&swapped, &DARK);
    assert_ne!(fourth, third);
    assert_eq!(labels(&view), ["Maps", "Mail"]);

    let mut missing = swapped;
    missing.access = Access::Missing;
    assert_ne!(view.show_cards(&missing, &DARK), fourth);
}

#[test]
fn a_tapped_card_opens_its_app_from_the_card() {
    // REC-06, ANIM-08
    let (mut view, sent) = view();
    let recent = mail_and_maps();
    let revision = view.show_cards(&recent, &LIGHT);

    assert!(view.card_tapped(revision, 1, CARD));
    assert!(view.card_tapped(revision, 0, Rect::default()));

    let sent = sent.borrow();
    let RecentIntent::Launch { app, from } = &sent[0] else {
        panic!("a launch");
    };
    assert_eq!(*app, recent.cards[1].app.id);
    assert_eq!(*from, Some(CARD));
    let RecentIntent::Launch { app, from } = &sent[1] else {
        panic!("a launch");
    };
    assert_eq!(*app, recent.cards[0].app.id);
    assert_eq!(*from, None, "no rect, no growing from it");
}

#[test]
fn a_tap_on_cards_that_changed_since_they_were_drawn_does_nothing() {
    let (mut view, sent) = view();
    let mut recent = mail_and_maps();
    let revision = view.show_cards(&recent, &LIGHT);
    recent.cards.reverse();
    view.show_cards(&recent, &LIGHT);

    assert!(!view.card_tapped(revision, 0, CARD));
    assert!(!view.card_tapped(revision.wrapping_add(1), 2, CARD));
    assert!(sent.borrow().is_empty());
}

#[test]
fn the_columns_follow_the_density() {
    // REC-05
    let (mut view, _) = view();
    let mut recent = mail_and_maps();
    assert_eq!(view.columns(), 3);

    recent.density = RecentDensity::TwoColumns;
    let revision = view.show_cards(&recent, &LIGHT);
    assert_eq!(view.columns(), 2);

    recent.density = RecentDensity::ThreeColumns;
    assert_eq!(view.show_cards(&recent, &LIGHT), revision);
    assert_eq!(view.columns(), 3);
}

#[test]
fn allow_asks_for_usage_access() {
    // REC-02
    let (view, sent) = view();

    view.allow_access_tapped();

    assert!(matches!(
        sent.borrow()[..],
        [RecentIntent::OpenAccessSettings]
    ));
}
