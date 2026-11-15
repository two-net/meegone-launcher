//! Settings state (SRS 4.8): the launcher's own settings and the status of each access.
//!
//! Settings owns the screens; the values live in the shared settings store (ADR 0002). Each
//! feature's own section, such as the lock-screen image, belongs to that feature and is put in
//! place by the shell.

use std::sync::Arc;

use meegone_core_data::{
    NotificationsAdapter, Settings, SettingsStore, SystemConfigAdapter, UsageAdapter,
};
use meegone_core_state::{Access, HomeTarget, StateHolder, ThemeChoice};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsState {
    pub theme: ThemeChoice,
    pub home_target: HomeTarget,
    pub wrap_around: bool,
    /// The Access group (SET-02).
    pub default_home: bool,
    pub notification_access: Access,
    pub usage_access: Access,
    pub problem: Option<String>,
}

#[derive(Debug)]
pub enum SettingsIntent {
    Refresh,
    /// THEME-01
    SetTheme(ThemeChoice),
    /// HOME-06
    SetHomeTarget(HomeTarget),
    /// HOME-04
    SetWrapAround(bool),
    OpenNotificationAccess,
    OpenUsageAccess,
}

pub struct SettingsHolder {
    store: Arc<dyn SettingsStore>,
    notifications: Arc<dyn NotificationsAdapter>,
    usage: Arc<dyn UsageAdapter>,
    system: Arc<dyn SystemConfigAdapter>,
    state: SettingsState,
}

impl SettingsHolder {
    pub fn new(
        store: Arc<dyn SettingsStore>,
        notifications: Arc<dyn NotificationsAdapter>,
        usage: Arc<dyn UsageAdapter>,
        system: Arc<dyn SystemConfigAdapter>,
    ) -> Self {
        Self {
            store,
            notifications,
            usage,
            system,
            state: SettingsState::default(),
        }
    }

    fn refresh(&mut self) {
        let settings = self.store.get();
        self.state.theme = settings.theme;
        self.state.home_target = settings.home_target;
        self.state.wrap_around = settings.wrap_around;
        self.state.default_home = self.system.is_default_home();
        self.state.notification_access = self.notifications.access();
        self.state.usage_access = self.usage.access();
    }

    /// Changes take effect at once; there is no Save button (SET-03).
    fn change(&mut self, mut change: impl FnMut(&mut Settings)) {
        self.state.problem = self
            .store
            .update(&mut change)
            .err()
            .map(|error| error.to_string());
        self.refresh();
    }
}

impl StateHolder for SettingsHolder {
    type State = SettingsState;
    type Intent = SettingsIntent;

    fn state(&self) -> &SettingsState {
        &self.state
    }

    fn handle(&mut self, intent: SettingsIntent) {
        match intent {
            SettingsIntent::Refresh => self.refresh(),
            SettingsIntent::SetTheme(theme) => self.change(|s| s.theme = theme),
            SettingsIntent::SetHomeTarget(target) => self.change(|s| s.home_target = target),
            SettingsIntent::SetWrapAround(on) => self.change(|s| s.wrap_around = on),
            SettingsIntent::OpenNotificationAccess => {
                self.state.problem = self
                    .notifications
                    .open_access_settings()
                    .err()
                    .map(|error| error.to_string());
            }
            SettingsIntent::OpenUsageAccess => {
                self.state.problem = self
                    .usage
                    .open_access_settings()
                    .err()
                    .map(|error| error.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::MemorySettingsStore;
    use meegone_core_data::fakes::Fakes;

    use super::*;

    fn setup() -> (Fakes, Arc<MemorySettingsStore>, SettingsHolder) {
        let fakes = Fakes::default();
        let store = Arc::new(MemorySettingsStore::default());
        let mut holder = SettingsHolder::new(
            store.clone(),
            fakes.notifications.clone(),
            fakes.usage.clone(),
            fakes.system.clone(),
        );
        holder.handle(SettingsIntent::Refresh);
        (fakes, store, holder)
    }

    #[test]
    fn a_change_is_saved_to_the_store_at_once() {
        // SET-03, ARCH-06: the store is the one source of truth.
        let (_, store, mut holder) = setup();

        holder.handle(SettingsIntent::SetTheme(ThemeChoice::Dark));
        holder.handle(SettingsIntent::SetWrapAround(false));

        assert_eq!(store.get().theme, ThemeChoice::Dark);
        assert!(!store.get().wrap_around);
        assert_eq!(holder.state().theme, ThemeChoice::Dark);
        assert!(!holder.state().wrap_around);
    }

    #[test]
    fn shows_the_status_of_each_access() {
        // SET-02
        let (fakes, _, mut holder) = setup();
        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        *fakes.system.default_home.lock().unwrap() = true;

        holder.handle(SettingsIntent::Refresh);

        assert_eq!(holder.state().notification_access, Access::Granted);
        assert_eq!(holder.state().usage_access, Access::Missing);
        assert!(holder.state().default_home);
    }

    #[test]
    fn access_shortcuts_open_the_system_screens() {
        let (fakes, _, mut holder) = setup();

        holder.handle(SettingsIntent::OpenNotificationAccess);
        holder.handle(SettingsIntent::OpenUsageAccess);

        assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
        assert_eq!(*fakes.usage.access_screen_opened.lock().unwrap(), 1);
    }
}
