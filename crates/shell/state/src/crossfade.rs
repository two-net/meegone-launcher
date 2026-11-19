//! Cross-fades: the image-of-the-day card's thumbnail fading to a new image (ANIM-22, 300 ms),
//! and the whole screen fading from one theme to the other (ANIM-19, 250 ms), both
//! decelerating.
//!
//! The host decodes a new thumbnail off its main thread, which takes tens of milliseconds. So
//! the host, not the cache, says when a new thumbnail is ready to fade in over the old one
//! (`ShellIntent::NewImageShown`, ADR 0022), and the fade is seen from its start. A theme fades
//! when the theme drawn changes, whether the device or Settings changed it (ADR 0033).
//!
//! Each runs on the shared [`Timed`] clock, so it plays when it is next drawn, takes a late
//! frame in small steps, and is instant with animations removed (ANIM-05).

use std::time::Duration;

use meegone_core_state::motion::Tween;

use crate::timed::Timed;

/// How a cross-fade is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossFadeState {
    /// How far the new picture has faded in over the old one, from 0 to 1.
    pub progress: f64,
    /// The fade is still playing, so the host should draw another frame.
    pub animating: bool,
}

impl Default for CrossFadeState {
    /// At rest: only the new picture shows.
    fn default() -> Self {
        Self {
            progress: 1.0,
            animating: false,
        }
    }
}

/// Runs one cross-fade. `HomeShell` owns one for each, and publishes its [`CrossFadeState`].
#[derive(Debug)]
pub(crate) struct CrossFade {
    tween: Tween,
    timed: Timed,
}

impl CrossFade {
    /// A cross-fade on `tween`, at rest.
    pub fn new(tween: Tween) -> Self {
        Self {
            tween,
            timed: Timed::default(),
        }
    }

    /// Starts the fade to the new picture. One that comes while another fades starts again:
    /// the theme's over a picture of the screen as it was last drawn (ADR 0033), the image
    /// card's with the newer thumbnail over the one shown in full (ADR 0022).
    pub fn start(&mut self, animation_scale: f64) {
        self.timed.start(self.tween, animation_scale);
    }

    /// Shows the new picture at once, as when animations are removed while it fades (ANIM-05).
    pub fn finish(&mut self) {
        self.timed.finish();
    }

    /// Advances the fade to the display's frame time (ANIM-04).
    pub fn frame(&mut self, at: Duration) {
        self.timed.frame(at);
    }

    pub fn state(&self) -> CrossFadeState {
        match self.timed.eased() {
            None => CrossFadeState::default(),
            Some(progress) => CrossFadeState {
                progress,
                animating: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_state::motion::{IMAGE_CROSSFADE, THEME_CROSSFADE};

    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// The display draws a frame every 8 ms from `from` up to `to`, in ms.
    fn frames(fade: &mut CrossFade, from: u64, to: u64) {
        for at in (from..=to).step_by(8) {
            fade.frame(ms(at));
        }
    }

    /// The fade plays for `length` ms, decelerating, from the first frame drawn after it
    /// starts at 5 000 ms.
    fn plays_for(tween: Tween) {
        let length = u64::try_from(tween.length.as_millis()).unwrap();
        let mut fade = CrossFade::new(tween);
        assert_eq!(fade.state(), CrossFadeState::default());

        fade.start(1.0);
        assert_eq!(
            fade.state(),
            CrossFadeState {
                progress: 0.0,
                animating: true
            }
        );

        // The clock starts at the first frame drawn after the fade started.
        fade.frame(ms(5_000));
        assert_eq!(fade.state().progress, 0.0);
        frames(&mut fade, 5_000, 5_000 + length / 2 - 6);
        let half = fade.state().progress;
        assert!(half > 0.5 && half < 1.0, "decelerating: {half}");

        frames(&mut fade, 5_000 + length / 2 + 2, 5_000 + length - 4);
        assert!(fade.state().animating);
        fade.frame(ms(5_000 + length + 4));
        assert_eq!(fade.state(), CrossFadeState::default());
    }

    #[test]
    fn the_new_thumbnail_fades_in_over_300_ms_decelerating() {
        // ANIM-22
        plays_for(IMAGE_CROSSFADE);
    }

    #[test]
    fn a_new_theme_fades_in_over_250_ms_decelerating() {
        // ANIM-19
        plays_for(THEME_CROSSFADE);
    }

    #[test]
    fn it_follows_the_system_animation_speed() {
        // ANIM-05: half speed takes twice as long, and with animations removed the new
        // picture shows at once.
        let mut fade = CrossFade::new(IMAGE_CROSSFADE);
        fade.start(2.0);
        frames(&mut fade, 0, 592);
        assert!(fade.state().animating);
        fade.frame(ms(600));
        assert!(!fade.state().animating);

        fade.start(0.0);
        assert_eq!(fade.state(), CrossFadeState::default());
    }
}
