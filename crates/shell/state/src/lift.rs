//! The picked-up icon (APPS-11, APPS-12, ANIM-13; ADR 0013, ADR 0014). A long-press on an app's
//! icon opens its menu and keeps the rest of the gesture. If the finger then moves past the
//! touch slop, the icon is picked up: edit mode starts, and the icon lifts to 110 % and follows
//! the finger.
//!
//! - **Taking a place.** The place under the icon's centre becomes its own once the centre has
//!   stayed there for [`REORDER_DWELL`], and the others move aside. A little hysteresis keeps a
//!   finger resting on the line between two cells from flipping between them.
//! - **Auto-scroll.** Near the top or bottom of the visible grid, the grid scrolls, faster the
//!   deeper the icon is, until it can go no further. An icon picked up near an edge scrolls only
//!   once it has been away from that edge.
//! - **Dropping.** When the finger lifts, the icon takes the place under it and settles into its
//!   cell on the [`DROP`] spring, to the size edit mode draws icons at.
//!
//! Like the icon press, it is pure: the host passes each touch and frame, with its time
//! (ADR 0006), and the shell passes the grid as it is now ([`DragGrid`]). The icon is drawn from
//! its cell's *current* place, which moves as it takes new ones, so it stays under the finger.

use std::time::Duration;

use meegone_apps_state::GridLayout;
use meegone_core_state::AppId;
use meegone_core_state::motion::{
    AUTO_SCROLL_MAX_DP, AUTO_SCROLL_ZONE_DP, DRAG_LIFT_SCALE, DROP, REORDER_DWELL, SpringMotion,
};

use crate::carousel::{PointerEvent, PointerKind};
use crate::grid_change::MAX_FRAME_STEP;

/// A place stays the icon's target while its centre is this far, as a fraction of a cell, past
/// the place's edges.
const HYSTERESIS: f64 = 0.15;
/// Closer than this to where it is going, in px, and slower than [`REST_SPEED_PX`] per second, a
/// settling icon has landed.
const REST_DISTANCE_PX: f64 = 0.5;
const REST_SPEED_PX: f64 = 30.0;
/// The same for its scale, which is about 1.
const REST_DISTANCE_SCALE: f64 = 0.001;
const REST_SPEED_SCALE: f64 = 0.05;

/// The lifted icon as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiftState {
    /// The app whose icon is lifted or settling, or `None` while no icon is.
    pub app: Option<AppId>,
    /// How far the icon is drawn from its cell's place now, in pixels.
    pub dx: f64,
    pub dy: f64,
    /// The icon's scale: 1.1 while it is lifted (ANIM-13), springing to the size edit mode
    /// draws icons at as it settles. It takes the place of edit mode's scale.
    pub scale: f64,
    /// The finger has lifted, and the icon is settling into its cell (ANIM-13).
    pub settling: bool,
    /// It is moving, waiting to take a place, or scrolling the grid, so the host should draw
    /// another frame.
    pub animating: bool,
}

impl Default for LiftState {
    /// No icon lifted.
    fn default() -> Self {
        Self {
            app: None,
            dx: 0.0,
            dy: 0.0,
            scale: 1.0,
            settling: false,
            animating: false,
        }
    }
}

/// The Applications grid as a drag needs it, as it is now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DragGrid {
    /// How the host laid the grid out, or `None` until it has: then the icon only follows the
    /// finger.
    pub layout: Option<GridLayout>,
    /// Right to left mirrors the columns (L10N-02).
    pub rtl: bool,
    /// How far the grid is scrolled, in whole pixels, as the host draws it.
    pub scroll_px: f64,
    /// How many cells it has, and rows, the last one empty.
    pub count: usize,
    pub rows: usize,
    /// The place of the dragged app's own cell now, if it has one.
    pub place: Option<usize>,
}

/// What a touch did to the lift.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LiftEvent {
    /// The icon was picked up: edit mode starts.
    PickedUp,
    /// The finger lifted: `app` takes place `to`, if the grid is laid out, and then the shell
    /// lets it [`Lift::settle`].
    Dropped { app: AppId, to: Option<usize> },
}

/// What a frame asks of the grid while an icon is lifted.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LiftStep {
    /// The icon has stayed over this place long enough to take it.
    pub arrange: Option<(AppId, usize)>,
    /// Auto-scroll moved the grid here, in px from its top.
    pub scroll_to: Option<f64>,
}

