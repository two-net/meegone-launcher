//! The Events list's motion as notifications come and go (ANIM-20): a new group slides in at
//! the top and a dismissed one collapses, over 200 ms, decelerating.
//!
//! Which rows came and went is the Events view's to say: each row knows where it was
//! (`rows_kept`, ADR 0020). This is only how far the change has come, on the shared [`Timed`]
//! clock, so it plays when Events is next drawn, takes a late frame in small steps, and is
//! instant with animations removed (ANIM-05).

use std::time::Duration;

use meegone_core_state::motion::NOTIFICATIONS_CHANGE;

use crate::timed::Timed;

/// How the Events list's changing rows are drawn now: an immutable snapshot inside
/// `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventsChangeState {
    /// How far the change has come, from 0 to 1: a row that came in grows to its height and
    /// fades in as it slides down, and a row that went fades and collapses.
    pub progress: f64,
    /// The change is still playing, so the host should draw another frame.
    pub animating: bool,
}

impl Default for EventsChangeState {
    /// At rest: every row at full height, and the rows that went gone.
    fn default() -> Self {
        Self {
            progress: 1.0,
            animating: false,
        }
    }
}

/// Runs the Events list's change motion. `HomeShell` owns it and publishes its
/// [`EventsChangeState`].
#[derive(Debug, Default)]
pub(crate) struct EventsChange {
    timed: Timed,
}

impl EventsChange {
    /// Starts the motion for rows that just came or went. A change that comes while another
    /// plays starts again from the rows as they were last drawn.
    pub fn start(&mut self, animation_scale: f64) {
        self.timed.start(NOTIFICATIONS_CHANGE, animation_scale);
    }

    /// Advances the motion to the display's frame time (ANIM-04).
    pub fn frame(&mut self, at: Duration) {
        self.timed.frame(at);
    }

    /// Puts everything in place at once, as when animations are removed while it plays
    /// (ANIM-05).
    pub fn finish(&mut self) {
        self.timed.finish();
    }

    pub fn state(&self) -> EventsChangeState {
        match self.timed.eased() {
            None => EventsChangeState::default(),
            Some(progress) => EventsChangeState {
                progress,
                animating: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meegone_core_state::motion::MAX_FRAME_STEP;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// The display draws a frame every 8 ms from `from` up to `to`, in ms.
    fn frames(change: &mut EventsChange, from: u64, to: u64) {
        for at in (from..=to).step_by(8) {
            change.frame(ms(at));
        }
    }

    #[test]
    fn rows_slide_in_and_collapse_in_200_ms_decelerating() {
        // ANIM-20
        let mut change = EventsChange::default();
        assert_eq!(change.state(), EventsChangeState::default());

        change.start(1.0);
        assert_eq!(
            change.state(),
            EventsChangeState {
                progress: 0.0,
                animating: true
            }
        );

        // The clock starts at the first frame drawn after the change.
        change.frame(ms(3_000));
        assert_eq!(change.state().progress, 0.0);
        frames(&mut change, 3_000, 3_096);
        let half = change.state().progress;
        assert!(half > 0.5 && half < 1.0, "decelerating: {half}");

        frames(&mut change, 3_104, 3_200);
        assert_eq!(change.state(), EventsChangeState::default());
    }

    #[test]
    fn it_follows_the_system_animation_speed() {
        // ANIM-05
        let mut change = EventsChange::default();
        change.start(2.0);
        frames(&mut change, 0, 392);
        assert!(change.state().animating);
        change.frame(ms(400));
        assert!(!change.state().animating);

        change.start(0.0);
        assert_eq!(change.state(), EventsChangeState::default());
    }

    #[test]
    fn a_late_frame_advances_it_by_two_frames_at_most() {
        // ANIM-04: taking on new rows costs the host a frame or two.
        let mut change = EventsChange::default();
        change.start(1.0);
        change.frame(ms(0));
        change.frame(ms(90));

        let mut steady = EventsChange::default();
        steady.start(1.0);
        steady.frame(ms(0));
        steady.frame(MAX_FRAME_STEP);
        assert_eq!(change.state(), steady.state());
    }

    #[test]
    fn a_second_change_starts_it_again() {
        // ANIM-03
        let mut change = EventsChange::default();
        change.start(1.0);
        frames(&mut change, 0, 152);

        change.start(1.0);
        change.frame(ms(160));

        assert_eq!(change.state().progress, 0.0);
        assert!(change.state().animating);
    }
}
