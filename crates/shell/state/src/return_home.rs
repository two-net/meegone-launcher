//! The return-home motion (ANIM-09): when the user comes back from an app, the home content
//! scales from 94 % to 100 % and fades in over 250 ms, decelerating.
//!
//! The shell skips it when the system animates the return itself, so the two don't double up,
//! and when animations are removed (ANIM-05). Like the carousel, it is pure: the host passes
//! each frame's time (ADR 0006).

use std::time::Duration;

use meegone_core_state::motion::{RETURN_HOME, RETURN_HOME_FROM_SCALE, decelerate, scaled};

/// The home content as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnHomeState {
    /// The content's scale, from 0.94 to 1.
    pub scale: f64,
    /// The content's opacity, from 0 to 1.
    pub alpha: f64,
    /// The content is still coming in, so the host should draw another frame.
    pub animating: bool,
}

impl Default for ReturnHomeState {
    /// At rest: full size and opaque.
    fn default() -> Self {
        Self {
            scale: 1.0,
            alpha: 1.0,
            animating: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Motion {
    /// The whole motion, at the system animation speed.
    length: Duration,
    /// The first frame's time, or `None` until the first frame.
    start: Option<Duration>,
    /// How far the motion has come, eased, from 0 to 1.
    eased: f64,
}

/// Runs the return-home motion. `HomeShell` owns it and publishes its [`ReturnHomeState`].
#[derive(Debug, Default)]
pub(crate) struct ReturnHome {
    motion: Option<Motion>,
}

impl ReturnHome {
    /// Starts the motion from 94 % and transparent. Its clock starts at the next frame, so
    /// time spent before the home is drawn again doesn't count. With animations removed
    /// (`animation_scale` 0), the content shows at once (ANIM-05).
    pub fn start(&mut self, animation_scale: f64) {
        let length = scaled(RETURN_HOME, animation_scale);
        self.motion = (!length.is_zero()).then_some(Motion {
            length,
            start: None,
            eased: 0.0,
        });
    }

    /// Shows the content in full at once: a touch never waits for the motion (ANIM-03).
    pub fn finish(&mut self) {
        self.motion = None;
    }

    /// Advances the motion to the display's frame time (ANIM-04).
    pub fn frame(&mut self, at: Duration) {
        let Some(motion) = &mut self.motion else {
            return;
        };
        let start = *motion.start.get_or_insert(at);
        let progress = at.saturating_sub(start).as_secs_f64() / motion.length.as_secs_f64();
        if progress >= 1.0 {
            self.motion = None;
        } else {
            motion.eased = decelerate(progress);
        }
    }

    pub fn state(&self) -> ReturnHomeState {
        match self.motion {
            None => ReturnHomeState::default(),
            Some(Motion { eased, .. }) => ReturnHomeState {
                scale: RETURN_HOME_FROM_SCALE + (1.0 - RETURN_HOME_FROM_SCALE) * eased,
                alpha: eased,
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

    #[test]
    fn the_content_scales_from_94_percent_and_fades_in_over_250_ms() {
        // ANIM-09
        let mut motion = ReturnHome::default();
        motion.start(1.0);
        let first = motion.state();
        assert_eq!(
            (first.scale, first.alpha, first.animating),
            (0.94, 0.0, true)
        );

        // Time before the first frame doesn't count.
        motion.frame(ms(5_000));
        assert_eq!(motion.state(), first);

        motion.frame(ms(5_125));
        let half = motion.state();
        // Decelerating: more than half-way at half the time.
        assert!(half.alpha > 0.5, "{half:?}");
        assert!(half.scale > 0.97 && half.scale < 1.0, "{half:?}");

        motion.frame(ms(5_249));
        assert!(motion.state().animating);
        motion.frame(ms(5_250));
        assert_eq!(motion.state(), ReturnHomeState::default());
    }

    #[test]
    fn it_follows_the_system_animation_speed() {
        // ANIM-05
        let mut slow = ReturnHome::default();
        slow.start(2.0);
        slow.frame(ms(0));
        slow.frame(ms(400));
        assert!(slow.state().animating);
        slow.frame(ms(500));
        assert!(!slow.state().animating);

        let mut removed = ReturnHome::default();
        removed.start(0.0);
        assert_eq!(removed.state(), ReturnHomeState::default());
    }

    #[test]
    fn finishing_shows_the_content_at_once() {
        // ANIM-03
        let mut motion = ReturnHome::default();
        motion.start(1.0);
        motion.frame(ms(0));

        motion.finish();

        assert_eq!(motion.state(), ReturnHomeState::default());
    }
}