/// A value springing to `target` on the [`DROP`] spring.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Toward {
    motion: SpringMotion,
    target: f64,
    /// The time the motion has reached, or `None` until the first frame.
    last: Option<Duration>,
}

impl Toward {
    fn new(from: f64, target: f64) -> Self {
        Self {
            motion: SpringMotion::new(DROP, from - target, 0.0),
            target,
            last: None,
        }
    }

    /// Advances to `at`. Spring time runs slower by the animation scale, and with animations
    /// removed it is at its target at once (ANIM-05).
    fn advance(&mut self, at: Duration, animation_scale: f64) {
        if animation_scale <= 0.0 {
            self.motion = SpringMotion::new(DROP, 0.0, 0.0);
            return;
        }
        if let Some(last) = self.last {
            self.motion
                .advance(at.saturating_sub(last).div_f64(animation_scale));
        }
        self.last = Some(self.last.map_or(at, |last| last.max(at)));
    }

    fn value(&self) -> f64 {
        self.target + self.motion.displacement()
    }

    fn at_rest(&self, distance: f64, speed: f64) -> bool {
        self.motion.is_at_rest(distance, speed)
    }

    /// Lands on the target once within `distance` of it and slower than `speed`, so what is
    /// drawn at rest is exactly the target.
    fn land(&mut self, distance: f64, speed: f64) {
        if self.at_rest(distance, speed) {
            self.motion = SpringMotion::new(DROP, 0.0, 0.0);
        }
    }
}

/// An icon being dragged.
#[derive(Debug, Clone, PartialEq)]
struct Drag {
    app: AppId,
    /// Where the finger went down, and where it is now.
    origin: (f64, f64),
    finger: (f64, f64),
    /// The place it was picked up from, and how far the grid was scrolled then.
    picked: Option<usize>,
    scroll: f64,
    /// The place under its centre, and since when.
    over: Option<(usize, Duration)>,
    /// How fast it scrolls the grid, in px per second, down positive.
    speed: f64,
    /// Whether it has been away from the top and from the bottom edge since it was picked up,
    /// so it may scroll that way.
    may_scroll: (bool, bool),
    /// Its scale, from the cell's to 110 %.
    scale: Toward,
    /// How far it is drawn from its cell's place now.
    offset: (f64, f64),
    /// It is waiting to take a place, or still lifting.
    waiting: bool,
    /// The last frame's time, for auto-scroll.
    last_frame: Option<Duration>,
    /// The latest time seen.
    now: Duration,
}

#[derive(Debug, Clone, Default, PartialEq)]
enum Phase {
    #[default]
    Idle,
    /// A long-press on `app`'s icon opened its menu. The finger went down at `origin` and is
    /// still down.
    Armed { app: AppId, origin: (f64, f64) },
    /// The icon follows the finger.
    Lifted(Box<Drag>),
    /// The finger lifted, and the icon springs into its cell.
    Settling {
        app: AppId,
        x: Toward,
        y: Toward,
        scale: Toward,
    },
}

/// Picks an icon up after a long-press, and drags it. `HomeShell` owns it and publishes its
/// [`LiftState`].
#[derive(Debug, Default)]
pub(crate) struct Lift {
    phase: Phase,
}

impl Lift {
    /// A long-press that went down at `origin` opened `app`'s menu, and the finger is still down.
    pub fn arm(&mut self, app: AppId, origin: (f64, f64)) {
        self.phase = Phase::Armed { app, origin };
    }

