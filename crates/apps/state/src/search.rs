//! Search (SRS 4.2.3): which apps match a query, best first (SRCH-02, L10N-04, ADR 0016).
//!
//! Pure logic, tested without a device (MAINT-01). Labels and queries arrive folded by the
//! platform ([`App::search_key`] and `AppsAdapter::search_key`), so matching here ignores case
//! and accents without knowing Unicode's rules, and ties follow the platform's collation keys.

use std::collections::HashSet;

use meegone_core_state::App;

use crate::GridItem;
use crate::reconcile::alphabetical;

/// How well an app's label matches a query. The better match sorts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Match {
    /// At the start of one of the label's words.
    WordStart,
    /// Only inside its words.
    InsideWord,
}

/// The apps `items` show, in cells of their own or in folders, whose labels match `query`, a
/// query the platform folded: first those where it starts a word, then those where it is only
/// inside one, each group in the locale's alphabetical order (SRCH-02, L10N-04). Each app comes
/// once, as an app's cell, and keeps its dot (APPS-07). An empty query matches nothing.
///
/// The candidates are the grid's apps, so an app the grid leaves out is never found.
pub fn rank(items: &[GridItem], query: &str) -> Vec<GridItem> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut found: Vec<(Match, &App, bool)> = apps(items)
        .filter(|(app, _)| seen.insert(&app.id))
        .filter_map(|(app, new)| matches(&app.search_key, query).map(|how| (how, app, new)))
        .collect();
    found.sort_by(|(how_a, a, _), (how_b, b, _)| how_a.cmp(how_b).then_with(|| alphabetical(a, b)));
    found
        .into_iter()
        .map(|(_, app, new)| GridItem::App {
            app: app.clone(),
            new,
        })
        .collect()
}

/// Every app `items` show, and whether it carries a dot. Apps in folders carry none.
fn apps(items: &[GridItem]) -> impl Iterator<Item = (&App, bool)> {
    items.iter().flat_map(|item| {
        let (apps, new) = match item {
            GridItem::App { app, new } => (std::slice::from_ref(app), *new),
            GridItem::Folder { apps, .. } => (apps.as_slice(), false),
        };
        apps.iter().map(move |app| (app, new))
    })
}

/// How `key` matches `query`, if it does: at a word's start if any occurrence is there.
fn matches(key: &str, query: &str) -> Option<Match> {
    let mut found = None;
    for (at, _) in key.char_indices() {
        if key[at..].starts_with(query) {
            if starts_word(key, at) {
                return Some(Match::WordStart);
            }
            found = Some(Match::InsideWord);
        }
    }
    found
}

