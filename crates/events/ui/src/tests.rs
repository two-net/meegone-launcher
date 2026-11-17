use std::cell::RefCell;
use std::rc::Rc;

use meegone_core_ui::tokens::{DARK, LIGHT};
use meegone_events_state::{App, Notification};

use super::*;

fn mail() -> App {
    App {
        id: meegone_events_state::AppId {
            profile: ProfileId(0),
            package: "mail".into(),
            activity: "mail.Inbox".into(),
        },
        label: "Mail".into(),
        sort_key: b"mail".to_vec(),
        search_key: "mail".into(),
        // Too light to read on the light background as it is (THEME-07).
        accent: Rgb::hex(0xFFE082),
        available: true,
        work: false,
        can_uninstall: true,
        installed_at_ms: 0,
    }
}

fn notification(key: &str, package: &str, posted_at_ms: u64) -> Notification {
    Notification {
        key: key.into(),
        profile: ProfileId(0),
        package: package.into(),
        app_label: package.into(),
        title: "Anna".into(),
        text: "See you at eight".into(),
        posted_at_ms,
        clearable: true,
        ongoing: false,
        summary: false,
    }
}

/// A group of `notifications`, newest first, of an app in the grid if `app` is given.
fn group(app: Option<App>, package: &str, notifications: Vec<Notification>) -> NotificationGroup {
    NotificationGroup {
        profile: ProfileId(0),
        package: package.into(),
        label: app.as_ref().map_or(package.into(), |app| app.label.clone()),
        app,
        work: false,
        notifications,
    }
}

fn state(access: Access, groups: Vec<NotificationGroup>) -> EventsState {
    EventsState {
        access,
        groups: groups.into(),
        ..EventsState::default()
    }
}

fn view() -> (EventsView, Rc<RefCell<Vec<EventsIntent>>>) {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let view = EventsView::new(Dispatch::new({
        let sent = Rc::clone(&sent);
        move |intent| sent.borrow_mut().push(intent)
    }));
    (view, sent)
}

fn packages(view: &EventsView) -> Vec<(&str, Option<usize>)> {
    view.rows()
        .iter()
        .map(|row| (row.package.as_str(), row.from))
        .collect()
}

#[test]
fn without_access_the_explainer_shows_and_with_nothing_the_empty_state() {
    // EVT-03, EVT-11
    let (mut view, _) = view();
    let explainer = view.show_list(&state(Access::Missing, Vec::new()), &LIGHT);
    assert_eq!(view.mode(), EventsListMode::Explainer);
    assert!(view.rows().is_empty());

    let empty = view.show_list(&state(Access::Granted, Vec::new()), &LIGHT);
    assert_eq!(view.mode(), EventsListMode::Empty);
    assert_ne!(empty, explainer);
}

#[test]
fn each_group_shows_its_app_its_count_and_its_newest_notification() {
    // EVT-04, EVT-09
    let (mut view, _) = view();
    let groups = vec![group(
        Some(mail()),
        "mail",
        vec![
            notification("m2", "mail", 20),
            notification("m1", "mail", 10),
        ],
    )];

    view.show_list(&state(Access::Granted, groups), &LIGHT);

    assert_eq!(view.mode(), EventsListMode::List);
    assert_eq!(
        view.rows(),
        [EventsRow {
            from: None,
            profile: ProfileId(0),
            package: "mail".into(),
            activity: Some("mail.Inbox".into()),
            label: "Mail".into(),
            work: false,
            accent: LIGHT.app_accent(Rgb::hex(0xFFE082)),
            count: 2,
            title: "Anna".into(),
            text: "See you at eight".into(),
        }]
    );
}

#[test]
fn the_marker_keeps_its_contrast_in_each_theme_and_is_neutral_without_an_accent() {
    // EVT-09, THEME-07, ADR 0020
    let (mut view, _) = view();
    let state = state(
        Access::Granted,
        vec![
            group(Some(mail()), "mail", vec![notification("m1", "mail", 20)]),
            group(None, "android", vec![notification("s1", "android", 10)]),
        ],
    );

    let light = view.show_list(&state, &LIGHT);
    let light_marker = view.rows()[0].accent;
    assert!(light_marker.contrast_ratio(LIGHT.background) >= 3.0);
    assert_eq!(view.rows()[1].accent, LIGHT.text_secondary);
    assert_eq!(view.rows()[1].activity, None);

    let dark = view.show_list(&state, &DARK);
    assert_ne!(dark, light, "a new theme is drawn again");
    assert!(view.rows()[0].accent.contrast_ratio(DARK.background) >= 3.0);
    assert_eq!(view.rows()[1].accent, DARK.text_secondary);
}