    /// Follows the finger that made the long-press. A move past `slop` from where it went down,
    /// either way, picks the icon up from its cell, drawn at `scale`. Lifting the finger drops
    /// it; the system cancelling the gesture puts it back. A new finger lands a settling icon at
    /// once.
    pub fn pointer(
        &mut self,
        event: &PointerEvent,
        slop: f64,
        grid: &DragGrid,
        scale: f64,
    ) -> Option<LiftEvent> {
        let finger = (event.x, event.y);
        match (event.kind, &mut self.phase) {
            (PointerKind::Move, Phase::Armed { app, origin }) => {
                let (dx, dy) = (finger.0 - origin.0, finger.1 - origin.1);
                if dx.abs() <= slop && dy.abs() <= slop {
                    return None;
                }
                let mut drag = Drag {
                    app: app.clone(),
                    origin: *origin,
                    finger: *origin,
                    picked: grid.place,
                    scroll: grid.scroll_px,
                    over: None,
                    speed: 0.0,
                    may_scroll: (false, false),
                    scale: Toward::new(scale, DRAG_LIFT_SCALE),
                    offset: (dx, dy),
                    waiting: true,
                    last_frame: None,
                    now: event.at,
                };
                // Where it was picked up decides which edges it may scroll towards at once.
                drag.track(grid);
                drag.finger = finger;
                drag.track(grid);
                self.phase = Phase::Lifted(Box::new(drag));
                Some(LiftEvent::PickedUp)
            }
            (PointerKind::Move, Phase::Lifted(drag)) => {
                drag.finger = finger;
                drag.now = drag.now.max(event.at);
                drag.track(grid);
                None
            }
            (PointerKind::Up, Phase::Lifted(drag)) => {
                drag.finger = finger;
                drag.now = drag.now.max(event.at);
                drag.track(grid);
                // It takes the place under it now, whether or not it has waited there.
                let to = drag.over.map(|(place, _)| place).or(grid.place);
                Some(LiftEvent::Dropped {
                    app: drag.app.clone(),
                    to: grid.layout.and(to),
                })
            }
            (PointerKind::Move, Phase::Idle | Phase::Settling { .. }) => None,
            (PointerKind::Down | PointerKind::Up | PointerKind::Cancel, _) => {
                self.phase = Phase::Idle;
                None
            }
        }
    }

    /// The dropped icon settles into its cell's place in `grid`, from where it is drawn now, to
    /// `rest_scale`, the size edit mode draws icons at (ANIM-13). With animations removed, it is
    /// there at once.
    pub fn settle(&mut self, grid: &DragGrid, rest_scale: f64, animation_scale: f64) {
        let Phase::Lifted(drag) = &mut self.phase else {
            return;
        };
        drag.track(grid);
        if animation_scale <= 0.0 {
            self.phase = Phase::Idle;
            return;
        }
        let (dx, dy) = drag.offset;
        self.phase = Phase::Settling {
            app: drag.app.clone(),
            x: Toward::new(dx, 0.0),
            y: Toward::new(dy, 0.0),
            scale: Toward::new(drag.scale.value(), rest_scale),
        };
    }

    /// Advances to the display's frame time (ANIM-04): the lift and the drop's springs, the
    /// wait before a new place is taken, and auto-scroll.
    pub fn frame(&mut self, at: Duration, grid: &DragGrid, animation_scale: f64) -> LiftStep {
        match &mut self.phase {
            Phase::Lifted(drag) => drag.frame(at, grid, animation_scale),
            Phase::Settling { x, y, scale, .. } => {
                for motion in [&mut *x, &mut *y] {
                    motion.advance(at, animation_scale);
                }
                scale.advance(at, animation_scale);
                let landed = x.at_rest(REST_DISTANCE_PX, REST_SPEED_PX)
                    && y.at_rest(REST_DISTANCE_PX, REST_SPEED_PX)
                    && scale.at_rest(REST_DISTANCE_SCALE, REST_SPEED_SCALE);
                if landed {
                    self.phase = Phase::Idle;
                }
                LiftStep::default()
            }
            Phase::Idle | Phase::Armed { .. } => LiftStep::default(),
        }
    }

    /// Takes on the grid as it is now, after anything else changed it, such as a new place or a
    /// scroll.
    pub fn sync(&mut self, grid: &DragGrid) {
        if let Phase::Lifted(drag) = &mut self.phase {
            drag.track(grid);
        }
    }

    /// The app whose menu a long-press opened, while its finger is still down and the icon
    /// isn't lifted yet.
    pub fn armed(&self) -> Option<&AppId> {
        match &self.phase {
            Phase::Armed { app, .. } => Some(app),
            Phase::Idle | Phase::Lifted(_) | Phase::Settling { .. } => None,
        }
    }

    /// The app whose icon is lifted or settling.
    pub fn app(&self) -> Option<&AppId> {
        match &self.phase {
            Phase::Lifted(drag) => Some(&drag.app),
            Phase::Settling { app, .. } => Some(app),
            Phase::Idle | Phase::Armed { .. } => None,
        }
    }

    /// Whether an icon follows the finger.
    pub fn is_lifted(&self) -> bool {
        matches!(self.phase, Phase::Lifted(_))
    }

