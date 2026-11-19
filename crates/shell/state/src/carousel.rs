//! The carousel of home views (HOME-02 to HOME-04, HOME-09, HOME-13): one horizontal drag moves
//! between them, the row follows the finger and wraps around, a spring settles it, and a view
//! indicator shows while it moves (ANIM-01 to ANIM-03, ANIM-07, ANIM-11).
//!
//! It is pure. The host passes every pointer event and frame with its time on the host's
//! monotonic clock (ADR 0006), so tests drive it with explicit times. Positions are logical:
//! a right-to-left layout mirrors the input and the drawing (L10N-02).

use std::time::Duration;

use meegone_core_state::motion::{
    CATCH_SLOP_FRACTION, Easing, INDICATOR_HOLD, INDICATOR_IN, INDICATOR_OUT, MAX_OVERSHOOT,
    REST_DISTANCE_PX, REST_SPEED_PX, SETTLE, SpringMotion, Tween, VIEW_EDGE_RESISTANCE,
    VIEW_EDGE_STRETCH, VIEW_FLING_VELOCITY_DP, VIEW_SWIPE_THRESHOLD, VelocityTracker, motion_time,
    speed,
};

use crate::{Direction, HomeView};

/// The size and direction of the area the home views fill, as the host lays it out.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Layout {
    pub width_px: f64,
    /// Pixels per dp.
    pub density: f64,
    /// How far a finger moves before it drags rather than taps, in pixels: the platform's own
    /// distance.
    pub touch_slop_px: f64,
    /// A right-to-left layout mirrors the carousel order and swipe directions (L10N-02).
    pub rtl: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    /// The first finger touched the home views.
    Down,
    Move,
    /// The finger lifted.
    Up,
    /// The system took the gesture away, or a child element claimed it.
    Cancel,
}

/// One touch event on the home views. Only the first finger is passed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    pub kind: PointerKind,
    /// From the left edge of the home views, in pixels.
    pub x: f64,
    /// From the top edge of the home views, in pixels.
    pub y: f64,
    /// When it happened, on the host's monotonic clock.
    pub at: Duration,
}

/// The carousel as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarouselState {
    /// How far the row has moved from the current view towards the next one, in view widths:
    /// 0 at rest, negative towards the previous view.
    pub offset: f64,
    pub layout: Layout,
    /// Whether moving past either end wraps around (HOME-04).
    pub wrap: bool,
    /// The view indicator's opacity, from 0 to 1 (HOME-09).
    pub indicator: f64,
    /// A finger is moving the row, so the content below gets no touches.
    pub dragging: bool,
    /// A long-press took the finger that is down (HOME-10): neither the row nor the content
    /// below gets the rest of its gesture, so lifting it never taps anything.
    pub held: bool,
    /// Something is still moving or fading, so the host should draw another frame.
    pub animating: bool,
}

impl Default for CarouselState {
    fn default() -> Self {
        Self {
            offset: 0.0,
            layout: Layout::default(),
            // As in the default settings.
            wrap: true,
            indicator: 0.0,
            dragging: false,
            held: false,
            animating: false,
        }
    }
}

impl CarouselState {
    /// How far `view` is drawn from where it fills the screen, in pixels, while `current` is
    /// the current view. With wrap-around on, the views sit on a ring, each within one and a half
    /// widths, so moving past either end is seamless (HOME-04).
    pub fn page_offset_px(&self, current: HomeView, view: HomeView) -> f64 {
        let mut position = steps(current, view, self.wrap) - self.offset;
        if self.wrap {
            position = around_the_ring(position);
        }
        let logical = position * self.layout.width_px;
        if self.layout.rtl { -logical } else { logical }
    }

    /// The view nearest to the row's position, which the indicator highlights.
    pub fn nearest(&self, current: HomeView) -> HomeView {
        let direction = if self.offset > 0.5 {
            Direction::Next
        } else if self.offset < -0.5 {
            Direction::Previous
        } else {
            return current;
        };
        current.neighbour(direction, self.wrap).unwrap_or(current)
    }
}

/// The same place on a ring of three views, within one and a half views of the current one.
fn around_the_ring(position: f64) -> f64 {
    position - 3.0 * ((position + 1.5) / 3.0).floor()
}

/// How many views `to` is from `from`, towards the next view: ±1 for a neighbour, and ±2 between
/// Events and Recent when wrap-around is off.
fn steps(from: HomeView, to: HomeView, wrap: bool) -> f64 {
    if from == to {
        0.0
    } else if from.neighbour(Direction::Next, wrap) == Some(to) {
        1.0
    } else if from.neighbour(Direction::Previous, wrap) == Some(to) {
        -1.0
    } else {
        to.index() as f64 - from.index() as f64
    }
}

/// A drag ended: the view to settle on, and how fast the row was moving towards the next view,
/// in widths per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Release {
    pub target: HomeView,
    pub velocity: f64,
    pub at: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum Gesture {
    #[default]
    Idle,
    /// A finger is down but hasn't moved past the touch slop.
    Pressed { x: f64, y: f64 },
    /// The finger moved vertically first, so the content below keeps the gesture until it
    /// lifts.
    Declined,
    /// A long-press took the gesture (HOME-10): nothing gets the rest of it until it lifts.
    Held,
    /// The finger moves the row. At `origin_x` the row was at `start`; `travel` is how far the
    /// finger has moved since, in widths. Both are before the stretch past an end.
    Dragging {
        origin_x: f64,
        start: f64,
        travel: f64,
    },
    /// The row settles with a spring. `last` is when it was last advanced, or `None` until the
    /// next frame.
    Settling {
        motion: SpringMotion,
        last: Option<Duration>,
    },
}

/// A fade of the indicator's opacity, on [`INDICATOR_IN`] or [`INDICATOR_OUT`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fade {
    from: f64,
    to: f64,
    start: Duration,
    /// How long it lasts, at the system animation speed.
    length: Duration,
    easing: Easing,
}

impl Fade {
    fn opacity_at(&self, at: Duration) -> f64 {
        let progress = if self.length.is_zero() {
            1.0
        } else {
            at.saturating_sub(self.start).as_secs_f64() / self.length.as_secs_f64()
        };
        self.from + (self.to - self.from) * self.easing.at(progress)
    }

    fn is_done(&self, at: Duration) -> bool {
        at >= self.start + self.length
    }
}

/// The view indicator: it fades in when a drag starts and out a while after the views settle
/// (HOME-09, ANIM-11). How long it stays is a wait, not motion, so it never scales with the
/// system animation speed: with animations removed it still shows for [`INDICATOR_HOLD`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Indicator {
    opacity: f64,
    fade: Option<Fade>,
    /// When it starts fading out.
    hide_at: Option<Duration>,
    /// The system animation speed its fades take.
    speed: f64,
}

