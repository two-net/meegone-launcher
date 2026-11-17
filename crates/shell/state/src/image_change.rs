//! The image-of-the-day card's thumbnail cross-fading to a new image (ANIM-22): 300 ms,
//! decelerating.
//!
//! The host decodes a new thumbnail off its main thread, which takes tens of milliseconds. So
//! the host, not the cache, says when a new thumbnail is ready to fade in over the old one
//! (`ShellIntent::NewImageShown`, ADR 0022), and the fade is seen from its start. It runs on the
//! shared [`Timed`] clock, so it plays when Events is next drawn, takes a late frame in small
//! steps, and is instant with animations removed (ANIM-05).

use std::time::Duration;

use meegone_core_state::motion::IMAGE_CROSSFADE;

use crate::timed::Timed;

/// How the card's thumbnail is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageChangeState {
    /// How far the new thumbnail has faded in over the old one, from 0 to 1.
    pub progress: f64,
    /// The fade is still playing, so the host should draw another frame.
    pub animating: bool,
}

impl Default for ImageChangeState {
    /// At rest: only the newest thumbnail shows.
    fn default() -> Self {
        Self {
            progress: 1.0,
            animating: false,
        }
    }
}

/// Runs the thumbnail's cross-fade. `HomeShell` owns it and publishes its
/// [`ImageChangeState`].
#[derive(Debug, Default)]
pub(crate) struct ImageChange {
    timed: Timed,
}

impl ImageChange {
    /// Starts the fade to the thumbnail the host just decoded. One that comes while another
    /// fades starts again over the thumbnail as it was last drawn.
    pub fn start(&mut self, animation_scale: f64) {
        self.timed.start(IMAGE_CROSSFADE, animation_scale);
    }

    /// Advances the fade to the display's frame time (ANIM-04).
    pub fn frame(&mut self, at: Duration) {
        self.timed.frame(at);
    }

    pub fn state(&self) -> ImageChangeState {
        match self.timed.eased() {
            None => ImageChangeState::default(),
            Some(progress) => ImageChangeState {
                progress,
                animating: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// The display draws a frame every 8 ms from `from` up to `to`, in ms.
    fn frames(change: &mut ImageChange, from: u64, to: u64) {
        for at in (from..=to).step_by(8) {
            change.frame(ms(at));
        }
    }

    #[test]
    fn the_new_thumbnail_fades_in_over_300_ms_decelerating() {
        // ANIM-22
        let mut change = ImageChange::default();
        assert_eq!(change.state(), ImageChangeState::default());

        change.start(1.0);
        assert_eq!(
            change.state(),
            ImageChangeState {
                progress: 0.0,
                animating: true
            }
        );

        // The clock starts at the first frame drawn after the host showed the new thumbnail.
        change.frame(ms(5_000));
        assert_eq!(change.state().progress, 0.0);
        frames(&mut change, 5_000, 5_144);
        let half = change.state().progress;
        assert!(half > 0.5 && half < 1.0, "decelerating: {half}");

        frames(&mut change, 5_152, 5_296);
        assert!(change.state().animating);
        change.frame(ms(5_304));
        assert_eq!(change.state(), ImageChangeState::default());
    }

    #[test]
    fn it_follows_the_system_animation_speed() {
        // ANIM-05: half speed takes twice as long, and with animations removed the new
        // thumbnail shows at once.
        let mut change = ImageChange::default();
        change.start(2.0);
        frames(&mut change, 0, 592);
        assert!(change.state().animating);
        change.frame(ms(600));
        assert!(!change.state().animating);

        change.start(0.0);
        assert_eq!(change.state(), ImageChangeState::default());
    }
}