    /// Puts the icon back in its cell at once, or forgets the long-press, such as when the
    /// layout changes or the menu closes before a drag.
    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
    }

    pub fn state(&self) -> LiftState {
        match &self.phase {
            Phase::Idle | Phase::Armed { .. } => LiftState::default(),
            Phase::Lifted(drag) => LiftState {
                app: Some(drag.app.clone()),
                dx: drag.offset.0,
                dy: drag.offset.1,
                scale: drag.scale.value(),
                settling: false,
                animating: drag.waiting || drag.speed != 0.0,
            },
            Phase::Settling { app, x, y, scale } => LiftState {
                app: Some(app.clone()),
                dx: x.value(),
                dy: y.value(),
                scale: scale.value(),
                settling: true,
                animating: true,
            },
        }
    }
}

impl Drag {
    fn frame(&mut self, at: Duration, grid: &DragGrid, animation_scale: f64) -> LiftStep {
        self.now = self.now.max(at);
        self.scale.advance(at, animation_scale);
        self.scale.land(REST_DISTANCE_SCALE, REST_SPEED_SCALE);
        let mut step = LiftStep::default();
        // A late frame scrolls no further than a frame or two would have.
        let elapsed = self
            .last_frame
            .map_or(Duration::ZERO, |last| at.saturating_sub(last))
            .min(MAX_FRAME_STEP);
        self.last_frame = Some(self.last_frame.map_or(at, |last| last.max(at)));
        if let Some(layout) = grid.layout.filter(|_| self.speed != 0.0) {
            let max = layout.max_scroll(grid.rows);
            let to = (grid.scroll_px + self.speed * elapsed.as_secs_f64()).clamp(0.0, max);
            if to != grid.scroll_px {
                step.scroll_to = Some(to);
            }
        }
        self.track(grid);
        if let Some((place, since)) = self.over {
            if grid.place != Some(place) && at.saturating_sub(since) >= REORDER_DWELL {
                step.arrange = Some((self.app.clone(), place));
            }
        }
        step
    }

    /// Works out, from the finger and the grid now, where the icon is drawn, the place under
    /// its centre, and how fast it scrolls the grid.
    fn track(&mut self, grid: &DragGrid) {
        let moved = (
            self.finger.0 - self.origin.0,
            self.finger.1 - self.origin.1 + grid.scroll_px - self.scroll,
        );
        self.offset = moved;
        self.speed = 0.0;
        let lifting = !self.scale.at_rest(REST_DISTANCE_SCALE, REST_SPEED_SCALE);
        let (Some(layout), Some(picked)) = (grid.layout, self.picked) else {
            self.over = None;
            self.waiting = lifting;
            return;
        };
        // Drawn from its cell's place now, which moves as it takes new ones.
        if let Some(place) = grid.place {
            self.offset.0 -= layout.cell_left(place, grid.rtl) - layout.cell_left(picked, grid.rtl);
            self.offset.1 -= layout.cell_top(place) - layout.cell_top(picked);
        }
        // Its centre, in px from the cells' top left.
        let row = layout.row_height_px.round();
        let centre = (
            layout.cell_left(picked, grid.rtl)
                + layout.cell_width(picked, grid.rtl) / 2.0
                + moved.0,
            layout.cell_top(picked) + row / 2.0 + moved.1,
        );
        self.over_place(&layout, grid, centre);
        self.speed = self.scroll_speed(&layout, grid, centre.1);
        let waiting = self
            .over
            .is_some_and(|(place, _)| grid.place != Some(place));
        self.waiting = lifting || waiting;
    }

    /// The place under `centre`. The place it is over now stays while the centre is within
    /// [`HYSTERESIS`] of it; a new one starts its wait now.
    fn over_place(&mut self, layout: &GridLayout, grid: &DragGrid, centre: (f64, f64)) {
        let Some(under) = layout.place_at(centre.0, centre.1, grid.count, grid.rtl) else {
            self.over = None;
            return;
        };
        if let Some((place, _)) = self.over {
            if place == under || near(layout, grid.rtl, place, centre) {
                return;
            }
        }
        self.over = Some((under, self.now));
    }