impl Indicator {
    fn fade_to(&mut self, to: f64, start: Duration, tween: Tween) {
        self.advance(start);
        self.fade = Some(Fade {
            from: self.opacity,
            to,
            start,
            length: tween.length_at(self.speed),
            easing: tween.easing,
        });
        self.advance(start);
    }

    fn advance(&mut self, at: Duration) {
        if let Some(hide_at) = self.hide_at.filter(|hide_at| at >= *hide_at) {
            self.hide_at = None;
            self.fade_to(0.0, hide_at, INDICATOR_OUT);
        }
        if let Some(fade) = self.fade {
            self.opacity = fade.opacity_at(at);
            if fade.is_done(at) {
                self.fade = None;
            }
        }
    }

    fn is_animating(&self) -> bool {
        self.fade.is_some() || self.hide_at.is_some()
    }

    /// Ends a fade at once, where it was going. The wait before it fades out stays.
    fn finish(&mut self) {
        if let Some(fade) = self.fade.take() {
            self.opacity = fade.to;
        }
    }
}

/// Moves the row of home views with the finger and settles it. `HomeShell` owns it and
/// publishes its [`CarouselState`].
#[derive(Debug)]
pub(crate) struct Carousel {
    state: CarouselState,
    gesture: Gesture,
    tracker: VelocityTracker,
    indicator: Indicator,
    /// The system animation speed: 0 means animations are removed (ANIM-05).
    animation_scale: f64,
    /// The latest time the host reported.
    now: Duration,
    /// Something over the views, such as an open folder, takes touches itself: a finger still
    /// counts as a press, so a long-press can count, but it never drags the row (FOLD-04).
    locked: bool,
}

impl Carousel {
    pub fn new(wrap: bool, animation_scale: f64) -> Self {
        let mut carousel = Self {
            state: CarouselState::default(),
            gesture: Gesture::Idle,
            tracker: VelocityTracker::default(),
            indicator: Indicator::default(),
            animation_scale: 1.0,
            now: Duration::ZERO,
            locked: false,
        };
        carousel.configure(wrap, animation_scale);
        carousel
    }

    pub fn state(&self) -> CarouselState {
        self.state
    }

    /// Takes on the wrap-around setting and the system animation speed, cleaned up by
    /// [`speed`].
    pub fn configure(&mut self, wrap: bool, animation_scale: f64) {
        self.state.wrap = wrap;
        self.animation_scale = speed(animation_scale);
        self.indicator.speed = self.animation_scale;
    }

    /// Ends the row's motion and the indicator's fade at once, as when animations are removed
    /// while they move (ANIM-05).
    pub fn finish(&mut self) {
        if matches!(self.gesture, Gesture::Settling { .. }) {
            self.rest(self.now);
        }
        self.indicator.finish();
        self.publish();
    }

    /// A finger touched the home views at `at`. A settling row closer to rest than a touch
    /// catches (see [`CATCH_SLOP_FRACTION`]) lands at once, so the touch goes to what is under
    /// it, not to the row (ANIM-03). Called before anything else decides what the touch does.
    pub fn touch_down(&mut self, at: Duration) {
        if matches!(self.gesture, Gesture::Settling { .. }) && !self.catchable() {
            self.rest(at);
            self.publish();
        }
    }

    /// The system animation speed, as the carousel takes it: 0 means animations are removed
    /// (ANIM-05).
    pub fn animation_scale(&self) -> f64 {
        self.animation_scale
    }

    /// Puts the row at rest on the current view at once, with the indicator hidden. Used when
    /// the view changes while the home is out of sight, so nothing is seen moving (HOME-06,
    /// REL-05).
    pub fn reset(&mut self) {
        self.state.offset = 0.0;
        self.gesture = Gesture::Idle;
        self.tracker.clear();
        self.indicator = Indicator {
            speed: self.indicator.speed,
            ..Indicator::default()
        };
        self.publish();
    }

    /// The host laid the home views out again. A drag in progress ends, because its finger
    /// positions no longer fit.
    pub fn layout(&mut self, layout: Layout, current: HomeView) -> Option<Release> {
        if layout == self.state.layout {
            return None;
        }
        let release = self.end_gesture(current);
        self.state.layout = layout;
        self.publish();
        release
    }

    /// Ends the gesture in progress, if any, as if the system had cancelled it: something now
    /// covers the home views, or their layout changed. A gesture a long-press took stays taken
    /// until the finger lifts.
    pub fn end_gesture(&mut self, current: HomeView) -> Option<Release> {
        let release = match self.gesture {
            Gesture::Dragging { .. } => Some(self.release(self.now, current, false)),
            Gesture::Pressed { .. } | Gesture::Declined => {
                self.gesture = Gesture::Idle;
                None
            }
            Gesture::Idle | Gesture::Settling { .. } | Gesture::Held => None,
        };
        self.publish();
        release
    }

    /// Locks the row in place, or lets it move again (FOLD-04): see `locked`.
    pub fn lock(&mut self, locked: bool) {
        self.locked = locked;
    }

    /// Where the finger went down, while the gesture is still a press: down, not past the touch
    /// slop, not dragging or catching the row. Only then can a long-press count.
    pub fn pressed_at(&self) -> Option<(f64, f64)> {
        match self.gesture {
            Gesture::Pressed { x, y } => Some((x, y)),
            _ => None,
        }
    }

    /// The finger has stayed down for the platform's long-press time, as the host measures it,
    /// on empty space (HOME-10) or on an app's icon (APPS-11). It counts only while the gesture
    /// is still a press (see [`Carousel::pressed_at`]). Returns where the finger went down, and
    /// takes the rest of the gesture from the row and the content below.
    pub fn long_press(&mut self) -> Option<(f64, f64)> {
        let at = self.pressed_at()?;
        self.gesture = Gesture::Held;
        self.publish();
        Some(at)
    }

    /// Follows the finger (HOME-03, ANIM-01). Returns where the row settles when a drag ends.
    pub fn pointer(&mut self, event: PointerEvent, current: HomeView) -> Option<Release> {
        if self.state.layout.width_px <= 0.0 {
            return None;
        }
        self.now = self.now.max(event.at);
        let release = match (event.kind, self.gesture) {
            (PointerKind::Down, _) => {
                self.press(event, current);
                None
            }
            (PointerKind::Move, Gesture::Pressed { x, y }) => {
                self.tracker.add(event.at, self.logical(event.x));
                self.claim(event, x, y, current);
                None
            }
            (PointerKind::Move, Gesture::Dragging { .. }) => {
                self.drag(event, current);
                None
            }
            (PointerKind::Up, Gesture::Dragging { .. }) => {
                // Lifting repeats the last position, so it says nothing about the speed. As on
                // Android, a finger that stopped before lifting has none.
                self.move_row(event.x, current);
                Some(self.release(event.at, current, true))
            }
            (PointerKind::Cancel, Gesture::Dragging { .. }) => {
                Some(self.release(event.at, current, false))
            }
            (
                PointerKind::Up | PointerKind::Cancel,
                Gesture::Pressed { .. } | Gesture::Declined | Gesture::Held,
            ) => {
                self.gesture = Gesture::Idle;
                None
            }
            _ => None,
        };
        self.publish();
        release
    }

