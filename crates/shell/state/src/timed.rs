//! A tween that plays once: the grid's change as apps come and go (ANIM-21), the Events list's
//! as notifications do (ANIM-20), Recent's reflow (ANIM-17) and the cross-fades (ANIM-19,
//! ANIM-22), each with its spec from `meegone_core_state::motion`.
//!
//! It is pure, and its clock starts at the first frame after it starts, so a change made while
//! the home is out of sight plays when it is drawn again. With animations removed, it is at rest
//! at once (ANIM-05). A late frame advances it by at most
//! [`MAX_FRAME_STEP`](meegone_core_state::motion::MAX_FRAME_STEP), so it is seen from its start
//! instead of popping in part-way.

use std::time::Duration;

use meegone_core_state::motion::{Easing, MAX_FRAME_STEP, Tween};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Motion {
    /// The whole motion, at the system animation speed.
    length: Duration,
    easing: Easing,
    /// The last frame's time, or `None` until the first frame.
    last: Option<Duration>,
    /// How much of the motion has played.
    played: Duration,
    /// How far the motion has come, eased, from 0 to 1.
    eased: f64,
    /// A touch holds it where it is drawn (ANIM-03).
    frozen: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Timed {
    motion: Option<Motion>,
}

impl Timed {
    /// Starts `tween` from its beginning, at the system animation `speed`. A motion that is
    /// playing starts again: nothing waits for it to finish (ANIM-03).
    pub fn start(&mut self, tween: Tween, speed: f64) {
        let length = tween.length_at(speed);
        self.motion = (!length.is_zero()).then_some(Motion {
            length,
            easing: tween.easing,
            last: None,
            played: Duration::ZERO,
            eased: 0.0,
            frozen: false,
        });
    }

    /// Advances the motion to the display's frame time (ANIM-04), by at most
    /// [`MAX_FRAME_STEP`] since the last frame.
    pub fn frame(&mut self, at: Duration) {
        let Some(motion) = &mut self.motion else {
            return;
        };
        if motion.frozen {
            return;
        }
        let last = *motion.last.get_or_insert(at);
        motion.played += at.saturating_sub(last).min(MAX_FRAME_STEP);
        motion.last = Some(at.max(last));
        let progress = motion.played.as_secs_f64() / motion.length.as_secs_f64();
        if progress >= 1.0 {
            self.motion = None;
        } else {
            motion.eased = motion.easing.at(progress);
        }
    }

    /// Holds the motion where it is drawn, as a touch that catches it does (ANIM-03). It stays
    /// there until it starts again or finishes.
    pub fn freeze(&mut self) {
        if let Some(motion) = &mut self.motion {
            motion.frozen = true;
        }
    }

    /// Ends the motion at once, as when animations are removed while it plays (ANIM-05).
    pub fn finish(&mut self) {
        self.motion = None;
    }

    /// How far the motion has come, eased, from 0 to 1, or `None` once it is at rest.
    pub fn eased(&self) -> Option<f64> {
        self.motion.map(|motion| motion.eased)
    }

    /// Whether it is still moving, so the host should draw another frame: it plays, and no
    /// touch holds it.
    pub fn moving(&self) -> bool {
        self.motion.is_some_and(|motion| !motion.frozen)
    }
}
