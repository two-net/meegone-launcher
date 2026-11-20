//! Colour tokens (SRS 5.3) and every pairing of them the host draws ([`PAIRINGS`]). Values may
//! change in design review as long as the contrast tests below still pass (A11Y-03).

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

/// The opacity, in percent, of the press highlight: the theme's primary text colour over
/// whatever is pressed. The host's ripples draw it as alpha `0x1F` (`SettingsRows.ripple`).
///
/// A press lasts only while the finger is down, over the very text it covers, so only primary
/// text is held to 4.5:1 on it. Secondary and accent text are not (A11Y-03, ADR 0038).
pub const PRESSED_PERCENT: u8 = 12;

/// A colour token, named so a [`Pairing`] can say which colours it puts together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Background,
    Surface,
    TextPrimary,
    TextSecondary,
    Accent,
}

impl Tokens {
    /// The colour `ink` names in this theme.
    pub fn ink(&self, ink: Ink) -> Rgb {
        match ink {
            Ink::Background => self.background,
            Ink::Surface => self.surface,
            Ink::TextPrimary => self.text_primary,
            Ink::TextSecondary => self.text_secondary,
            Ink::Accent => self.accent,
        }
    }
}

/// How much contrast a pairing needs, as WCAG 2.2 sets it (A11Y-03, R8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Text: 4.5:1.
    Text,
    /// Large text, 18 sp or 14 sp bold: 3:1.
    LargeText,
    /// A graphic that carries meaning, such as a switch, a marker or the ✕: 3:1.
    Graphic,
}

impl Need {
    /// The least contrast ratio it needs.
    pub fn ratio(self) -> f64 {
        match self {
            Need::Text => 4.5,
            Need::LargeText | Need::Graphic => 3.0,
        }
    }
}

/// One way the host puts two token colours together: a foreground drawn on each of the
/// colours it is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pairing {
    /// What is drawn, in words.
    pub what: &'static str,
    /// The Java classes that draw it.
    pub drawn_by: &'static str,
    pub foreground: Ink,
    pub on: &'static [Ink],
    pub need: Need,
}

const BACKGROUNDS: &[Ink] = &[Ink::Background, Ink::Surface];

