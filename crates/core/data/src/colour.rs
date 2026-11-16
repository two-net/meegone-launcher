//! Colour maths on [`Rgb`]: luminance and contrast as WCAG 2.2 defines them (R8), and the
//! nearest colour of the same hue that has enough contrast against a background (THEME-07).

use crate::model::Rgb;

impl Rgb {
    pub const BLACK: Rgb = Rgb::hex(0x000000);
    pub const WHITE: Rgb = Rgb::hex(0xFFFFFF);

    /// The relative luminance, from 0 for black to 1 for white (WCAG 2.2).
    pub fn luminance(self) -> f64 {
        fn channel(value: u8) -> f64 {
            let c = f64::from(value) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// The WCAG 2.2 contrast ratio between two colours, from 1 to 21 (R8).
    pub fn contrast_ratio(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (lighter, darker) = if a > b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// This colour if it has at least `min` contrast against `background`. Otherwise the colour
    /// of the same hue and saturation whose lightness is the closest that reaches it: darker
    /// against a light background, lighter against a dark one (THEME-07).
    ///
    /// If even black or white wouldn't reach `min`, it is whichever of the two has more
    /// contrast.
    pub fn with_contrast(self, background: Rgb, min: f64) -> Rgb {
        if self.contrast_ratio(background) >= min {
            return self;
        }
        let darker = Rgb::BLACK.contrast_ratio(background) >= Rgb::WHITE.contrast_ratio(background);
        let (hue, saturation, lightness) = self.hsl();
        let end = if darker { 0.0 } else { 1.0 };
        let reaches = |lightness: f64| {
            Rgb::from_hsl(hue, saturation, lightness).contrast_ratio(background) >= min
        };
        if !reaches(end) {
            return Rgb::from_hsl(hue, saturation, end);
        }
        // Luminance only grows with lightness, so the lightnesses that reach `min` lie on one
        // side of a single boundary. Halve the gap to it until it is far below one step of a
        // channel.
        let (mut short, mut enough) = (lightness, end);
        for _ in 0..40 {
            let middle = (short + enough) / 2.0;
            if reaches(middle) {
                enough = middle;
            } else {
                short = middle;
            }
        }
        Rgb::from_hsl(hue, saturation, enough)
    }

    /// Hue (0 to 1, a full turn), saturation and lightness, each from 0 to 1.
    fn hsl(self) -> (f64, f64, f64) {
        let [r, g, b] = [self.r, self.g, self.b].map(|value| f64::from(value) / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let lightness = (max + min) / 2.0;
        let chroma = max - min;
        if chroma == 0.0 {
            return (0.0, 0.0, lightness);
        }
        let saturation = chroma / (1.0 - (2.0 * lightness - 1.0).abs());
        let sector = if max == r {
            ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            (b - r) / chroma + 2.0
        } else {
            (r - g) / chroma + 4.0
        };
        (sector / 6.0, saturation, lightness)
    }

    fn from_hsl(hue: f64, saturation: f64, lightness: f64) -> Rgb {
        let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
        let sector = hue.rem_euclid(1.0) * 6.0;
        let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match sector as u8 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let m = lightness - chroma / 2.0;
        let channel = |value: f64| ((value + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb {
            r: channel(r),
            g: channel(g),
            b: channel(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The home views' backgrounds in Dark and Light (SRS 5.3).
    const BACKGROUNDS: [Rgb; 2] = [Rgb::hex(0x000000), Rgb::hex(0xF2F2F2)];

    /// 4096 colours spread over the whole sRGB cube.
    fn sweep() -> impl Iterator<Item = Rgb> {
        (0..16u8).flat_map(|r| {
            (0..16u8).flat_map(move |g| {
                (0..16u8).map(move |b| Rgb {
                    r: r * 17,
                    g: g * 17,
                    b: b * 17,
                })
            })
        })
    }

    #[test]
    fn black_on_white_is_21_to_1_and_the_order_does_not_matter() {
        let ratio = Rgb::BLACK.contrast_ratio(Rgb::WHITE);
        assert!((ratio - 21.0).abs() < 1e-9);
        assert_eq!(
            Rgb::hex(0x3DA5FF).contrast_ratio(Rgb::BLACK),
            Rgb::BLACK.contrast_ratio(Rgb::hex(0x3DA5FF))
        );
        assert_eq!(Rgb::hex(0x808080).contrast_ratio(Rgb::hex(0x808080)), 1.0);
    }

    #[test]
    fn luminance_runs_from_black_to_white() {
        assert_eq!(Rgb::BLACK.luminance(), 0.0);
        assert!((Rgb::WHITE.luminance() - 1.0).abs() < 1e-9);
        // Green looks far brighter than blue.
        assert!(Rgb::hex(0x00FF00).luminance() > Rgb::hex(0x0000FF).luminance());
    }

    #[test]
    fn every_colour_round_trips_through_hsl() {
        for colour in sweep() {
            let (hue, saturation, lightness) = colour.hsl();
            assert_eq!(Rgb::from_hsl(hue, saturation, lightness), colour);
        }
    }

    #[test]
    fn every_colour_reaches_3_to_1_against_both_theme_backgrounds() {
        // THEME-07
        for background in BACKGROUNDS {
            for colour in sweep() {
                let adjusted = colour.with_contrast(background, 3.0);
                let ratio = adjusted.contrast_ratio(background);
                assert!(
                    ratio >= 3.0,
                    "{colour:?} became {adjusted:?}, {ratio:.2} against {background:?}"
                );
            }
        }
    }

    #[test]
    fn a_colour_that_already_reaches_it_is_unchanged() {
        for background in BACKGROUNDS {
            for colour in sweep().filter(|c| c.contrast_ratio(background) >= 3.0) {
                assert_eq!(colour.with_contrast(background, 3.0), colour);
            }
        }
    }

    #[test]
    fn an_adjusted_colour_changes_no_more_than_it_needs_to() {
        // The closest lightness that reaches 3:1 lands just past it; one rounding step of a
        // channel moves the ratio by a few hundredths at most.
        for background in BACKGROUNDS {
            for colour in sweep().filter(|c| c.contrast_ratio(background) < 3.0) {
                let adjusted = colour.with_contrast(background, 3.0);
                let ratio = adjusted.contrast_ratio(background);
                assert!(
                    ratio < 3.1,
                    "{colour:?} went too far, to {adjusted:?}: {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn an_adjusted_colour_keeps_its_hue() {
        for background in BACKGROUNDS {
            for colour in [0xFFD500, 0x00E5FF, 0x7CFC00, 0x1A0033, 0x330000, 0x002200] {
                let colour = Rgb::hex(colour);
                let adjusted = colour.with_contrast(background, 3.0);
                if adjusted == colour {
                    continue;
                }
                let (before, _, _) = colour.hsl();
                let (after, _, _) = adjusted.hsl();
                let turn = (before - after).abs();
                assert!(
                    turn.min(1.0 - turn) < 0.02,
                    "{colour:?} became {adjusted:?} against {background:?}"
                );
            }
        }
    }

    #[test]
    fn darkens_against_a_light_background_and_lightens_against_a_dark_one() {
        let light_grey = Rgb::hex(0xE0E0E0);
        let dark_grey = Rgb::hex(0x202020);

        let on_light = light_grey.with_contrast(Rgb::hex(0xF2F2F2), 3.0);
        let on_dark = dark_grey.with_contrast(Rgb::BLACK, 3.0);

        assert!(on_light.luminance() < light_grey.luminance());
        assert!(on_dark.luminance() > dark_grey.luminance());
    }

    #[test]
    fn beyond_what_black_or_white_can_reach_it_takes_the_better_of_them() {
        let mid_grey = Rgb::hex(0x777777);

        assert_eq!(mid_grey.with_contrast(Rgb::hex(0x808080), 21.0), Rgb::BLACK);
        assert_eq!(mid_grey.with_contrast(Rgb::hex(0x404040), 21.0), Rgb::WHITE);
    }
}