    /// Settles the row on `to`, which becomes the current view, from wherever it is now. It
    /// leaves `velocity` (widths per second towards the next view) as fast as it arrived, unless
    /// that would overshoot by more than 3 % (ANIM-02). `at` is when the motion starts, or `None`
    /// for the next frame.
    pub fn settle(&mut self, from: HomeView, to: HomeView, velocity: f64, at: Option<Duration>) {
        let offset = self.state.offset - steps(from, to, self.state.wrap);
        // A move that starts before the last one has settled, such as two quick screen-reader
        // actions, goes the shorter way round the ring, or no further than half a view past the
        // neighbour with wrap-around off. The row never moves more than one and a half views,
        // so it never overshoots by more than 3 % (ANIM-02).
        let offset = if self.state.wrap {
            around_the_ring(offset)
        } else {
            offset.clamp(-1.5, 1.5)
        };
        self.state.offset = offset;
        if self.animation_scale == 0.0 || (offset == 0.0 && velocity == 0.0) {
            // With animations removed, views change at once (ANIM-05).
            self.rest(at.unwrap_or(self.now));
        } else {
            // Spring time runs slower by the animation scale; the release speed stays the
            // finger's.
            let velocity = velocity * self.animation_scale;
            let velocity = SETTLE.limit_velocity(offset, velocity, MAX_OVERSHOOT);
            self.gesture = Gesture::Settling {
                motion: SpringMotion::new(SETTLE, offset, velocity),
                last: at,
            };
        }
        self.publish();
    }

    /// Advances the motion to the display's frame time (ANIM-04).
    pub fn frame(&mut self, at: Duration) {
        if let Gesture::Settling { mut motion, last } = self.gesture {
            // Spring time runs slower by the animation scale (ANIM-05).
            let scale = self.animation_scale;
            if let Some(last) = last.filter(|_| scale > 0.0) {
                motion.advance(motion_time(at.saturating_sub(last), scale));
            }
            self.state.offset = motion.displacement();
            let width = self.state.layout.width_px.max(1.0);
            let rest_speed = REST_SPEED_PX / width * scale;
            // Animations may have been removed while the row was moving.
            if scale == 0.0 || motion.is_at_rest(REST_DISTANCE_PX / width, rest_speed) {
                self.rest(at);
            } else {
                let last = Some(last.map_or(at, |last| last.max(at)));
                self.gesture = Gesture::Settling { motion, last };
            }
        }
        self.now = self.now.max(at);
        self.indicator.advance(at);
        self.publish();
    }

    fn press(&mut self, event: PointerEvent, current: HomeView) {
        self.tracker.clear();
        self.tracker.add(event.at, self.logical(event.x));
        let moving = matches!(self.gesture, Gesture::Dragging { .. }) || self.catchable();
        if moving && !self.locked {
            // Touching the moving row catches it where it is (ANIM-03).
            let start = self.unstretched(self.state.offset, current);
            self.start_drag(event.x, start, event.at);
        } else {
            // A row as good as at rest, or one locked under an open folder or search, lands at
            // once: the touch is the content's (ANIM-03).
            if self.state.offset != 0.0 || matches!(self.gesture, Gesture::Settling { .. }) {
                self.rest(event.at);
            }
            self.gesture = Gesture::Pressed {
                x: event.x,
                y: event.y,
            };
        }
    }

    /// Whether the row is far enough from rest for a touch to catch it: further than
    /// [`CATCH_SLOP_FRACTION`] of the touch slop.
    fn catchable(&self) -> bool {
        let away_px = self.state.offset.abs() * self.state.layout.width_px;
        self.state.offset != 0.0 && away_px > self.state.layout.touch_slop_px * CATCH_SLOP_FRACTION
    }

    /// Starts a drag once the finger has moved past the touch slop, sideways rather than up or
    /// down, anywhere in the view (HOME-03, HOME-13).
    fn claim(&mut self, event: PointerEvent, x: f64, y: f64, current: HomeView) {
        let (dx, dy) = (event.x - x, event.y - y);
        let slop = self.state.layout.touch_slop_px;
        if dx.abs() <= slop && dy.abs() <= slop {
            return;
        }
        if dx.abs() > dy.abs() && !self.locked {
            // Measured from where the finger crossed the slop, so the row doesn't jump.
            self.start_drag(x + slop.copysign(dx), 0.0, event.at);
            self.drag(event, current);
        } else {
            self.gesture = Gesture::Declined;
        }
    }

    fn start_drag(&mut self, origin_x: f64, start: f64, at: Duration) {
        self.gesture = Gesture::Dragging {
            origin_x,
            start,
            travel: 0.0,
        };
        self.indicator.hide_at = None;
        self.indicator.fade_to(1.0, at, INDICATOR_IN);
    }

    fn drag(&mut self, event: PointerEvent, current: HomeView) {
        self.tracker.add(event.at, self.logical(event.x));
        self.move_row(event.x, current);
    }

    /// Puts the row where the finger at `x` takes it.
    fn move_row(&mut self, x: f64, current: HomeView) {
        let Gesture::Dragging {
            origin_x, start, ..
        } = self.gesture
        else {
            return;
        };
        let travel = (self.logical(x) - self.logical(origin_x)) / self.state.layout.width_px;
        self.gesture = Gesture::Dragging {
            origin_x,
            start,
            travel,
        };
        self.state.offset = self.stretched(start + travel, current);
    }

    /// Picks the view to settle on (ANIM-07): the next one after a fling or a drag past 35 % of
    /// the width, otherwise the nearest. A cancelled drag has no fling.
    fn release(&mut self, at: Duration, current: HomeView, may_fling: bool) -> Release {
        let Gesture::Dragging { travel, .. } = self.gesture else {
            unreachable!("only a drag is released");
        };
        self.gesture = Gesture::Idle;
        let speed_px = if may_fling {
            self.tracker.velocity(at)
        } else {
            0.0
        };
        let density = if self.state.layout.density > 0.0 {
            self.state.layout.density
        } else {
            1.0
        };
        let towards_next = if speed_px.abs() >= VIEW_FLING_VELOCITY_DP * density {
            Some(speed_px > 0.0)
        } else if travel.abs() > VIEW_SWIPE_THRESHOLD {
            Some(travel > 0.0)
        } else {
            None
        };
        let offset = self.state.offset;
        let direction = match towards_next {
            Some(true) => (offset >= 0.0).then_some(Direction::Next),
            Some(false) => (offset <= 0.0).then_some(Direction::Previous),
            None if offset > 0.5 => Some(Direction::Next),
            None if offset < -0.5 => Some(Direction::Previous),
            None => None,
        };
        Release {
            target: direction
                .and_then(|direction| current.neighbour(direction, self.state.wrap))
                .unwrap_or(current),
            velocity: speed_px / self.state.layout.width_px,
            at,
        }
    }

