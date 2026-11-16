//! The grid's motion when apps come and go (ANIM-21): a new icon scales and fades in, a removed
//! icon shrinks away, and the cells around them slide to their new places, over 200 ms,
//! decelerating.
//!
//! Which cells came, went or moved is the Applications view's to say: each cell knows where it
//! was (ADR 0012). This is only how far the change has come. Like the return-home motion, it is
//! pure, and its clock starts at the first frame after the change, so a change made while the
//! home is out of sight plays when it is drawn again. With animations removed, the grid takes on
//! the change at once (ANIM-05).
//!
//! Taking on new cells costs the host a frame or two: on a Galaxy S22, the frame after the first
//! one came 75 to 92 ms late. So a late frame advances the motion by at most
//! [`MAX_FRAME_STEP`], and the motion is seen from its start instead of popping in part-way.

use std::time::Duration;

use meegone_core_state::motion::{
    APP_INSTALL_REMOVE, APP_INSTALL_REMOVE_SCALE, decelerate, scaled,
};

/// How the grid's changing cells are drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridChangeState {
    /// The scale of a cell that just joined the grid, from 0.5 to 1.
    pub appear_scale: f64,
    /// Its opacity, from 0 to 1.
    pub appear_alpha: f64,
    /// The scale of a cell that left the grid, from 1 to 0.5.
    pub leave_scale: f64,
    /// Its opacity, from 1 to 0.
    pub leave_alpha: f64,
    /// How far each other cell has come from its old place to its new one, from 0 to 1.
    pub shift: f64,
    /// The change is still playing, so the host should draw another frame.
    pub animating: bool,
}

impl Default for GridChangeState {
    /// At rest: every cell in its place at full size, and the cells that left gone.
    fn default() -> Self {
        Self::at(1.0, false)
    }
}

impl GridChangeState {
    /// The change `eased` of the way through, from 0 to 1.
    fn at(eased: f64, animating: bool) -> Self {
        let shrink = 1.0 - APP_INSTALL_REMOVE_SCALE;
        Self {
            appear_scale: APP_INSTALL_REMOVE_SCALE + shrink * eased,
            appear_alpha: eased,
            leave_scale: 1.0 - shrink * eased,
            leave_alpha: 1.0 - eased,
            shift: eased,
            animating,
        }
    }
}

/// The most one frame advances the motion: two frames at 60 Hz.
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

/// Runs the grid's change motion. `HomeShell` owns it and publishes its [`GridChangeState`].
#[derive(Debug, Default)]
pub(crate) struct GridChange {
    motion: Option<Motion>,
}

impl GridChange {
    /// Starts the motion for cells that just came, went or moved. A change that comes while
    /// another plays starts again from the cells as they were last drawn.
    pub fn start(&mut self, animation_scale: f64) {
        let length = scaled(APP_INSTALL_REMOVE, animation_scale);
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

    pub fn state(&self) -> GridChangeState {
        match self.motion {
            None => GridChangeState::default(),
            Some(Motion { eased, .. }) => GridChangeState::at(eased, true),
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
    fn frames(change: &mut GridChange, from: u64, to: u64) {
        for at in (from..=to).step_by(8) {
            change.frame(ms(at));
        }
    }

    #[test]
    fn new_cells_grow_and_fade_in_while_removed_ones_shrink_away_in_200_ms() {
        // ANIM-21
        let mut change = GridChange::default();
        assert_eq!(change.state(), GridChangeState::default());

        change.start(1.0);
        let first = change.state();
        assert_eq!(
            (first.appear_scale, first.appear_alpha, first.animating),
            (0.5, 0.0, true)
        );
        assert_eq!(
            (first.leave_scale, first.leave_alpha, first.shift),
            (1.0, 1.0, 0.0)
        );

        // The clock starts at the first frame drawn after the change.
        change.frame(ms(5_000));
        assert_eq!(change.state(), first);
        frames(&mut change, 5_000, 5_096);
        let half = change.state();
        // Decelerating: more than half-way at half the time.
        assert!(half.shift > 0.5 && half.shift < 1.0, "{half:?}");
        assert!(
            half.appear_scale > 0.75 && half.leave_scale < 0.75,
            "{half:?}"
        );
        assert_eq!(half.appear_alpha + half.leave_alpha, 1.0);

        frames(&mut change, 5_104, 5_200);
        let rest = change.state();
        assert_eq!(rest, GridChangeState::default());
        assert_eq!(
            (
                rest.appear_scale,
                rest.appear_alpha,
                rest.leave_alpha,
                rest.shift
            ),
            (1.0, 1.0, 0.0, 1.0)
        );
        assert!(!rest.animating);
    }

    #[test]
    fn it_follows_the_system_animation_speed() {
        // ANIM-05
        let mut change = GridChange::default();
        change.start(2.0);
        frames(&mut change, 0, 392);
        assert!(change.state().animating);
        change.frame(ms(400));
        assert!(!change.state().animating);

        change.start(0.0);
        assert_eq!(change.state(), GridChangeState::default());
    }

    #[test]
    fn a_late_frame_doesnt_skip_the_start_of_the_motion() {
        // ANIM-21, ANIM-04: the host's frame after taking on new cells comes late.
        let mut change = GridChange::default();
        change.start(1.0);
        change.frame(ms(1_000));

        change.frame(ms(1_080));
        let after_the_hitch = change.state();
        let mut steady = GridChange::default();
        steady.start(1.0);
        steady.frame(ms(0));
        steady.frame(MAX_FRAME_STEP);
        assert_eq!(after_the_hitch, steady.state());

        // A frame time from before the last one moves nothing.
        change.frame(ms(1_070));
        assert_eq!(change.state(), after_the_hitch);

        let mut at = 1_080;
        while change.state().animating {
            at += 8;
            change.frame(ms(at));
        }
        assert_eq!(at, 1_080 + 168);
    }

    #[test]
    fn a_second_change_starts_the_motion_again() {
        // ANIM-03: nothing waits for the first change to finish.
        let mut change = GridChange::default();
        change.start(1.0);
        frames(&mut change, 0, 152);

        change.start(1.0);
        change.frame(ms(160));

        assert_eq!(change.state().shift, 0.0);
        assert!(change.state().animating);
    }
}
