//! The picked-up icon (APPS-11, ADR 0013). A long-press on an app's icon opens its menu and keeps
//! the rest of the gesture. If the finger then moves past the touch slop, the icon is picked up:
//! edit mode starts and the icon lifts and follows the finger. When the finger lifts, the icon
//! is back in its own cell.
//!
//! Like the icon press, it is pure: the host passes each touch (ADR 0006). Dropping the icon in
//! a new place, and the lift's motion (ANIM-13), come with edit mode's drag (#13).

use meegone_core_state::AppId;
use meegone_core_state::motion::DRAG_LIFT_SCALE;

use crate::carousel::{PointerEvent, PointerKind};

/// The lifted icon as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiftState {
    /// The app whose icon is lifted, or `None` while no icon is.
    pub app: Option<AppId>,
    /// How far the finger has moved since it went down, in pixels. The icon is drawn this far
    /// from its cell.
    pub dx: f64,
    pub dy: f64,
    /// The icon's scale: 1.1 while it is lifted (ANIM-13), and 1 otherwise.
    pub scale: f64,
}

impl Default for LiftState {
    /// No icon lifted.
    fn default() -> Self {
        Self {
            app: None,
            dx: 0.0,
            dy: 0.0,
            scale: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
enum Phase {
    #[default]
    Idle,
    /// A long-press on `app`'s icon opened its menu. The finger went down at `origin` and is
    /// still down.
    Armed { app: AppId, origin: (f64, f64) },
    /// The icon follows the finger, which went down at `origin` and is at `finger` now.
    Lifted {
        app: AppId,
        origin: (f64, f64),
        finger: (f64, f64),
    },
}

/// Picks an icon up after a long-press. `HomeShell` owns it and publishes its [`LiftState`].
#[derive(Debug, Default)]
pub(crate) struct Lift {
    phase: Phase,
}

impl Lift {
    /// A long-press that went down at `origin` opened `app`'s menu, and the finger is still down.
    pub fn arm(&mut self, app: AppId, origin: (f64, f64)) {
        self.phase = Phase::Armed { app, origin };
    }

    /// Follows the finger that made the long-press. Returns true when this event picks the icon
    /// up: the finger has moved past `slop` from where it went down, either way. Lifting the
    /// finger, or the system cancelling the gesture, puts the icon back in its cell.
    pub fn pointer(&mut self, event: &PointerEvent, slop: f64) -> bool {
        let finger = (event.x, event.y);
        match (event.kind, &mut self.phase) {
            (PointerKind::Move, Phase::Armed { app, origin }) => {
                let (dx, dy) = (finger.0 - origin.0, finger.1 - origin.1);
                if dx.abs() <= slop && dy.abs() <= slop {
                    return false;
                }
                self.phase = Phase::Lifted {
                    app: app.clone(),
                    origin: *origin,
                    finger,
                };
                true
            }
            (PointerKind::Move, Phase::Lifted { finger: at, .. }) => {
                *at = finger;
                false
            }
            (PointerKind::Move, Phase::Idle) => false,
            (PointerKind::Down | PointerKind::Up | PointerKind::Cancel, _) => {
                self.phase = Phase::Idle;
                false
            }
        }
    }

    /// The app whose menu a long-press opened, while its finger is still down and the icon
    /// isn't lifted yet.
    pub fn armed(&self) -> Option<&AppId> {
        match &self.phase {
            Phase::Armed { app, .. } => Some(app),
            Phase::Idle | Phase::Lifted { .. } => None,
        }
    }

    /// Whether an icon is lifted.
    pub fn is_lifted(&self) -> bool {
        matches!(self.phase, Phase::Lifted { .. })
    }

    /// Puts the icon back in its cell at once, or forgets the long-press, such as when the
    /// layout changes or the menu closes before a drag.
    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
    }

    pub fn state(&self) -> LiftState {
        match &self.phase {
            Phase::Idle | Phase::Armed { .. } => LiftState::default(),
            Phase::Lifted {
                app,
                origin,
                finger,
            } => LiftState {
                app: Some(app.clone()),
                dx: finger.0 - origin.0,
                dy: finger.1 - origin.1,
                scale: DRAG_LIFT_SCALE,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use meegone_core_state::ProfileId;

    use super::*;

    fn mail() -> AppId {
        AppId {
            profile: ProfileId(0),
            package: "mail".into(),
            activity: "mail.Main".into(),
        }
    }

    fn event(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
        PointerEvent {
            kind,
            x,
            y,
            at: Duration::ZERO,
        }
    }

    const SLOP: f64 = 16.0;

    #[test]
    fn moving_past_the_slop_picks_the_icon_up_and_it_follows_the_finger() {
        // APPS-11, ANIM-13
        let mut lift = Lift::default();
        lift.arm(mail(), (100.0, 200.0));
        assert_eq!(lift.armed(), Some(&mail()));
        assert_eq!(lift.state(), LiftState::default());

        // Within the slop, either way, it stays a long-press.
        assert!(!lift.pointer(&event(PointerKind::Move, 116.0, 184.0), SLOP));
        assert!(!lift.is_lifted());

        assert!(lift.pointer(&event(PointerKind::Move, 100.0, 217.0), SLOP));
        assert!(lift.is_lifted());
        assert_eq!(lift.armed(), None);
        assert_eq!(
            lift.state(),
            LiftState {
                app: Some(mail()),
                dx: 0.0,
                dy: 17.0,
                scale: DRAG_LIFT_SCALE,
            }
        );

        // Picked up once: later moves only follow.
        assert!(!lift.pointer(&event(PointerKind::Move, 40.0, 500.0), SLOP));
        let state = lift.state();
        assert_eq!((state.dx, state.dy), (-60.0, 300.0));
    }

    #[test]
    fn lifting_the_finger_or_a_cancel_puts_the_icon_back() {
        for end in [PointerKind::Up, PointerKind::Cancel] {
            let mut lift = Lift::default();
            lift.arm(mail(), (100.0, 200.0));
            lift.pointer(&event(PointerKind::Move, 300.0, 200.0), SLOP);

            lift.pointer(&event(end, 300.0, 200.0), SLOP);

            assert_eq!(lift.state(), LiftState::default());
            assert!(!lift.is_lifted());
            // A later move picks nothing up.
            assert!(!lift.pointer(&event(PointerKind::Move, 0.0, 0.0), SLOP));
        }
    }

    #[test]
    fn a_long_press_that_ends_without_a_drag_picks_nothing_up() {
        let mut lift = Lift::default();
        lift.arm(mail(), (100.0, 200.0));

        lift.pointer(&event(PointerKind::Up, 100.0, 200.0), SLOP);

        assert_eq!(lift.armed(), None);
        assert!(!lift.pointer(&event(PointerKind::Move, 300.0, 200.0), SLOP));
    }

    #[test]
    fn cancelling_puts_it_back_at_once() {
        let mut lift = Lift::default();
        lift.arm(mail(), (0.0, 0.0));
        lift.pointer(&event(PointerKind::Move, 50.0, 0.0), SLOP);

        lift.cancel();

        assert_eq!(lift.state(), LiftState::default());
    }
}