    /// The views have settled: the indicator stays a while, then fades out (ANIM-11).
    fn rest(&mut self, at: Duration) {
        self.state.offset = 0.0;
        self.gesture = Gesture::Idle;
        if self.indicator.opacity > 0.0 || self.indicator.fade.is_some() {
            self.indicator.hide_at = Some(at + INDICATOR_HOLD);
            self.indicator.advance(at);
        }
    }

    fn publish(&mut self) {
        self.state.indicator = self.indicator.opacity;
        self.state.dragging = matches!(self.gesture, Gesture::Dragging { .. });
        self.state.held = self.gesture == Gesture::Held;
        self.state.animating =
            matches!(self.gesture, Gesture::Settling { .. }) || self.indicator.is_animating();
    }

    /// A position along the row that grows towards the next view (L10N-02).
    fn logical(&self, x: f64) -> f64 {
        if self.state.layout.rtl { x } else { -x }
    }

    /// Whether the row can't move in the direction of `offset`: past an end, with wrap-around
    /// off.
    fn is_past_end(&self, offset: f64, current: HomeView) -> bool {
        let direction = if offset > 0.0 {
            Direction::Next
        } else {
            Direction::Previous
        };
        offset != 0.0 && current.neighbour(direction, self.state.wrap).is_none()
    }

    /// Where the row is drawn for a drag to `offset`: at most one and a half views either way,
    /// so a drag never shows more than the next view, and only a short, resisting stretch past
    /// an end (HOME-04).
    fn stretched(&self, offset: f64, current: HomeView) -> f64 {
        let offset = offset.clamp(-1.5, 1.5);
        if !self.is_past_end(offset, current) {
            return offset;
        }
        let pull = offset.abs() * VIEW_EDGE_RESISTANCE;
        (pull / (1.0 + pull / VIEW_EDGE_STRETCH)).copysign(offset)
    }

