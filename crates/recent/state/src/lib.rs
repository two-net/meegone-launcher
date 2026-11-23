//! Recent state (SRS 4.4): recently used apps as cards, how far back they go, their density and
//! columns, and close mode.
//!
//! Usage history stays in memory. The only things stored are the time window, the density, more
//! cards or bigger ones, and the list of apps the user removed from Recent with when, all in the
//! settings store (PRIV-03, REC-05, REC-11, ADR 0002). A removal is forgotten once it can no
//! longer hide anything.

use std::collections::HashMap;
use std::sync::Arc;

use meegone_core_data::{
    AppsAdapter, DataError, RecentUse, Settings, SettingsStore, SystemConfigAdapter, UsageAdapter,
};
use meegone_core_state::{
    Access, App, AppId, Problem, ProfileId, RecentDensity, RecentWindow, Rect, StateHolder,
};

/// At most this many cards (REC-03).
pub const MAX_CARDS: usize = 18;
/// At most this many rows of cards: rows always hold two cards or more (REC-05).
pub const MAX_ROWS: usize = MAX_CARDS.div_ceil(2);
/// With more cards, columns are added so no card is wider than this, in dp (REC-05). A phone's
/// cards area is under 504 dp, so it always has three.
pub const MAX_CARD_DP: f64 = 160.0;
/// Between cards side by side, in dp, as the host lays them out (ADR 0023).
pub const CARD_GAP_DP: f64 = 12.0;

const MINUTE_MS: u64 = 60 * 1000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
const DAY_MS: u64 = 24 * HOUR_MS;
/// A removal older than this is forgotten when Recent next saves one: the longest window Recent
/// can show, three days (REC-11), holds no use from before it, so it hides nothing (PRIV-03).
pub const REMOVALS_KEPT_MS: u64 = 3 * DAY_MS;

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

/// How many cards each row holds on a phone with `density`: three, or two for bigger cards. It is
/// also how many rows of cards one phone screen shows (REC-05: 3×3 or 2×2).
pub fn phone_columns(density: RecentDensity) -> u8 {
    match density {
        RecentDensity::ThreeColumns => 3,
        RecentDensity::TwoColumns => 2,
    }
}

/// How many cards each row holds with `density` in a cards area `width_dp` wide (REC-05).
///
/// With more cards, three, unless three cards would be wider than [`MAX_CARD_DP`]: then as few
/// columns as keep every card at most that wide, so a large screen adds columns and cards keep a
/// phone's size. Bigger cards take two columns for every three, rounded, so they are about half
/// as wide again, and always fewer. A width not known yet gives a phone's columns.
pub fn columns_for(density: RecentDensity, width_dp: f64) -> u8 {
    let more = if width_dp.is_finite() && width_dp > 0.0 {
        let fit = ((width_dp + CARD_GAP_DP) / (MAX_CARD_DP + CARD_GAP_DP)).ceil();
        fit.clamp(3.0, f64::from(u8::MAX)) as u8
    } else {
        3
    };
    match density {
        RecentDensity::ThreeColumns => more,
        RecentDensity::TwoColumns => {
            let bigger = (f64::from(more) * 2.0 / 3.0).round() as u8;
            bigger.max(phone_columns(density))
        }
    }
}

