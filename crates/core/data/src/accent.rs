//! An app's accent colour, derived once from its icon and reused wherever the app appears
//! (APPS-17, SRS P4, ADR 0011).
//!
//! The host draws the icon in MeeGone's shape at a fixed size and hands its pixels to
//! [`derive`]. Each theme then adjusts the accent for contrast when it draws it (THEME-07).

use crate::model::Rgb;

/// The accent of an app whose icon hasn't been read yet, or couldn't be.
pub const NEUTRAL: Rgb = Rgb::hex(0x808080);

/// Pixels at least this opaque count; the rest, such as the shape's corners, are left out.
const MIN_ALPHA: u32 = 0x80;
/// A colourful pixel is at least this saturated and this bright (HSV, 0 to 1). Below them,
/// pixels read as white, grey or black.
const MIN_SATURATION: f64 = 0.3;
const MIN_VALUE: f64 = 0.25;
/// Colour wins over grey once this share of the icon is colourful, in percent: a coloured logo
/// on a white plate gives its colour, not white.
const MIN_COLOURFUL_PERCENT: usize = 10;
/// Colourful pixels are grouped by hue into this many sectors of 15°, centred on red, yellow,
/// green, cyan, blue and magenta among others.
const HUE_SECTORS: usize = 24;
/// Without enough colour, pixels are grouped by luminance into this many bands.
const GREY_BANDS: usize = 8;

/// The accent of an icon, from its pixels as Android's `0xAARRGGBB` ints, in any order.
///
/// It is the mean colour of the most common hue, if enough of the icon is colourful, or else of
/// the most common shade of grey. A tie goes to the hue nearer red, or the darker shade, so the
/// same icon always gives the same accent. An icon with no opaque pixels gives [`NEUTRAL`].
pub fn derive(argb: &[u32]) -> Rgb {
    let mut hues = [Mean::default(); HUE_SECTORS];
    let mut greys = [Mean::default(); GREY_BANDS];
    let (mut opaque, mut colourful) = (0, 0);
    for &pixel in argb {
        if pixel >> 24 < MIN_ALPHA {
            continue;
        }
        let colour = Rgb::hex(pixel & 0x00FF_FFFF);
        opaque += 1;
        if let Some(sector) = hue_sector(colour) {
            hues[sector].add(colour);
            colourful += 1;
        }
        let band = (colour.luminance() * GREY_BANDS as f64) as usize;
        greys[band.min(GREY_BANDS - 1)].add(colour);
    }
    if opaque == 0 {
        NEUTRAL
    } else if colourful * 100 >= opaque * MIN_COLOURFUL_PERCENT {
        fullest(&hues)
    } else {
        fullest(&greys)
    }
}

/// The hue sector of a colourful pixel, or `None` for one that reads as grey.
fn hue_sector(colour: Rgb) -> Option<usize> {
    let [r, g, b] = [colour.r, colour.g, colour.b].map(f64::from);
    let max = r.max(g).max(b);
    let chroma = max - r.min(g).min(b);
    if max / 255.0 < MIN_VALUE || chroma / max < MIN_SATURATION {
        return None;
    }
    let sixths = if max == r {
        ((g - b) / chroma).rem_euclid(6.0)
    } else if max == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    // Half a sector's turn first, so each sector is centred on its hue: reds just below and
    // just above 0° fall together.
    let sectors = sixths / 6.0 * HUE_SECTORS as f64 + 0.5;
    Some(sectors as usize % HUE_SECTORS)
}

/// The mean colour of the group with the most pixels; on a tie, the first.
fn fullest(groups: &[Mean]) -> Rgb {
    let mut best = &groups[0];
    for group in groups {
        if group.count > best.count {
            best = group;
        }
    }
    best.colour()
}

/// A running mean of colours.
#[derive(Debug, Clone, Copy, Default)]
struct Mean {
    count: u64,
    r: u64,
    g: u64,
    b: u64,
}

impl Mean {
    fn add(&mut self, colour: Rgb) {
        self.count += 1;
        self.r += u64::from(colour.r);
        self.g += u64::from(colour.g);
        self.b += u64::from(colour.b);
    }