    /// How fast the grid scrolls, in px per second, with the icon's centre `y` px below the
    /// cells' top: 0 outside the zones at the visible grid's top and bottom, up to
    /// [`AUTO_SCROLL_MAX_DP`] at its very edge, and 0 once the grid can go no further.
    fn scroll_speed(&mut self, layout: &GridLayout, grid: &DragGrid, y: f64) -> f64 {
        if layout.height_px <= 0.0 {
            return 0.0;
        }
        let zone = AUTO_SCROLL_ZONE_DP * layout.density;
        // From the top of the visible grid, below the safe area.
        let on_screen = y - grid.scroll_px;
        let visible = layout.height_px - layout.top_px - layout.bottom_px;
        let up = ((zone - on_screen) / zone).min(1.0);
        let down = ((on_screen - (visible - zone)) / zone).min(1.0);
        // Picked up near an edge, it scrolls only once it has been away from it.
        self.may_scroll.0 |= up <= 0.0;
        self.may_scroll.1 |= down <= 0.0;
        let max = AUTO_SCROLL_MAX_DP * layout.density;
        let speed = if up > 0.0 && self.may_scroll.0 {
            -max * up
        } else if down > 0.0 && self.may_scroll.1 {
            max * down
        } else {
            0.0
        };
        let stuck = (speed < 0.0 && grid.scroll_px <= 0.0)
            || (speed > 0.0 && grid.scroll_px >= layout.max_scroll(grid.rows));
        if stuck { 0.0 } else { speed }
    }
}

/// Whether `point` is within [`HYSTERESIS`] of cell `place`.
fn near(layout: &GridLayout, rtl: bool, place: usize, point: (f64, f64)) -> bool {
    let (left, top) = (layout.cell_left(place, rtl), layout.cell_top(place));
    let (width, height) = (layout.cell_width(place, rtl), layout.row_height_px.round());
    let (x, y) = (width * HYSTERESIS, height * HYSTERESIS);
    (left - x..=left + width + x).contains(&point.0)
        && (top - y..=top + height + y).contains(&point.1)
}

#[cfg(test)]
mod tests {
    use meegone_core_state::ProfileId;
    use meegone_core_state::motion::MAX_OVERSHOOT;

    use super::*;

    fn mail() -> AppId {
        AppId {
            profile: ProfileId(0),
            package: "mail".into(),
            activity: "mail.Main".into(),
        }
    }

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn event(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
        at(kind, x, y, 0)
    }

    fn at(kind: PointerKind, x: f64, y: f64, millis: u64) -> PointerEvent {
        PointerEvent {
            kind,
            x,
            y,
            at: ms(millis),
        }
    }

    const SLOP: f64 = 16.0;

    /// Not laid out yet: the icon only follows the finger.
    fn unlaid() -> DragGrid {
        DragGrid {
            layout: None,
            rtl: false,
            scroll_px: 0.0,
            count: 8,
            rows: 3,
            place: Some(5),
        }
    }

    /// 4 columns of 250 px, rows of 300 px, 100 px of safe area above and below, 1000 px tall:
    /// 800 px of grid show. 40 cells, so 11 rows, scroll up to 2500 px. 2.5 px per dp, so the
    /// auto-scroll zones are 160 px deep, and it scrolls up to 1600 px a second.
    fn laid_out(place: usize, scroll_px: f64) -> DragGrid {
        DragGrid {
            layout: Some(GridLayout {
                width_px: 1000.0,
                row_height_px: 300.0,
                density: 2.5,
                top_px: 100.0,
                bottom_px: 100.0,
                height_px: 1000.0,
            }),
            rtl: false,
            scroll_px,
            count: 40,
            rows: 11,
            place: Some(place),
        }
    }

    /// Mail picked up from place 5 (row 1, column 1) by a finger that went down at (300, 400)
    /// and moved to (`x`, `y`).
    fn picked_up(grid: &DragGrid, x: f64, y: f64) -> Lift {
        let mut lift = Lift::default();
        lift.arm(mail(), (300.0, 400.0));
        assert_eq!(
            lift.pointer(&event(PointerKind::Move, x, y), SLOP, grid, 1.0),
            Some(LiftEvent::PickedUp)
        );
        lift
    }

