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

/// The WCAG 2.2 contrast ratio between two colours, from 1 to 21 (R8).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}

fn luminance(colour: Rgb) -> f64 {
    fn channel(value: u8) -> f64 {
        let c = f64::from(value) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(colour.r) + 0.7152 * channel(colour.g) + 0.0722 * channel(colour.b)
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
                    let ratio = contrast_ratio(text, behind);
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
                let ratio = contrast_ratio(t.accent, behind);
                assert!(
                    ratio >= 3.0,
                    "{theme:?}: accent on {behind:?} is {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn dark_home_views_are_true_black() {
        // THEME-04
        assert_eq!(DARK.background, Rgb::hex(0x000000));
    }

    #[test]
    fn black_on_white_is_21_to_1() {
        let ratio = contrast_ratio(Rgb::hex(0x000000), Rgb::hex(0xFFFFFF));
        assert!((ratio - 21.0).abs() < 1e-9);
    }
}