    fn colour(&self) -> Rgb {
        let channel = |sum: u64| ((sum + self.count / 2) / self.count.max(1)) as u8;
        Rgb {
            r: channel(self.r),
            g: channel(self.g),
            b: channel(self.b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPAQUE: u32 = 0xFF00_0000;

    /// `count` opaque pixels of `rgb`.
    fn pixels(rgb: u32, count: usize) -> Vec<u32> {
        vec![OPAQUE | rgb; count]
    }

    /// A 48 × 48 icon: `share` percent of `logo` on a white plate.
    fn logo_on_white(logo: u32, share: usize) -> Vec<u32> {
        let logo_pixels = 48 * 48 * share / 100;
        let mut icon = pixels(logo, logo_pixels);
        icon.extend(pixels(0xFFFFFF, 48 * 48 - logo_pixels));
        icon
    }

    #[test]
    fn a_single_colour_icon_gives_that_colour() {
        for rgb in [0x25D366, 0x1877F2, 0xE4405F, 0xFFC107, 0x808080, 0xFFFFFF] {
            assert_eq!(derive(&pixels(rgb, 100)), Rgb::hex(rgb));
        }
    }

    #[test]
    fn a_coloured_logo_on_a_white_plate_gives_the_logo_colour() {
        // SRS P4: colour as wayfinding, so white is not every app's accent.
        assert_eq!(derive(&logo_on_white(0xEA4335, 20)), Rgb::hex(0xEA4335));
    }

    #[test]
    fn a_speck_of_colour_on_a_white_plate_gives_white() {
        assert_eq!(derive(&logo_on_white(0xEA4335, 5)), Rgb::WHITE);
    }

    #[test]
    fn near_white_and_near_black_icons_give_near_white_and_near_black() {
        let mut near_white = pixels(0xF8F8F8, 300);
        near_white.extend(pixels(0xF4F6F8, 100));
        let mut near_black = pixels(0x0A0A0A, 300);
        near_black.extend(pixels(0x101418, 100));

        let white = derive(&near_white);
        let black = derive(&near_black);

        assert!(white.luminance() > 0.9, "{white:?}");
        assert!(black.luminance() < 0.01, "{black:?}");
    }

    #[test]
    fn a_black_logo_on_a_white_plate_gives_the_larger_of_the_two() {
        assert_eq!(derive(&logo_on_white(0x000000, 30)), Rgb::WHITE);
        assert_eq!(derive(&logo_on_white(0x000000, 70)), Rgb::BLACK);
    }

    #[test]
    fn transparent_pixels_are_left_out() {
        // The shape's corners, and the space around a legacy icon's own outline.
        let mut icon = vec![0x0000_0000; 400];
        icon.extend(vec![0x40FF_FFFF; 400]);
        icon.extend(pixels(0x1877F2, 100));

        assert_eq!(derive(&icon), Rgb::hex(0x1877F2));
    }

    #[test]
    fn an_icon_with_nothing_opaque_gives_the_neutral_accent() {
        assert_eq!(derive(&[]), NEUTRAL);
        assert_eq!(derive(&[0x00FF_0000, 0x7FFF_0000]), NEUTRAL);
    }

    #[test]
    fn shades_of_one_hue_give_their_mean() {
        let mut icon = pixels(0xC80000, 100);
        icon.extend(pixels(0xDC1414, 100));

        assert_eq!(derive(&icon), Rgb::hex(0xD20A0A));
    }

    #[test]
    fn reds_either_side_of_0_degrees_fall_together() {
        // One red just past magenta and one just past orange outnumber the blue together.
        let mut icon = pixels(0xFF0008, 40);
        icon.extend(pixels(0xFF0800, 40));
        icon.extend(pixels(0x0000FF, 60));

        let accent = derive(&icon);

        assert_eq!(accent, Rgb::hex(0xFF0404));
    }

    #[test]
    fn the_most_common_hue_wins_and_a_tie_goes_the_same_way_every_time() {
        let mut more_green = pixels(0x34A853, 60);
        more_green.extend(pixels(0x4285F4, 40));
        assert_eq!(derive(&more_green), Rgb::hex(0x34A853));

        let mut tie = pixels(0x4285F4, 50);
        tie.extend(pixels(0xEA4335, 50));
        let mut reversed = tie.clone();
        reversed.reverse();
        assert_eq!(derive(&tie), Rgb::hex(0xEA4335));
        assert_eq!(derive(&reversed), Rgb::hex(0xEA4335));
    }

    #[test]
    fn dark_or_washed_out_pixels_read_as_grey() {
        // Value below 25 %, and saturation below 30 %.
        assert_eq!(hue_sector(Rgb::hex(0x300000)), None);
        assert_eq!(hue_sector(Rgb::hex(0xFFE0E0)), None);
        assert_eq!(hue_sector(Rgb::hex(0xFF0000)), Some(0));
        assert_eq!(hue_sector(Rgb::hex(0x0000FF)), Some(16));
    }
}