    #[test]
    fn moving_past_the_slop_picks_the_icon_up_and_it_follows_the_finger() {
        // APPS-11, ANIM-13
        let grid = unlaid();
        let mut lift = Lift::default();
        lift.arm(mail(), (100.0, 200.0));
        assert_eq!(lift.armed(), Some(&mail()));
        assert_eq!(lift.state(), LiftState::default());

        // Within the slop, either way, it stays a long-press.
        let within = event(PointerKind::Move, 116.0, 184.0);
        assert_eq!(lift.pointer(&within, SLOP, &grid, 1.0), None);
        assert!(!lift.is_lifted());

        let past = event(PointerKind::Move, 100.0, 217.0);
        assert_eq!(
            lift.pointer(&past, SLOP, &grid, 1.0),
            Some(LiftEvent::PickedUp)
        );
        assert!(lift.is_lifted());
        assert_eq!(lift.armed(), None);
        let state = lift.state();
        assert_eq!((state.app, state.dx, state.dy), (Some(mail()), 0.0, 17.0));
        // It lifts from its cell's size to 110 %.
        assert_eq!(state.scale, 1.0);
        assert!(state.animating);

        // Picked up once: later moves only follow.
        let far = event(PointerKind::Move, 40.0, 500.0);
        assert_eq!(lift.pointer(&far, SLOP, &grid, 1.0), None);
        let state = lift.state();
        assert_eq!((state.dx, state.dy), (-60.0, 300.0));
    }

    #[test]
    fn the_icon_lifts_to_110_percent_on_a_spring() {
        // ANIM-13: from edit mode's 92 %.
        let grid = unlaid();
        let mut lift = Lift::default();
        lift.arm(mail(), (0.0, 0.0));
        lift.pointer(&event(PointerKind::Move, 0.0, 50.0), SLOP, &grid, 0.92);
        assert_eq!(lift.state().scale, 0.92);

        lift.frame(ms(0), &grid, 1.0);
        lift.frame(ms(60), &grid, 1.0);
        let midway = lift.state().scale;
        assert!(midway > 0.92 && midway < DRAG_LIFT_SCALE, "{midway}");
        for t in (60..=400).step_by(8) {
            lift.frame(ms(t), &grid, 1.0);
        }
        let state = lift.state();
        assert!(
            (state.scale - DRAG_LIFT_SCALE).abs() < 0.001,
            "{}",
            state.scale
        );
        // Lifted and still, with nothing to wait for: no more frames.
        assert!(!state.animating);
    }

    #[test]
    fn the_cancel_or_a_new_finger_puts_the_icon_back() {
        for end in [PointerKind::Cancel, PointerKind::Down] {
            let grid = unlaid();
            let mut lift = picked_up(&grid, 500.0, 400.0);

            assert_eq!(
                lift.pointer(&event(end, 300.0, 200.0), SLOP, &grid, 1.0),
                None
            );

            assert_eq!(lift.state(), LiftState::default());
            assert!(!lift.is_lifted());
            // A later move picks nothing up.
            let later = event(PointerKind::Move, 0.0, 0.0);
            assert_eq!(lift.pointer(&later, SLOP, &grid, 1.0), None);
        }
    }

    #[test]
    fn a_long_press_that_ends_without_a_drag_picks_nothing_up() {
        let grid = unlaid();
        let mut lift = Lift::default();
        lift.arm(mail(), (100.0, 200.0));

        lift.pointer(&event(PointerKind::Up, 100.0, 200.0), SLOP, &grid, 1.0);

        assert_eq!(lift.armed(), None);
        let later = event(PointerKind::Move, 300.0, 200.0);
        assert_eq!(lift.pointer(&later, SLOP, &grid, 1.0), None);
    }

    #[test]
    fn cancelling_puts_it_back_at_once() {
        let grid = unlaid();
        let mut lift = picked_up(&grid, 350.0, 400.0);

        lift.cancel();

        assert_eq!(lift.state(), LiftState::default());
    }

    #[test]
    fn a_place_the_icon_stays_over_becomes_its_own() {
        // APPS-12: from place 5 to place 1, a row up, after REORDER_DWELL.
        let grid = laid_out(5, 0.0);
        let mut lift = picked_up(&grid, 300.0, 100.0);
        let step = lift.frame(ms(0), &grid, 1.0);
        assert_eq!(step.arrange, None);
        assert!(lift.state().animating);

        let step = lift.frame(REORDER_DWELL - ms(1), &grid, 1.0);
        assert_eq!(step.arrange, None);
        let step = lift.frame(REORDER_DWELL, &grid, 1.0);
        assert_eq!(step.arrange, Some((mail(), 1)));

        // Once the cell is there, it is drawn from its new place, still under the finger.
        let moved = laid_out(1, 0.0);
        lift.sync(&moved);
        let state = lift.state();
        assert_eq!((state.dx, state.dy), (0.0, 0.0));
        assert_eq!(lift.frame(REORDER_DWELL + ms(8), &moved, 1.0).arrange, None);
    }

