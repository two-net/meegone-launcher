//! Entering and leaving edit mode, or Recent's close mode (ANIM-12): the cells or cards shrink to
//! 92 % over 150 ms, decelerating, and the ✕ badges pop in row by row, the first visible row at
//! once and the last at most 120 ms later, each over 150 ms. Leaving plays it backwards: the
//! badges go, last row first, and the cells grow back.
//!
//! The badges stagger by visible row, so the host needs one value per row rather than per cell
//! (ADR 0014). Recent's rows all count as visible: there are at most nine (ADR 0025). Like the other motions, it is pure, its clock starts at the first frame after the
//! change, and with animations removed it is instant (ANIM-05). A change of mind part-way
//! turns it back from where it is.

use std::time::Duration;

use meegone_core_state::motion::{MODE_CHANGE, MODE_SCALE, MODE_STAGGER, decelerate, pop, scaled};

use crate::timed::MAX_FRAME_STEP;

/// How many visible rows have a badge scale of their own. Rows further down take the last one.
pub const BADGE_ROWS: usize = 12;

/// The whole motion: the last row's badges start [`MODE_STAGGER`] after the first's.
const TOTAL: Duration = MODE_CHANGE.saturating_add(MODE_STAGGER);

/// Edit or close mode as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeState {
    /// The mode is on: badges can be tapped, icons and cards don't launch, and Done or Clear
    /// all shows.
    pub on: bool,
    /// The cells' or cards' scale: 1 out of the mode, 0.92 in it (ANIM-12).
    pub scale: f64,
    /// How far the cells have come into the mode, eased, from 0 to 1. Done and Clear all fade
    /// with it.
    pub progress: f64,
    /// The grid's first visible row, which `badges[0]` is for.
    pub first_row: usize,
    /// The ✕ scale of each visible row from `first_row`, from 0 to a little over 1 as it pops.
    pub badges: [f64; BADGE_ROWS],
    /// It is still moving, so the host should draw another frame.
    pub animating: bool,
}

impl Default for ModeState {
    /// Out of edit mode, at rest.
    fn default() -> Self {
        Mode::default().state(0, BADGE_ROWS)
    }
}

/// Runs edit mode's motion, or close mode's. `HomeShell` owns one of each and publishes their
/// [`ModeState`]s.
#[derive(Debug, Default)]
pub(crate) struct Mode {
    on: bool,
    /// Where the motion is, from zero, out of edit mode, to [`TOTAL`], in it.
    position: Duration,
    /// The last frame's time, or `None` until the first frame after a change.
    last: Option<Duration>,
    animation_scale: f64,
}

impl Mode {
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// The mode turned on or off: the motion heads that way from where it is.
    pub fn set(&mut self, on: bool, animation_scale: f64) {
        self.on = on;
        self.last = None;
        self.animation_scale = animation_scale;
        if scaled(MODE_CHANGE, animation_scale).is_zero() {
            self.position = if on { TOTAL } else { Duration::ZERO };
        }
    }

    /// Advances to the display's frame time (ANIM-04), by at most [`MAX_FRAME_STEP`] of
    /// motion since the last frame.
    pub fn frame(&mut self, at: Duration) {
        if !self.moving() {
            return;
        }
        let last = *self.last.get_or_insert(at);
        self.last = Some(at.max(last));
        let step = at
            .saturating_sub(last)
            .min(MAX_FRAME_STEP)
            .div_f64(self.animation_scale.max(f64::MIN_POSITIVE));
        self.position = if self.on {
            self.position.saturating_add(step).min(TOTAL)
        } else {
            self.position.saturating_sub(step)
        };
    }

    fn moving(&self) -> bool {
        if self.on {
            self.position < TOTAL
        } else {
            !self.position.is_zero()
        }
    }