/// Whether `at` starts a word of `key`: nothing, or something other than a letter or digit,
/// comes before it.
fn starts_word(key: &str, at: usize) -> bool {
    key[..at]
        .chars()
        .next_back()
        .is_none_or(|before| !before.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use meegone_apps_data::FolderId;
    use meegone_core_data::fakes::{app, work_app};

    use super::*;

    fn cells(apps: &[(&str, &str)]) -> Vec<GridItem> {
        apps.iter()
            .map(|(package, label)| GridItem::App {
                app: app(package, label),
                new: false,
            })
            .collect()
    }

    fn labels(results: &[GridItem]) -> Vec<&str> {
        results.iter().map(GridItem::label).collect()
    }

    #[test]
    fn matches_at_a_words_start_come_before_matches_inside_a_word() {
        // SRCH-02, the issue's first acceptance criterion.
        let items = cells(&[("gmail", "Gmail"), ("maps", "Maps"), ("mail", "Mail")]);

        assert_eq!(labels(&rank(&items, "ma")), ["Mail", "Maps", "Gmail"]);
    }

    #[test]
    fn any_word_of_the_label_can_start_the_match() {
        // SRCH-02: "Google Maps" starts a word with "ma", and so does "7-Eleven" with "el".
        let items = cells(&[
            ("gmail", "Gmail"),
            ("maps", "Google Maps"),
            ("eleven", "7-Eleven"),
            ("shell", "Shell"),
        ]);

        assert_eq!(labels(&rank(&items, "ma")), ["Google Maps", "Gmail"]);
        assert_eq!(labels(&rank(&items, "el")), ["7-Eleven", "Shell"]);
    }

    #[test]
    fn a_word_start_later_in_the_label_still_counts() {
        // "Amama" has "ma" only inside its word. "Pama Mama" has it inside "Pama" first, then
        // at the start of "Mama".
        let items = cells(&[("mama", "Amama"), ("both", "Pama Mama")]);

        assert_eq!(labels(&rank(&items, "ma")), ["Pama Mama", "Amama"]);
    }

    #[test]
    fn ties_follow_the_locales_alphabetical_order() {
        // L10N-04: the collation keys decide, not the labels' bytes. Here the locale sorts
        // "Ångström" after "Zoom", as Swedish does.
        let mut angstrom = app("angstrom", "Ångström");
        angstrom.sort_key = b"zz".to_vec();
        angstrom.search_key = "angstrom".into();
        let mut zoom = app("zoom", "Zoom Ang");
        zoom.sort_key = b"zoom".to_vec();
        let items = [angstrom, zoom, app("anki", "Anki")]
            .into_iter()
            .map(|app| GridItem::App { app, new: false })
            .collect::<Vec<_>>();

        assert_eq!(
            labels(&rank(&items, "an")),
            ["Anki", "Zoom Ang", "Ångström"]
        );
    }

    #[test]
    fn case_and_accents_are_folded_away_by_the_platform() {
        // SRCH-02, L10N-04, the issue's second acceptance criterion: the keys are folded, so
        // "cafe" finds "Café", whatever case either is typed in.
        let items = cells(&[("cafe", "Café"), ("cab", "Cab")]);

        assert_eq!(labels(&rank(&items, "cafe")), ["Café"]);
        assert_eq!(labels(&rank(&items, "caf")), ["Café"]);
    }

    #[test]
    fn labels_that_dont_match_are_left_out() {
        let items = cells(&[("mail", "Mail"), ("clock", "Clock")]);

        assert_eq!(labels(&rank(&items, "zz")), Vec::<&str>::new());
        assert_eq!(labels(&rank(&items, "mailbox")), Vec::<&str>::new());
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        let items = cells(&[("mail", "Mail")]);

        assert!(rank(&items, "").is_empty());
    }

    #[test]
    fn apps_in_folders_are_found_and_each_app_comes_once() {
        let mail = app("mail", "Mail");
        let items = vec![
            GridItem::Folder {
                id: FolderId(1),
                name: "Mapping".into(),
                apps: vec![app("maps", "Maps"), mail.clone()],
            },
            GridItem::App {
                app: mail,
                new: true,
            },
        ];

        let results = rank(&items, "ma");

        // The folder's own name is no app's label.
        assert_eq!(labels(&results), ["Mail", "Maps"]);
        assert!(
            results
                .iter()
                .all(|item| matches!(item, GridItem::App { .. }))
        );
    }

    #[test]
    fn a_new_app_keeps_its_dot() {
        // APPS-07
        let items = vec![GridItem::App {
            app: app("maps", "Maps"),
            new: true,
        }];

        assert_eq!(
            rank(&items, "ma"),
            [GridItem::App {
                app: app("maps", "Maps"),
                new: true,
            }]
        );
    }

    #[test]
    fn a_work_app_follows_its_personal_twin_and_unavailable_apps_are_found() {
        // APPS-09, APPS-20
        let mut paused = work_app("mail", "Mail");
        paused.available = false;
        let items = vec![
            GridItem::App {
                app: paused.clone(),
                new: false,
            },
            GridItem::App {
                app: app("mail", "Mail"),
                new: false,
            },
        ];

        let results = rank(&items, "mail");

        assert_eq!(
            results,
            [
                GridItem::App {
                    app: app("mail", "Mail"),
                    new: false,
                },
                GridItem::App {
                    app: paused,
                    new: false,
                },
            ]
        );
    }

    #[test]
    fn three_hundred_apps_rank_in_one_pass() {
        // PERF-05: 300 apps, as the requirement measures. Only correctness is checked here; the
        // time is measured on the reference device.
        let apps: Vec<(String, String)> = (0..300)
            .map(|i| (format!("app{i:03}"), format!("App {i:03} Tool{i:03}")))
            .collect();
        let items: Vec<GridItem> = apps
            .iter()
            .map(|(package, label)| GridItem::App {
                app: app(package, label),
                new: false,
            })
            .collect();

        assert_eq!(rank(&items, "app").len(), 300);
        assert_eq!(labels(&rank(&items, "tool299")), ["App 299 Tool299"]);
        assert_eq!(labels(&rank(&items, "9 tool1")).len(), 10);
    }
}
