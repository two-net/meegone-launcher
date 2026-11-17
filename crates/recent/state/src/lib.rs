//! Recent state (SRS 4.4): recently used apps as cards, and close mode.
//!
//! Usage history stays in memory. The only thing stored is the list of apps the user removed
//! from Recent, which lives in the settings store (PRIV-03, ADR 0002).

use std::collections::HashMap;
use std::sync::Arc;

use meegone_core_data::{
    AppsAdapter, DataError, RecentUse, Settings, SettingsStore, SystemConfigAdapter, UsageAdapter,
};
use meegone_core_state::{Access, App, AppId, ProfileId, RecentDensity, Rect, StateHolder};

/// At most this many cards (REC-03).
pub const MAX_CARDS: usize = 18;

const MINUTE_MS: u64 = 60 * 1000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
const DAY_MS: u64 = 24 * HOUR_MS;

/// How long ago an app was last used, as its card says (REC-04). Each unit counts whole ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Age {
    /// Under a minute ago, or a time the clock hasn't reached.
    #[default]
    JustNow,
    /// Under an hour ago.
    Minutes(u32),
    /// Under a day ago.
    Hours(u32),
    Days(u32),
}

impl Age {
    /// The age at `now_ms` of a use at `last_used_ms`.
    pub fn between(last_used_ms: u64, now_ms: u64) -> Age {
        let ago = now_ms.saturating_sub(last_used_ms);
        let whole = |unit: u64| u32::try_from(ago / unit).unwrap_or(u32::MAX);
        if ago < MINUTE_MS {
            Age::JustNow
        } else if ago < HOUR_MS {
            Age::Minutes(whole(MINUTE_MS))
        } else if ago < DAY_MS {
            Age::Hours(whole(HOUR_MS))
        } else {
            Age::Days(whole(DAY_MS))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentCard {
    pub app: App,
    pub last_used_ms: u64,
    /// As of the refresh that read it; Recent refreshes whenever it becomes visible (REC-09).
    pub age: Age,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecentState {
    /// Without usage access, Recent shows its explainer card and no cards (REC-02).
    pub access: Access,
    /// Most recently used first (REC-03).
    pub cards: Vec<RecentCard>,
    /// REC-07
    pub close_mode: bool,
    pub density: RecentDensity,
    pub problem: Option<String>,
}

#[derive(Debug)]
pub enum RecentIntent {
    Refresh,
    /// The user tapped Allow on the explainer card (REC-02).
    OpenAccessSettings,
    /// The user tapped a card: the app comes to the front, resumed if the system kept it, and
    /// its window grows from `from`, the card (REC-06, ANIM-08).
    Launch {
        app: AppId,
        from: Option<Rect>,
    },
    EnterCloseMode,
    /// Removes a card from Recent only; the app keeps running (REC-08).
    Remove(AppId),
}

/// The cards for `uses`, newest first (REC-03). Pure, so Recent's filtering is tested without a
/// device (MAINT-01).
///
/// - Each use shows as the launchable app with its profile and package: the one whose activity
///   was last resumed, otherwise the package's first in `apps`. A use with no launchable app,
///   such as a service or MeeGone itself, shows nothing.
/// - Hidden apps are left out, and so are apps removed from Recent until they are used again
///   (APPS-19, REC-08).
/// - An app shows once, as of its newest use. Equal times keep the label's order, so the
///   order never flickers.
pub fn cards(
    uses: &[RecentUse],
    apps: &[App],
    settings: &Settings,
    now_ms: u64,
) -> Vec<RecentCard> {
    let mut packages: HashMap<(ProfileId, &str), Vec<&App>> = HashMap::new();
    for app in apps {
        packages
            .entry((app.id.profile, app.id.package.as_str()))
            .or_default()
            .push(app);
    }

    let mut newest: HashMap<&AppId, (&App, u64)> = HashMap::new();
    for used in uses {
        let Some(in_package) = packages.get(&(used.profile, used.package.as_str())) else {
            continue;
        };
        let Some(app) = in_package
            .iter()
            .find(|app| app.id.activity == used.activity)
            .or(in_package.first())
        else {
            continue;
        };
        let removed = settings
            .removed_from_recent
            .get(&app.id)
            .is_some_and(|removed_ms| used.last_used_ms <= *removed_ms);
        if removed || settings.hidden_apps.contains(&app.id) {
            continue;
        }
        let last_used = newest.entry(&app.id).or_insert((app, used.last_used_ms));
        last_used.1 = last_used.1.max(used.last_used_ms);
    }

    let mut cards: Vec<RecentCard> = newest
        .into_values()
        .map(|(app, last_used_ms)| RecentCard {
            app: app.clone(),
            last_used_ms,
            age: Age::between(last_used_ms, now_ms),
        })
        .collect();
    cards.sort_by(|a, b| {
        b.last_used_ms
            .cmp(&a.last_used_ms)
            .then_with(|| a.app.sort_key.cmp(&b.app.sort_key))
            .then_with(|| a.app.id.cmp(&b.app.id))
    });
    cards.truncate(MAX_CARDS);
    cards
}

pub struct RecentHolder {
    usage: Arc<dyn UsageAdapter>,
    apps: Arc<dyn AppsAdapter>,
    settings: Arc<dyn SettingsStore>,
    system: Arc<dyn SystemConfigAdapter>,
    state: RecentState,
}

impl RecentHolder {
    pub fn new(
        usage: Arc<dyn UsageAdapter>,
        apps: Arc<dyn AppsAdapter>,
        settings: Arc<dyn SettingsStore>,
        system: Arc<dyn SystemConfigAdapter>,
    ) -> Self {
        Self {
            usage,
            apps,
            settings,
            system,
            state: RecentState::default(),
        }
    }

    fn refresh(&mut self) {
        let settings = self.settings.get();
        self.state.density = settings.recent_density;
        self.state.access = self.usage.access();
        self.state.problem = None;
        if self.state.access == Access::Missing {
            self.state.cards.clear();
            return;
        }

        let now = self.system.now_ms();
        let since = now.saturating_sub(u64::from(settings.recent_window_hours) * HOUR_MS);
        let uses = match self.usage.used_since(since) {
            Ok(uses) => uses,
            // Access was revoked since it was asked: the explainer shows, not a problem
            // (REC-02).
            Err(DataError::AccessDenied) => {
                self.state.access = Access::Missing;
                self.state.cards.clear();
                return;
            }
            Err(error) => {
                self.state.problem = Some(error.to_string());
                return;
            }
        };
        match self.apps.launchable_apps() {
            Ok(apps) => self.state.cards = cards(&uses, &apps, &settings, now),
            Err(error) => self.state.problem = Some(error.to_string()),
        }
    }

    fn remove(&mut self, app: AppId) {
        let now = self.system.now_ms();
        let saved = self.settings.update(&mut |settings| {
            settings.removed_from_recent.insert(app.clone(), now);
        });
        match saved {
            Ok(_) => self.state.cards.retain(|card| card.app.id != app),
            Err(error) => self.state.problem = Some(error.to_string()),
        }
    }
}

impl StateHolder for RecentHolder {
    type State = RecentState;
    type Intent = RecentIntent;

    fn state(&self) -> &RecentState {
        &self.state
    }

    fn handle(&mut self, intent: RecentIntent) {
        match intent {
            RecentIntent::Refresh => self.refresh(),
            RecentIntent::OpenAccessSettings => {
                self.state.problem = self
                    .usage
                    .open_access_settings()
                    .err()
                    .map(|error| error.to_string());
            }
            RecentIntent::Launch { app, from } => {
                self.state.problem = self
                    .apps
                    .launch(&app, from)
                    .err()
                    .map(|error| error.to_string());
            }
            RecentIntent::EnterCloseMode => self.state.close_mode = true,
            RecentIntent::Remove(app) => self.remove(app),
        }
    }

    fn on_back(&mut self) -> bool {
        std::mem::take(&mut self.state.close_mode)
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::{self, Fakes, app, app_id, work_app};
    use meegone_core_data::{DataError, PersistentSettingsStore, RecentUse};

    use super::*;

    const NOW: u64 = 100 * HOUR_MS;

    fn used(package: &str, hours_ago: u64) -> RecentUse {
        fakes::used(package, NOW - hours_ago * HOUR_MS)
    }

    fn setup(uses: Vec<RecentUse>) -> (Fakes, Arc<PersistentSettingsStore>, RecentHolder) {
        let fakes = Fakes::default();
        *fakes.usage.access.lock().unwrap() = Access::Granted;
        *fakes.usage.uses.lock().unwrap() = uses;
        *fakes.system.now_ms.lock().unwrap() = NOW;
        *fakes.apps.apps.lock().unwrap() = ["mail", "maps", "music", "hidden"]
            .iter()
            .map(|package| app(package, package))
            .collect();
        let settings = Arc::new(PersistentSettingsStore::new(fakes.storage.clone()));
        let holder = RecentHolder::new(
            fakes.usage.clone(),
            fakes.apps.clone(),
            settings.clone(),
            fakes.system.clone(),
        );
        (fakes, settings, holder)
    }

    fn packages(holder: &RecentHolder) -> Vec<&str> {
        holder
            .state()
            .cards
            .iter()
            .map(|card| card.app.id.package.as_str())
            .collect()
    }

    /// The packages of [`cards`] for `uses` among `apps`, with default settings.
    fn shown(uses: &[RecentUse], apps: &[App]) -> Vec<String> {
        cards(uses, apps, &Settings::default(), NOW)
            .into_iter()
            .map(|card| format!("{}/{}", card.app.id.package, card.app.id.activity))
            .collect()
    }

    /// `package` used `hours_ago`, in `activity`.
    fn used_in(package: &str, activity: &str, hours_ago: u64) -> RecentUse {
        RecentUse {
            activity: activity.to_owned(),
            ..used(package, hours_ago)
        }
    }

    /// An app of `package` whose launcher activity is `activity`.
    fn launcher(package: &str, activity: &str, label: &str) -> App {
        let mut app = app(package, label);
        app.id.activity = activity.to_owned();
        app
    }

    #[test]
    fn without_access_recent_shows_no_cards() {
        // REC-02
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        *fakes.usage.access.lock().unwrap() = Access::Missing;

        holder.handle(RecentIntent::Refresh);

        assert_eq!(holder.state().access, Access::Missing);
        assert!(holder.state().cards.is_empty());
    }

    #[test]
    fn access_revoked_while_reading_shows_the_explainer_not_a_problem() {
        // REC-02: access went between asking for it and reading the uses.
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["mail"]);

        *fakes.usage.error.lock().unwrap() = Some(DataError::AccessDenied);
        holder.handle(RecentIntent::Refresh);

        assert_eq!(holder.state().access, Access::Missing);
        assert!(holder.state().cards.is_empty());
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn a_failed_read_keeps_the_cards_and_says_so() {
        // ARCH-09
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);

        *fakes.usage.error.lock().unwrap() = Some(DataError::Failed("No uses".into()));
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["mail"]);
        assert_eq!(holder.state().problem.as_deref(), Some("No uses"));

        *fakes.usage.error.lock().unwrap() = None;
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed("No apps".into()));
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["mail"]);
        assert_eq!(holder.state().problem.as_deref(), Some("No apps"));
    }

    #[test]
    fn cards_are_newest_first_within_the_window_without_hidden_apps() {
        // REC-03
        let (_, settings, mut holder) = setup(vec![
            used("maps", 5),
            used("mail", 1),
            used("music", 30),
            used("hidden", 2),
            used("uninstalled", 3),
        ]);
        settings
            .update(&mut |s| {
                s.hidden_apps.insert(app_id("hidden"));
            })
            .unwrap();

        holder.handle(RecentIntent::Refresh);

        assert_eq!(packages(&holder), ["mail", "maps"]);
    }

    #[test]
    fn a_use_shows_as_the_launchable_app_of_its_package() {
        // REC-03: the system reports the activity last resumed, rarely a launcher activity.
        let apps = [
            launcher("mail", "mail.Inbox", "Mail"),
            launcher("phone", "phone.Dialer", "Phone"),
            launcher("phone", "phone.Contacts", "Contacts"),
        ];

        assert_eq!(
            shown(&[used_in("mail", "mail.Compose", 1)], &apps),
            ["mail/mail.Inbox"]
        );
        // With two launcher activities, the one last resumed, otherwise the first.
        assert_eq!(
            shown(&[used_in("phone", "phone.Contacts", 1)], &apps),
            ["phone/phone.Contacts"]
        );
        assert_eq!(
            shown(&[used_in("phone", "phone.Call", 1)], &apps),
            ["phone/phone.Dialer"]
        );
        // Not in another profile, and not without a launchable app.
        let mut work = used("mail", 1);
        work.profile = fakes::WORK_PROFILE;
        assert!(shown(&[work, used("service", 1)], &apps).is_empty());
    }

    #[test]
    fn a_work_app_shows_from_its_own_profile() {
        // APPS-20
        let apps = [app("mail", "Mail"), work_app("mail", "Mail")];
        let mut work = used("mail", 1);
        work.profile = fakes::WORK_PROFILE;

        let cards = cards(&[work], &apps, &Settings::default(), NOW);

        assert_eq!(cards.len(), 1);
        assert!(cards[0].app.work);
    }

    #[test]
    fn an_app_shows_once_as_of_its_newest_use() {
        // REC-03
        let apps = [app("mail", "Mail"), app("maps", "Maps")];
        let uses = [
            used_in("mail", "mail.Compose", 5),
            used("maps", 3),
            used_in("mail", "mail.Inbox", 1),
        ];

        let cards = cards(&uses, &apps, &Settings::default(), NOW);

        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].app.id, app_id("mail"));
        assert_eq!(cards[0].last_used_ms, NOW - HOUR_MS);
        assert_eq!(cards[0].age, Age::Hours(1));
    }

    #[test]
    fn hidden_and_removed_apps_are_left_out_whichever_activity_was_used() {
        // APPS-19, REC-08
        let apps = [
            app("mail", "Mail"),
            app("maps", "Maps"),
            app("music", "Music"),
        ];
        let mut settings = Settings::default();
        settings.hidden_apps.insert(app_id("mail"));
        settings
            .removed_from_recent
            .insert(app_id("maps"), NOW - 2 * HOUR_MS);
        let uses = [
            used_in("mail", "mail.Compose", 1),
            used_in("maps", "maps.Route", 3),
            used_in("music", "music.Player", 4),
        ];

        let shown = cards(&uses, &apps, &settings, NOW);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].app.id, app_id("music"));

        // Maps, used again since it was removed, comes back.
        let uses = [
            used_in("maps", "maps.Route", 3),
            used_in("maps", "maps.Route", 1),
        ];
        assert_eq!(cards(&uses, &apps, &settings, NOW).len(), 1);
    }

    #[test]
    fn at_most_eighteen_cards_show_the_newest() {
        // REC-03
        let packages: Vec<String> = (0..20).map(|i| format!("app{i:02}")).collect();
        let apps: Vec<App> = packages.iter().map(|p| app(p, p)).collect();
        let uses: Vec<RecentUse> = packages
            .iter()
            .enumerate()
            .map(|(i, p)| fakes::used(p, NOW - i as u64 * MINUTE_MS))
            .collect();

        let cards = cards(&uses, &apps, &Settings::default(), NOW);

        assert_eq!(cards.len(), MAX_CARDS);
        assert_eq!(cards[0].app.id.package, "app00");
        assert_eq!(cards[MAX_CARDS - 1].app.id.package, "app17");
    }

    #[test]
    fn apps_used_at_the_same_time_keep_their_labels_order() {
        // REC-03: the order never flickers.
        let apps = [app("b", "Bravo"), app("a", "Alpha"), app("c", "Charlie")];
        let uses = [used("c", 1), used("b", 1), used("a", 1)];

        assert_eq!(shown(&uses, &apps), ["a/a.Main", "b/b.Main", "c/c.Main"]);
    }

    #[test]
    fn a_card_says_how_long_ago_its_app_was_used() {
        // REC-04
        let at = |ago: u64| Age::between(NOW - ago, NOW);
        assert_eq!(at(0), Age::JustNow);
        assert_eq!(at(59_999), Age::JustNow);
        assert_eq!(at(MINUTE_MS), Age::Minutes(1));
        assert_eq!(at(HOUR_MS - 1), Age::Minutes(59));
        assert_eq!(at(HOUR_MS), Age::Hours(1));
        assert_eq!(at(DAY_MS - 1), Age::Hours(23));
        assert_eq!(at(DAY_MS), Age::Days(1));
        assert_eq!(at(3 * DAY_MS + HOUR_MS), Age::Days(3));
        // A use the clock hasn't reached, after the clock was set back.
        assert_eq!(Age::between(NOW + HOUR_MS, NOW), Age::JustNow);
    }

    #[test]
    fn a_removed_app_comes_back_when_it_is_used_again() {
        // REC-08
        let (fakes, _, mut holder) = setup(vec![used("mail", 2), used("maps", 1)]);
        holder.handle(RecentIntent::Refresh);

        holder.handle(RecentIntent::Remove(app_id("mail")));
        assert_eq!(packages(&holder), ["maps"]);
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["maps"]);

        *fakes.system.now_ms.lock().unwrap() = NOW + HOUR_MS;
        fakes.usage.uses.lock().unwrap()[0].last_used_ms = NOW + HOUR_MS;
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["mail", "maps"]);
    }

    #[test]
    fn a_tapped_card_opens_its_app_from_the_card() {
        // REC-06, ANIM-08
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);
        let card = Rect {
            left: 10,
            top: 20,
            right: 110,
            bottom: 220,
        };

        holder.handle(RecentIntent::Launch {
            app: app_id("mail"),
            from: Some(card),
        });

        assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
        assert_eq!(*fakes.apps.launched_from.lock().unwrap(), [Some(card)]);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn an_app_that_cant_open_says_why() {
        // REC-06
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed("Mail can't open".into()));

        holder.handle(RecentIntent::Launch {
            app: app_id("mail"),
            from: None,
        });

        assert_eq!(holder.state().problem.as_deref(), Some("Mail can't open"));
    }

    #[test]
    fn allow_opens_the_usage_access_screen() {
        // REC-02
        let (fakes, _, mut holder) = setup(vec![]);
        *fakes.usage.access.lock().unwrap() = Access::Missing;

        holder.handle(RecentIntent::OpenAccessSettings);

        assert_eq!(*fakes.usage.access_screen_opened.lock().unwrap(), 1);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn back_leaves_close_mode() {
        // REC-07
        let (_, _, mut holder) = setup(vec![]);
        holder.handle(RecentIntent::EnterCloseMode);

        assert!(holder.on_back());
        assert!(!holder.state().close_mode);
        assert!(!holder.on_back());
    }
}
