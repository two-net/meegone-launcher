//! Pinching Recent's cards between more and bigger ones (REC-05, ANIM-17, ANIM-01, ADR 0024).
//!
//! Two fingers on Recent's cards scale them: spreading the fingers grows them towards the size
//! of bigger cards, and bringing them together shrinks them towards more cards. The scale
//! follows each event at once, so the cards are at most one frame behind (ANIM-01). Past either
//! density's size they stretch with resistance. Released past [`RECENT_PINCH_THRESHOLD`] of the
//! way, the cards switch density; either way they reflow from where they are drawn to their
//! places in about 300 ms (ANIM-17). Any change of density reflows the same way, such as one
//! made from a screen reader (A11Y-05).
//!
//! Once a pinch starts, the rest of the gesture is its own: the carousel and the content below
//! get none of it, until the next gesture starts. Like the other motions it is pure, takes the
//! time with each event, and with animations removed the reflow is instant (ANIM-05).

use std::time::Duration;

use meegone_core_state::motion::{
    RECENT_PINCH, RECENT_PINCH_RESISTANCE, RECENT_PINCH_STRETCH, RECENT_PINCH_THRESHOLD,
};

use crate::timed::Timed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinchKind {
    /// A second finger touched the home views.
    Start,
    /// Either finger moved.
    Move,
    /// One of the two fingers lifted.
    End,
    /// The system took the gesture away.
    Cancel,
}

/// Two fingers on the home views (ANIM-17).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinchEvent {
    pub kind: PinchKind,
    /// How far apart the two fingers are, in pixels.
    pub span_px: f64,
    /// The point halfway between them, from the home views' top left, in pixels.
    pub x: f64,
    pub y: f64,
    /// When it happened, on the host's monotonic clock.
    pub at: Duration,
}

/// Recent's cards as a pinch draws them now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinchState {
    /// The cards' scale while two fingers pinch them; 1 otherwise.
    pub scale: f64,
    /// Where the fingers first met, from the home views' top left, in pixels: the cards scale
    /// about it.
    pub pivot_x: f64,
    pub pivot_y: f64,
    /// A pinch took the current gesture: neither the carousel nor the content below gets any
    /// more of it.
    pub holding: bool,
    /// Changes each time the cards start to reflow from where they are drawn to their places.
    pub reflow: u32,
    /// How far the reflow has come, eased, from 0 to 1; 1 at rest.
    pub progress: f64,
    /// The reflow is still playing, so the host should draw another frame.
    pub animating: bool,
}

impl Default for PinchState {
    /// At rest: the cards at their size, in their places.
    fn default() -> Self {
        Self {
            scale: 1.0,
            pivot_x: 0.0,
            pivot_y: 0.0,
            holding: false,
            reflow: 0,
            progress: 1.0,
            animating: false,
        }
    }
}

/// How a pinch ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    /// Past the threshold: the other density.
    Switch,
    /// Short of it, or cancelled: the cards go back.
    Stay,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    Idle,
    /// Two fingers scale the cards. They started `start_span` px apart, and the cards are the
    /// other density's size at `target`, as a scale.
    Pinching {
        start_span: f64,
        target: f64,
    },
}

/// Runs the pinch and the cards' reflow. `HomeShell` owns it and publishes its [`PinchState`].
#[derive(Debug)]
pub(crate) struct PinchMotion {
    gesture: Gesture,
    holding: bool,
    scale: f64,
    pivot: (f64, f64),
    reflow: u32,
    timed: Timed,
}

impl Default for PinchMotion {
    fn default() -> Self {
        Self {
            gesture: Gesture::Idle,
            holding: false,
            scale: 1.0,
            pivot: (0.0, 0.0),
            reflow: 0,
            timed: Timed::default(),
        }
    }
}

impl PinchMotion {
    /// A finger touched the home views: a new gesture, which no pinch holds yet.
    pub fn new_gesture(&mut self, animation_scale: f64) {
        self.holding = false;
        if self.pinching() {
            self.end(animation_scale);
        }
    }

    /// Follows one pinch event. `target` is the scale at which the cards would be the other
    /// density's size, or `None` while no pinch may start. Returns how a pinch ended, for the
    /// shell to switch the density.
    pub fn pinch(
        &mut self,
        event: PinchEvent,
        target: Option<f64>,
        animation_scale: f64,
    ) -> Option<Ended> {
        match (event.kind, self.gesture) {
            (PinchKind::Start, Gesture::Idle) => {
                let target = target.filter(|t| t.is_finite() && *t > 0.0 && *t != 1.0)?;
                if !event.span_px.is_finite() {
                    return None;
                }
                // A touch never waits for the reflow (ANIM-03): the cards are in place at once.
                self.timed = Timed::default();
                self.holding = true;
                self.scale = 1.0;
                self.pivot = (event.x, event.y);
                self.gesture = Gesture::Pinching {
                    start_span: event.span_px.max(1.0),
                    target,
                };
                None
            }
            (PinchKind::Move, Gesture::Pinching { start_span, target }) => {
                if event.span_px.is_finite() {
                    self.scale = stretched(event.span_px.max(1.0) / start_span, target);
                }
                None
            }
            (PinchKind::End, Gesture::Pinching { target, .. }) => {
                let way = self.scale.ln() / target.ln();
                let ended = if way >= RECENT_PINCH_THRESHOLD {
                    Ended::Switch
                } else {
                    Ended::Stay
                };
                self.end(animation_scale);
                Some(ended)
            }
            (PinchKind::Cancel, Gesture::Pinching { .. }) => {
                self.end(animation_scale);
                Some(Ended::Stay)
            }
            _ => None,
        }
    }