/// Every pairing of token colours the host draws, which the tests below hold to its [`Need`] in
/// both themes (A11Y-03). The Java host takes every colour it draws from these tokens, and
/// `tools/arch-check` fails on any other colour in its code (ADR 0038).
///
/// Some colours are left out on purpose:
/// - press highlights, which last only while a finger covers them ([`PRESSED_PERCENT`]);
/// - surfaces against the background, dividers and the rings around folders and the ✕, which
///   only decorate: cards, menus, the search field and folders are known by their text;
/// - disabled rows, drawn at 38 %, which WCAG exempts as inactive; the reason they are
///   disabled is shown in secondary text;
/// - app icons, which their labels identify, and each app's accent, which [`Tokens::app_accent`]
///   holds to [`APP_ACCENT_CONTRAST`] itself.
pub const PAIRINGS: &[Pairing] = &[
    Pairing {
        what: "Labels, titles and body text",
        drawn_by: "AppCell, RecentList, EventsList, EventsHeader, SettingsRows, ImageCard, ImageDetails, PastImages, SetupScreens",
        foreground: Ink::TextPrimary,
        on: BACKGROUNDS,
        need: Need::Text,
    },
    Pairing {
        what: "Section headers, details, ages, notes and the search field's hint",
        drawn_by: "SectionHeader, RecentList, EventsList, SettingsRows, AccessSettings, SearchPanel",
        foreground: Ink::TextSecondary,
        on: BACKGROUNDS,
        need: Need::Text,
    },
    Pairing {
        what: "Text buttons and links: Clear, Clear all, Allow, Search the web, Unhide, Settings' and setup's buttons, the banner's and the dialogs'",
        drawn_by: "EventsList, RecentList, AccessCard, SearchPanel, SettingsRows, HiddenAppsList, ImageDetails, PastImages, HomeBannerView, Dialogs",
        foreground: Ink::Accent,
        on: BACKGROUNDS,
        need: Need::Text,
    },
    Pairing {
        what: "Done's text on its pill (APPS-12)",
        drawn_by: "HomeViews",
        foreground: Ink::Background,
        on: &[Ink::Accent],
        need: Need::Text,
    },
    Pairing {
        what: "An unavailable app's label, and its Recent card's label and age (APPS-09)",
        drawn_by: "AppCell, RecentList",
        foreground: Ink::TextSecondary,
        on: &[Ink::Background],
        need: Need::Text,
    },
    Pairing {
        what: "Dialogs' titles, messages, fields and the region list",
        drawn_by: "Dialogs",
        foreground: Ink::TextPrimary,
        on: &[Ink::Surface],
        need: Need::Text,
    },
    Pairing {
        what: "The view indicator's highlighted segment, against the others and the background",
        drawn_by: "HomeViews",
        foreground: Ink::TextPrimary,
        on: &[Ink::TextSecondary, Ink::Background],
        need: Need::Graphic,
    },
    Pairing {
        what: "The view indicator's other segments",
        drawn_by: "HomeViews",
        foreground: Ink::TextSecondary,
        on: &[Ink::Background],
        need: Need::Graphic,
    },
    Pairing {
        what: "The ✕ on its disc, in edit and close mode (APPS-13, REC-08)",
        drawn_by: "Badge",
        foreground: Ink::TextPrimary,
        on: &[Ink::Surface],
        need: Need::Graphic,
    },
    Pairing {
        what: "A switch's thumb when off, and a radio button when not chosen",
        drawn_by: "SettingsRows, ChoiceSettings",
        foreground: Ink::TextPrimary,
        on: BACKGROUNDS,
        need: Need::Graphic,
    },
    Pairing {
        what: "A switch's track when off, and a radio button when not chosen",
        drawn_by: "SettingsRows, ChoiceSettings",
        foreground: Ink::TextSecondary,
        on: BACKGROUNDS,
        need: Need::Graphic,
    },
    Pairing {
        what: "A switch's thumb and track when on, and a chosen radio button",
        drawn_by: "SettingsRows, ChoiceSettings",
        foreground: Ink::Accent,
        on: BACKGROUNDS,
        need: Need::Graphic,
    },
    Pairing {
        what: "The dot on a newly installed app (APPS-07)",
        drawn_by: "AppCell",
        foreground: Ink::Accent,
        on: &[Ink::Background],
        need: Need::Graphic,
    },
    Pairing {
        what: "An unavailable app's dot, and its Recent card's plate (APPS-09)",
        drawn_by: "AppCell, RecentList",
        foreground: Ink::TextSecondary,
        on: &[Ink::Background],
        need: Need::Graphic,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    const THEMES: [Theme; 2] = [Theme::Light, Theme::Dark];

    /// `top` at `percent` opacity over `bottom`, as the screen blends them.
    fn over(top: Rgb, percent: u8, bottom: Rgb) -> Rgb {
        let a = f64::from(percent) / 100.0;
        let mix = |t: u8, b: u8| (f64::from(t) * a + f64::from(b) * (1.0 - a)).round() as u8;
        Rgb {
            r: mix(top.r, bottom.r),
            g: mix(top.g, bottom.g),
            b: mix(top.b, bottom.b),
        }
    }

    #[test]
    fn every_pairing_the_host_draws_has_the_contrast_it_needs_in_both_themes() {
        // A11Y-03, SRS 5.3
        let mut failures = Vec::new();
        for theme in THEMES {
            let t = tokens(theme);
            for pairing in PAIRINGS {
                for &on in pairing.on {
                    let ratio = t.ink(pairing.foreground).contrast_ratio(t.ink(on));
                    if ratio < pairing.need.ratio() {
                        failures.push(format!(
                            "{theme:?}: {} ({:?} on {on:?}, {}) is {ratio:.2}, needs {}",
                            pairing.what,
                            pairing.foreground,
                            pairing.drawn_by,
                            pairing.need.ratio()
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn every_text_colour_meets_4_5_to_1_on_background_and_surface() {
        // SRS 5.3: "Every text colour shall meet A11Y-03 on both background and surface."
        for ink in [Ink::TextPrimary, Ink::TextSecondary, Ink::Accent] {
            let pairing = PAIRINGS
                .iter()
                .find(|p| p.foreground == ink && p.on == BACKGROUNDS && p.need == Need::Text);
            assert!(pairing.is_some(), "{ink:?} isn't held to 4.5:1 as text");
        }
    }

    #[test]
    fn the_press_highlight_is_the_hosts_alpha_and_keeps_primary_text_readable() {
        // A11Y-03, ADR 0038: SettingsRows.ripple draws text_primary at alpha 0x1F.
        assert_eq!(
            (u16::from(PRESSED_PERCENT) * 255 + 50) / 100,
            0x1F,
            "the host's ripple alpha"
        );
        for theme in THEMES {
            let t = tokens(theme);
            for behind in [t.background, t.surface] {
                let pressed = over(t.text_primary, PRESSED_PERCENT, behind);
                let ratio = t.text_primary.contrast_ratio(pressed);
                assert!(
                    ratio >= 4.5,
                    "{theme:?}: pressed on {behind:?} is {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn dones_text_stays_readable_while_it_is_pressed() {
        // A11Y-03: Done's press is the same highlight, over the accent (HomeViews.applyDone).
        for theme in THEMES {
            let t = tokens(theme);
            let pressed = over(t.text_primary, PRESSED_PERCENT, t.accent);
            let ratio = t.background.contrast_ratio(pressed);
            assert!(ratio >= 4.5, "{theme:?}: Done pressed is {ratio:.2}");
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
