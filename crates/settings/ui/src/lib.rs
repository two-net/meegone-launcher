//! Settings screens (SRS 4.8). They render [`SettingsState`], turn input into
//! [`SettingsIntent`]s and show the sections other features provide, which the shell adds
//! (ADR 0002).
//!
//! This feature also provides [`HomeBanner`], which the shell shows on the home views while
//! MeeGone isn't the default home app (ONB-04).

use meegone_core_state::HomeTarget;
pub use meegone_core_state::ThemeChoice;
use meegone_core_ui::{Dispatch, Latest, Slot, Tokens, View};
use meegone_settings_state::{SettingsIntent, SettingsState};

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
}

impl View for SettingsView {
    type State = SettingsState;

    fn render(&mut self, _state: &SettingsState, tokens: &Tokens) {
        // Drawing Settings' own groups arrives with #34, in Android's views (ADR 0006).
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
        // Drawing the banner when `is_visible` isn't built yet (ADR 0006).
    }
}
