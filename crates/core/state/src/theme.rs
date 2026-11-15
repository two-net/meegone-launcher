use std::sync::Arc;

use meegone_core_data::{SettingsStore, SystemConfigAdapter, ThemeChoice};

use crate::StateHolder;

/// The theme every MeeGone surface is drawn in (THEME-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

/// System follows the device's dark-theme setting; Light and Dark are fixed (THEME-01,
/// THEME-02).
pub fn resolve_theme(choice: ThemeChoice, system_dark: bool) -> Theme {
    match choice {
        ThemeChoice::System if system_dark => Theme::Dark,
        ThemeChoice::System | ThemeChoice::Light => Theme::Light,
        ThemeChoice::Dark => Theme::Dark,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThemeState {
    pub choice: ThemeChoice,
    pub theme: Theme,
}

#[derive(Debug)]
pub enum ThemeIntent {
    /// The stored choice or the device's dark-theme setting may have changed.
    Refresh,
}

/// Resolves the theme from the settings store and the device.
///
/// Settings writes the choice and the shell applies the result. Theme lives in core because
/// both use it (ADR 0002).
pub struct ThemeHolder {
    settings: Arc<dyn SettingsStore>,
    system: Arc<dyn SystemConfigAdapter>,
    state: ThemeState,
}

impl ThemeHolder {
    pub fn new(settings: Arc<dyn SettingsStore>, system: Arc<dyn SystemConfigAdapter>) -> Self {
        Self {
            settings,
            system,
            state: ThemeState::default(),
        }
    }
}

impl StateHolder for ThemeHolder {
    type State = ThemeState;
    type Intent = ThemeIntent;

    fn state(&self) -> &ThemeState {
        &self.state
    }

    fn handle(&mut self, intent: ThemeIntent) {
        match intent {
            ThemeIntent::Refresh => {
                let choice = self.settings.get().theme;
                self.state = ThemeState {
                    choice,
                    theme: resolve_theme(choice, self.system.is_dark()),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::MemorySettingsStore;
    use meegone_core_data::fakes::Fakes;

    use super::*;

    #[test]
    fn system_follows_the_device_and_fixed_choices_do_not() {
        assert_eq!(resolve_theme(ThemeChoice::System, true), Theme::Dark);
        assert_eq!(resolve_theme(ThemeChoice::System, false), Theme::Light);
        assert_eq!(resolve_theme(ThemeChoice::Light, true), Theme::Light);
        assert_eq!(resolve_theme(ThemeChoice::Dark, false), Theme::Dark);
    }

    #[test]
    fn a_light_choice_stays_light_when_the_device_turns_dark() {
        // AC-06
        let fakes = Fakes::default();
        let settings = Arc::new(MemorySettingsStore::default());
        settings
            .update(&mut |s| s.theme = ThemeChoice::Light)
            .unwrap();
        let mut holder = ThemeHolder::new(settings, fakes.system.clone());

        *fakes.system.dark.lock().unwrap() = true;
        holder.handle(ThemeIntent::Refresh);

        assert_eq!(holder.state().theme, Theme::Light);
    }

    #[test]
    fn the_system_choice_turns_dark_with_the_device() {
        // AC-05
        let fakes = Fakes::default();
        let mut holder = ThemeHolder::new(
            Arc::new(MemorySettingsStore::default()),
            fakes.system.clone(),
        );
        holder.handle(ThemeIntent::Refresh);
        assert_eq!(holder.state().theme, Theme::Light);

        *fakes.system.dark.lock().unwrap() = true;
        holder.handle(ThemeIntent::Refresh);

        assert_eq!(holder.state().theme, Theme::Dark);
    }
}
