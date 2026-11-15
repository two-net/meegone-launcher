//! Recent state (SRS 4.4): recently used apps as cards, and close mode.
//!
//! Usage history stays in memory. The only thing stored is the list of apps the user removed
//! from Recent, which lives in the settings store (PRIV-03, ADR 0002).

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use meegone_core_data::{AppsAdapter, SettingsStore, SystemConfigAdapter, UsageAdapter};
use meegone_core_state::{Access, App, AppId, RecentDensity, StateHolder};

/// At most this many cards (REC-03).
pub const MAX_CARDS: usize = 18;

const HOUR_MS: u64 = 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentCard {
    pub app: App,
    pub last_used_ms: u64,
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
    EnterCloseMode,
    /// Removes a card from Recent only; the app keeps running (REC-08).
    Remove(AppId),
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

        let window_ms = u64::from(settings.recent_window_hours) * HOUR_MS;
        let since = self.system.now_ms().saturating_sub(window_ms);
        let (uses, apps) = match (self.usage.used_since(since), self.apps.launchable_apps()) {
            (Ok(uses), Ok(apps)) => (uses, apps),
            (Err(error), _) | (_, Err(error)) => {
                self.state.problem = Some(error.to_string());
                return;
            }
        };
        let launchable: HashMap<&AppId, &App> = apps.iter().map(|app| (&app.id, app)).collect();

        let mut cards: Vec<RecentCard> = uses
            .into_iter()
            .filter(|used| !settings.hidden_apps.contains(&used.app))
            // A removed app comes back once it is used again (REC-08).
            .filter(|used| {
                settings
                    .removed_from_recent
                    .get(&used.app)
                    .is_none_or(|removed_ms| used.last_used_ms > *removed_ms)
            })
            .filter_map(|used| {
                launchable.get(&used.app).map(|app| RecentCard {
                    app: (*app).clone(),
                    last_used_ms: used.last_used_ms,
                })
            })
            .collect();
        cards.sort_by_key(|card| Reverse(card.last_used_ms));
        cards.truncate(MAX_CARDS);
        self.state.cards = cards;
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
    use meegone_core_data::fakes::{Fakes, app, app_id};
    use meegone_core_data::{PersistentSettingsStore, RecentUse};

    use super::*;

    const NOW: u64 = 100 * HOUR_MS;

    fn used(package: &str, hours_ago: u64) -> RecentUse {
        RecentUse {
            app: app_id(package),
            last_used_ms: NOW - hours_ago * HOUR_MS,
        }
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
    fn back_leaves_close_mode() {
        // REC-07
        let (_, _, mut holder) = setup(vec![]);
        holder.handle(RecentIntent::EnterCloseMode);

        assert!(holder.on_back());
        assert!(!holder.state().close_mode);
        assert!(!holder.on_back());
    }
}