    /// Ends a pinch in progress without switching, as a new layout or a menu does: the cards
    /// go back. The rest of the gesture stays the pinch's.
    pub fn end_gesture(&mut self, animation_scale: f64) {
        if self.pinching() {
            self.end(animation_scale);
        }
    }

    pub fn pinching(&self) -> bool {
        matches!(self.gesture, Gesture::Pinching { .. })
    }

    pub fn holding(&self) -> bool {
        self.holding
    }

    fn end(&mut self, animation_scale: f64) {
        self.gesture = Gesture::Idle;
        self.reflow(animation_scale);
    }

    /// The cards reflow from where they are drawn to their places: after a pinch, and whenever
    /// the density changes (ANIM-17).
    pub fn reflow(&mut self, animation_scale: f64) {
        self.scale = 1.0;
        self.reflow = self.reflow.wrapping_add(1);
        self.timed.start(RECENT_PINCH, animation_scale);
    }

    /// Advances the reflow to the display's frame time (ANIM-04).
    pub fn frame(&mut self, at: Duration) {
        self.timed.frame(at);
    }

    pub fn state(&self) -> PinchState {
        let progress = self.timed.eased();
        PinchState {
            scale: self.scale,
            pivot_x: self.pivot.0,
            pivot_y: self.pivot.1,
            holding: self.holding,
            reflow: self.reflow,
            progress: progress.unwrap_or(1.0),
            animating: progress.is_some(),
        }
    }
}

