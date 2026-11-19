//! Settings and setup state (SRS 4.8, 4.9): the launcher's own settings, the status of each
//! access, first-run setup and which of its steps shows (ONB-01, ADR 0036), and the banner that
//! offers to make MeeGone the default home app (ONB-04).
//!
//! Settings owns the screens; the values live in the shared settings store (ADR 0002). Each
//! feature's own section, such as the lock-screen image, belongs to that feature and is put in
//! place by the shell.

use std::sync::Arc;

use meegone_core_data::{
    DataError, NotificationsAdapter, Settings, SettingsStore, SystemConfigAdapter, UsageAdapter,
    setup_done,
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
    /// Written instructions for an Access item whose shortcut found no system screen to open,
    /// shown below that item until Settings is opened again or MeeGone comes back to the
    /// foreground (SRS 8.1).
    pub instructions: Option<(AccessItem, String)>,
    /// First-run setup, while it shows (ONB-01). It covers the home views.
    pub setup: Option<Setup>,
    pub problem: Option<String>,
}

/// A screen of first-run setup, in order (ONB-01). Setup has no theme step: System applies
/// (ONB-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Welcome,
    /// Make MeeGone the default home app, with the system's dialog (HOME-01). Skipped while it
    /// is already.
    DefaultHome,
    /// The daily lock-screen image, which that feature provides (WALL-02, ADR 0002).
    LockScreenImage,
    /// Notification access for Events and usage access for Recent, each with Allow and Not now
    /// (ONB-02). Skipped while both are allowed.
    Access,
}

/// First-run setup while it shows (ONB-01, ADR 0036). Nothing of it is saved until it is
/// finished, so leaving MeeGone part-way starts it again at Welcome next time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    pub step: SetupStep,
    /// "Not now" on notification access: answered, so setup doesn't wait for it (ONB-02).
    pub notifications_put_off: bool,
    /// "Not now" on usage access (ONB-02).
    pub usage_put_off: bool,
}

impl Setup {
    fn start() -> Self {
        Self {
            step: SetupStep::Welcome,
            notifications_put_off: false,
            usage_put_off: false,
        }
    }
}

/// A group of Settings (SET-02) that Settings can open at, rather than at its top. Only the
/// groups something opens directly are listed: the others are reached by scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsGroup {
    /// The lock-screen image's options, opened from the home menu (HOME-10).
    LockScreenImage,
}

