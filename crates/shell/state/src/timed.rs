//! A motion that plays once for a set length, decelerating: the grid's change as apps come and
//! go (ANIM-21), and the Events list's as notifications do (ANIM-20).
//!
//! It is pure, and its clock starts at the first frame after it starts, so a change made while
//! the home is out of sight plays when it is drawn again. With animations removed, it is at rest
//! at once (ANIM-05).
//!
//! Taking on new content costs the host a frame or two: on a Galaxy S22, the frame after the
//! grid took on new cells came 75 to 92 ms late. So a late frame advances the motion by at most
//! [`MAX_FRAME_STEP`], and the motion is seen from its start instead of popping in part-way.

use std::time::Duration;

use meegone_core_state::motion::{decelerate, scaled};

/// The most one frame advances a motion: two frames at 60 Hz.
pub const MAX_FRAME_STEP: Duration = Duration::from_millis(34);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Motion {
    /// The whole motion, at the system animation speed.
    length: Duration,
    /// The last frame's time, or `None` until the first frame.
    last: Option<Duration>,
    /// How much of the motion has played.
    played: Duration,
    /// How far the motion has come, eased, from 0 to 1.
    eased: f64,
}

#[derive(Debug, Default)]
pub(crate) struct Timed {
    motion: Option<Motion>,
}

impl Timed {
    /// Starts the motion from its beginning, `length` long at normal speed. A motion that is
    /// playing starts again: nothing waits for it to finish (ANIM-03).
    pub fn start(&mut self, length: Duration, animation_scale: f64) {
        let length = scaled(length, animation_scale);
        self.motion = (!length.is_zero()).then_some(Motion {
            length,
            last: None,
            played: Duration::ZERO,
            eased: 0.0,
        });
    }

    /// Advances the motion to the display's frame time (ANIM-04), by at most
    /// [`MAX_FRAME_STEP`] since the last frame.
    pub fn frame(&mut self, at: Duration) {
        let Some(motion) = &mut self.motion else {
            return;
        };
        let last = *motion.last.get_or_insert(at);
        motion.played += at.saturating_sub(last).min(MAX_FRAME_STEP);
        motion.last = Some(at.max(last));
        let progress = motion.played.as_secs_f64() / motion.length.as_secs_f64();
        if progress >= 1.0 {
            self.motion = None;
        } else {
            motion.eased = decelerate(progress);
        }
    }

    /// How far the motion has come, eased, from 0 to 1, or `None` once it is at rest.
    pub fn eased(&self) -> Option<f64> {
        self.motion.map(|motion| motion.eased)
    }
}
