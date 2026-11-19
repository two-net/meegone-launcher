//! The Settings screen (SRS 4.8, ADR 0035). It renders [`SettingsState`], turns input into
//! [`SettingsIntent`]s and shows the sections other features provide, which the shell adds
//! (ADR 0002). Android's views draw it, groups in SET-02's order (ADR 0006).
//!
//! This feature also provides first-run setup, [`SetupView`], which covers the home views until
//! it is finished, with the lock-screen image's step put in by the shell (ONB-01, ADR 0036), and
//! [`HomeBanner`], which the shell shows on the home views while MeeGone isn't the default home
//! app (ONB-04).

use meegone_core_state::Access;
pub use meegone_core_state::{HomeTarget, ThemeChoice};
use meegone_core_ui::{Dispatch, Latest, Slot, Tokens, View};
pub use meegone_settings_state::{AccessItem, SetupStep};
use meegone_settings_state::{SettingsIntent, SettingsState};

/// Settings → Access as the host draws it: each item's status, and the one whose written
/// instructions show below it (SET-02, SRS 8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessStatus {
    /// MeeGone is the default home app (HOME-01).
    pub default_home: bool,
    /// Notification access is granted (EVT-03).
    pub notifications: bool,
    /// Usage access is granted (REC-02).
    pub usage: bool,
    /// The item whose shortcut found no system screen, so its instructions show instead.
    pub instructions: Option<AccessItem>,
}

impl AccessStatus {
    pub fn of(state: &SettingsState) -> Self {
        Self {
            default_home: state.default_home,
            notifications: state.notification_access == Access::Granted,
            usage: state.usage_access == Access::Granted,
            instructions: state.instructions.as_ref().map(|(item, _)| *item),
        }
    }
}

/// First-run setup as the host draws it (ONB-01, ADR 0036): the step showing, and the accesses
/// put off with "Not now". Each access's status and instructions are [`AccessStatus`]'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupFrame {
    pub step: SetupStep,
    pub notifications_put_off: bool,
    pub usage_put_off: bool,
}

impl SetupFrame {
    /// Setup, while it shows.
    pub fn of(state: &SettingsState) -> Option<Self> {
        state.setup.map(|setup| Self {
            step: setup.step,
            notifications_put_off: setup.notifications_put_off,
            usage_put_off: setup.usage_put_off,
        })
    }
}

/// First-run setup (ONB-01, ADR 0036): Welcome, the default home app, the lock-screen image's
/// step, which the shell puts in (ADR 0002), and access. It never asks about the theme (ONB-05).
pub struct SetupView {
    dispatch: Dispatch<SettingsIntent>,
    image_step: Option<Box<dyn Slot>>,
}

impl SetupView {
    pub fn new(dispatch: Dispatch<SettingsIntent>) -> Self {
        Self {
            dispatch,
            image_step: None,
        }
    }

    /// Puts in the lock-screen image's step. Only the shell calls this (ARCH-03).
    pub fn set_image_step(&mut self, step: Box<dyn Slot>) {
        self.image_step = Some(step);
    }

    /// Continue, on Welcome or the lock-screen image step (ONB-01).
    pub fn continue_tapped(&self) {
        self.dispatch.send(SettingsIntent::SetupContinue);
    }

    /// Allow, or "Make default" on the default-home step: the item's system dialog or screen
    /// opens (HOME-01, ONB-02).
    pub fn allow_tapped(&self, item: AccessItem) {
        self.dispatch.send(match item {
            AccessItem::DefaultHome => SettingsIntent::RequestDefaultHome,
            AccessItem::Notifications => SettingsIntent::OpenNotificationAccess,
            AccessItem::Usage => SettingsIntent::OpenUsageAccess,
        });
    }

    /// "Not now" on the item: declining never blocks setup (ONB-02).
    pub fn not_now_tapped(&self, item: AccessItem) {
        self.dispatch.send(SettingsIntent::SetupNotNow(item));
    }
}

impl View for SetupView {
    type State = SettingsState;

    fn render(&mut self, _state: &SettingsState, tokens: &Tokens) {
        // The host draws setup's own steps from the frame (ADR 0036).
        if let Some(step) = &mut self.image_step {
            step.render(tokens);
        }
    }
}

pub struct SettingsView {
    dispatch: Dispatch<SettingsIntent>,
    sections: Vec<Box<dyn Slot>>,
}

impl SettingsView {
    pub fn new(dispatch: Dispatch<SettingsIntent>) -> Self {
        Self {
            dispatch,
            sections: Vec::new(),
        }
    }

    /// Adds another feature's section. Only the shell calls this (ARCH-03).
    pub fn add_section(&mut self, section: Box<dyn Slot>) {
        self.sections.push(section);
    }

    /// THEME-01
    pub fn theme_chosen(&self, theme: ThemeChoice) {
        self.dispatch.send(SettingsIntent::SetTheme(theme));
    }

    /// "Home button opens": the last view or always Applications (HOME-06).
    pub fn home_target_chosen(&self, target: HomeTarget) {
        self.dispatch.send(SettingsIntent::SetHomeTarget(target));
    }

    /// HOME-04
    pub fn wrap_around_toggled(&self, on: bool) {
        self.dispatch.send(SettingsIntent::SetWrapAround(on));
    }

    /// The default-home-app item of the Access group (SET-02, HOME-01).
    pub fn default_home_tapped(&self) {
        self.dispatch.send(SettingsIntent::RequestDefaultHome);
    }

    /// The notification-access item of the Access group: its system screen opens (SET-02,
    /// EVT-03).
    pub fn notification_access_tapped(&self) {
        self.dispatch.send(SettingsIntent::OpenNotificationAccess);
    }

    /// The usage-access item of the Access group: its system screen opens (SET-02, REC-02).
    pub fn usage_access_tapped(&self) {
        self.dispatch.send(SettingsIntent::OpenUsageAccess);
    }

    /// "Reset launcher", after the user confirmed it (SET-06).
    pub fn reset_launcher_confirmed(&self) {
        self.dispatch.send(SettingsIntent::ResetLauncher);
    }
}

impl View for SettingsView {
    type State = SettingsState;

    fn render(&mut self, _state: &SettingsState, tokens: &Tokens) {
        // The host draws Settings' own groups from the frame (ADR 0035).
        for section in &mut self.sections {
            section.render(tokens);
        }
    }
}

/// The dismissible banner offering to make MeeGone the default home app (ONB-04). The shell
/// shows it on the home views, so it reads the latest settings state through a [`Latest`]
/// (ADR 0002).
pub struct HomeBanner {
    latest: Latest<SettingsState>,
    dispatch: Dispatch<SettingsIntent>,
}

impl HomeBanner {
    pub fn new(latest: Latest<SettingsState>, dispatch: Dispatch<SettingsIntent>) -> Self {
        Self { latest, dispatch }
    }

    /// Shown while MeeGone isn't the default home app, at most once a day (ONB-04).
    pub fn is_visible(&self) -> bool {
        self.latest.get().home_banner
    }

    /// Opens the system dialog for choosing the default home app (HOME-01).
    pub fn make_default_tapped(&self) {
        self.dispatch.send(SettingsIntent::RequestDefaultHome);
    }

    /// Hides the banner until tomorrow (ONB-04).
    pub fn dismissed(&self) {
        self.dispatch.send(SettingsIntent::DismissHomeBanner);
    }
}

impl Slot for HomeBanner {
    fn render(&mut self, _tokens: &Tokens) {
        // The host draws the banner while the frame says so: the shell shows it only on the
        // home views as nothing covers them (ADR 0006, ADR 0036).
    }
}