/// An item of Settings → Access, each with its status and a shortcut to its system screen
/// (SET-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessItem {
    /// MeeGone is the default home app (HOME-01).
    DefaultHome,
    /// Notification access, for Events (EVT-03).
    Notifications,
    /// Usage access, for Recent (REC-02).
    Usage,
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
    /// "Reset launcher", once confirmed (SET-06). Settings restores its own options to their
    /// defaults; the shell has every other feature restore its own (ADR 0002, ADR 0035).
    ResetLauncher,
    /// Continue on Welcome or the lock-screen image step: the next step that still needs the
    /// user, or the end of setup (ONB-01). The shell has the lock-screen image apply its step
    /// first (ADR 0036).
    SetupContinue,
    /// "Not now" in setup (ONB-02): on the default-home step it moves on, and counts as today's
    /// answer to the banner (ONB-04); on notification or usage access it answers that item.
    /// Setup ends once both are answered, granted or put off.
    SetupNotNow(AccessItem),
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
    /// Setup was finished in this session, even if saving that failed, so it never comes back
    /// before MeeGone restarts (ONB-01).
    setup_finished: bool,
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
            setup_finished: false,
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
        self.follow_setup(settings.setup_install_ms);
    }

    /// Starts setup on an install that hasn't finished it (ONB-01), and moves it on with what
    /// the system reports: the default-home step once MeeGone holds the role, as after the
    /// system's dialog, and the end once both accesses are answered (ADR 0036).
    fn follow_setup(&mut self, finished_by: Option<u64>) {
        if self.setup_finished || setup_done(finished_by, self.system.installed_ms()) {
            self.state.setup = None;
            return;
        }
        let setup = *self.state.setup.get_or_insert_with(Setup::start);
        match setup.step {
            SetupStep::DefaultHome if self.state.default_home => self.advance(),
            SetupStep::Access if self.both_answered(&setup) => self.finish_setup(),
            _ => {}
        }
    }

    /// Whether both accesses are allowed, or put off with "Not now" (ONB-02).
    fn both_answered(&self, setup: &Setup) -> bool {
        (setup.notifications_put_off || self.state.notification_access == Access::Granted)
            && (setup.usage_put_off || self.state.usage_access == Access::Granted)
    }

    /// The step after `step` that still needs the user, or `None` at the end (ONB-01).
    fn next_step(&self, step: SetupStep) -> Option<SetupStep> {
        match step {
            SetupStep::Welcome if self.state.default_home => Some(SetupStep::LockScreenImage),
            SetupStep::Welcome => Some(SetupStep::DefaultHome),
            SetupStep::DefaultHome => Some(SetupStep::LockScreenImage),
            SetupStep::LockScreenImage
                if self.state.notification_access == Access::Granted
                    && self.state.usage_access == Access::Granted =>
            {
                None
            }
            SetupStep::LockScreenImage => Some(SetupStep::Access),
            SetupStep::Access => None,
        }
    }

    /// The step Back returns to from `step`, skipping the default-home step while MeeGone holds
    /// the role.
    fn previous_step(&self, step: SetupStep) -> Option<SetupStep> {
        match step {
            SetupStep::Welcome => None,
            SetupStep::DefaultHome => Some(SetupStep::Welcome),
            SetupStep::LockScreenImage if self.state.default_home => Some(SetupStep::Welcome),
            SetupStep::LockScreenImage => Some(SetupStep::DefaultHome),
            SetupStep::Access => Some(SetupStep::LockScreenImage),
        }
    }

    fn advance(&mut self) {
        let Some(setup) = self.state.setup else {
            return;
        };
        match self.next_step(setup.step) {
            Some(step) => self.state.setup = Some(Setup { step, ..setup }),
            None => self.finish_setup(),
        }
    }

    fn not_now(&mut self, item: AccessItem) {
        let Some(mut setup) = self.state.setup else {
            return;
        };
        match (setup.step, item) {
            (SetupStep::DefaultHome, AccessItem::DefaultHome) => {
                // Today's answer to the banner too, so it doesn't show as setup ends (ONB-04).
                self.dismiss_banner();
                self.advance();
            }
            (SetupStep::Access, AccessItem::Notifications) => {
                setup.notifications_put_off = true;
                self.state.setup = Some(setup);
                self.refresh();
            }
            (SetupStep::Access, AccessItem::Usage) => {
                setup.usage_put_off = true;
                self.state.setup = Some(setup);
                self.refresh();
            }
            _ => {}
        }
    }

    /// Setup is done on this install, and Applications shows (HOME-05). A save that fails
    /// still ends it until MeeGone restarts, and says why.
    fn finish_setup(&mut self) {
        self.setup_finished = true;
        self.state.setup = None;
        let install = self.system.installed_ms();
        self.change(|s| s.setup_install_ms = Some(install));
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
        let result = self.system.request_default_home();
        let asked = result.is_ok();
        self.opened(AccessItem::DefaultHome, result);
        // After, so a dismissal that can't be saved stays the problem.
        if asked {
            self.dismiss_banner();
        }
    }

    /// After a shortcut to `item`'s system screen: where the system has no such screen, its
    /// written instructions show below the item instead (SRS 8.1); any other failure is a
    /// problem to report.
    fn opened(&mut self, item: AccessItem, result: Result<(), DataError>) {
        self.state.instructions = None;
        self.state.problem = None;
        match result {
            Ok(()) => {}
            Err(DataError::Unsupported(instructions)) => {
                self.state.instructions = Some((item, instructions));
            }
            Err(error) => self.state.problem = Some(error.to_string()),
        }
    }

    /// Settings' own options go back to their defaults (SET-06). Everything else in the store
    /// belongs to another feature, which restores its own (ADR 0002).
    fn reset(&mut self) {
        let defaults = Settings::default();
        self.change(|s| {
            s.theme = defaults.theme;
            s.home_target = defaults.home_target;
            s.wrap_around = defaults.wrap_around;
        });
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
            SettingsIntent::Refresh => {
                self.state.instructions = None;
                self.refresh();
            }
            SettingsIntent::SetTheme(theme) => self.change(|s| s.theme = theme),
            SettingsIntent::SetHomeTarget(target) => self.change(|s| s.home_target = target),
            SettingsIntent::SetWrapAround(on) => self.change(|s| s.wrap_around = on),
            SettingsIntent::OpenNotificationAccess => {
                let result = self.notifications.open_access_settings();
                self.opened(AccessItem::Notifications, result);
            }
            SettingsIntent::OpenUsageAccess => {
                let result = self.usage.open_access_settings();
                self.opened(AccessItem::Usage, result);
            }
            SettingsIntent::RequestDefaultHome => self.request_default_home(),
            SettingsIntent::DismissHomeBanner => self.dismiss_banner(),
            SettingsIntent::ResetLauncher => self.reset(),
            SettingsIntent::SetupContinue => self.advance(),
            SettingsIntent::SetupNotNow(item) => self.not_now(item),
        }
    }

    /// Back in setup returns to the previous step; on Welcome there is none (ONB-01).
    fn on_back(&mut self) -> bool {
        let Some(setup) = self.state.setup else {
            return false;
        };
        match self.previous_step(setup.step) {
            Some(step) => {
                self.state.setup = Some(Setup { step, ..setup });
                true
            }
            None => false,
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
    fn a_request_whose_answer_cant_be_saved_says_why() {
        // ONB-04, REL-03: the dialog opened, but the day's answer wasn't saved.
        let (fakes, _, mut holder) = setup();
        let failure = DataError::Failed("Not enough storage space".into());
        *fakes.storage.error.lock().unwrap() = Some(failure.clone());

        holder.handle(SettingsIntent::RequestDefaultHome);

        assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 1);
        assert_eq!(holder.state().problem, Some(failure.to_string()));
        assert!(!holder.state().home_banner);
    }

    #[test]
    fn a_request_the_system_cant_show_keeps_the_banner_and_says_why() {
        // SRS 8.1: without a system screen, show written instructions instead.
        let (fakes, _, mut holder) = setup();
        let instructions = "Choose MeeGone in Settings";
        *fakes.system.error.lock().unwrap() = Some(DataError::Unsupported(instructions.into()));

        holder.handle(SettingsIntent::RequestDefaultHome);

        assert!(holder.state().home_banner);
        assert_eq!(
            holder.state().instructions,
            Some((AccessItem::DefaultHome, instructions.to_owned()))
        );
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn an_access_shortcut_without_a_system_screen_shows_written_instructions_below_its_item() {
        // SET-02, SRS 8.1: until Settings is opened again or MeeGone comes back.
        let (fakes, _, mut holder) = setup();
        let instructions = "Open Settings, then Notification access";
        *fakes.notifications.error.lock().unwrap() =
            Some(DataError::Unsupported(instructions.into()));

        holder.handle(SettingsIntent::OpenNotificationAccess);
        assert_eq!(
            holder.state().instructions,
            Some((AccessItem::Notifications, instructions.to_owned()))
        );
        assert_eq!(holder.state().problem, None);

        holder.handle(SettingsIntent::SetWrapAround(false));
        assert!(holder.state().instructions.is_some());
        holder.handle(SettingsIntent::Refresh);
        assert_eq!(holder.state().instructions, None);
    }

    #[test]
    fn a_shortcut_that_fails_otherwise_reports_a_problem_and_the_next_one_clears_it() {
        let (fakes, _, mut holder) = setup();
        let failure = DataError::Failed("Something went wrong".into());
        *fakes.usage.error.lock().unwrap() = Some(failure.clone());

        holder.handle(SettingsIntent::OpenUsageAccess);
        assert_eq!(holder.state().problem, Some(failure.to_string()));
        assert_eq!(holder.state().instructions, None);

        *fakes.usage.error.lock().unwrap() = None;
        holder.handle(SettingsIntent::OpenUsageAccess);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn reset_launcher_restores_settings_own_options_and_nothing_else() {
        // SET-06, ADR 0002: each feature restores the fields it writes.
        let (_, store, mut holder) = setup();
        holder.handle(SettingsIntent::SetTheme(ThemeChoice::Dark));
        holder.handle(SettingsIntent::SetHomeTarget(HomeTarget::Applications));
        holder.handle(SettingsIntent::SetWrapAround(false));
        holder.handle(SettingsIntent::DismissHomeBanner);
        store
            .update(&mut |s| {
                s.image.use_mobile_data = true;
                s.setup_install_ms = Some(0);
            })
            .unwrap();

        holder.handle(SettingsIntent::ResetLauncher);

        let defaults = Settings::default();
        let settings = store.get();
        assert_eq!(settings.theme, defaults.theme);
        assert_eq!(settings.home_target, defaults.home_target);
        assert_eq!(settings.wrap_around, defaults.wrap_around);
        assert!(settings.image.use_mobile_data);
        assert!(settings.home_banner_dismissed_ms.is_some());
        // Setup isn't a setting: it doesn't come back (ADR 0036).
        assert_eq!(settings.setup_install_ms, Some(0));
        assert_eq!(holder.state().theme, defaults.theme);
        assert!(holder.state().wrap_around);
    }

    const INSTALLED_MS: u64 = 1_763_200_000_000;

    fn step(holder: &SettingsHolder) -> Option<SetupStep> {
        holder.state().setup.map(|setup| setup.step)
    }

    /// A fresh install at [`INSTALLED_MS`], with `system` as the device reports it.
    fn fresh_install(
        system: impl FnOnce(&Fakes),
    ) -> (Fakes, Arc<PersistentSettingsStore>, SettingsHolder) {
        let fakes = Fakes::default();
        *fakes.system.installed_ms.lock().unwrap() = INSTALLED_MS;
        system(&fakes);
        let (store, holder) = holder(&fakes);
        (fakes, store, holder)
    }

    #[test]
    fn a_fresh_install_starts_setup_at_welcome() {
        // ONB-01
        let (_, store, holder) = fresh_install(|_| {});

        assert_eq!(
            holder.state().setup,
            Some(Setup {
                step: SetupStep::Welcome,
                notifications_put_off: false,
                usage_put_off: false,
            })
        );
        assert_eq!(store.get().setup_install_ms, None);
    }

    #[test]
    fn setup_goes_through_its_steps_and_is_done_on_this_install() {
        // ONB-01, ONB-02, HOME-01
        let (fakes, store, mut holder) = fresh_install(|_| {});

        holder.handle(SettingsIntent::SetupContinue);
        assert_eq!(step(&holder), Some(SetupStep::DefaultHome));
        holder.handle(SettingsIntent::RequestDefaultHome);
        assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 1);
        // MeeGone is chosen in the system's dialog, and comes back to the foreground.
        *fakes.system.default_home.lock().unwrap() = true;
        holder.handle(SettingsIntent::Refresh);
        assert_eq!(step(&holder), Some(SetupStep::LockScreenImage));
        holder.handle(SettingsIntent::SetupContinue);
        assert_eq!(step(&holder), Some(SetupStep::Access));

        holder.handle(SettingsIntent::OpenNotificationAccess);
        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        holder.handle(SettingsIntent::Refresh);
        assert_eq!(
            step(&holder),
            Some(SetupStep::Access),
            "usage is unanswered"
        );
        holder.handle(SettingsIntent::OpenUsageAccess);
        *fakes.usage.access.lock().unwrap() = Access::Granted;
        holder.handle(SettingsIntent::Refresh);

        assert_eq!(holder.state().setup, None);
        assert_eq!(store.get().setup_install_ms, Some(INSTALLED_MS));
        let (_, restarted) = self::holder(&fakes);
        assert_eq!(restarted.state().setup, None);
    }

    #[test]
    fn not_now_on_both_accesses_ends_setup() {
        // ONB-02: declining never blocks.
        let (_, store, mut holder) = fresh_install(|_| {});
        holder.handle(SettingsIntent::SetupContinue);
        holder.handle(SettingsIntent::SetupContinue);
        holder.handle(SettingsIntent::SetupContinue);
        assert_eq!(step(&holder), Some(SetupStep::Access));

        holder.handle(SettingsIntent::SetupNotNow(AccessItem::Notifications));
        assert!(holder.state().setup.unwrap().notifications_put_off);
        holder.handle(SettingsIntent::SetupNotNow(AccessItem::Usage));

        assert_eq!(holder.state().setup, None);
        assert_eq!(store.get().setup_install_ms, Some(INSTALLED_MS));
        assert_eq!(holder.state().notification_access, Access::Missing);
    }

    #[test]
    fn not_now_on_the_default_home_step_moves_on_and_answers_the_banner_for_today() {
        // ONB-04: the banner doesn't show as soon as setup ends.
        let (fakes, store, mut holder) = fresh_install(|_| {});
        holder.handle(SettingsIntent::SetupNotNow(AccessItem::DefaultHome));
        assert_eq!(
            step(&holder),
            Some(SetupStep::Welcome),
            "only on its own step"
        );
        holder.handle(SettingsIntent::SetupContinue);

        holder.handle(SettingsIntent::SetupNotNow(AccessItem::DefaultHome));

        assert_eq!(step(&holder), Some(SetupStep::LockScreenImage));
        assert_eq!(*fakes.system.default_home_requested.lock().unwrap(), 0);
        assert!(store.get().home_banner_dismissed_ms.is_some());
        assert!(!holder.state().home_banner);
    }

    #[test]
    fn setup_skips_the_steps_already_done() {
        // ONB-01: MeeGone is the default and has both accesses, as when chosen at the Home key.
        let (_, store, mut holder) = fresh_install(|fakes| {
            *fakes.system.default_home.lock().unwrap() = true;
            *fakes.notifications.access.lock().unwrap() = Access::Granted;
            *fakes.usage.access.lock().unwrap() = Access::Granted;
        });

        holder.handle(SettingsIntent::SetupContinue);
        assert_eq!(step(&holder), Some(SetupStep::LockScreenImage));
        holder.handle(SettingsIntent::SetupContinue);

        assert_eq!(holder.state().setup, None);
        assert_eq!(store.get().setup_install_ms, Some(INSTALLED_MS));
    }

    #[test]
    fn back_returns_to_the_previous_step_and_not_past_welcome() {
        // ONB-01, HOME-08
        let (fakes, _, mut holder) = fresh_install(|_| {});
        assert!(!holder.on_back());
        for _ in 0..3 {
            holder.handle(SettingsIntent::SetupContinue);
        }
        assert_eq!(step(&holder), Some(SetupStep::Access));

        let mut steps = Vec::new();
        while holder.on_back() {
            steps.push(step(&holder).unwrap());
        }
        assert_eq!(
            steps,
            [
                SetupStep::LockScreenImage,
                SetupStep::DefaultHome,
                SetupStep::Welcome
            ]
        );

        // The default-home step is skipped once MeeGone holds the role.
        *fakes.system.default_home.lock().unwrap() = true;
        holder.handle(SettingsIntent::Refresh);
        holder.handle(SettingsIntent::SetupContinue);
        assert_eq!(step(&holder), Some(SetupStep::LockScreenImage));
        assert!(holder.on_back());
        assert_eq!(step(&holder), Some(SetupStep::Welcome));
    }

    #[test]
    fn setup_runs_once_per_install() {
        // ADR 0036, ADR 0018: settings restored from another install's backup run setup again;
        // settings from before setup existed don't.
        for (finished_by, runs) in [
            (None, true),
            (Some(INSTALLED_MS), false),
            (Some(0), false),
            (Some(INSTALLED_MS - 1), true),
        ] {
            let fakes = Fakes::default();
            *fakes.system.installed_ms.lock().unwrap() = INSTALLED_MS;
            let store = PersistentSettingsStore::new(fakes.storage.clone());
            store
                .update(&mut |s| s.setup_install_ms = finished_by)
                .unwrap();

            let (_, holder) = holder(&fakes);

            assert_eq!(holder.state().setup.is_some(), runs, "{finished_by:?}");
        }
    }

    #[test]
    fn a_setup_whose_end_cant_be_saved_still_ends_and_says_why() {
        // REL-03: never stuck in setup, though it runs again after a restart.
        let (fakes, store, mut holder) = fresh_install(|_| {});
        for _ in 0..3 {
            holder.handle(SettingsIntent::SetupContinue);
        }
        let failure = DataError::Failed("Not enough storage space".into());
        *fakes.storage.error.lock().unwrap() = Some(failure.clone());

        holder.handle(SettingsIntent::SetupNotNow(AccessItem::Notifications));
        holder.handle(SettingsIntent::SetupNotNow(AccessItem::Usage));

        assert_eq!(holder.state().setup, None);
        assert_eq!(holder.state().problem, Some(failure.to_string()));
        holder.handle(SettingsIntent::Refresh);
        assert_eq!(holder.state().setup, None);
        *fakes.storage.error.lock().unwrap() = None;
        assert_eq!(store.get().setup_install_ms, None);
    }
}