    /// The motion now, for a grid whose first visible row is `first_row`, with `visible_rows`
    /// rows on screen.
    pub fn state(&self, first_row: usize, visible_rows: usize) -> ModeState {
        let cells = decelerate(self.position.as_secs_f64() / MODE_CHANGE.as_secs_f64());
        let rows = visible_rows.clamp(1, BADGE_ROWS);
        let badges = std::array::from_fn(|row| {
            let rank = row.min(rows - 1);
            let delay = if rows > 1 {
                MODE_STAGGER.mul_f64(rank as f64 / (rows - 1) as f64)
            } else {
                Duration::ZERO
            };
            let own = self.position.saturating_sub(delay);
            if own.is_zero() {
                0.0
            } else {
                pop(own.as_secs_f64() / MODE_CHANGE.as_secs_f64())
            }
        });
        ModeState {
            on: self.on,
            scale: MODE_SCALE * cells + (1.0 - cells),
            progress: cells,
            first_row,
            badges,
            animating: self.moving(),
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
    fn frames(mode: &mut Mode, from: u64, to: u64) {
        for at in (from..=to).step_by(8) {
            mode.frame(ms(at));
        }
    }

    #[test]
    fn out_of_edit_mode_nothing_shows_or_moves() {
        let state = ModeState::default();
        assert!(!state.on && !state.animating);
        assert_eq!(state.scale, 1.0);
        assert_eq!(state.progress, 0.0);
        assert_eq!(state.badges, [0.0; BADGE_ROWS]);
    }

    #[test]
    fn entering_shrinks_the_cells_in_150_ms_and_pops_the_badges_row_by_row() {
        // ANIM-12: 8 rows show, so the eighth starts 120 ms after the first.
        let mut mode = Mode::default();
        mode.set(true, 1.0);
        assert!(mode.state(0, 8).animating);
        // The clock starts at the first frame.
        mode.frame(ms(1_000));
        assert_eq!(mode.state(0, 8).scale, 1.0);

        frames(&mut mode, 1_008, 1_072);
        let state = mode.state(0, 8);
        assert!(state.scale < 1.0 && state.scale > MODE_SCALE);
        // Earlier rows are further along; the eighth hasn't started.
        assert!(state.badges[0] > state.badges[3] && state.badges[3] > 0.0);
        assert_eq!(state.badges[7], 0.0);
        assert_eq!(state.badges[11], state.badges[7]);

        frames(&mut mode, 1_080, 1_152);
        let state = mode.state(0, 8);
        assert_eq!(state.scale, MODE_SCALE);
        assert_eq!(state.progress, 1.0);
        assert!(state.badges[7] > 0.0);
        assert!(state.animating);

        frames(&mut mode, 1_158, 1_270);
        let state = mode.state(0, 8);
        assert!(!state.animating);
        assert_eq!(state.badges, [1.0; BADGE_ROWS]);
    }

    #[test]
    fn a_badge_pops_a_little_past_full_size() {
        let mut mode = Mode::default();
        mode.set(true, 1.0);
        let mut peak: f64 = 0.0;
        for at in (0..=300).step_by(4) {
            mode.frame(ms(at));
            peak = peak.max(mode.state(0, 1).badges[0]);
        }
        assert!((1.01..=1.03).contains(&peak), "{peak}");
    }

    #[test]
    fn leaving_plays_it_backwards() {
        // ANIM-12: the last row's badges go first, then the cells grow back.
        let mut mode = Mode::default();
        mode.set(true, 0.0);
        mode.set(false, 1.0);
        mode.frame(ms(0));
        frames(&mut mode, 8, 120);
        let state = mode.state(0, 8);
        assert!(!state.on && state.animating);
        assert!(state.badges[7] < 1.0 && state.badges[0] == 1.0);
        assert_eq!(state.scale, MODE_SCALE);

        frames(&mut mode, 128, 272);
        assert_eq!(mode.state(0, 8), ModeState::default());
    }

    #[test]
    fn a_change_of_mind_turns_back_from_where_it_is() {
        let mut mode = Mode::default();
        mode.set(true, 1.0);
        frames(&mut mode, 0, 64);
        let part_way = mode.state(0, 4).scale;
        mode.set(false, 1.0);
        mode.frame(ms(72));
        assert_eq!(mode.state(0, 4).scale, part_way);
        frames(&mut mode, 80, 104);
        assert!(mode.state(0, 4).scale > part_way);
    }

    #[test]
    fn with_animations_removed_it_changes_at_once() {
        // ANIM-05
        let mut mode = Mode::default();
        mode.set(true, 0.0);
        let state = mode.state(3, 8);
        assert!(state.on && !state.animating);
        assert_eq!(state.scale, MODE_SCALE);
        assert_eq!(state.first_row, 3);
        assert_eq!(state.badges, [1.0; BADGE_ROWS]);

        mode.set(false, 0.0);
        assert_eq!(mode.state(0, BADGE_ROWS), ModeState::default());
    }

    #[test]
    fn it_follows_the_system_animation_speed() {
        // ANIM-05: at half speed it takes twice as long.
        let mut mode = Mode::default();
        mode.set(true, 2.0);
        frames(&mut mode, 0, 296);
        assert!(mode.state(0, 1).progress < 1.0);
        frames(&mut mode, 304, 560);
        assert!(!mode.state(0, 1).animating);
    }
}
