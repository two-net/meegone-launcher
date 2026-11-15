//! Settings and setup state (SRS 4.8, 4.9): the launcher's own settings, the status of each
//! access, and the banner that offers to make MeeGone the default home app (ONB-04).
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
    /// The Access group (SET-02). Shortcuts in menus need it too (APPS-11).
    pub default_home: bool,
    pub notification_access: Access,
    pub usage_access: Access,
    /// MeeGone isn't the default home app and the user hasn't dismissed or answered the banner
    /// today, so the home views show it (ONB-04).
    pub home_banner: bool,
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
    /// Opens the system dialog for choosing the default home app, from the banner, Settings →
    /// Access or setup (HOME-01, ONB-01, ONB-04).
    RequestDefaultHome,
    /// Hides the banner until tomorrow (ONB-04).
    DismissHomeBanner,
}

/// One day in milliseconds.
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

pub struct SettingsHolder {
    store: Arc<dyn SettingsStore>,
    notifications: Arc<dyn NotificationsAdapter>,
    usage: Arc<dyn UsageAdapter>,
    system: Arc<dyn SystemConfigAdapter>,
    /// The last time the banner was dismissed in this session, even if saving it failed, so a
    /// failed save never brings the banner back (ONB-04).
    banner_dismissed_ms: Option<u64>,
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
            banner_dismissed_ms: None,
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
        let dismissed = settings
            .home_banner_dismissed_ms
            .max(self.banner_dismissed_ms);
        self.state.home_banner =
            !self.state.default_home && dismissed.is_none_or(|at| !self.is_today(at));
    }

    /// Whether `at_ms` falls on today's local calendar day (ONB-04).
    fn is_today(&self, at_ms: u64) -> bool {
        self.local_day(at_ms) == self.local_day(self.system.now_ms())
    }

    /// Days since the Unix epoch, in local time.
    fn local_day(&self, at_ms: u64) -> i64 {
        let local = i64::try_from(at_ms)
            .unwrap_or(i64::MAX)
            .saturating_add(self.system.utc_offset_ms(at_ms));
        local.div_euclid(DAY_MS)
    }

    /// Hides the banner until tomorrow, here and in the store (ONB-04).
    fn dismiss_banner(&mut self) {
        let now = self.system.now_ms();
        self.banner_dismissed_ms = Some(now);
        self.change(|s| s.home_banner_dismissed_ms = Some(now));
    }

    /// A request the system accepted counts as today's answer to the banner, so a user who
    /// declines in the dialog isn't reminded again the same day (ONB-04).
    fn request_default_home(&mut self) {
        match self.system.request_default_home() {
            Ok(()) => self.dismiss_banner(),
            Err(error) => self.state.problem = Some(error.to_string()),
        }
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
            SettingsIntent::RequestDefaultHome => self.request_default_home(),
            SettingsIntent::DismissHomeBanner => self.dismiss_banner(),
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::Fakes;
    use meegone_core_data::{DataError, PersistentSettingsStore};

    use super::*;

    const HOUR_MS: u64 = 60 * 60 * 1000;
    /// Midnight UTC on some day.
    const MIDNIGHT_UTC: u64 = 20_000 * 24 * HOUR_MS;

    fn setup() -> (Fakes, Arc<PersistentSettingsStore>, SettingsHolder) {
        let fakes = Fakes::default();
        let (store, holder) = holder(&fakes);
        (fakes, store, holder)
    }

    /// A holder on `fakes`' storage, as after a restart.
    fn holder(fakes: &Fakes) -> (Arc<PersistentSettingsStore>, SettingsHolder) {
        let store = Arc::new(PersistentSettingsStore::new(fakes.storage.clone()));
        let mut holder = SettingsHolder::new(
            store.clone(),
            fakes.notifications.clone(),
            fakes.usage.clone(),
            fakes.system.clone(),
        );
        holder.handle(SettingsIntent::Refresh);
        (store, holder)
    }

    /// Sets the clock and refreshes, as MeeGone does each time it comes to the foreground.
    fn resume_at(fakes: &Fakes, holder: &mut SettingsHolder, now_ms: u64) {
        *fakes.system.now_ms.lock().unwrap() = now_ms;
        holder.handle(SettingsIntent::Refresh);
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
        holder.handle(SettingsIntent::RequestDefaultHome);

        assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
        assert_eq!(*fakes.usage.access_screen_opened.lock().unwrap(), 1);
        assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 1);
    }

    #[test]
    fn the_banner_shows_only_while_meegone_isnt_the_default_home_app() {
        // ONB-04
        let (fakes, _, mut holder) = setup();
        assert!(holder.state().home_banner);

        *fakes.system.default_home.lock().unwrap() = true;
        holder.handle(SettingsIntent::Refresh);

        assert!(holder.state().default_home);
        assert!(!holder.state().home_banner);
    }

    #[test]
    fn a_dismissed_banner_stays_away_until_the_next_local_day() {
        // ONB-04: "a day" is the local calendar day, here two hours ahead of UTC.
        let fakes = Fakes::default();
        *fakes.system.utc_offset_ms.lock().unwrap() = 2 * HOUR_MS as i64;
        let local_midnight = MIDNIGHT_UTC - 2 * HOUR_MS;
        let (store, mut holder) = holder(&fakes);
        resume_at(&fakes, &mut holder, local_midnight - HOUR_MS);

        holder.handle(SettingsIntent::DismissHomeBanner);
        assert!(!holder.state().home_banner);
        assert_eq!(
            store.get().home_banner_dismissed_ms,
            Some(local_midnight - HOUR_MS)
        );

        resume_at(&fakes, &mut holder, local_midnight - 1);
        assert!(!holder.state().home_banner);
        resume_at(&fakes, &mut holder, local_midnight);
        assert!(holder.state().home_banner);
    }

    #[test]
    fn a_dismissal_is_remembered_after_a_restart() {
        // ONB-04: at most once a day, even if MeeGone is restarted.
        let (fakes, _, mut before) = setup();
        resume_at(&fakes, &mut before, MIDNIGHT_UTC + HOUR_MS);
        before.handle(SettingsIntent::DismissHomeBanner);

        let (_, restarted) = holder(&fakes);

        assert!(!restarted.state().home_banner);
    }

    #[test]
    fn a_dismissal_that_cant_be_saved_still_hides_the_banner_today() {
        let (fakes, store, mut holder) = setup();
        resume_at(&fakes, &mut holder, MIDNIGHT_UTC + HOUR_MS);
        let failure = DataError::Failed("Not enough storage space".into());
        *fakes.storage.error.lock().unwrap() = Some(failure.clone());

        holder.handle(SettingsIntent::DismissHomeBanner);

        assert!(!holder.state().home_banner);
        assert_eq!(holder.state().problem, Some(failure.to_string()));
        assert_eq!(store.get().home_banner_dismissed_ms, None);
        resume_at(&fakes, &mut holder, MIDNIGHT_UTC + 2 * HOUR_MS);
        assert!(!holder.state().home_banner);
    }

    #[test]
    fn requesting_the_role_opens_the_system_dialog_and_answers_the_banner_for_today() {
        // HOME-01, ONB-04: declining in the dialog doesn't bring the banner back today.
        let (fakes, _, mut holder) = setup();
        resume_at(&fakes, &mut holder, MIDNIGHT_UTC + HOUR_MS);

        holder.handle(SettingsIntent::RequestDefaultHome);
        resume_at(&fakes, &mut holder, MIDNIGHT_UTC + 2 * HOUR_MS);

        assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 1);
        assert!(!holder.state().default_home);
        assert!(!holder.state().home_banner);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn a_request_the_system_cant_show_keeps_the_banner_and_says_why() {
        // SRS 8.1: without a system screen, show written instructions instead.
        let (fakes, _, mut holder) = setup();
        let instructions = DataError::Unsupported("Choose MeeGone in Settings".into());
        *fakes.system.error.lock().unwrap() = Some(instructions.clone());

        holder.handle(SettingsIntent::RequestDefaultHome);

        assert!(holder.state().home_banner);
        assert_eq!(holder.state().problem, Some(instructions.to_string()));
    }
}
