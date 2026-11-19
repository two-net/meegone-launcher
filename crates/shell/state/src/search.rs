//! Pulling the search field down, and pushing it away (ANIM-16, SRCH-01, SRCH-04, ADR 0016).
//!
//! At the top of Applications, a finger pulling down reveals the search field's band in step
//! with it (ANIM-01). Past the whole band it stretches with resistance. Released with the whole
//! band showing, or flung down, the field springs open and search opens; otherwise it springs
//! back. While search is open with nothing typed, the same finger pushes the field back up:
//! pushed all the way, or flung up, search closes. Every settle is a spring that overshoots by
//! at most 3 % of the band (ANIM-02), and a touch catches it where it is (ANIM-03).
//!
//! Like the other motions it is pure, takes the time with each event, and with animations
//! removed it is instant (ANIM-05).

use std::time::Duration;

use meegone_core_state::motion::{
    MAX_OVERSHOOT, REST_DISTANCE_PX, REST_SPEED_PX, SEARCH_PULL_RESISTANCE, SEARCH_PULL_STRETCH,
    SETTLE, SpringMotion, VelocityTracker, motion_time,
};

use crate::carousel::{PointerEvent, PointerKind};

/// The search field as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SearchState {
    /// How much of the field's band shows, in px: 0 hidden, the band's height open, and more
    /// while a pull stretches past it (ANIM-16).
    pub reveal_px: f64,
    /// Search is open, or springing open: the field takes the keyboard (SRCH-01).
    pub open: bool,
    /// A finger is pulling or pushing the field, so the content below gets none of the gesture.
    pub pulling: bool,
    /// It is still moving, so the host should draw another frame.
    pub animating: bool,
}

/// What a vertical drag may do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pull {
    /// Nothing: the grid scrolls, or something else has the views.
    None,
    /// Pull search open: Applications is at the top of its grid with nothing open (SRCH-01).
    Open,
    /// Push open search away: nothing has been typed (SRCH-04).
    Away,
}

/// The sizes a pull is measured in, in px, as the host lays them out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Geometry {
    /// How far the finger moves before it drags rather than taps.
    pub slop_px: f64,
    /// How fast a release is a fling, in px per second.
    pub fling_px_per_s: f64,
}

/// A pull changed whether search is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Opened,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    Idle,
    /// A finger is down where a pull may start, and hasn't moved past the touch slop.
    Pressed {
        x: f64,
        y: f64,
    },
    /// The finger went sideways, or the wrong way: the rest of its gesture isn't a pull.
    Declined,
    /// A finger caught the moving field where it is, and hasn't moved past the touch slop. Up
    /// or down from here pulls it; a tap or a sideways move lets it carry on.
    Caught {
        x: f64,
        y: f64,
    },
    /// The finger moves the field: the band showed `base` px, unstretched, when it was at
    /// `origin_y`.
    Pulling {
        origin_y: f64,
        base: f64,
    },
    /// The field springs to its side: open, the whole band, or closed, none of it.
    Settling {
        motion: SpringMotion,
        last: Option<Duration>,
    },
}

/// Runs the pull and the field's motion. `HomeShell` owns it and publishes its [`SearchState`].
#[derive(Debug)]
pub(crate) struct SearchMotion {
    gesture: Gesture,
    tracker: VelocityTracker,
    /// How much of the band shows, in px.
    reveal: f64,
    /// The band's height, in px: how far the field shows when it is open.
    band: f64,
    /// The side it is on, or springing to.
    open: bool,
    animation_scale: f64,
}

impl Default for SearchMotion {
    fn default() -> Self {
        Self {
            gesture: Gesture::Idle,
            tracker: VelocityTracker::default(),
            reveal: 0.0,
            band: 0.0,
            open: false,
            animation_scale: 1.0,
        }
    }
}

