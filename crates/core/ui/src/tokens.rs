//! Colour tokens (SRS 5.3). Values may change in design review as long as the contrast tests
//! below still pass (A11Y-03).

use meegone_core_state::{Rgb, Theme};

/// The colours of one theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tokens {
    /// Home views. True black in Dark (THEME-04).
    pub background: Rgb,
    /// Folders, menus, cards, dialogs and settings.
    pub surface: Rgb,
    pub text_primary: Rgb,
    pub text_secondary: Rgb,
    pub divider: Rgb,
    /// Buttons, switches, links and focus.
    pub accent: Rgb,
    /// Opacity of the black scrim behind folders and dialogs, in percent.
    pub scrim_percent: u8,
}

pub const DARK: Tokens = Tokens {
    background: Rgb::hex(0x000000),
    surface: Rgb::hex(0x1A1A1A),
    text_primary: Rgb::hex(0xFFFFFF),
    text_secondary: Rgb::hex(0x8C8C8C),
    divider: Rgb::hex(0x2A2A2A),
    accent: Rgb::hex(0x3DA5FF),
    scrim_percent: 60,
};

pub const LIGHT: Tokens = Tokens {
    background: Rgb::hex(0xF2F2F2),
    surface: Rgb::hex(0xFFFFFF),
    text_primary: Rgb::hex(0x000000),
    text_secondary: Rgb::hex(0x5F5F5F),
    divider: Rgb::hex(0xD9D9D9),
    accent: Rgb::hex(0x0066CC),
    scrim_percent: 40,
};

pub fn tokens(theme: Theme) -> &'static Tokens {
    match theme {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

/// The least contrast an app's accent keeps against the home views' background, as a
/// meaningful graphic (THEME-07, A11Y-03).
pub const APP_ACCENT_CONTRAST: f64 = 3.0;

impl Tokens {
    /// An app's accent as this theme draws it: the colour derived from its icon, made darker or
    /// lighter, keeping its hue, until it has at least 3:1 contrast against the background
    /// (APPS-17, THEME-07). The Events group marker and the Recent card fill use it (EVT-09,
    /// REC-04).
    pub fn app_accent(&self, accent: Rgb) -> Rgb {
        accent.with_contrast(self.background, APP_ACCENT_CONTRAST)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEMES: [Theme; 2] = [Theme::Light, Theme::Dark];

    #[test]
    fn text_meets_4_5_to_1_on_background_and_surface() {
        // A11Y-03, SRS 5.3
        for theme in THEMES {
            let t = tokens(theme);
            for text in [t.text_primary, t.text_secondary] {
                for behind in [t.background, t.surface] {
                    let ratio = text.contrast_ratio(behind);
                    assert!(
                        ratio >= 4.5,
                        "{theme:?}: {text:?} on {behind:?} is {ratio:.2}"
                    );
                }
            }
        }
    }

    #[test]
    fn accent_meets_3_to_1_on_background_and_surface() {
        // A11Y-03: meaningful graphics such as switches and focus rings.
        for theme in THEMES {
            let t = tokens(theme);
            for behind in [t.background, t.surface] {
                let ratio = t.accent.contrast_ratio(behind);
                assert!(
                    ratio >= 3.0,
                    "{theme:?}: accent on {behind:?} is {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn text_on_an_accent_button_meets_4_5_to_1() {
        // A11Y-03: edit mode's Done is the background colour on the accent (APPS-12).
        for theme in THEMES {
            let t = tokens(theme);
            let ratio = t.background.contrast_ratio(t.accent);
            assert!(ratio >= 4.5, "{theme:?}: Done's text is {ratio:.2}");
        }
    }

    #[test]
    fn dark_home_views_are_true_black() {
        // THEME-04
        assert_eq!(DARK.background, Rgb::hex(0x000000));
    }

    #[test]
    fn app_accents_reach_3_to_1_against_the_background_in_both_themes() {
        // THEME-07: white and black icons, and colours a theme's background washes out.
        for theme in THEMES {
            let t = tokens(theme);
            for accent in [0xFFFFFF, 0xF8F8F8, 0xFFEB3B, 0x000000, 0x0A0A0A, 0x1A237E] {
                let drawn = t.app_accent(Rgb::hex(accent));
                let ratio = drawn.contrast_ratio(t.background);
                assert!(
                    ratio >= APP_ACCENT_CONTRAST,
                    "{theme:?}: {accent:06X} is drawn {drawn:?}, {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn an_accent_with_enough_contrast_is_drawn_as_derived() {
        let blue = Rgb::hex(0x1877F2);

        assert_eq!(LIGHT.app_accent(blue), blue);
        assert_eq!(DARK.app_accent(blue), blue);
    }
}