    #[test]
    fn crossing_cells_quickly_takes_no_place_on_the_way() {
        // APPS-12: each new place starts its wait again.
        let grid = laid_out(5, 0.0);
        let mut lift = picked_up(&grid, 600.0, 400.0);
        let mut taken = Vec::new();
        for (i, x) in [600.0, 850.0, 600.0, 350.0].into_iter().enumerate() {
            let t = 100 * i as u64;
            lift.pointer(&at(PointerKind::Move, x, 400.0, t), SLOP, &grid, 1.0);
            taken.extend(lift.frame(ms(t + 99), &grid, 1.0).arrange);
        }
        assert!(taken.is_empty(), "{taken:?}");
    }

    #[test]
    fn a_finger_resting_on_a_line_between_cells_keeps_its_place() {
        // The line between places 6 and 7 is at x = 750; the centre is 75 px right of the
        // finger. A little either side of it stays over 6, which keeps its wait.
        let grid = laid_out(5, 0.0);
        let mut lift = picked_up(&grid, 670.0, 400.0);
        lift.frame(ms(0), &grid, 1.0);
        for (i, x) in [680.0, 672.0, 700.0, 690.0].into_iter().enumerate() {
            let t = 40 * (i as u64 + 1);
            lift.pointer(&at(PointerKind::Move, x, 400.0, t), SLOP, &grid, 1.0);
        }
        assert_eq!(lift.frame(ms(200), &grid, 1.0).arrange, Some((mail(), 6)));
    }

    #[test]
    fn near_an_edge_the_grid_scrolls_faster_the_deeper_and_stops_at_its_end() {
        // APPS-12: the bottom zone is the last 160 px of the 800 px that show.
        let grid = laid_out(5, 0.0);
        // The centre at y = 450 + 200 = 650 from the cells' top: 10 px into the zone.
        let mut lift = picked_up(&grid, 300.0, 600.0);
        lift.frame(ms(0), &grid, 1.0);
        let slow = lift.frame(ms(16), &grid, 1.0).scroll_to.unwrap();
        assert!((slow - 100.0 * 0.016).abs() < 0.01, "{slow}");

        lift.pointer(&at(PointerKind::Move, 300.0, 900.0, 16), SLOP, &grid, 1.0);
        let fast = lift.frame(ms(32), &grid, 1.0).scroll_to.unwrap();
        // At the edge itself, and past it: 640 dp a second.
        assert!((fast - 1600.0 * 0.016).abs() < 0.01, "{fast}");
        assert!(lift.state().animating);

        // A late frame scrolls no further than two would.
        let late = lift.frame(ms(500), &grid, 1.0).scroll_to.unwrap();
        assert!((late - 1600.0 * 0.034).abs() < 0.01, "{late}");

        // At the end it scrolls no further, and the icon stays under the finger.
        let end = laid_out(5, 2500.0);
        lift.sync(&end);
        assert_eq!(lift.frame(ms(516), &end, 1.0).scroll_to, None);
        let state = lift.state();
        assert_eq!((state.dx, state.dy), (0.0, 500.0 + 2500.0));

        // Over the last cell, it takes its place, and then asks for no more frames.
        let step = lift.frame(ms(650), &end, 1.0);
        assert_eq!(step.arrange, Some((mail(), 39)));
        let last = laid_out(39, 2500.0);
        lift.sync(&last);
        let state = lift.state();
        assert!(!state.animating, "{state:?}");
        assert_eq!((state.dx, state.dy), (-500.0, 600.0));
    }

    #[test]
    fn the_grid_scrolls_up_near_the_top_but_not_past_it() {
        let grid = laid_out(5, 200.0);
        // Place 5 shows 250 px down the visible grid; the finger moves up 1000 px.
        let mut lift = picked_up(&grid, 300.0, -600.0);
        lift.frame(ms(0), &grid, 1.0);
        let up = lift.frame(ms(16), &grid, 1.0).scroll_to.unwrap();
        assert!((up - (200.0 - 1600.0 * 0.016)).abs() < 0.01, "{up}");
        let top = laid_out(5, 0.0);
        lift.sync(&top);
        assert_eq!(lift.frame(ms(32), &top, 1.0).scroll_to, None);
    }