impl SearchMotion {
    /// Follows one touch event. `pull` says what a vertical drag may do now. Returns the change
    /// a release made, for the shell to tell Applications.
    pub fn pointer(&mut self, event: PointerEvent, pull: Pull, size: Geometry) -> Option<Change> {
        match (event.kind, self.gesture) {
            (PointerKind::Down, gesture) => {
                self.tracker.clear();
                self.tracker.add(event.at, event.y);
                self.gesture = match (gesture, pull) {
                    // Nothing to pull: a moving field carries on.
                    (Gesture::Settling { .. }, Pull::None) => gesture,
                    (_, Pull::None) => Gesture::Idle,
                    // Touching the moving field catches it where it is (ANIM-03).
                    (Gesture::Settling { .. }, _) => Gesture::Caught {
                        x: event.x,
                        y: event.y,
                    },
                    _ => Gesture::Pressed {
                        x: event.x,
                        y: event.y,
                    },
                };
                None
            }
            (PointerKind::Move, Gesture::Pressed { x, y }) => {
                self.tracker.add(event.at, event.y);
                self.claim(event, x, y, pull, size.slop_px);
                None
            }
            (PointerKind::Move, Gesture::Caught { x, y }) => {
                self.tracker.add(event.at, event.y);
                // Caught, it moves either way.
                self.claim(event, x, y, Pull::Away, size.slop_px);
                if self.gesture == Gesture::Declined {
                    self.settle(Some(event.at), 0.0);
                }
                None
            }
            (PointerKind::Up | PointerKind::Cancel, Gesture::Caught { .. }) => {
                // A tap on the moving field: it carries on to its side.
                self.settle(Some(event.at), 0.0);
                None
            }
            (PointerKind::Move, Gesture::Pulling { origin_y, base }) => {
                self.tracker.add(event.at, event.y);
                self.reveal = self.stretched(base + event.y - origin_y);
                None
            }
            (PointerKind::Up, Gesture::Pulling { .. }) => {
                let velocity = self.tracker.velocity(event.at);
                self.release(event.at, velocity, size.fling_px_per_s)
            }
            (PointerKind::Cancel, Gesture::Pulling { .. }) => {
                // The system took the gesture: back to the side it was on.
                self.settle(Some(event.at), 0.0);
                None
            }
            (
                PointerKind::Up | PointerKind::Cancel,
                Gesture::Pressed { .. } | Gesture::Declined,
            ) => {
                self.gesture = Gesture::Idle;
                None
            }
            _ => None,
        }
    }

    /// Takes the gesture once the finger has moved past the touch slop, up or down rather than
    /// sideways, as the carousel would decline it, and in a direction `pull` allows.
    fn claim(&mut self, event: PointerEvent, x: f64, y: f64, pull: Pull, slop: f64) {
        let (dx, dy) = (event.x - x, event.y - y);
        if dx.abs() <= slop && dy.abs() <= slop {
            return;
        }
        let vertical = dy.abs() >= dx.abs();
        let allowed = match pull {
            Pull::Open => dy > 0.0,
            Pull::Away => true,
            Pull::None => false,
        };
        self.gesture = if vertical && allowed {
            // Measured from where the finger crossed the slop, so the field doesn't jump.
            let origin_y = y + slop.copysign(dy);
            let base = self.unstretched(self.reveal);
            self.reveal = self.stretched(base + event.y - origin_y);
            Gesture::Pulling { origin_y, base }
        } else {
            Gesture::Declined
        };
    }

    /// The finger lifted: open with the whole band showing or after a fling down, closed when
    /// pushed all the way away or after a fling up, and otherwise back to the side it was on.
    fn release(&mut self, at: Duration, velocity: f64, fling: f64) -> Option<Change> {
        let open = if velocity >= fling {
            true
        } else if velocity <= -fling {
            false
        } else if self.open {
            self.reveal > 0.0
        } else {
            self.reveal >= self.band
        };
        let change =
            (open != self.open).then_some(if open { Change::Opened } else { Change::Closed });
        self.open = open;
        self.settle(Some(at), velocity);
        change
    }