/// The cards' scale for fingers at `ratio` of their starting distance, when the other density
/// is at `target`. Between 1 and `target` it follows the fingers one for one; past either, a
/// third as much, and at most a quarter of the way between them further. Measured on a log
/// scale, so growing by half and shrinking by a third are the same distance.
fn stretched(ratio: f64, target: f64) -> f64 {
    let (pinched, way) = (ratio.ln(), target.ln());
    let (low, high) = if way > 0.0 { (0.0, way) } else { (way, 0.0) };
    let limit = way.abs() * RECENT_PINCH_STRETCH;
    let past = |beyond: f64| (beyond * RECENT_PINCH_RESISTANCE).min(limit);
    let logged = if pinched > high {
        high + past(pinched - high)
    } else if pinched < low {
        low - past(low - pinched)
    } else {
        pinched
    };
    logged.exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROW: f64 = 1.5;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn event(kind: PinchKind, span_px: f64, at: u64) -> PinchEvent {
        PinchEvent {
            kind,
            span_px,
            x: 540.0,
            y: 1_200.0,
            at: ms(at),
        }
    }

    /// Two fingers 300 px apart, moving to `to` px apart; not yet lifted.
    fn pinched(to: f64, target: f64) -> PinchMotion {
        let mut pinch = PinchMotion::default();
        pinch.pinch(event(PinchKind::Start, 300.0, 0), Some(target), 1.0);
        pinch.pinch(event(PinchKind::Move, to, 16), Some(target), 1.0);
        pinch
    }

    #[test]
    fn the_cards_follow_the_fingers_at_once() {
        // ANIM-01, ANIM-17
        let mut pinch = pinched(360.0, GROW);
        assert!((pinch.state().scale - 1.2).abs() < 1e-9);
        assert!(pinch.state().holding);
        assert_eq!(
            (pinch.state().pivot_x, pinch.state().pivot_y),
            (540.0, 1_200.0)
        );

        pinch.pinch(event(PinchKind::Move, 420.0, 24), None, 1.0);
        assert!((pinch.state().scale - 1.4).abs() < 1e-9);
        assert!(
            !pinch.state().animating,
            "nothing plays while the fingers move"
        );
    }

    #[test]
    fn past_either_size_the_cards_stretch_with_resistance() {
        // From three columns, bigger cards are half as big again; there is nothing smaller.
        let grown = pinched(900.0, GROW).state().scale;
        assert!(grown > GROW && grown <= GROW * GROW.powf(RECENT_PINCH_STRETCH) + 1e-9);
        let shrunk = pinched(30.0, GROW).state().scale;
        assert!(shrunk < 1.0 && shrunk >= GROW.powf(-RECENT_PINCH_STRETCH) - 1e-9);

        // From two, more cards are two thirds the size.
        let target = 2.0 / 3.0;
        assert!((pinched(240.0, target).state().scale - 0.8).abs() < 1e-9);
        let shrunk = pinched(30.0, target).state().scale;
        assert!(shrunk < target && shrunk >= target * target.powf(RECENT_PINCH_STRETCH) - 1e-9);
        let grown = pinched(900.0, target).state().scale;
        assert!(grown > 1.0 && grown <= target.powf(-RECENT_PINCH_STRETCH) + 1e-9);
    }

    #[test]
    fn released_past_the_threshold_the_density_switches() {
        // REC-05: a third of the way on a log scale from 1 to 1.5 is about 1.15.
        for (to, ended) in [(360.0, Ended::Switch), (330.0, Ended::Stay)] {
            let mut pinch = pinched(to, GROW);
            assert_eq!(
                pinch.pinch(event(PinchKind::End, to, 32), Some(GROW), 1.0),
                Some(ended),
                "{to}"
            );
        }
        // Back from two columns, by bringing the fingers together.
        let mut pinch = pinched(240.0, 2.0 / 3.0);
        assert_eq!(
            pinch.pinch(event(PinchKind::End, 240.0, 32), None, 1.0),
            Some(Ended::Switch)
        );
        // The wrong way never switches.
        let mut pinch = pinched(200.0, GROW);
        assert_eq!(
            pinch.pinch(event(PinchKind::End, 200.0, 32), None, 1.0),
            Some(Ended::Stay)
        );
    }

    #[test]
    fn a_release_reflows_the_cards_in_about_300_ms() {
        // ANIM-17
        let mut pinch = pinched(450.0, GROW);
        let before = pinch.state().reflow;
        pinch.pinch(event(PinchKind::End, 450.0, 32), None, 1.0);

        let state = pinch.state();
        assert_eq!(state.reflow, before.wrapping_add(1));
        assert_eq!(state.scale, 1.0);
        assert!(state.animating);
        assert!(
            state.holding,
            "the rest of the gesture is still the pinch's"
        );

        let mut at = 40;
        pinch.frame(ms(at));
        while pinch.state().animating {
            at += 8;
            pinch.frame(ms(at));
        }
        assert!((290..=320).contains(&(at - 40)), "{} ms", at - 40);
        assert_eq!(pinch.state().progress, 1.0);
    }

    #[test]
    fn with_animations_removed_the_cards_are_in_place_at_once() {
        // ANIM-05
        let mut pinch = pinched(450.0, GROW);
        pinch.pinch(event(PinchKind::End, 450.0, 32), None, 0.0);

        assert!(!pinch.state().animating);
        assert_eq!(pinch.state().progress, 1.0);
    }

    #[test]
    fn a_cancelled_or_interrupted_pinch_never_switches() {
        let mut pinch = pinched(600.0, GROW);
        assert_eq!(
            pinch.pinch(event(PinchKind::Cancel, 600.0, 32), None, 1.0),
            Some(Ended::Stay)
        );

        let mut pinch = pinched(600.0, GROW);
        pinch.end_gesture(1.0);
        assert!(!pinch.pinching());
        assert!(pinch.state().animating, "the cards go back");
        assert_eq!(
            pinch.pinch(event(PinchKind::End, 600.0, 32), None, 1.0),
            None,
            "the lifted finger does nothing more"
        );
    }

    #[test]
    fn no_pinch_starts_where_none_is_offered() {
        let mut pinch = PinchMotion::default();
        for target in [None, Some(1.0), Some(f64::NAN)] {
            pinch.pinch(event(PinchKind::Start, 300.0, 0), target, 1.0);
            assert!(!pinch.pinching());
            assert!(!pinch.holding());
        }
        assert_eq!(
            pinch.pinch(event(PinchKind::End, 600.0, 16), Some(GROW), 1.0),
            None
        );
        assert_eq!(pinch.state(), PinchState::default());
    }

    #[test]
    fn a_new_gesture_lets_go_and_a_pinch_catches_the_reflow() {
        // ANIM-03
        let mut pinch = pinched(450.0, GROW);
        pinch.pinch(event(PinchKind::End, 450.0, 32), None, 1.0);
        pinch.new_gesture(1.0);
        assert!(!pinch.holding());
        assert!(pinch.state().animating);

        pinch.pinch(event(PinchKind::Start, 300.0, 48), Some(GROW), 1.0);
        assert!(
            !pinch.state().animating,
            "the cards are in place to be pinched"
        );
        assert!(pinch.holding());
    }

    #[test]
    fn fingers_starting_together_never_divide_by_nothing() {
        let mut pinch = PinchMotion::default();
        pinch.pinch(event(PinchKind::Start, 0.0, 0), Some(GROW), 1.0);
        pinch.pinch(event(PinchKind::Move, 30.0, 8), None, 1.0);
        assert!(pinch.state().scale.is_finite());
        pinch.pinch(event(PinchKind::Move, f64::NAN, 16), None, 1.0);
        assert!(pinch.state().scale.is_finite());
    }
}