#[test]
fn the_revision_changes_only_when_what_shows_does() {
    // EVT-10: the host reads the rows again only when they change.
    let (mut view, _) = view();
    let first = state(
        Access::Granted,
        vec![group(None, "mail", vec![notification("m1", "mail", 10)])],
    );
    let revision = view.show_list(&first, &LIGHT);
    assert_eq!(view.show_list(&first, &LIGHT), revision);
    assert_eq!(view.rows()[0].from, None, "it came in");

    // The same groups, shared anew, after the host read them: the row now says it kept its
    // view, which the host reads once; then nothing changes.
    let same = state(
        Access::Granted,
        vec![group(None, "mail", vec![notification("m1", "mail", 10)])],
    );
    assert_eq!(view.show_list(&same, &LIGHT), revision.wrapping_add(1));
    assert_eq!(view.rows()[0].from, Some(0));
    assert_eq!(view.show_list(&same, &LIGHT), revision.wrapping_add(1));
    let again = state(
        Access::Granted,
        vec![group(None, "mail", vec![notification("m1", "mail", 10)])],
    );
    assert_eq!(view.show_list(&again, &LIGHT), revision.wrapping_add(1));
}

#[test]
fn rows_say_where_they_were_as_groups_come_and_go() {
    // ANIM-20: a new group comes in at the top; the rest keep their views.
    let (mut view, _) = view();
    let mail = |posted_at_ms| group(None, "mail", vec![notification("m1", "mail", posted_at_ms)]);
    let chat =
        |key, posted_at_ms| group(None, "chat", vec![notification(key, "chat", posted_at_ms)]);
    view.show_list(
        &state(Access::Granted, vec![mail(20), chat("c1", 10)]),
        &LIGHT,
    );
    assert_eq!(packages(&view), [("mail", None), ("chat", None)]);

    view.show_list(
        &state(Access::Granted, vec![mail(20), chat("c1", 10)]),
        &LIGHT,
    );
    assert_eq!(packages(&view), [("mail", Some(0)), ("chat", Some(1))]);

    // A newer chat message: chat's old row goes, and a new one comes in at the top.
    view.show_list(
        &state(Access::Granted, vec![chat("c2", 30), mail(20)]),
        &LIGHT,
    );
    assert_eq!(packages(&view), [("chat", None), ("mail", Some(0))]);

    // Chat goes.
    view.show_list(&state(Access::Granted, vec![mail(20)]), &LIGHT);
    assert_eq!(packages(&view), [("mail", Some(1))]);
}

#[test]
fn rows_say_where_they_were_in_what_the_host_last_read() {
    // ANIM-20: two changes can come between the host's reads.
    let (mut view, _) = view();
    let app = |package: &str, posted_at_ms| {
        group(
            None,
            package,
            vec![notification(package, package, posted_at_ms)],
        )
    };
    view.show_list(
        &state(Access::Granted, vec![app("mail", 20), app("chat", 10)]),
        &LIGHT,
    );
    let _read = view.rows();

    // News comes, then mail goes, before the host reads again.
    view.show_list(
        &state(
            Access::Granted,
            vec![app("news", 30), app("mail", 20), app("chat", 10)],
        ),
        &LIGHT,
    );
    view.show_list(
        &state(Access::Granted, vec![app("news", 30), app("chat", 10)]),
        &LIGHT,
    );

    assert_eq!(packages(&view), [("news", None), ("chat", Some(1))]);
}

#[test]
fn a_row_never_prints_what_the_notification_says() {
    // EVT-13
    let (mut view, _) = view();
    view.show_list(
        &state(
            Access::Granted,
            vec![group(None, "chat", vec![notification("c1", "chat", 10)])],
        ),
        &LIGHT,
    );

    let printed = format!("{:?}", view.rows());

    assert!(printed.contains("chat"));
    assert!(!printed.contains("Anna"));
    assert!(!printed.contains("See you at eight"));
}

#[test]
fn allow_asks_for_the_access_screen() {
    // EVT-03
    let (view, sent) = view();

    view.allow_access_tapped();

    assert!(matches!(
        sent.borrow().as_slice(),
        [EventsIntent::OpenAccessSettings]
    ));
}