    /// Search is open or closed now, as Applications says, and its field's band is `band_px`
    /// tall: the field springs that way from where it is. A finger pulling it keeps it, unless
    /// search changed under it, such as after Back.
    pub fn set(&mut self, open: bool, band_px: f64, animation_scale: f64) {
        let band = band_px.max(0.0);
        let changed = open != self.open || band != self.band;
        self.band = band;
        self.animation_scale = animation_scale;
        if open == self.open && matches!(self.gesture, Gesture::Pulling { .. }) {
            return;
        }
        self.open = open;
        if changed {
            let velocity = match self.gesture {
                Gesture::Settling { motion, .. } => motion.velocity() / animation_scale.max(1e-9),
                _ => 0.0,
            };
            self.settle(None, velocity);
        }
    }

    /// Ends a pull in progress, as a new layout or Settings does: the field springs back to the
    /// side it was on.
    pub fn end_gesture(&mut self) {
        match self.gesture {
            Gesture::Pulling { .. } | Gesture::Caught { .. } => self.settle(None, 0.0),
            Gesture::Pressed { .. } | Gesture::Declined => self.gesture = Gesture::Idle,
            Gesture::Idle | Gesture::Settling { .. } => {}
        }
    }

    /// Puts a springing field where it is going at once, as when animations are removed while
    /// it moves (ANIM-05). A finger on it keeps it.
    pub fn finish(&mut self) {
        if matches!(self.gesture, Gesture::Settling { .. }) {
            self.rest();
        }
    }

    /// Springs to the side it is on from where it is, leaving at `velocity` px per second unless
    /// that would overshoot by more than 3 % of the band (ANIM-02). `at` is when the motion
    /// starts, or `None` for the next frame.
    fn settle(&mut self, at: Option<Duration>, velocity: f64) {
        let displacement = self.reveal - self.target();
        let scale = self.animation_scale;
        if scale == 0.0 || (displacement == 0.0 && velocity == 0.0) {
            // With animations removed, the field is there at once (ANIM-05).
            self.rest();
            return;
        }
        // Stretched past its rest, the field never carries on outward: it heads straight back.
        let velocity = if displacement * velocity > 0.0 {
            0.0
        } else {
            velocity
        };
        // Spring time runs slower by the animation scale; the release speed stays the finger's.
        let velocity = SETTLE.limit_velocity(
            displacement,
            velocity * scale,
            MAX_OVERSHOOT * self.band.max(1.0),
        );
        self.gesture = Gesture::Settling {
            motion: SpringMotion::new(SETTLE, displacement, velocity),
            last: at,
        };
    }

    /// Advances the spring to the display's frame time (ANIM-04).
    pub fn frame(&mut self, at: Duration) {
        let Gesture::Settling { mut motion, last } = self.gesture else {
            return;
        };
        let scale = self.animation_scale;
        if let Some(last) = last.filter(|_| scale > 0.0) {
            motion.advance(motion_time(at.saturating_sub(last), scale));
        }
        self.reveal = (self.target() + motion.displacement()).max(0.0);
        // Animations may have been removed while the field was moving.
        if scale == 0.0 || motion.is_at_rest(REST_DISTANCE_PX, REST_SPEED_PX * scale) {
            self.rest();
        } else {
            let last = Some(last.map_or(at, |last| last.max(at)));
            self.gesture = Gesture::Settling { motion, last };
        }
    }

    fn rest(&mut self) {
        self.reveal = self.target();
        self.gesture = Gesture::Idle;
    }

    /// Where the field rests on its side: the whole band open, none of it closed.
    fn target(&self) -> f64 {
        if self.open { self.band } else { 0.0 }
    }