/// The other choice: what a pinch or the screen-reader action switches to (REC-05, A11Y-05).
pub fn other_density(density: RecentDensity) -> RecentDensity {
    match density {
        RecentDensity::ThreeColumns => RecentDensity::TwoColumns,
        RecentDensity::TwoColumns => RecentDensity::ThreeColumns,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentCard {
    pub app: App,
    pub last_used_ms: u64,
    /// As of the refresh that read it; Recent refreshes whenever it becomes visible (REC-09).
    pub age: Age,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecentState {
    /// Without usage access, Recent shows its explainer card and no cards (REC-02).
    pub access: Access,
    /// Most recently used first (REC-03).
    pub cards: Vec<RecentCard>,
    /// A ✕ on every card and Clear all (REC-07). Only while there are cards.
    pub close_mode: bool,
    /// Changes each time apps are removed from Recent, by a ✕ or Clear all, so the host confirms
    /// it: "Removed from Recent" (REC-08, A11Y-08).
    pub removals: u32,
    /// More cards or bigger ones, as last chosen by a pinch or from a screen reader (REC-05).
    pub density: RecentDensity,
    /// How far back the cards go, as chosen in Settings (REC-11).
    pub window: RecentWindow,
    /// How wide the cards area is, inside its margins, in dp, as the host last laid it out; 0
    /// until it has.
    pub width_dp: f64,
    pub problem: Option<Problem>,
}

impl RecentState {
    /// How many cards each row holds now (REC-05).
    pub fn columns(&self) -> u8 {
        self.columns_of(self.density)
    }

    /// How many cards each row would hold with `density`, at the current width.
    pub fn columns_of(&self, density: RecentDensity) -> u8 {
        columns_for(density, self.width_dp)
    }

    /// Whether the cards show: with access and at least one app used (REC-03).
    pub fn shows_cards(&self) -> bool {
        self.access == Access::Granted && !self.cards.is_empty()
    }
}

#[derive(Debug)]
pub enum RecentIntent {
    Refresh,
    /// The user tapped Allow on the explainer card (REC-02).
    OpenAccessSettings,
    /// The user tapped a card: the app comes to the front, resumed if the system kept it, and
    /// its window grows from `from`, the card (REC-06, ANIM-08). In close mode it does nothing.
    Launch {
        app: AppId,
        from: Option<Rect>,
    },
    /// A long-press on the grid, or a screen reader, enters close mode while there are cards
    /// (REC-07, A11Y-05).
    EnterCloseMode,
    /// A tap on empty space leaves close mode (REC-07).
    ExitCloseMode,
    /// Removes a card from Recent only; the app keeps running (REC-08).
    Remove(AppId),
    /// Removes every card from Recent, in one save; no app is stopped (REC-08, AC-14).
    ClearAll,
    /// The host laid the cards out `width_dp` wide, inside their margins (REC-05).
    Layout {
        width_dp: f64,
    },
    /// More cards or bigger ones, chosen by a pinch or from a screen reader (REC-05, A11Y-05).
    /// The choice is saved, so it stays after a restart.
    SetDensity(RecentDensity),
    /// How far back the cards go, chosen in Settings → Recent → Time window (REC-11). The choice
    /// is saved and the cards follow it at once (SET-03).
    SetWindow(RecentWindow),
    /// Settings → Reset launcher (SET-06): the cards' size and the time window go back to
    /// their defaults, apps removed from Recent may show again, and close mode ends, in one
    /// save. A failed save changes nothing and says why.
    Reset,
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
        self.read();
        // Close mode needs cards to close (REC-07).
        self.state.close_mode &= self.state.shows_cards();
    }

    fn read(&mut self) {
        let settings = self.settings.get();
        self.state.density = settings.recent_density;
        self.state.window = settings.recent_window;
        self.state.access = self.usage.access();
        self.state.problem = None;
        if self.state.access == Access::Missing {
            self.state.cards.clear();
            return;
        }

        let now = self.system.now_ms();
        let since = now.saturating_sub(u64::from(settings.recent_window.hours()) * HOUR_MS);
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
                self.state.problem = Some(error.problem());
                return;
            }
        };
        match self.apps.launchable_apps() {
            Ok(apps) => self.state.cards = cards(&uses, &apps, &settings, now),
            Err(error) => self.state.problem = Some(error.problem()),
        }
    }

    /// Saves `density`. A failed save keeps the cards as they were and says why (ARCH-09).
    fn set_density(&mut self, density: RecentDensity) {
        if density == self.state.density {
            return;
        }
        let saved = self.settings.update(&mut |settings| {
            settings.recent_density = density;
        });
        match saved {
            Ok(settings) => {
                self.state.density = settings.recent_density;
                self.state.problem = None;
            }
            Err(error) => self.state.problem = Some(error.problem()),
        }
    }

    /// Saves `window` and reads the cards again, so they follow it at once (SET-03). A failed
    /// save keeps the window and the cards and says why (ARCH-09).
    fn set_window(&mut self, window: RecentWindow) {
        if window == self.state.window {
            return;
        }
        let saved = self.settings.update(&mut |settings| {
            settings.recent_window = window;
        });
        match saved {
            Ok(_) => self.refresh(),
            Err(error) => self.state.problem = Some(error.problem()),
        }
    }

    /// Settings → Reset launcher (SET-06): Recent's fields of the settings store go back to
    /// their defaults in one write, and the cards are read again.
    fn reset(&mut self) {
        let defaults = Settings::default();
        let saved = self.settings.update(&mut |settings| {
            settings.recent_density = defaults.recent_density;
            settings.recent_window = defaults.recent_window;
            settings.removed_from_recent.clear();
        });
        match saved {
            Ok(_) => {
                self.state.close_mode = false;
                self.refresh();
            }
            Err(error) => self.state.problem = Some(error.problem()),
        }
    }

    /// Saves `apps` as removed from Recent now, in one write, forgetting removals that can no
    /// longer hide anything (PRIV-03). Their cards go, and close mode ends with the last one.
    /// A failed save keeps the cards and says why (ARCH-09).
    fn remove(&mut self, apps: &[AppId]) {
        if apps.is_empty() {
            return;
        }
        let now = self.system.now_ms();
        let saved = self.settings.update(&mut |settings| {
            let removed = &mut settings.removed_from_recent;
            removed.retain(|_, removed_ms| now.saturating_sub(*removed_ms) < REMOVALS_KEPT_MS);
            removed.extend(apps.iter().map(|app| (app.clone(), now)));
        });
        match saved {
            Ok(_) => {
                self.state.cards.retain(|card| !apps.contains(&card.app.id));
                self.state.close_mode &= self.state.shows_cards();
                self.state.removals = self.state.removals.wrapping_add(1);
                self.state.problem = None;
            }
            Err(error) => self.state.problem = Some(error.problem()),
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
                    .map(|error| error.problem());
            }
            RecentIntent::Launch { .. } if self.state.close_mode => {}
            RecentIntent::Launch { app, from } => {
                self.state.problem = self
                    .apps
                    .launch(&app, from)
                    .err()
                    .map(|error| error.problem());
            }
            RecentIntent::EnterCloseMode => self.state.close_mode = self.state.shows_cards(),
            RecentIntent::ExitCloseMode => self.state.close_mode = false,
            RecentIntent::Remove(app) => {
                if self.state.cards.iter().any(|card| card.app.id == app) {
                    self.remove(&[app]);
                }
            }
            RecentIntent::ClearAll => {
                let apps: Vec<AppId> = self.state.cards.iter().map(|c| c.app.id.clone()).collect();
                self.remove(&apps);
            }
            RecentIntent::Layout { width_dp } => {
                self.state.width_dp = if width_dp.is_finite() {
                    width_dp.max(0.0)
                } else {
                    0.0
                };
            }
            RecentIntent::SetDensity(density) => self.set_density(density),
            RecentIntent::SetWindow(window) => self.set_window(window),
            RecentIntent::Reset => self.reset(),
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

        *fakes.usage.error.lock().unwrap() = Some(DataError::Failed(Problem::RecentUnreadable));
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["mail"]);
        assert_eq!(holder.state().problem, Some(Problem::RecentUnreadable));

        *fakes.usage.error.lock().unwrap() = None;
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed(Problem::AppListUnavailable));
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["mail"]);
        assert_eq!(holder.state().problem, Some(Problem::AppListUnavailable));
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
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed(Problem::AppUnavailable));

        holder.handle(RecentIntent::Launch {
            app: app_id("mail"),
            from: None,
        });

        assert_eq!(holder.state().problem, Some(Problem::AppUnavailable));
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
    fn a_phone_shows_three_columns_or_two() {
        // REC-05: 3×3 by default, 2×2 after a pinch, on any phone's cards area.
        for tenths in 2_880..=5_040 {
            let width = f64::from(tenths) / 10.0;
            assert_eq!(
                columns_for(RecentDensity::ThreeColumns, width),
                3,
                "{width} dp"
            );
            assert_eq!(
                columns_for(RecentDensity::TwoColumns, width),
                2,
                "{width} dp"
            );
        }
    }

    #[test]
    fn a_large_screen_adds_columns_so_cards_keep_their_size() {
        // REC-05: both layouts add columns, and bigger cards always have fewer.
        let card = |columns: u8, width: f64| {
            let columns = f64::from(columns);
            (width - (columns - 1.0) * CARD_GAP_DP) / columns
        };
        for tenths in 5_050..=25_600 {
            let width = f64::from(tenths) / 10.0;
            let more = columns_for(RecentDensity::ThreeColumns, width);
            let bigger = columns_for(RecentDensity::TwoColumns, width);
            assert!(bigger < more, "{width} dp: {bigger} and {more}");
            assert!(
                (115.0..=MAX_CARD_DP).contains(&card(more, width)),
                "{width} dp: {more} columns"
            );
            let ratio = card(bigger, width) / card(more, width);
            assert!((1.2..=1.8).contains(&ratio), "{width} dp: {ratio}");
        }
    }

    #[test]
    fn a_width_not_known_yet_gives_a_phones_columns() {
        for width in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(columns_for(RecentDensity::ThreeColumns, width), 3);
            assert_eq!(columns_for(RecentDensity::TwoColumns, width), 2);
        }
    }

    #[test]
    fn the_columns_follow_the_layout() {
        // REC-05
        let (_, _, mut holder) = setup(vec![]);
        assert_eq!(holder.state().columns(), 3);

        holder.handle(RecentIntent::Layout { width_dp: 1_000.0 });
        assert_eq!(holder.state().columns(), 6);
        assert_eq!(holder.state().columns_of(RecentDensity::TwoColumns), 4);

        holder.handle(RecentIntent::Layout { width_dp: f64::NAN });
        assert_eq!(holder.state().columns(), 3);
    }

    #[test]
    fn the_density_chosen_stays_after_a_restart() {
        // REC-05
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);
        assert_eq!(holder.state().density, RecentDensity::ThreeColumns);

        holder.handle(RecentIntent::SetDensity(RecentDensity::TwoColumns));
        assert_eq!(holder.state().density, RecentDensity::TwoColumns);
        assert_eq!(holder.state().columns(), 2);
        assert_eq!(holder.state().problem, None);

        let mut restarted = RecentHolder::new(
            fakes.usage.clone(),
            fakes.apps.clone(),
            Arc::new(PersistentSettingsStore::new(fakes.storage.clone())),
            fakes.system.clone(),
        );
        restarted.handle(RecentIntent::Refresh);
        assert_eq!(restarted.state().density, RecentDensity::TwoColumns);
    }

    #[test]
    fn an_app_used_30_hours_ago_shows_with_three_days_and_not_with_24_hours() {
        // REC-11, issue #25
        let (_, settings, mut holder) = setup(vec![used("mail", 1), used("maps", 30)]);
        holder.handle(RecentIntent::Refresh);
        assert_eq!(holder.state().window, RecentWindow::TwentyFourHours);
        assert_eq!(packages(&holder), ["mail"]);

        holder.handle(RecentIntent::SetWindow(RecentWindow::ThreeDays));

        // At once, without another refresh (SET-03).
        assert_eq!(holder.state().window, RecentWindow::ThreeDays);
        assert_eq!(packages(&holder), ["mail", "maps"]);
        assert_eq!(holder.state().cards[1].age, Age::Days(1));
        assert_eq!(settings.get().recent_window, RecentWindow::ThreeDays);
        assert_eq!(holder.state().problem, None);

        holder.handle(RecentIntent::SetWindow(RecentWindow::TwentyFourHours));
        assert_eq!(packages(&holder), ["mail"]);
    }

    #[test]
    fn twelve_hours_leaves_out_an_app_used_13_hours_ago() {
        // REC-11
        let (_, _, mut holder) = setup(vec![used("mail", 1), used("maps", 13)]);
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["mail", "maps"]);

        holder.handle(RecentIntent::SetWindow(RecentWindow::TwelveHours));

        assert_eq!(packages(&holder), ["mail"]);
    }

    #[test]
    fn the_window_chosen_stays_after_a_restart() {
        // REC-11
        let (fakes, _, mut holder) = setup(vec![used("mail", 1), used("maps", 30)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::SetWindow(RecentWindow::ThreeDays));

        let mut restarted = RecentHolder::new(
            fakes.usage.clone(),
            fakes.apps.clone(),
            Arc::new(PersistentSettingsStore::new(fakes.storage.clone())),
            fakes.system.clone(),
        );
        restarted.handle(RecentIntent::Refresh);

        assert_eq!(restarted.state().window, RecentWindow::ThreeDays);
        assert_eq!(packages(&restarted), ["mail", "maps"]);
    }

    #[test]
    fn choosing_the_same_window_saves_nothing() {
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);

        holder.handle(RecentIntent::SetWindow(RecentWindow::TwentyFourHours));

        assert!(fakes.storage.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn a_window_that_cant_be_saved_keeps_the_cards_and_says_why() {
        // ARCH-09
        let (fakes, settings, mut holder) = setup(vec![used("mail", 1), used("maps", 30)]);
        holder.handle(RecentIntent::Refresh);
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed(Problem::StorageFull));

        holder.handle(RecentIntent::SetWindow(RecentWindow::ThreeDays));

        assert_eq!(holder.state().window, RecentWindow::TwentyFourHours);
        assert_eq!(holder.state().problem, Some(Problem::StorageFull));
        assert_eq!(settings.get().recent_window, RecentWindow::TwentyFourHours);
        assert_eq!(packages(&holder), ["mail"]);
    }

    #[test]
    fn reset_restores_the_defaults_and_brings_removed_apps_back() {
        // SET-06: in one save, and close mode ends.
        let (fakes, settings, mut holder) =
            setup(vec![used("mail", 1), used("maps", 2), used("music", 30)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::SetWindow(RecentWindow::ThreeDays));
        holder.handle(RecentIntent::SetDensity(RecentDensity::TwoColumns));
        holder.handle(RecentIntent::Remove(app_id("maps")));
        holder.handle(RecentIntent::EnterCloseMode);
        assert_eq!(packages(&holder), ["mail", "music"]);
        let writes = fakes.storage.writes.lock().unwrap().len();

        holder.handle(RecentIntent::Reset);

        assert_eq!(fakes.storage.writes.lock().unwrap().len(), writes + 1);
        let defaults = Settings::default();
        assert_eq!(holder.state().window, defaults.recent_window);
        assert_eq!(holder.state().density, defaults.recent_density);
        assert!(!holder.state().close_mode);
        assert_eq!(packages(&holder), ["mail", "maps"]);
        assert!(settings.get().removed_from_recent.is_empty());
    }

    #[test]
    fn a_reset_that_cant_be_saved_keeps_everything_and_says_why() {
        // SET-06, ARCH-09
        let (fakes, settings, mut holder) = setup(vec![used("mail", 1), used("maps", 30)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::SetWindow(RecentWindow::ThreeDays));
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed(Problem::StorageFull));

        holder.handle(RecentIntent::Reset);

        assert_eq!(holder.state().window, RecentWindow::ThreeDays);
        assert_eq!(holder.state().problem, Some(Problem::StorageFull));
        assert_eq!(settings.get().recent_window, RecentWindow::ThreeDays);
        assert_eq!(packages(&holder), ["mail", "maps"]);
    }

    #[test]
    fn a_density_that_cant_be_saved_keeps_the_cards_and_says_why() {
        // ARCH-09
        let (fakes, settings, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed(Problem::StorageFull));

        holder.handle(RecentIntent::SetDensity(RecentDensity::TwoColumns));

        assert_eq!(holder.state().density, RecentDensity::ThreeColumns);
        assert_eq!(holder.state().problem, Some(Problem::StorageFull));
        assert_eq!(settings.get().recent_density, RecentDensity::ThreeColumns);
        assert_eq!(packages(&holder), ["mail"]);
    }

    #[test]
    fn choosing_the_density_already_shown_saves_nothing() {
        let (fakes, _, mut holder) = setup(vec![]);
        holder.handle(RecentIntent::SetDensity(RecentDensity::ThreeColumns));

        assert!(fakes.storage.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn each_density_switches_to_the_other() {
        for density in [RecentDensity::ThreeColumns, RecentDensity::TwoColumns] {
            assert_ne!(other_density(density), density);
            assert_eq!(other_density(other_density(density)), density);
        }
        assert_eq!(phone_columns(RecentDensity::ThreeColumns), 3);
        assert_eq!(phone_columns(RecentDensity::TwoColumns), 2);
    }

    #[test]
    fn back_leaves_close_mode() {
        // REC-07
        let (_, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);

        assert!(holder.on_back());
        assert!(!holder.state().close_mode);
        assert!(!holder.on_back());
    }

    #[test]
    fn close_mode_needs_cards_and_a_tap_on_empty_space_leaves_it() {
        // REC-07
        let (fakes, _, mut holder) = setup(vec![]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);
        assert!(!holder.state().close_mode);

        *fakes.usage.uses.lock().unwrap() = vec![used("mail", 1)];
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);
        assert!(holder.state().close_mode);

        holder.handle(RecentIntent::ExitCloseMode);
        assert!(!holder.state().close_mode);
    }

    #[test]
    fn close_mode_ends_when_the_cards_go() {
        // REC-07: without access, or with nothing left to remove.
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);

        *fakes.usage.access.lock().unwrap() = Access::Missing;
        holder.handle(RecentIntent::Refresh);
        assert!(!holder.state().close_mode);

        *fakes.usage.access.lock().unwrap() = Access::Granted;
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);
        *fakes.usage.uses.lock().unwrap() = vec![];
        holder.handle(RecentIntent::Refresh);
        assert!(!holder.state().close_mode);
    }

    #[test]
    fn a_card_tapped_in_close_mode_opens_nothing() {
        // REC-07, as in edit mode (ADR 0014).
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);

        holder.handle(RecentIntent::Launch {
            app: app_id("mail"),
            from: None,
        });

        assert!(fakes.apps.launched.lock().unwrap().is_empty());
    }

    #[test]
    fn removing_a_card_keeps_close_mode_until_the_last_one_goes() {
        // REC-07, REC-08
        let (_, settings, mut holder) = setup(vec![used("mail", 2), used("maps", 1)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);

        holder.handle(RecentIntent::Remove(app_id("maps")));
        assert_eq!(packages(&holder), ["mail"]);
        assert!(holder.state().close_mode);
        assert_eq!(holder.state().removals, 1);
        assert_eq!(
            settings.get().removed_from_recent.get(&app_id("maps")),
            Some(&NOW)
        );

        holder.handle(RecentIntent::Remove(app_id("mail")));
        assert!(holder.state().cards.is_empty());
        assert!(!holder.state().close_mode);
        assert_eq!(holder.state().removals, 2);
    }

    #[test]
    fn removing_an_app_that_shows_no_card_saves_nothing() {
        let (fakes, _, mut holder) = setup(vec![used("mail", 1)]);
        holder.handle(RecentIntent::Refresh);

        holder.handle(RecentIntent::Remove(app_id("maps")));

        assert!(fakes.storage.writes.lock().unwrap().is_empty());
        assert_eq!(holder.state().removals, 0);
    }

    #[test]
    fn clear_all_removes_every_card_in_one_save_and_stops_nothing() {
        // REC-08, AC-14
        let (fakes, settings, mut holder) =
            setup(vec![used("mail", 3), used("maps", 2), used("music", 1)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);

        holder.handle(RecentIntent::ClearAll);

        assert!(holder.state().cards.is_empty());
        assert!(!holder.state().close_mode);
        assert_eq!(holder.state().removals, 1);
        assert_eq!(
            *fakes.storage.writes.lock().unwrap(),
            [meegone_core_data::StoredFile::Settings]
        );
        let removed = settings.get().removed_from_recent;
        assert_eq!(removed.len(), 3);
        assert!(removed.values().all(|removed_ms| *removed_ms == NOW));
        assert!(fakes.apps.launched.lock().unwrap().is_empty());
        assert!(fakes.apps.uninstall_requested.lock().unwrap().is_empty());

        // Each comes back when it is next used.
        *fakes.system.now_ms.lock().unwrap() = NOW + HOUR_MS;
        fakes.usage.uses.lock().unwrap()[1].last_used_ms = NOW + HOUR_MS;
        holder.handle(RecentIntent::Refresh);
        assert_eq!(packages(&holder), ["maps"]);
    }

    #[test]
    fn a_removal_that_cant_be_saved_keeps_the_cards_and_says_why() {
        // ARCH-09
        let (fakes, settings, mut holder) = setup(vec![used("mail", 2), used("maps", 1)]);
        holder.handle(RecentIntent::Refresh);
        holder.handle(RecentIntent::EnterCloseMode);
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed(Problem::StorageFull));

        holder.handle(RecentIntent::Remove(app_id("maps")));
        holder.handle(RecentIntent::ClearAll);

        assert_eq!(packages(&holder), ["maps", "mail"]);
        assert!(holder.state().close_mode);
        assert_eq!(holder.state().removals, 0);
        assert_eq!(holder.state().problem, Some(Problem::StorageFull));
        assert!(settings.get().removed_from_recent.is_empty());

        *fakes.storage.error.lock().unwrap() = None;
        holder.handle(RecentIntent::ClearAll);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn removals_that_can_no_longer_hide_anything_are_forgotten() {
        // PRIV-03: no window reaches back past three days, so older removals go at the next save.
        let (_, settings, mut holder) = setup(vec![used("mail", 1)]);
        settings
            .update(&mut |s| {
                s.removed_from_recent
                    .insert(app_id("maps"), NOW - REMOVALS_KEPT_MS);
                s.removed_from_recent
                    .insert(app_id("music"), NOW - REMOVALS_KEPT_MS + 1);
            })
            .unwrap();
        holder.handle(RecentIntent::Refresh);

        holder.handle(RecentIntent::Remove(app_id("mail")));

        let removed = settings.get().removed_from_recent;
        assert_eq!(
            removed
                .keys()
                .map(|app| app.package.as_str())
                .collect::<Vec<_>>(),
            ["mail", "music"]
        );
    }

    #[test]
    fn rows_always_hold_two_cards_or_more() {
        // REC-05: so MAX_ROWS rows hold every card.
        for width in [0.0, 300.0, 1_000.0, 2_560.0] {
            for density in [RecentDensity::ThreeColumns, RecentDensity::TwoColumns] {
                assert!(columns_for(density, width) >= 2);
            }
        }
        const { assert!(MAX_ROWS * 2 >= MAX_CARDS) };
    }
}