    #[test]
    fn an_icon_picked_up_near_an_edge_scrolls_only_once_it_has_been_away() {
        // Place 9, in the third row, its centre 750 px down: inside the bottom zone already.
        let mut grid = laid_out(9, 0.0);
        grid.place = Some(9);
        let mut lift = Lift::default();
        lift.arm(mail(), (300.0, 800.0));
        lift.pointer(&event(PointerKind::Move, 300.0, 820.0), SLOP, &grid, 1.0);
        lift.frame(ms(0), &grid, 1.0);
        assert_eq!(lift.frame(ms(16), &grid, 1.0).scroll_to, None);

        // Up and out of the zone, then back in.
        lift.pointer(&at(PointerKind::Move, 300.0, 600.0, 20), SLOP, &grid, 1.0);
        lift.pointer(&at(PointerKind::Move, 300.0, 840.0, 30), SLOP, &grid, 1.0);
        assert!(lift.frame(ms(48), &grid, 1.0).scroll_to.is_some());
    }

    #[test]
    fn lifting_the_finger_takes_the_place_under_it_and_settles_there() {
        // ANIM-13: dropped over place 1 before its wait was over.
        let grid = laid_out(5, 0.0);
        let mut lift = picked_up(&grid, 300.0, 100.0);
        lift.frame(ms(0), &grid, 1.0);

        let dropped = lift.pointer(&at(PointerKind::Up, 320.0, 90.0, 50), SLOP, &grid, 1.0);
        assert_eq!(
            dropped,
            Some(LiftEvent::Dropped {
                app: mail(),
                to: Some(1)
            })
        );

        // The shell moved it there; it springs from where it is drawn to its new cell.
        let moved = laid_out(1, 0.0);
        lift.settle(&moved, 0.92, 1.0);
        let state = lift.state();
        assert!(state.settling && state.animating);
        assert_eq!((state.dx, state.dy), (20.0, -10.0));
        assert!(!lift.is_lifted());

        let mut overshoot: f64 = 0.0;
        let mut t = 50;
        while lift.state().app.is_some() {
            t += 8;
            lift.frame(ms(t), &moved, 1.0);
            overshoot = overshoot.max(-lift.state().dx);
            assert!(t < 600, "still settling");
        }
        // A short drop lands within half a pixel sooner than the spring's 220 ms, and
        // overshoots by about 1.5 % (ANIM-13, ANIM-02).
        assert!((120..=300).contains(&(t - 50)), "{t}");
        assert!(overshoot <= 20.0 * MAX_OVERSHOOT, "{overshoot}");
        assert_eq!(lift.state(), LiftState::default());
    }

    #[test]
    fn with_animations_removed_a_dropped_icon_is_in_its_cell_at_once() {
        // ANIM-05
        let grid = laid_out(5, 0.0);
        let mut lift = picked_up(&grid, 300.0, 100.0);
        lift.pointer(&event(PointerKind::Up, 300.0, 100.0), SLOP, &grid, 1.0);

        lift.settle(&laid_out(1, 0.0), 0.92, 0.0);

        assert_eq!(lift.state(), LiftState::default());
    }

    #[test]
    fn a_new_finger_lands_a_settling_icon_at_once() {
        let grid = unlaid();
        let mut lift = picked_up(&grid, 500.0, 400.0);
        let dropped = lift.pointer(&event(PointerKind::Up, 500.0, 400.0), SLOP, &grid, 1.0);
        // Not laid out: it takes no place, and settles back.
        assert_eq!(
            dropped,
            Some(LiftEvent::Dropped {
                app: mail(),
                to: None
            })
        );
        lift.settle(&grid, 1.0, 1.0);
        assert!(lift.state().settling);

        lift.pointer(&event(PointerKind::Down, 0.0, 0.0), SLOP, &grid, 1.0);

        assert_eq!(lift.state(), LiftState::default());
    }

    #[test]
    fn right_to_left_mirrors_the_places() {
        // L10N-02: moving left from place 5 is moving to a later place.
        let mut grid = laid_out(5, 0.0);
        grid.rtl = true;
        let mut lift = picked_up(&grid, 50.0, 400.0);
        lift.frame(ms(0), &grid, 1.0);
        assert_eq!(
            lift.frame(REORDER_DWELL, &grid, 1.0).arrange,
            Some((mail(), 6))
        );
    }
}