    /// How much of the band shows for a pull of `pulled` px: all of it, one for one, then a
    /// third of what goes past it, up to half a band more (ANIM-16). Never less than none.
    fn stretched(&self, pulled: f64) -> f64 {
        let pulled = pulled.max(0.0);
        if pulled <= self.band {
            return pulled;
        }
        let past = (pulled - self.band) * SEARCH_PULL_RESISTANCE;
        self.band + past.min(self.band * SEARCH_PULL_STRETCH)
    }

    /// The pull that shows `reveal` px of the band: [`SearchMotion::stretched`] undone.
    fn unstretched(&self, reveal: f64) -> f64 {
        if reveal <= self.band {
            return reveal.max(0.0);
        }
        self.band + (reveal - self.band) / SEARCH_PULL_RESISTANCE
    }

    pub fn pulling(&self) -> bool {
        matches!(self.gesture, Gesture::Pulling { .. })
    }

    pub fn state(&self) -> SearchState {
        SearchState {
            reveal_px: self.reveal,
            open: self.open,
            pulling: self.pulling(),
            animating: matches!(self.gesture, Gesture::Settling { .. }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAND: f64 = 200.0;
    const SIZE: Geometry = Geometry {
        slop_px: 16.0,
        fling_px_per_s: 1000.0,
    };

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn at(kind: PointerKind, x: f64, y: f64, millis: u64) -> PointerEvent {
        PointerEvent {
            kind,
            x,
            y,
            at: ms(millis),
        }
    }

    fn closed() -> SearchMotion {
        let mut motion = SearchMotion::default();
        motion.set(false, BAND, 1.0);
        motion
    }

    fn opened() -> SearchMotion {
        let mut motion = SearchMotion::default();
        motion.set(true, BAND, 0.0);
        motion.set(true, BAND, 1.0);
        motion
    }

    /// A slow drag from `from` to `to` px down the screen, without lifting, from `start` ms on.
    fn drag(motion: &mut SearchMotion, pull: Pull, from: f64, to: f64, start: u64) -> u64 {
        motion.pointer(at(PointerKind::Down, 100.0, from, start), pull, SIZE);
        let steps = 40;
        let mut t = start;
        for step in 1..=steps {
            t += 16;
            let y = from + (to - from) * f64::from(step) / f64::from(steps);
            motion.pointer(at(PointerKind::Move, 100.0, y, t), pull, SIZE);
        }
        t
    }

    /// Lifts the finger after it has rested at `y`, so it carries no speed.
    fn lift(motion: &mut SearchMotion, pull: Pull, y: f64, t: u64) -> Option<Change> {
        motion.pointer(at(PointerKind::Move, 100.0, y, t + 50), pull, SIZE);
        motion.pointer(at(PointerKind::Up, 100.0, y, t + 100), pull, SIZE)
    }

    fn settle(motion: &mut SearchMotion, from: u64) -> u64 {
        let mut t = from;
        while motion.state().animating {
            t += 8;
            motion.frame(ms(t));
            assert!(t < from + 5_000, "never settled");
        }
        t
    }

    #[test]
    fn the_field_follows_the_finger_one_for_one_from_the_slop() {
        // ANIM-01, ANIM-16: in proportion to the pull, with no jump as it starts.
        let mut motion = closed();
        motion.pointer(at(PointerKind::Down, 100.0, 300.0, 0), Pull::Open, SIZE);
        motion.pointer(at(PointerKind::Move, 100.0, 310.0, 16), Pull::Open, SIZE);
        assert!(!motion.state().pulling);
        assert_eq!(motion.state().reveal_px, 0.0);

        motion.pointer(at(PointerKind::Move, 100.0, 320.0, 32), Pull::Open, SIZE);
        assert!(motion.state().pulling);
        assert_eq!(motion.state().reveal_px, 4.0);
        motion.pointer(at(PointerKind::Move, 100.0, 416.0, 48), Pull::Open, SIZE);
        assert_eq!(motion.state().reveal_px, 100.0);
    }

    #[test]
    fn past_the_band_the_field_stretches_with_resistance_up_to_half_a_band() {
        let mut motion = closed();
        drag(&mut motion, Pull::Open, 0.0, 16.0 + BAND + 60.0, 0);
        assert!((motion.state().reveal_px - (BAND + 20.0)).abs() < 1e-9);

        let mut far = closed();
        drag(&mut far, Pull::Open, 0.0, 16.0 + BAND + 3_000.0, 0);
        assert_eq!(far.state().reveal_px, BAND * 1.5);
    }

    #[test]
    fn released_with_the_whole_band_showing_it_springs_open() {
        // SRCH-01, ANIM-16, ANIM-02
        let mut motion = closed();
        let t = drag(&mut motion, Pull::Open, 0.0, 16.0 + BAND + 30.0, 0);

        assert_eq!(
            lift(&mut motion, Pull::Open, 16.0 + BAND + 30.0, t),
            Some(Change::Opened)
        );

        let state = motion.state();
        assert!(state.open && !state.pulling && state.animating);
        settle(&mut motion, t + 100);
        assert_eq!(motion.state().reveal_px, BAND);
        assert!(!motion.state().animating);
    }

    #[test]
    fn released_short_of_the_band_it_springs_back_shut() {
        let mut motion = closed();
        let t = drag(&mut motion, Pull::Open, 0.0, 16.0 + BAND - 10.0, 0);

        assert_eq!(lift(&mut motion, Pull::Open, 16.0 + BAND - 10.0, t), None);

        assert!(!motion.state().open);
        settle(&mut motion, t + 100);
        assert_eq!(motion.state().reveal_px, 0.0);
    }

    #[test]
    fn a_fling_down_opens_it_however_short() {
        let mut motion = closed();
        motion.pointer(at(PointerKind::Down, 100.0, 0.0, 0), Pull::Open, SIZE);
        for (step, y) in [20.0, 40.0, 60.0, 80.0].into_iter().enumerate() {
            motion.pointer(
                at(PointerKind::Move, 100.0, y, 10 * (step as u64 + 1)),
                Pull::Open,
                SIZE,
            );
        }

        let change = motion.pointer(at(PointerKind::Up, 100.0, 80.0, 41), Pull::Open, SIZE);

        assert_eq!(change, Some(Change::Opened));
    }

    /// A hard fling down, lifted after `steps` moves of 22 px, 5 ms apart. Returns the furthest
    /// the field went and where it was let go.
    fn fling_open(steps: u32) -> (f64, f64) {
        let mut motion = closed();
        motion.pointer(at(PointerKind::Down, 100.0, 0.0, 0), Pull::Open, SIZE);
        for step in 1..=steps {
            let y = f64::from(step) * 22.0;
            motion.pointer(
                at(PointerKind::Move, 100.0, y, u64::from(step) * 5),
                Pull::Open,
                SIZE,
            );
        }
        let end = u64::from(steps) * 5 + 1;
        let y = f64::from(steps) * 22.0;
        let let_go = motion.state().reveal_px;
        motion.pointer(at(PointerKind::Up, 100.0, y, end), Pull::Open, SIZE);
        assert!(motion.state().open);
        let mut furthest: f64 = 0.0;
        let mut t = end;
        while motion.state().animating {
            t += 4;
            motion.frame(ms(t));
            furthest = furthest.max(motion.state().reveal_px);
        }
        assert_eq!(motion.state().reveal_px, BAND);
        (furthest, let_go)
    }

    #[test]
    fn the_spring_overshoots_by_at_most_three_percent_of_the_band() {
        // ANIM-02: flung hard while short of the band…
        let (furthest, let_go) = fling_open(7);
        assert!(let_go < BAND);
        assert!(furthest <= BAND * 1.03 + 1e-6, "{furthest}");

        // …and past it, where it heads straight back rather than further out.
        let (furthest, let_go) = fling_open(12);
        assert!(let_go > BAND);
        assert!(furthest <= let_go + 1e-6, "{furthest} {let_go}");
    }

    #[test]
    fn up_sideways_or_without_an_offer_is_no_pull() {
        for (pull, to) in [
            (Pull::Open, (100.0, -100.0)),
            (Pull::Open, (300.0, 50.0)),
            (Pull::None, (100.0, 200.0)),
        ] {
            let mut motion = closed();
            motion.pointer(at(PointerKind::Down, 100.0, 0.0, 0), pull, SIZE);
            motion.pointer(at(PointerKind::Move, to.0, to.1, 16), pull, SIZE);
            // Once declined, the gesture stays declined.
            motion.pointer(at(PointerKind::Move, 100.0, 300.0, 32), pull, SIZE);

            assert!(!motion.state().pulling, "{pull:?} {to:?}");
            assert_eq!(motion.state().reveal_px, 0.0);
            assert_eq!(
                motion.pointer(at(PointerKind::Up, 100.0, 300.0, 48), pull, SIZE),
                None
            );
        }
    }

    #[test]
    fn pushed_all_the_way_away_search_closes() {
        // SRCH-04: "scrolling down past the field".
        let mut motion = opened();
        let t = drag(
            &mut motion,
            Pull::Away,
            600.0,
            600.0 - 16.0 - BAND - 50.0,
            0,
        );
        assert_eq!(motion.state().reveal_px, 0.0);

        assert_eq!(
            lift(&mut motion, Pull::Away, 334.0, t),
            Some(Change::Closed)
        );
        assert!(!motion.state().open);
    }

    #[test]
    fn pushed_part_of_the_way_it_springs_back_open() {
        let mut motion = opened();
        let t = drag(&mut motion, Pull::Away, 600.0, 600.0 - 16.0 - BAND / 2.0, 0);
        assert_eq!(motion.state().reveal_px, BAND / 2.0);

        assert_eq!(
            lift(&mut motion, Pull::Away, 600.0 - 16.0 - BAND / 2.0, t),
            None
        );

        settle(&mut motion, t + 100);
        assert_eq!(motion.state().reveal_px, BAND);
    }

    #[test]
    fn a_fling_up_closes_it_however_short() {
        let mut motion = opened();
        motion.pointer(at(PointerKind::Down, 100.0, 600.0, 0), Pull::Away, SIZE);
        for step in 1..=4u64 {
            let y = 600.0 - 20.0 * step as f64;
            motion.pointer(at(PointerKind::Move, 100.0, y, 10 * step), Pull::Away, SIZE);
        }

        let change = motion.pointer(at(PointerKind::Up, 100.0, 520.0, 41), Pull::Away, SIZE);

        assert_eq!(change, Some(Change::Closed));
    }

    #[test]
    fn a_touch_catches_the_moving_field_where_it_is() {
        // ANIM-03: here as it closes after Back, at the top of the grid.
        let mut motion = opened();
        motion.set(false, BAND, 1.0);
        for t in [8, 16, 24, 32, 40] {
            motion.frame(ms(t));
        }
        let caught = motion.state().reveal_px;
        assert!(caught > 0.0 && caught < BAND, "{caught}");

        motion.pointer(at(PointerKind::Down, 100.0, 500.0, 45), Pull::Open, SIZE);
        motion.frame(ms(60));

        assert!(!motion.state().animating && !motion.state().pulling);
        assert_eq!(motion.state().reveal_px, caught);
        // Up or down from where it was caught, measured from the touch slop.
        motion.pointer(at(PointerKind::Move, 100.0, 450.0, 66), Pull::Open, SIZE);
        assert!(motion.state().pulling);
        assert!((motion.state().reveal_px - (caught - 34.0)).abs() < 1e-9);
    }

    #[test]
    fn a_tap_or_a_sideways_move_on_the_moving_field_lets_it_carry_on() {
        // A tap on the field as it springs open never closes search.
        for end in [
            at(PointerKind::Up, 100.0, 500.0, 30),
            at(PointerKind::Move, 300.0, 500.0, 30),
        ] {
            let mut motion = closed();
            motion.set(true, BAND, 1.0);
            motion.pointer(at(PointerKind::Down, 100.0, 500.0, 10), Pull::Away, SIZE);

            assert_eq!(motion.pointer(end, Pull::Away, SIZE), None);
            motion.pointer(at(PointerKind::Up, 300.0, 500.0, 40), Pull::Away, SIZE);

            assert!(motion.state().open && !motion.state().pulling);
            settle(&mut motion, 40);
            assert_eq!(motion.state().reveal_px, BAND);
        }
    }

    #[test]
    fn a_touch_with_nothing_to_pull_leaves_the_field_moving() {
        let mut motion = opened();
        motion.set(false, BAND, 1.0);
        motion.frame(ms(8));

        motion.pointer(at(PointerKind::Down, 100.0, 500.0, 10), Pull::None, SIZE);

        assert!(motion.state().animating && !motion.state().pulling);
    }

    #[test]
    fn a_cancelled_pull_springs_back_to_where_it_was() {
        let mut motion = closed();
        let t = drag(&mut motion, Pull::Open, 0.0, 16.0 + BAND + 30.0, 0);

        let change = motion.pointer(at(PointerKind::Cancel, 0.0, 0.0, t + 16), Pull::Open, SIZE);

        assert_eq!(change, None);
        assert!(!motion.state().open && !motion.state().pulling);
        settle(&mut motion, t + 16);
        assert_eq!(motion.state().reveal_px, 0.0);
    }

    #[test]
    fn opening_and_closing_without_a_pull_springs_the_field() {
        // A screen reader's "Search apps" (A11Y-05), Back, Home or a launch.
        let mut motion = closed();

        motion.set(true, BAND, 1.0);
        assert!(motion.state().open && motion.state().animating);
        let t = settle(&mut motion, 0);
        assert_eq!(motion.state().reveal_px, BAND);

        motion.set(false, BAND, 1.0);
        settle(&mut motion, t);
        assert_eq!(motion.state().reveal_px, 0.0);
    }

    #[test]
    fn search_closed_under_a_pull_ends_it() {
        // Back while pushing the field away.
        let mut motion = opened();
        drag(&mut motion, Pull::Away, 600.0, 500.0, 0);
        assert!(motion.state().pulling);

        motion.set(false, BAND, 1.0);

        assert!(!motion.state().pulling && !motion.state().open);
    }

    #[test]
    fn a_new_band_moves_an_open_field_with_it() {
        let mut motion = opened();
        assert_eq!(motion.state().reveal_px, BAND);

        motion.set(true, 240.0, 0.0);

        assert_eq!(motion.state().reveal_px, 240.0);
    }

    #[test]
    fn with_animations_removed_the_field_is_there_at_once() {
        // ANIM-05
        let mut motion = SearchMotion::default();
        motion.set(false, BAND, 0.0);
        let t = drag(&mut motion, Pull::Open, 0.0, 16.0 + BAND + 30.0, 0);

        assert_eq!(
            lift(&mut motion, Pull::Open, 16.0 + BAND + 30.0, t),
            Some(Change::Opened)
        );

        assert_eq!(motion.state().reveal_px, BAND);
        assert!(!motion.state().animating);
        motion.set(false, BAND, 0.0);
        assert_eq!(motion.state().reveal_px, 0.0);
    }

    #[test]
    fn ending_the_gesture_lets_the_field_go() {
        let mut motion = closed();
        drag(&mut motion, Pull::Open, 0.0, 120.0, 0);

        motion.end_gesture();

        assert!(!motion.state().pulling);
        assert!(motion.state().animating);
    }
}
