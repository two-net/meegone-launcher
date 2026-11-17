//! Cards leaving Recent (ANIM-18, ADR 0025): a card removed with its ✕ shrinks and fades where
//! it was over 180 ms, as the others close the gap. Clear all collapses the cards row by row:
//! each row over the same 180 ms, the first at once and the last 220 ms later, so it is over
//! within 400 ms.
//!
//! The rows are those of the cards the host last drew, so the host needs one value per row
//! rather than per card, as edit mode's badges do (ADR 0014). Like the other motions it is pure,
//! its clock starts at the first frame after the change, and with animations removed it is
//! instant (ANIM-05). Cards that leave while others are still leaving start it again: nothing
//! waits (ANIM-03).

use std::time::Duration;

use meegone_core_state::motion::{CARD_REMOVE, CLEAR_ALL_STAGGER, decelerate, scaled};
use meegone_recent_state::MAX_ROWS;

use crate::timed::MAX_FRAME_STEP;

/// The cards leaving Recent as they are drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecentChangeState {
    /// How far the cards that left each row have gone, from 0, as drawn, to 1, gone. Rows are
    /// those of the cards the host last read; a row no card left from is at 1.
    pub leave: [f64; MAX_ROWS],
    /// It is still moving, so the host should draw another frame.
    pub animating: bool,
}

impl Default for RecentChangeState {
    /// Nothing leaving.
    fn default() -> Self {
        RecentChange::default().state()
    }
}

/// Runs the cards' leaving. `HomeShell` owns it and publishes its [`RecentChangeState`].
#[derive(Debug, Default)]
pub(crate) struct RecentChange {
    /// When each row starts to go after the motion starts, or `None` for a row nothing left.
    delays: [Option<Duration>; MAX_ROWS],
    /// How much of the motion has played, at normal speed, or `None` at rest.
    played: Option<Duration>,
    /// The last frame's time, or `None` until the first frame after a start.
    last: Option<Duration>,
    animation_scale: f64,
}

impl RecentChange {
    /// Cards left `rows`, each a row of the cards the host last read: they go, row by row from
    /// the top. Rows past [`MAX_ROWS`] go with the last.
    pub fn start(&mut self, rows: impl IntoIterator<Item = usize>, animation_scale: f64) {
        let mut leaving = [false; MAX_ROWS];
        for row in rows {
            leaving[row.min(MAX_ROWS - 1)] = true;
        }
        let count = leaving.iter().filter(|left| **left).count();
        if count == 0 {
            return;
        }
        let mut rank = 0;
        self.delays = std::array::from_fn(|row| {
            leaving[row].then(|| {
                let delay = if count > 1 {
                    CLEAR_ALL_STAGGER.mul_f64(rank as f64 / (count - 1) as f64)
                } else {
                    Duration::ZERO
                };
                rank += 1;
                delay
            })
        });
        self.animation_scale = animation_scale;
        self.last = None;
        self.played = (!scaled(CARD_REMOVE, animation_scale).is_zero()).then_some(Duration::ZERO);
    }

    /// The whole motion: until the last row leaving has gone.
    fn length(&self) -> Duration {
        let last = self.delays.iter().flatten().max().copied();
        last.unwrap_or_default().saturating_add(CARD_REMOVE)
    }

    /// Advances to the display's frame time (ANIM-04), by at most [`MAX_FRAME_STEP`] of motion
    /// since the last frame.
    pub fn frame(&mut self, at: Duration) {
        let Some(played) = self.played else {
            return;
        };
        let last = *self.last.get_or_insert(at);
        self.last = Some(at.max(last));
        let step = at
            .saturating_sub(last)
            .min(MAX_FRAME_STEP)
            .div_f64(self.animation_scale.max(f64::MIN_POSITIVE));
        let played = played.saturating_add(step);
        self.played = (played < self.length()).then_some(played);
    }

    pub fn state(&self) -> RecentChangeState {
        let leave = std::array::from_fn(|row| match (self.played, self.delays[row]) {
            (Some(played), Some(delay)) => {
                let own = played.saturating_sub(delay);
                decelerate(own.as_secs_f64() / CARD_REMOVE.as_secs_f64())
            }
            _ => 1.0,
        });
        RecentChangeState {
            leave,
            animating: self.played.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_state::motion::CLEAR_ALL_MAX;

    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// The display draws a frame every 8 ms from `from` up to `to`, in ms.
    fn frames(change: &mut RecentChange, from: u64, to: u64) {
        for at in (from..=to).step_by(8) {
            change.frame(ms(at));
        }
    }

    #[test]
    fn at_rest_nothing_leaves() {
        let state = RecentChangeState::default();
        assert!(!state.animating);
        assert_eq!(state.leave, [1.0; MAX_ROWS]);
    }

    #[test]
    fn a_removed_card_goes_in_180_ms() {
        // ANIM-18
        let mut change = RecentChange::default();
        change.start([2], 1.0);
        let state = change.state();
        assert!(state.animating);
        assert_eq!(state.leave[2], 0.0);
        assert_eq!(state.leave[0], 1.0, "no card left the first row");

        // The clock starts at the first frame.
        frames(&mut change, 1_000, 1_088);
        let state = change.state();
        assert!(state.leave[2] > 0.5 && state.leave[2] < 1.0);
        assert!(state.animating);

        frames(&mut change, 1_096, 1_184);
        assert_eq!(change.state(), RecentChangeState::default());
    }

    #[test]
    fn clear_all_collapses_row_by_row_within_400_ms() {
        // ANIM-18: six rows, the first at once and the last 220 ms later.
        let mut change = RecentChange::default();
        change.start(0..6, 1.0);
        frames(&mut change, 0, 104);
        let state = change.state();
        assert!(state.leave[0] > state.leave[2] && state.leave[2] > state.leave[4]);
        assert_eq!(state.leave[5], 0.0);

        let length = CLEAR_ALL_MAX.as_millis() as u64;
        frames(&mut change, 112, length - 16);
        let state = change.state();
        assert!(state.animating && state.leave[5] < 1.0);
        assert_eq!(state.leave[0], 1.0);

        frames(&mut change, length - 8, length);
        assert!(!change.state().animating);
    }

    #[test]
    fn rows_past_the_last_go_with_it() {
        let mut change = RecentChange::default();
        change.start([MAX_ROWS + 3], 1.0);
        assert_eq!(change.state().leave[MAX_ROWS - 1], 0.0);
    }

    #[test]
    fn with_animations_removed_cards_go_at_once() {
        // ANIM-05, AC-18
        let mut change = RecentChange::default();
        change.start(0..6, 0.0);
        assert_eq!(change.state(), RecentChangeState::default());
    }

    #[test]
    fn it_follows_the_system_animation_speed() {
        // ANIM-05: at half speed it takes twice as long.
        let mut change = RecentChange::default();
        change.start([0], 2.0);
        frames(&mut change, 0, 296);
        assert!(change.state().animating);
        frames(&mut change, 304, 400);
        assert!(!change.state().animating);
    }

    #[test]
    fn cards_that_leave_while_others_go_start_it_again() {
        // ANIM-03
        let mut change = RecentChange::default();
        change.start([0], 1.0);
        frames(&mut change, 0, 120);
        change.start([1], 1.0);
        frames(&mut change, 128, 200);
        let state = change.state();
        assert!(state.animating);
        assert_eq!(state.leave[0], 1.0);
        assert!(state.leave[1] < 1.0);

        change.start([], 1.0);
        assert!(change.state().animating, "nothing leaving changes nothing");
    }
}
