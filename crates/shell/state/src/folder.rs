//! A folder's panel opening and closing (ANIM-15): it grows from the folder's icon over a
//! dimmed grid in 250 ms, decelerating, and shrinks back into it when it closes. A change of mind
//! part-way turns it back from where it is; another folder opening starts from its own icon.
//!
//! Like the other motions it is pure, its clock starts at the first frame after the change, and
//! with animations removed it is instant (ANIM-05). The host keeps drawing the folder that is
//! closing until it has.

use std::time::Duration;

use meegone_apps_state::FolderId;
use meegone_core_state::motion::{FOLDER, decelerate, scaled};

use crate::timed::MAX_FRAME_STEP;

/// The open folder's panel as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FolderState {
    /// The folder the panel shows: open, or closing.
    pub folder: Option<FolderId>,
    /// It is open, rather than closing.
    pub open: bool,
    /// How far it has grown from the folder's icon, eased, from 0 to 1 (ANIM-15).
    pub progress: f64,
    /// It is still moving, so the host should draw another frame.
    pub animating: bool,
}

/// Runs the panel's motion. `HomeShell` owns it and publishes its [`FolderState`].
#[derive(Debug, Default)]
pub(crate) struct FolderMotion {
    folder: Option<FolderId>,
    open: bool,
    /// How far it has come, from zero, closed, to [`FOLDER`], open.
    position: Duration,
    /// The last frame's time, or `None` until the first frame after a change.
    last: Option<Duration>,
    animation_scale: f64,
}

impl FolderMotion {
    /// The folder open now, or `None`: the panel heads that way from where it is.
    pub fn set(&mut self, open: Option<FolderId>, animation_scale: f64) {
        let opening = open.is_some();
        if opening == self.open && (open.is_none() || open == self.folder) {
            return;
        }
        if open.is_some() && open != self.folder {
            // Another folder grows from its own icon.
            self.position = Duration::ZERO;
            self.folder = open;
        }
        self.open = opening;
        self.last = None;
        self.animation_scale = animation_scale;
        if scaled(FOLDER, animation_scale).is_zero() {
            self.position = if opening { FOLDER } else { Duration::ZERO };
        }
        self.forget_when_closed();
    }

    /// Advances to the display's frame time (ANIM-04), by at most [`MAX_FRAME_STEP`] since the
    /// last frame.
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
        self.position = if self.open {
            self.position.saturating_add(step).min(FOLDER)
        } else {
            self.position.saturating_sub(step)
        };
        self.forget_when_closed();
    }

    /// Whether the panel is all the way open, so a drag can start in it: its cells are where
    /// the host laid them out.
    pub fn fully_open(&self) -> bool {
        self.open && self.position >= FOLDER
    }

    fn moving(&self) -> bool {
        if self.open {
            self.position < FOLDER
        } else {
            !self.position.is_zero()
        }
    }

    fn forget_when_closed(&mut self) {
        if !self.open && self.position.is_zero() {
            self.folder = None;
        }
    }

    pub fn state(&self) -> FolderState {
        FolderState {
            folder: self.folder,
            open: self.open,
            progress: decelerate(self.position.as_secs_f64() / FOLDER.as_secs_f64()),
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

    fn frames(motion: &mut FolderMotion, from: u64, to: u64) {
        for at in (from..=to).step_by(8) {
            motion.frame(ms(at));
        }
    }

    #[test]
    fn a_folder_grows_open_in_250_ms_and_shrinks_closed_again() {
        // ANIM-15
        let mut motion = FolderMotion::default();
        motion.set(Some(FolderId(1)), 1.0);
        let state = motion.state();
        assert_eq!(
            (state.folder, state.open, state.progress),
            (Some(FolderId(1)), true, 0.0)
        );
        assert!(state.animating && !motion.fully_open());

        frames(&mut motion, 0, 120);
        let half = motion.state().progress;
        assert!(half > 0.5 && half < 1.0, "{half}");
        frames(&mut motion, 128, 256);
        assert!(motion.fully_open());
        assert!(!motion.state().animating);

        motion.set(None, 1.0);
        frames(&mut motion, 300, 400);
        let closing = motion.state();
        assert_eq!(closing.folder, Some(FolderId(1)));
        assert!(!closing.open && closing.progress < 1.0);
        frames(&mut motion, 408, 600);
        assert_eq!(motion.state(), FolderState::default());
    }

    #[test]
    fn a_change_of_mind_turns_back_and_another_folder_starts_from_its_icon() {
        let mut motion = FolderMotion::default();
        motion.set(Some(FolderId(1)), 1.0);
        frames(&mut motion, 0, 96);
        motion.set(None, 1.0);
        motion.set(Some(FolderId(1)), 1.0);
        assert!(motion.state().progress > 0.0);

        motion.set(Some(FolderId(2)), 1.0);
        assert_eq!(motion.state().progress, 0.0);
        assert_eq!(motion.state().folder, Some(FolderId(2)));
    }

    #[test]
    fn with_animations_removed_it_opens_and_closes_at_once() {
        // ANIM-05
        let mut motion = FolderMotion::default();
        motion.set(Some(FolderId(1)), 0.0);
        assert!(motion.fully_open());
        assert_eq!(motion.state().progress, 1.0);
        motion.set(None, 0.0);
        assert_eq!(motion.state(), FolderState::default());
    }
}