    /// The drag position that [`Self::stretched`] draws at `offset`, for catching the row.
    fn unstretched(&self, offset: f64, current: HomeView) -> f64 {
        if !self.is_past_end(offset, current) {
            return offset;
        }
        let stretch = offset.abs().min(VIEW_EDGE_STRETCH * 0.99);
        (stretch / (VIEW_EDGE_RESISTANCE * (1.0 - stretch / VIEW_EDGE_STRETCH))).copysign(offset)
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::{self, Fakes};
    use meegone_settings_state::SettingsIntent;

    use super::*;
    use crate::{HomeShell, ShellIntent};

    const WIDTH: f64 = 1000.0;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn assert_near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    /// 1000 px wide at 2 px per dp, with a 10 px touch slop.
    fn layout(rtl: bool) -> Layout {
        Layout {
            width_px: WIDTH,
            density: 2.0,
            touch_slop_px: 10.0,
            rtl,
        }
    }

    fn laid_out(fakes: &Fakes, rtl: bool) -> HomeShell {
        let mut shell = HomeShell::compose(fakes.platform());
        shell.dispatch(ShellIntent::Layout(layout(rtl)));
        shell
    }

    fn shell() -> HomeShell {
        laid_out(&Fakes::default(), false)
    }

    fn touch(shell: &mut HomeShell, kind: PointerKind, x: f64, at: u64) {
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x,
            y: 800.0,
            at: ms(at),
        }));
    }

    /// Puts a finger down at `from` at `start` ms and moves it steadily to `to` over `length`
    /// ms, one event every 8 ms or so. Returns when it got there.
    fn drag(shell: &mut HomeShell, from: f64, to: f64, start: u64, length: u64) -> u64 {
        touch(shell, PointerKind::Down, from, start);
        let steps = (length / 8).max(1);
        for step in 1..=steps {
            let x = from + (to - from) * step as f64 / steps as f64;
            touch(shell, PointerKind::Move, x, start + step * length / steps);
        }
        start + length
    }

    /// A slow drag that stops before the finger lifts, so it is no fling. Returns when the
    /// finger lifted.
    fn slow_swipe(shell: &mut HomeShell, from: f64, to: f64, start: u64) -> u64 {
        let end = drag(shell, from, to, start, 400) + 100;
        touch(shell, PointerKind::Up, to, end);
        end
    }

    /// A quick flick: from `from` to `to` in 48 ms. Returns when the finger lifted.
    fn flick(shell: &mut HomeShell, from: f64, to: f64, start: u64) -> u64 {
        let end = drag(shell, from, to, start, 48);
        touch(shell, PointerKind::Up, to, end);
        end
    }

    /// Draws a frame every 8 ms from `from` until the row is at rest. Returns when it was.
    fn settle(shell: &mut HomeShell, from: u64) -> u64 {
        let mut at = from;
        loop {
            shell.dispatch(ShellIntent::Frame(ms(at)));
            let carousel = shell.state().carousel;
            if carousel.offset == 0.0 && !carousel.dragging {
                return at;
            }
            at += 8;
            assert!(at < from + 2000, "still moving: {carousel:?}");
        }
    }

    fn settled_on(shell: &mut HomeShell, from: u64) -> HomeView {
        settle(shell, from);
        shell.state().current
    }

    #[test]
    fn the_row_follows_the_finger() {
        // HOME-03, ANIM-01: measured from where the finger passed the touch slop, so the row
        // never jumps.
        let mut shell = shell();

        drag(&mut shell, 600.0, 400.0, 0, 80);

        let state = shell.state();
        assert!(state.carousel.dragging);
        assert_near(state.carousel.offset, 0.19);
        assert_near(state.page_offset_px(HomeView::Applications), -190.0);
        assert_near(state.page_offset_px(HomeView::Recent), 810.0);
        assert_near(state.page_offset_px(HomeView::Events), -1190.0);
    }

    #[test]
    fn a_tap_or_a_vertical_drag_leaves_the_gesture_to_the_content() {
        // HOME-03: elements below keep taps and vertical scrolls.
        let mut shell = shell();

        drag(&mut shell, 500.0, 495.0, 0, 16);
        assert!(!shell.state().carousel.dragging);
        touch(&mut shell, PointerKind::Up, 495.0, 20);

        touch(&mut shell, PointerKind::Down, 500.0, 100);
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Move,
            x: 494.0,
            y: 860.0,
            at: ms(108),
        }));
        touch(&mut shell, PointerKind::Move, 100.0, 116);
        assert!(!shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, 0.0);
        touch(&mut shell, PointerKind::Up, 100.0, 124);
        assert_eq!(shell.state().current, HomeView::Applications);

        drag(&mut shell, 500.0, 300.0, 200, 80);
        assert!(shell.state().carousel.dragging);
    }

    #[test]
    fn a_drag_past_35_percent_settles_on_the_next_view_and_a_shorter_one_returns() {
        // ANIM-07, and the issue's acceptance criteria.
        let mut shell = shell();
        let end = slow_swipe(&mut shell, 800.0, 430.0, 0);
        assert_eq!(settled_on(&mut shell, end), HomeView::Recent);

        let end = slow_swipe(&mut shell, 200.0, 570.0, end + 100);
        assert_eq!(settled_on(&mut shell, end), HomeView::Applications);

        let end = slow_swipe(&mut shell, 200.0, 570.0, end + 100);
        assert_eq!(settled_on(&mut shell, end), HomeView::Events);

        let end = slow_swipe(&mut shell, 800.0, 450.0, end + 100);
        assert_eq!(settled_on(&mut shell, end), HomeView::Events);
        let end = slow_swipe(&mut shell, 200.0, 550.0, end + 100);
        assert_eq!(settled_on(&mut shell, end), HomeView::Events);
    }

    #[test]
    fn a_fling_settles_on_the_next_view_whatever_its_length() {
        // ANIM-07: 100 px in 48 ms is about 1000 dp/s.
        let mut shell = shell();

        let end = flick(&mut shell, 600.0, 500.0, 0);
        assert_eq!(shell.state().current, HomeView::Recent);
        assert!(shell.state().carousel.animating);
        assert_eq!(settled_on(&mut shell, end), HomeView::Recent);

        let end = flick(&mut shell, 500.0, 600.0, end + 100);
        assert_eq!(settled_on(&mut shell, end), HomeView::Applications);
    }

    #[test]
    fn a_flick_back_against_the_drag_returns() {
        // ANIM-07: the fling's direction wins over the distance dragged.
        let mut shell = shell();

        let moved = drag(&mut shell, 900.0, 450.0, 0, 800);
        let end = drag(&mut shell, 450.0, 520.0, moved, 32);
        touch(&mut shell, PointerKind::Up, 520.0, end);

        assert_eq!(settled_on(&mut shell, end), HomeView::Applications);
    }

    #[test]
    fn moving_past_either_end_wraps_around_seamlessly() {
        // HOME-04: Events comes in from the right of Recent, with no jump.
        let mut shell = shell();
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        settle(&mut shell, 0);
        assert_eq!(shell.state().current, HomeView::Recent);

        touch(&mut shell, PointerKind::Down, 900.0, 1000);
        for step in 1..=60 {
            let x = 900.0 - 10.0 * f64::from(step);
            touch(&mut shell, PointerKind::Move, x, 1000 + 8 * step as u64);
            if step >= 2 {
                let travel = 890.0 - x;
                assert_near(
                    shell.state().page_offset_px(HomeView::Events),
                    WIDTH - travel,
                );
                assert_near(shell.state().page_offset_px(HomeView::Recent), -travel);
            }
        }
        touch(&mut shell, PointerKind::Up, 300.0, 1600);
        assert_eq!(settled_on(&mut shell, 1600), HomeView::Events);

        let state = shell.state();
        assert_eq!(state.page_offset_px(HomeView::Events), 0.0);
        assert_eq!(state.page_offset_px(HomeView::Applications), WIDTH);
        assert_eq!(state.page_offset_px(HomeView::Recent), -WIDTH);

        let end = slow_swipe(&mut shell, 200.0, 600.0, 3000);
        assert_eq!(settled_on(&mut shell, end), HomeView::Recent);
    }

    #[test]
    fn with_wrap_around_off_the_row_resists_past_the_ends_and_returns() {
        // HOME-04
        let mut shell = shell();
        shell.dispatch(ShellIntent::Settings(SettingsIntent::SetWrapAround(false)));
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        settle(&mut shell, 0);

        let moved = drag(&mut shell, 900.0, 100.0, 1000, 400);
        let offset = shell.state().carousel.offset;
        assert!(offset > 0.0 && offset <= VIEW_EDGE_STRETCH, "{offset}");
        touch(&mut shell, PointerKind::Up, 100.0, moved + 100);

        assert_eq!(settled_on(&mut shell, moved + 100), HomeView::Recent);
        assert_eq!(shell.state().page_offset_px(HomeView::Events), -2.0 * WIDTH);
    }

    #[test]
    fn touching_the_moving_row_catches_it_where_it_is() {
        // ANIM-03
        let mut shell = shell();
        let end = flick(&mut shell, 600.0, 500.0, 0);
        for frame in 0..=15 {
            shell.dispatch(ShellIntent::Frame(ms(end + 8 * frame)));
        }
        let caught = shell.state().carousel.offset;
        assert!(caught < 0.0 && caught > -0.5, "{caught}");

        touch(&mut shell, PointerKind::Down, 300.0, end + 124);
        assert_eq!(shell.state().carousel.offset, caught);
        assert!(shell.state().carousel.dragging);
        touch(&mut shell, PointerKind::Move, 310.0, end + 132);
        assert_near(shell.state().carousel.offset, caught - 0.01);

        touch(&mut shell, PointerKind::Up, 310.0, end + 300);
        assert_eq!(settled_on(&mut shell, end + 300), HomeView::Recent);
    }

    #[test]
    fn a_touch_on_the_last_sliver_of_a_settle_goes_to_what_is_under_it() {
        // ANIM-03: within a third of the 10 px touch slop of rest, the row is as good as there.
        // It lands at once, and the touch is a press on the content, not a catch, so the tail
        // of a settle never swallows a tap.
        let mut shell = shell();
        let end = flick(&mut shell, 600.0, 500.0, 0);
        let mut at = end;
        while shell.state().carousel.offset.abs() * WIDTH >= 10.0 / 3.0 {
            at += 8;
            shell.dispatch(ShellIntent::Frame(ms(at)));
        }
        assert!(shell.state().carousel.offset != 0.0);
        assert!(shell.state().carousel.animating, "still settling");

        touch(&mut shell, PointerKind::Down, 300.0, at + 4);
        let carousel = shell.state().carousel;
        assert_eq!(carousel.offset, 0.0);
        assert!(!carousel.dragging);
        assert_eq!(shell.state().current, HomeView::Recent);
        touch(&mut shell, PointerKind::Move, 304.0, at + 12);
        assert!(!shell.state().carousel.dragging, "a press, within the slop");
        touch(&mut shell, PointerKind::Up, 304.0, at + 20);
        assert_eq!(shell.state().carousel.offset, 0.0);
    }

    #[test]
    fn a_locked_row_is_never_caught() {
        // ANIM-03, FOLD-04: a touch on an open folder or search, which lock the row, is theirs,
        // even if the row still moved; it lands at once.
        let mut carousel = Carousel::new(true, 1.0);
        carousel.layout(layout(false), HomeView::Applications);
        carousel.settle(HomeView::Applications, HomeView::Recent, 0.0, Some(ms(0)));
        carousel.frame(ms(48));
        assert!(carousel.state().offset.abs() * WIDTH > 10.0);

        carousel.lock(true);
        let down = PointerEvent {
            kind: PointerKind::Down,
            x: 300.0,
            y: 800.0,
            at: ms(56),
        };
        carousel.pointer(down, HomeView::Recent);
        assert_eq!(carousel.state().offset, 0.0);
        assert!(!carousel.state().dragging);
        assert_eq!(carousel.pressed_at(), Some((300.0, 800.0)));
    }

    #[test]
    fn a_right_to_left_layout_mirrors_the_order_and_the_swipe_directions() {
        // L10N-02: Recent is on the left, so a drag to the right shows it.
        let mut shell = laid_out(&Fakes::default(), true);

        drag(&mut shell, 400.0, 600.0, 0, 80);
        assert_near(shell.state().page_offset_px(HomeView::Recent), -810.0);
        assert_near(shell.state().page_offset_px(HomeView::Applications), 190.0);

        touch(&mut shell, PointerKind::Up, 600.0, 400);
        assert_eq!(settled_on(&mut shell, 400), HomeView::Applications);
        let end = slow_swipe(&mut shell, 400.0, 800.0, 1000);
        assert_eq!(settled_on(&mut shell, end), HomeView::Recent);
    }

    #[test]
    fn a_drag_that_starts_anywhere_moves_between_views() {
        // HOME-13: no edge swipe is needed, so the system Back gesture keeps the edges.
        for (from, to, view) in [
            (500.0, 100.0, HomeView::Recent),
            (995.0, 600.0, HomeView::Recent),
            (5.0, 400.0, HomeView::Events),
        ] {
            let mut shell = shell();
            let end = slow_swipe(&mut shell, from, to, 0);
            assert_eq!(settled_on(&mut shell, end), view, "from {from}");
        }
    }

    #[test]
    fn a_fast_fling_never_overshoots_by_more_than_3_percent() {
        // ANIM-02
        let mut shell = shell();
        let end = drag(&mut shell, 600.0, 120.0, 0, 40);
        touch(&mut shell, PointerKind::Up, 120.0, end);
        assert_eq!(shell.state().current, HomeView::Recent);

        let mut furthest: f64 = 0.0;
        for frame in 0..200 {
            shell.dispatch(ShellIntent::Frame(ms(end + 4 * frame)));
            furthest = furthest.max(shell.state().carousel.offset);
        }
        assert!(furthest <= 0.03, "{furthest}");
    }

    #[test]
    fn the_indicator_fades_in_with_a_drag_and_out_after_the_views_settle() {
        // HOME-09, ANIM-11: 100 ms in; 600 ms after settling, 200 ms out.
        let mut shell = shell();
        let opacity = |shell: &mut HomeShell, at: u64| {
            shell.dispatch(ShellIntent::Frame(ms(at)));
            shell.state().carousel.indicator
        };
        touch(&mut shell, PointerKind::Down, 600.0, 1000);
        touch(&mut shell, PointerKind::Move, 580.0, 1008);
        assert_eq!(shell.state().carousel.indicator, 0.0);
        assert!(shell.state().carousel.animating);
        assert_near(opacity(&mut shell, 1058), 0.5);
        assert_near(opacity(&mut shell, 1108), 1.0);

        touch(&mut shell, PointerKind::Up, 580.0, 1200);
        let rested = settle(&mut shell, 1200);
        assert_near(opacity(&mut shell, rested + 599), 1.0);
        assert_near(opacity(&mut shell, rested + 700), 0.5);
        assert_near(opacity(&mut shell, rested + 800), 0.0);
        assert!(!shell.state().carousel.animating);

        // A new drag while it fades out brings it back from where it is.
        touch(&mut shell, PointerKind::Down, 600.0, rested + 1000);
        touch(&mut shell, PointerKind::Move, 580.0, rested + 1008);
        touch(&mut shell, PointerKind::Up, 580.0, rested + 1100);
        let rested = settle(&mut shell, rested + 1100);
        assert_near(opacity(&mut shell, rested + 650), 0.75);
        touch(&mut shell, PointerKind::Down, 600.0, rested + 650);
        touch(&mut shell, PointerKind::Move, 580.0, rested + 650);
        assert_near(opacity(&mut shell, rested + 700), 0.875);
    }

    #[test]
    fn the_indicator_highlights_the_view_nearest_the_row() {
        // HOME-09
        let mut shell = shell();
        drag(&mut shell, 900.0, 500.0, 0, 200);
        assert_eq!(shell.state().nearest_view(), HomeView::Applications);
        touch(&mut shell, PointerKind::Move, 300.0, 208);
        assert_eq!(shell.state().nearest_view(), HomeView::Recent);
    }

    #[test]
    fn with_animations_removed_views_change_at_once() {
        // ANIM-05, A11Y-07
        let fakes = Fakes::default();
        *fakes.system.animation_scale.lock().unwrap() = 0.0;
        let mut shell = laid_out(&fakes, false);

        drag(&mut shell, 800.0, 400.0, 0, 400);
        assert_eq!(shell.state().carousel.indicator, 1.0);
        touch(&mut shell, PointerKind::Up, 400.0, 500);

        let state = shell.state();
        assert_eq!(state.current, HomeView::Recent);
        assert_eq!(state.carousel.offset, 0.0);
        // The indicator still stays its while, a wait rather than motion, so it is still seen
        // (ANIM-11): then it goes at once.
        assert_eq!(state.carousel.indicator, 1.0);
        shell.dispatch(ShellIntent::Frame(ms(1_099)));
        assert_eq!(shell.state().carousel.indicator, 1.0);
        shell.dispatch(ShellIntent::Frame(ms(1_100)));
        assert_eq!(shell.state().carousel.indicator, 0.0);
        assert!(!shell.state().carousel.animating);
    }

    #[test]
    fn animations_removed_while_the_row_moves_stop_it_at_once() {
        // ANIM-05: the speed is read again when MeeGone comes back to the foreground.
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes, false);
        let end = flick(&mut shell, 600.0, 500.0, 0);

        *fakes.system.animation_scale.lock().unwrap() = 0.0;
        shell.dispatch(ShellIntent::Resumed);
        shell.dispatch(ShellIntent::Frame(ms(end + 8)));

        assert_eq!(shell.state().carousel.offset, 0.0);
        assert_eq!(shell.state().current, HomeView::Recent);
    }

    #[test]
    fn slower_animations_settle_more_slowly() {
        // ANIM-05
        let fakes = Fakes::default();
        *fakes.system.animation_scale.lock().unwrap() = 2.0;
        let mut slow = laid_out(&fakes, false);
        let mut normal = shell();
        for shell in [&mut slow, &mut normal] {
            shell.dispatch(ShellIntent::Swipe(Direction::Next));
        }

        let normal_time = settle(&mut normal, 0);
        let slow_time = settle(&mut slow, 0);
        assert!(
            slow_time > normal_time + normal_time / 2,
            "{slow_time} vs {normal_time}"
        );
    }

    #[test]
    fn a_swipe_without_a_gesture_slides_the_view_in() {
        // A11Y-05: screen-reader actions animate like a swipe.
        let mut shell = shell();

        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        assert_eq!(shell.state().current, HomeView::Recent);
        assert_eq!(shell.state().page_offset_px(HomeView::Recent), WIDTH);
        assert_eq!(shell.state().carousel.indicator, 0.0);
        shell.dispatch(ShellIntent::Frame(ms(5000)));
        assert_eq!(shell.state().page_offset_px(HomeView::Recent), WIDTH);
        shell.dispatch(ShellIntent::Frame(ms(5050)));
        let offset = shell.state().page_offset_px(HomeView::Recent);
        assert!(offset > 0.0 && offset < WIDTH, "{offset}");

        settle(&mut shell, 5058);
        assert_eq!(shell.state().page_offset_px(HomeView::Recent), 0.0);
        assert!(!shell.state().carousel.animating);
    }

    #[test]
    fn home_during_a_drag_ends_it_and_goes_to_applications() {
        // HOME-07
        let mut shell = shell();
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        settle(&mut shell, 0);
        let moved = drag(&mut shell, 600.0, 400.0, 1000, 80);

        shell.dispatch(ShellIntent::Home);
        let offset = shell.state().carousel.offset;
        touch(&mut shell, PointerKind::Move, 100.0, moved + 8);

        assert!(!shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, offset);
        assert_eq!(shell.state().current, HomeView::Applications);
        touch(&mut shell, PointerKind::Up, 100.0, moved + 16);
        assert_eq!(settled_on(&mut shell, moved + 16), HomeView::Applications);

        // On Applications, Home returns a row that is being dragged.
        let moved = drag(&mut shell, 600.0, 300.0, 3000, 80);
        shell.dispatch(ShellIntent::Home);
        assert!(!shell.state().carousel.dragging);
        assert_eq!(settled_on(&mut shell, moved), HomeView::Applications);
    }

    #[test]
    fn touches_before_layout_or_under_settings_are_ignored() {
        let mut shell = HomeShell::compose(Fakes::default().platform());
        drag(&mut shell, 600.0, 100.0, 0, 80);
        assert!(!shell.state().carousel.dragging);

        shell.dispatch(ShellIntent::Layout(layout(false)));
        shell.dispatch(ShellIntent::OpenSettings);
        drag(&mut shell, 600.0, 100.0, 100, 80);
        assert!(!shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, 0.0);
    }

    #[test]
    fn a_cancelled_drag_settles_on_the_nearest_view() {
        // The system or a child element took the gesture: no fling.
        let mut shell = shell();
        let moved = drag(&mut shell, 900.0, 300.0, 0, 48);
        touch(&mut shell, PointerKind::Cancel, 300.0, moved);
        assert_eq!(settled_on(&mut shell, moved), HomeView::Recent);

        let moved = drag(&mut shell, 600.0, 300.0, 1000, 48);
        touch(&mut shell, PointerKind::Cancel, 300.0, moved);
        assert_eq!(settled_on(&mut shell, moved), HomeView::Recent);
    }

    #[test]
    fn a_new_layout_ends_a_drag() {
        // Rotating or resizing mid-drag: the finger's positions no longer fit.
        let mut shell = shell();
        let moved = drag(&mut shell, 600.0, 400.0, 0, 80);
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 2000.0,
            ..layout(false)
        }));
        assert!(!shell.state().carousel.dragging);
        assert_eq!(settled_on(&mut shell, moved), HomeView::Applications);

        // The same layout again changes nothing.
        drag(&mut shell, 600.0, 400.0, 1000, 80);
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 2000.0,
            ..layout(false)
        }));
        assert!(shell.state().carousel.dragging);
        touch(&mut shell, PointerKind::Up, 400.0, 1080);
        touch(&mut shell, PointerKind::Down, 400.0, 1200);
        shell.dispatch(ShellIntent::Layout(layout(false)));
        touch(&mut shell, PointerKind::Move, 100.0, 1208);
        assert!(!shell.state().carousel.dragging);
    }

    #[test]
    fn a_frame_from_before_the_release_moves_nothing() {
        let mut shell = shell();
        let end = flick(&mut shell, 600.0, 500.0, 1000);
        let released = shell.state().carousel.offset;

        shell.dispatch(ShellIntent::Frame(ms(end - 100)));

        assert_eq!(shell.state().carousel.offset, released);
    }

    #[test]
    fn a_lost_lift_is_treated_as_catching_the_row() {
        // A second Down without an Up: the row is caught where the first finger left it.
        let mut shell = shell();
        drag(&mut shell, 600.0, 400.0, 0, 80);
        let offset = shell.state().carousel.offset;

        touch(&mut shell, PointerKind::Down, 700.0, 200);

        assert!(shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, offset);
    }

    fn visible_pages(shell: &HomeShell) -> Vec<(HomeView, f64)> {
        HomeView::ALL
            .into_iter()
            .map(|view| (view, shell.state().page_offset_px(view)))
            .filter(|(_, offset)| offset.abs() < WIDTH)
            .collect()
    }

    /// The furthest the row travels past its target while it settles, in widths.
    fn overshoot(shell: &mut HomeShell, from: u64) -> f64 {
        let start = shell.state().carousel.offset;
        let mut furthest: f64 = 0.0;
        for frame in 0..300 {
            shell.dispatch(ShellIntent::Frame(ms(from + 4 * frame)));
            furthest = furthest.max(-start.signum() * shell.state().carousel.offset);
        }
        assert_eq!(shell.state().carousel.offset, 0.0);
        furthest
    }

    #[test]
    fn quick_moves_one_after_another_never_make_a_view_jump() {
        // HOME-04, ANIM-02: screen-reader actions faster than the views settle.
        for moves in [2, 3] {
            let mut shell = shell();
            shell.dispatch(ShellIntent::Swipe(Direction::Next));
            shell.dispatch(ShellIntent::Frame(ms(0)));
            for step in 1..moves {
                shell.dispatch(ShellIntent::Frame(ms(16 * step)));
                let before = visible_pages(&shell);
                assert!(!before.is_empty());
                shell.dispatch(ShellIntent::Swipe(Direction::Next));
                for (view, offset) in before {
                    assert_near(shell.state().page_offset_px(view), offset);
                }
            }
            let furthest = overshoot(&mut shell, 16 * moves);
            assert!(furthest <= MAX_OVERSHOOT, "{moves} moves: {furthest}");
        }
    }

    #[test]
    fn home_while_the_row_settles_never_makes_a_view_jump() {
        // HOME-04, HOME-07: from Recent, a flick to Events, then Home before it settles.
        let mut shell = shell();
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        let rested = settle(&mut shell, 0);
        let end = flick(&mut shell, 600.0, 500.0, rested + 100);
        assert_eq!(shell.state().current, HomeView::Events);
        for frame in 0..=10 {
            shell.dispatch(ShellIntent::Frame(ms(end + 8 * frame)));
        }

        let before = visible_pages(&shell);
        shell.dispatch(ShellIntent::Home);

        assert_eq!(shell.state().current, HomeView::Applications);
        for (view, offset) in before {
            assert_near(shell.state().page_offset_px(view), offset);
        }
        assert!(overshoot(&mut shell, end + 88) <= MAX_OVERSHOOT);
    }

    #[test]
    fn a_finger_that_stops_before_lifting_does_not_fling() {
        // ANIM-07: as on Android, only the movement before the lift counts.
        let mut shell = shell();
        let moved = drag(&mut shell, 600.0, 450.0, 0, 48);
        touch(&mut shell, PointerKind::Up, 450.0, moved + 50);
        assert_eq!(settled_on(&mut shell, moved + 50), HomeView::Applications);

        let moved = drag(&mut shell, 600.0, 450.0, 1000, 48);
        touch(&mut shell, PointerKind::Up, 450.0, moved + 16);
        assert_eq!(settled_on(&mut shell, moved + 16), HomeView::Recent);
    }

    #[test]
    fn an_animation_speed_that_cant_be_timed_is_made_safe() {
        // ANIM-05: whatever the system reports, nothing panics and the views settle.
        for (scale, instant) in [
            (f64::NAN, false),
            (f64::INFINITY, false),
            (1e30, false),
            (1e-17, true),
            (-1.0, true),
        ] {
            let fakes = Fakes::default();
            *fakes.system.animation_scale.lock().unwrap() = scale;
            let mut shell = laid_out(&fakes, false);
            shell.dispatch(ShellIntent::Swipe(Direction::Next));
            assert_eq!(shell.state().carousel.offset == 0.0, instant, "{scale}");
            let mut at = 0;
            while shell.state().carousel.offset != 0.0 {
                shell.dispatch(ShellIntent::Frame(ms(at)));
                at += 16;
                assert!(at < 60_000, "{scale}: still moving");
            }
        }
    }

    #[test]
    fn a_long_press_counts_only_while_the_finger_is_still_a_press() {
        // HOME-10: a finger that moved past the touch slop scrolls or drags instead.
        let fakes = Fakes::default();
        let ticks = || *fakes.system.haptic_ticks.lock().unwrap();
        let held = |shell: &mut HomeShell| {
            shell.dispatch(ShellIntent::LongPress);
            shell.state().carousel.held
        };

        // Before the host has laid the views out.
        let mut unlaid = HomeShell::compose(fakes.platform());
        touch(&mut unlaid, PointerKind::Down, 500.0, 0);
        assert!(!held(&mut unlaid));

        let mut shell = laid_out(&fakes, false);
        // Up or down past the slop.
        touch(&mut shell, PointerKind::Down, 500.0, 0);
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Move,
            x: 500.0,
            y: 830.0,
            at: ms(8),
        }));
        assert!(!held(&mut shell));
        touch(&mut shell, PointerKind::Up, 500.0, 16);

        // Sideways past the slop.
        touch(&mut shell, PointerKind::Down, 500.0, 100);
        touch(&mut shell, PointerKind::Move, 480.0, 200);
        assert!(shell.state().carousel.dragging);
        assert!(!held(&mut shell));
        touch(&mut shell, PointerKind::Up, 480.0, 300);
        let rested = settle(&mut shell, 300);

        // Lifted already.
        touch(&mut shell, PointerKind::Down, 500.0, rested + 100);
        touch(&mut shell, PointerKind::Up, 500.0, rested + 150);
        assert!(!held(&mut shell));

        // Catching the moving row.
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        touch(&mut shell, PointerKind::Down, 500.0, rested + 200);
        assert!(!held(&mut shell));
        touch(&mut shell, PointerKind::Up, 500.0, rested + 250);
        let rested = settle(&mut shell, rested + 250);
        assert_eq!(ticks(), 0);

        // Still, within the slop.
        touch(&mut shell, PointerKind::Down, 500.0, rested + 100);
        touch(&mut shell, PointerKind::Move, 508.0, rested + 108);
        assert!(held(&mut shell));
        assert_eq!(ticks(), 1);
    }

    #[test]
    fn after_a_long_press_the_finger_moves_nothing_until_it_lifts() {
        // HOME-10, REC-07: in Recent, which has no menu to cover the views.
        let fakes = Fakes::default();
        *fakes.usage.access.lock().unwrap() = meegone_core_data::Access::Granted;
        *fakes.apps.apps.lock().unwrap() = vec![fakes::app("mail", "Mail")];
        *fakes.system.now_ms.lock().unwrap() = 1_000_000;
        *fakes.usage.uses.lock().unwrap() = vec![fakes::used("mail", 900_000)];
        let mut shell = laid_out(&fakes, false);
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        settle(&mut shell, 0);
        touch(&mut shell, PointerKind::Down, 500.0, 1000);
        shell.dispatch(ShellIntent::LongPress);
        assert!(shell.recent().state().close_mode);

        touch(&mut shell, PointerKind::Move, 100.0, 1008);
        touch(&mut shell, PointerKind::Move, 50.0, 1016);
        let carousel = shell.state().carousel;
        assert!(carousel.held);
        assert!(!carousel.dragging);
        assert_eq!(carousel.offset, 0.0);

        touch(&mut shell, PointerKind::Up, 50.0, 1024);
        assert!(!shell.state().carousel.held);
        drag(&mut shell, 600.0, 300.0, 2000, 80);
        assert!(shell.state().carousel.dragging);
    }

    #[test]
    fn opening_settings_ends_a_drag() {
        // Settings covers the home views; touches after it opens belong to it.
        let mut shell = shell();
        let moved = drag(&mut shell, 600.0, 300.0, 0, 80);

        shell.dispatch(ShellIntent::OpenSettings);
        assert!(!shell.state().carousel.dragging);
        touch(&mut shell, PointerKind::Down, 300.0, moved + 8);
        assert!(!shell.state().carousel.dragging);
        assert_eq!(settled_on(&mut shell, moved + 16), HomeView::Applications);

        shell.dispatch(ShellIntent::Back);
        drag(&mut shell, 600.0, 300.0, 2000, 80);
        assert!(shell.state().carousel.dragging);
    }
}
