//! Settings screens (SRS 4.8). They render [`SettingsState`], turn input into
//! [`SettingsIntent`]s and show the sections other features provide, which the shell adds
//! (ADR 0002).

use meegone_core_state::ThemeChoice;
use meegone_core_ui::{Dispatch, Slot, Tokens, View};
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

    /// HOME-04
    pub fn wrap_around_toggled(&self, on: bool) {
        self.dispatch.send(SettingsIntent::SetWrapAround(on));
    }
}

impl View for SettingsView {
    type State = SettingsState;

    fn render(&mut self, _state: &SettingsState, tokens: &Tokens) {
        // Drawing Settings' own groups arrives with the UI toolkit (ADR 0002).
        for section in &mut self.sections {
            section.render(tokens);
        }
    }
}
