//! The dragged icon (APPS-11, APPS-12, FOLD-01, FOLD-05, ANIM-13, ANIM-14; ADR 0013 to
//! ADR 0015). A long-press on an app's or a folder's icon, in the grid or in the open folder,
//! opens its menu and keeps the rest of the gesture. If the finger then moves past the touch
//! slop, the icon is picked up: edit mode starts, and the icon lifts to 110 % and follows the
//! finger, drawn above everything by the host, while its cell stays as a gap.
//!
//! - **Where the icon's centre is.** Over another cell's icon, an app *hovers* over it: the icon
//!   grows and, after [`FOLDER_HOVER`], pulses, armed. Anywhere else in a cell, the place becomes
//!   the icon's own once the centre has stayed there for [`REORDER_DWELL`], and the others move
//!   aside. Each change of place or zone starts its wait again. Folders, and apps in the open
//!   folder, only take places: a folder never goes into anything.
//! - **Leaving the folder.** An app dragged out of the open folder's panel comes out of the
//!   folder, at the grid's place under it, and the drag goes on in the grid.
//! - **Auto-scroll.** Near the top or bottom of the visible grid or panel, it scrolls, faster
//!   the deeper the icon is, until it can go no further.
//! - **Dropping.** Over an armed icon, the app goes into that folder, or makes one with that
//!   app, and shrinks into the folder's preview (ANIM-14). Over a place, the icon takes it and
//!   settles into its cell on the [`DROP`] spring. Over an icon not yet armed, it settles back
//!   into its own gap.
//!
//! Like the icon press, it is pure: the host passes each touch and frame, with its time
//! (ADR 0006), and the shell passes the grid and the open folder as they are now
//! ([`DragSurfaces`]). The icon is tracked by its centre on screen, so it stays under the
//! finger as cells move, the grid scrolls, or it changes surface.

use std::f64::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use meegone_apps_state::{
    Bounds, CellKey, FolderId, GridItem, GridLayout, PREVIEW_APPS, Surface, preview_slot,
};
use meegone_core_state::AppId;
use meegone_core_state::motion::{
    AUTO_SCROLL_MAX_DP, AUTO_SCROLL_ZONE_DP, DRAG_LIFT_SCALE, DROP, FOLDER, FOLDER_HOVER,
    FOLDER_HOVER_SCALE, FOLDER_PULSE, FOLDER_PULSE_SCALE, REORDER_DWELL, SpringMotion, decelerate,
    scaled,
};

use crate::carousel::{PointerEvent, PointerKind};
use crate::grid_change::MAX_FRAME_STEP;

/// A place stays the icon's target while its centre is this far, as a fraction of a cell, past
/// the place's edges. Small, so the gaps beside an icon still take its place.
const HYSTERESIS: f64 = 0.05;
/// Closer than this to where it is going, in px, and slower than [`REST_SPEED_PX`] per second, a
/// settling icon has landed.
const REST_DISTANCE_PX: f64 = 0.5;
const REST_SPEED_PX: f64 = 30.0;
/// The same for its scale, which is about 1.
const REST_DISTANCE_SCALE: f64 = 0.001;
const REST_SPEED_SCALE: f64 = 0.05;
/// An app dropped into a folder whose preview is full shrinks to this fraction of its size as it
/// fades into the folder's icon.
const MERGE_AWAY_SCALE: f64 = 0.3;

/// The dragged icon as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiftState {
    /// The app or folder whose icon is lifted, settling or merging, or `None` while none is.
    pub key: Option<CellKey>,
    /// Where its cell is: the grid, or the open folder's panel.
    pub surface: Surface,
    /// Where the icon's centre is on screen, in px from the home views' top left.
    pub x: f64,
    pub y: f64,
    /// Its scale: 1.1 while lifted (ANIM-13), springing to the size edit mode draws icons at as
    /// it settles, and shrinking into a folder's preview as it merges (ANIM-14).
    pub scale: f64,
    /// Its opacity, from 0 to 1: it fades into a folder whose preview is full.
    pub alpha: f64,
    /// The finger has lifted, and the icon is settling into its cell (ANIM-13).
    pub settling: bool,
    /// The finger has lifted over a folder or an app, and the icon is going into the folder
    /// (ANIM-14).
    pub merging: bool,
    /// It is moving, waiting, pulsing, or scrolling, so the host should draw another frame.
    pub animating: bool,
}

impl Default for LiftState {
    /// No icon lifted.
    fn default() -> Self {
        Self {
            key: None,
            surface: Surface::Grid,
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            alpha: 1.0,
            settling: false,
            merging: false,
            animating: false,
        }
    }
}

/// The icon a dragged app is held over, which grows and pulses (ANIM-14).
#[derive(Debug, Clone, PartialEq)]
pub struct HoverState {
    /// The app's or folder's cell in the grid, or `None` while no icon is held over.
    pub key: Option<CellKey>,
    /// Its icon's scale: from 1 to 1.1 while the drop would only put the app back, then
    /// pulsing, armed, while the drop would make a folder or add to one (FOLD-01).
    pub scale: f64,
    /// Dropping now would make a folder or add to one.
    pub armed: bool,
}

impl Default for HoverState {
    fn default() -> Self {
        Self {
            key: None,
            scale: 1.0,
            armed: false,
        }
    }
}

/// A folder taking in a dropped app (ANIM-14).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MergeState {
    /// The folder, while an app is shrinking into it.
    pub folder: Option<FolderId>,
    /// The preview slot the app shrinks into, which stays empty until it lands, or `None` if
    /// the preview is full.
    pub slot: Option<usize>,
    /// The folder was just made: its first app shrinks into its slot from full size as well.
    pub formed: bool,
    /// How far it has come, eased, from 0 to 1.
    pub progress: f64,
}

/// One surface as a drag needs it, as it is now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceView {
    /// How the host laid it out, or `None` until it has: then the icon only follows the finger.
    pub layout: Option<GridLayout>,
    /// How far it is scrolled, in whole pixels, as the host draws it.
    pub scroll_px: f64,
    pub cells: Arc<[GridItem]>,
    /// How many rows it lays out.
    pub rows: usize,
}

/// The grid and the open folder as a drag needs them, as they are now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DragSurfaces {
    pub grid: SurfaceView,
    /// The open folder's panel, once it is all the way open.
    pub folder: Option<SurfaceView>,
    /// Where the open folder's panel is on screen.
    pub folder_bounds: Option<Bounds>,
    /// Right to left mirrors the columns (L10N-02).
    pub rtl: bool,
    /// The scale cells are drawn at now, and the one edit mode draws them at.
    pub scale: f64,
    pub rest_scale: f64,
}

impl DragSurfaces {
    fn view(&self, surface: Surface) -> Option<&SurfaceView> {
        match surface {
            Surface::Grid => Some(&self.grid),
            Surface::Folder => self.folder.as_ref(),
        }
    }

    /// The place of `key`'s cell on `surface`.
    fn place(&self, surface: Surface, key: &CellKey) -> Option<usize> {
        self.view(surface)?
            .cells
            .iter()
            .position(|item| item.is(key))
    }

    /// Where cell `place`'s icon centre is on screen, its cell drawn at `scale`.
    fn icon_centre(&self, surface: Surface, place: usize, scale: f64) -> Option<(f64, f64)> {
        let view = self.view(surface)?;
        let layout = view.layout?;
        let (x, y) = layout.icon_centre(place, self.rtl, scale);
        let (left, top) = layout.origin(view.scroll_px);
        Some((left + x, top + y))
    }
}

/// What a touch did to the lift.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LiftEvent {
    /// The icon was picked up: edit mode starts.
    PickedUp,
    /// An app was dragged out of the open folder's panel: it comes out of the folder, at grid
    /// place `to`, and the drag goes on in the grid (FOLD-05).
    LeftFolder { app: AppId, to: usize },
    /// The finger lifted. The shell applies the drop, then lets the icon settle or merge.
    Dropped(DropAt),
}

/// Where a dragged icon was dropped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DropAt {
    /// At place `to` in the grid.
    Grid { key: CellKey, to: usize },
    /// At place `to` in the open folder.
    Folder { app: AppId, to: usize },
    /// Into the folder `onto`, or with the app `onto` into a new one (FOLD-01, FOLD-05).
    Combine { app: AppId, onto: CellKey },
    /// Back into its own gap.
    Back,
}

/// What a frame asks of the surfaces while an icon is lifted.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LiftStep {
    /// The icon has stayed over this place long enough to take it.
    pub arrange: Option<DropAt>,
    /// Auto-scroll moved this surface here, in px from its top.
    pub scroll_to: Option<(Surface, f64)>,
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

/// What the icon's centre is over.
#[derive(Debug, Clone, PartialEq)]
enum Over {
    /// A place on a surface, which it takes after a wait.
    Place(Surface, usize),
    /// Another cell's icon in the grid, which it goes into after a wait.
    Icon(CellKey),
}

/// An icon being dragged.
#[derive(Debug, Clone, PartialEq)]
struct Drag {
    key: CellKey,
    surface: Surface,
    /// From the finger to the icon's centre, as it was picked up.
    grab: (f64, f64),
    finger: (f64, f64),
    /// What its centre is over, and since when.
    over: Option<(Over, Duration)>,
    /// How fast it scrolls its surface, in px per second, down positive.
    speed: f64,
    /// Whether it has been away from the top and from the bottom edge since it was picked up,
    /// so it may scroll that way.
    may_scroll: (bool, bool),
    /// Its scale, from its cell's to 110 %.
    scale: Toward,
    /// It is waiting to take a place, hovering, or still lifting.
    waiting: bool,
    /// The last frame's time, for auto-scroll.
    last_frame: Option<Duration>,
    /// The latest time seen.
    now: Duration,
    animation_scale: f64,
}

/// An icon shrinking into a folder (ANIM-14).
#[derive(Debug, Clone, PartialEq)]
struct Merging {
    key: CellKey,
    merge: MergeState,
    /// Where it starts and ends: centre, scale and opacity.
    from: (f64, f64, f64, f64),
    to: (f64, f64, f64, f64),
    length: Duration,
    /// The last frame's time, or `None` until the first frame.
    last: Option<Duration>,
    played: Duration,
}

#[derive(Debug, Clone, Default, PartialEq)]
enum Phase {
    #[default]
    Idle,
    /// A long-press on `key`'s icon on `surface` opened its menu. The finger went down at
    /// `origin` and is still down.
    Armed {
        key: CellKey,
        surface: Surface,
        origin: (f64, f64),
    },
    /// The icon follows the finger.
    Lifted(Box<Drag>),
    /// The finger lifted, and the icon springs into its cell.
    Settling {
        key: CellKey,
        surface: Surface,
        x: Toward,
        y: Toward,
        scale: Toward,
    },
    /// The finger lifted over a folder or an app, and the icon shrinks into the folder.
    Merging(Box<Merging>),
}

/// Picks an icon up after a long-press, and drags it. `HomeShell` owns it and publishes its
/// [`LiftState`], [`HoverState`] and [`MergeState`].
#[derive(Debug, Default)]
pub(crate) struct Lift {
    phase: Phase,
}

impl Lift {
    /// A long-press on `key`'s icon on `surface`, which went down at `origin`, opened its menu,
    /// and the finger is still down.
    pub fn arm(&mut self, key: CellKey, surface: Surface, origin: (f64, f64)) {
        self.phase = Phase::Armed {
            key,
            surface,
            origin,
        };
    }

    /// Follows the finger that made the long-press. A move past `slop` from where it went down,
    /// either way, picks the icon up. A move out of the open folder's panel takes an app out of
    /// the folder. Lifting the finger drops it; the system cancelling the gesture puts it back.
    /// A new finger lands a settling or merging icon at once.
    pub fn pointer(
        &mut self,
        event: &PointerEvent,
        slop: f64,
        surfaces: &DragSurfaces,
        animation_scale: f64,
    ) -> Option<LiftEvent> {
        let finger = (event.x, event.y);
        match (event.kind, &mut self.phase) {
            (
                PointerKind::Move,
                Phase::Armed {
                    key,
                    surface,
                    origin,
                },
            ) => {
                let (dx, dy) = (finger.0 - origin.0, finger.1 - origin.1);
                if dx.abs() <= slop && dy.abs() <= slop {
                    return None;
                }
                let place = surfaces.place(*surface, key);
                let grab = place
                    .and_then(|place| surfaces.icon_centre(*surface, place, surfaces.scale))
                    .map_or((0.0, 0.0), |(x, y)| (x - origin.0, y - origin.1));
                let mut drag = Drag {
                    key: key.clone(),
                    surface: *surface,
                    grab,
                    finger: *origin,
                    over: None,
                    speed: 0.0,
                    may_scroll: (false, false),
                    scale: Toward::new(surfaces.scale, DRAG_LIFT_SCALE),
                    waiting: true,
                    last_frame: None,
                    now: event.at,
                    animation_scale,
                };
                // Where it was picked up decides which edges it may scroll towards at once.
                drag.track(surfaces);
                drag.over = None;
                drag.finger = finger;
                drag.track(surfaces);
                self.phase = Phase::Lifted(Box::new(drag));
                Some(LiftEvent::PickedUp)
            }
            (PointerKind::Move, Phase::Lifted(drag)) => {
                drag.finger = finger;
                drag.now = drag.now.max(event.at);
                if let Some(left) = drag.leave_folder(surfaces) {
                    return Some(left);
                }
                drag.track(surfaces);
                None
            }
            (PointerKind::Up, Phase::Lifted(drag)) => {
                drag.finger = finger;
                drag.now = drag.now.max(event.at);
                drag.track(surfaces);
                Some(LiftEvent::Dropped(drag.drop_at()))
            }
            (PointerKind::Move, _) => None,
            (PointerKind::Down | PointerKind::Up | PointerKind::Cancel, _) => {
                self.phase = Phase::Idle;
                None
            }
        }
    }

    /// The dropped icon settles into its cell on `surfaces`, from where it is drawn now, to the
    /// size edit mode draws icons at (ANIM-13). With animations removed, or if its cell can't
    /// be found on screen, it is there at once.
    pub fn settle(&mut self, surfaces: &DragSurfaces, animation_scale: f64) {
        let Phase::Lifted(drag) = &mut self.phase else {
            return;
        };
        let target = surfaces
            .place(drag.surface, &drag.key)
            .and_then(|place| surfaces.icon_centre(drag.surface, place, surfaces.rest_scale));
        let (Some((x, y)), true) = (target, animation_scale > 0.0) else {
            self.phase = Phase::Idle;
            return;
        };
        let (from_x, from_y) = drag.centre();
        self.phase = Phase::Settling {
            key: drag.key.clone(),
            surface: drag.surface,
            x: Toward::new(from_x, x),
            y: Toward::new(from_y, y),
            scale: Toward::new(drag.scale.value(), surfaces.rest_scale),
        };
    }

    /// The dropped app went into `folder`, at preview slot `slot`, and the folder was just made
    /// if `formed`: the icon shrinks into the folder's preview over 250 ms (ANIM-14), or into
    /// its icon, fading, when the preview is full. With animations removed, or if the folder
    /// can't be found, it is there at once.
    pub fn merge(
        &mut self,
        surfaces: &DragSurfaces,
        folder: FolderId,
        slot: usize,
        formed: bool,
        animation_scale: f64,
    ) {
        let Phase::Lifted(drag) = &self.phase else {
            return;
        };
        let length = scaled(FOLDER, animation_scale);
        let folder_key = CellKey::Folder(folder);
        let at = surfaces
            .place(Surface::Grid, &folder_key)
            .and_then(|place| {
                let layout = surfaces.grid.layout?;
                let centre = surfaces.icon_centre(Surface::Grid, place, surfaces.rest_scale)?;
                Some((centre, layout.icon_px * surfaces.rest_scale))
            });
        let (Some(((x, y), icon)), false) = (at, length.is_zero()) else {
            self.phase = Phase::Idle;
            return;
        };
        let slot = (slot < PREVIEW_APPS).then_some(slot);
        let to = match slot {
            Some(slot) => {
                let (fx, fy, size) = preview_slot(slot, surfaces.rtl);
                (
                    x + (fx - 0.5) * icon,
                    y + (fy - 0.5) * icon,
                    size * surfaces.rest_scale,
                    1.0,
                )
            }
            None => (x, y, MERGE_AWAY_SCALE * surfaces.rest_scale, 0.0),
        };
        let (from_x, from_y) = drag.centre();
        self.phase = Phase::Merging(Box::new(Merging {
            key: drag.key.clone(),
            merge: MergeState {
                folder: Some(folder),
                slot,
                formed,
                progress: 0.0,
            },
            from: (from_x, from_y, drag.scale.value(), 1.0),
            to,
            length,
            last: None,
            played: Duration::ZERO,
        }));
    }

    /// Advances to the display's frame time (ANIM-04): the lift's, the drop's and the merge's
    /// motion, the waits before a place is taken or an icon armed, and auto-scroll.
    pub fn frame(
        &mut self,
        at: Duration,
        surfaces: &DragSurfaces,
        animation_scale: f64,
    ) -> LiftStep {
        match &mut self.phase {
            Phase::Lifted(drag) => drag.frame(at, surfaces, animation_scale),
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
            Phase::Merging(merging) => {
                let last = *merging.last.get_or_insert(at);
                merging.played += at.saturating_sub(last).min(MAX_FRAME_STEP);
                merging.last = Some(at.max(last));
                let progress = merging.played.as_secs_f64() / merging.length.as_secs_f64();
                if progress >= 1.0 {
                    self.phase = Phase::Idle;
                } else {
                    merging.merge.progress = decelerate(progress);
                }
                LiftStep::default()
            }
            Phase::Idle | Phase::Armed { .. } => LiftStep::default(),
        }
    }

    /// Takes on the surfaces as they are now, after anything else changed them, such as a new
    /// place or a scroll.
    pub fn sync(&mut self, surfaces: &DragSurfaces) {
        if let Phase::Lifted(drag) = &mut self.phase {
            drag.track(surfaces);
        }
    }

    /// The app or folder whose menu a long-press opened, and its surface, while its finger is
    /// still down and the icon isn't lifted yet.
    pub fn armed(&self) -> Option<(&CellKey, Surface)> {
        match &self.phase {
            Phase::Armed { key, surface, .. } => Some((key, *surface)),
            _ => None,
        }
    }

    /// The app or folder whose icon is lifted or settling, and its surface.
    pub fn held(&self) -> Option<(&CellKey, Surface)> {
        match &self.phase {
            Phase::Lifted(drag) => Some((&drag.key, drag.surface)),
            Phase::Settling { key, surface, .. } => Some((key, *surface)),
            _ => None,
        }
    }

    /// Whether an icon follows the finger.
    pub fn is_lifted(&self) -> bool {
        matches!(self.phase, Phase::Lifted(_))
    }

    /// Whether a dropped icon is shrinking into a folder.
    pub fn is_merging(&self) -> bool {
        matches!(self.phase, Phase::Merging(_))
    }

    /// Puts the icon back in its cell at once, or forgets the long-press, such as when the
    /// layout changes or the menu closes before a drag.
    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
    }

    pub fn state(&self) -> LiftState {
        match &self.phase {
            Phase::Idle | Phase::Armed { .. } => LiftState::default(),
            Phase::Lifted(drag) => {
                let (x, y) = drag.centre();
                LiftState {
                    key: Some(drag.key.clone()),
                    surface: drag.surface,
                    x,
                    y,
                    scale: drag.scale.value(),
                    alpha: 1.0,
                    settling: false,
                    merging: false,
                    animating: drag.waiting || drag.speed != 0.0,
                }
            }
            Phase::Settling {
                key,
                surface,
                x,
                y,
                scale,
            } => LiftState {
                key: Some(key.clone()),
                surface: *surface,
                x: x.value(),
                y: y.value(),
                scale: scale.value(),
                alpha: 1.0,
                settling: true,
                merging: false,
                animating: true,
            },
            Phase::Merging(merging) => {
                let t = merging.merge.progress;
                let between = |from: f64, to: f64| from + (to - from) * t;
                LiftState {
                    key: Some(merging.key.clone()),
                    surface: Surface::Grid,
                    x: between(merging.from.0, merging.to.0),
                    y: between(merging.from.1, merging.to.1),
                    scale: between(merging.from.2, merging.to.2),
                    alpha: between(merging.from.3, merging.to.3),
                    settling: false,
                    merging: true,
                    animating: true,
                }
            }
        }
    }

    /// The icon a dragged app is held over, growing and then pulsing (ANIM-14).
    pub fn hover(&self) -> HoverState {
        let Phase::Lifted(drag) = &self.phase else {
            return HoverState::default();
        };
        let Some((Over::Icon(key), since)) = &drag.over else {
            return HoverState::default();
        };
        let held = drag.now.saturating_sub(*since);
        let armed = held >= FOLDER_HOVER;
        let grow = FOLDER_HOVER_SCALE - 1.0;
        let scale = if drag.animation_scale <= 0.0 {
            if armed { FOLDER_HOVER_SCALE } else { 1.0 }
        } else if !armed {
            1.0 + grow * decelerate(held.as_secs_f64() / FOLDER_HOVER.as_secs_f64())
        } else {
            let pulse = (held - FOLDER_HOVER).as_secs_f64() / FOLDER_PULSE.as_secs_f64();
            let dip = (1.0 - (pulse * TAU).cos()) / 2.0;
            FOLDER_HOVER_SCALE - (FOLDER_HOVER_SCALE - FOLDER_PULSE_SCALE) * dip
        };
        HoverState {
            key: Some(key.clone()),
            scale,
            armed,
        }
    }

    /// The folder a dropped app is shrinking into (ANIM-14).
    pub fn merge_state(&self) -> MergeState {
        match &self.phase {
            Phase::Merging(merging) => merging.merge,
            _ => MergeState::default(),
        }
    }
}

impl Drag {
    fn centre(&self) -> (f64, f64) {
        (self.finger.0 + self.grab.0, self.finger.1 + self.grab.1)
    }

    fn frame(&mut self, at: Duration, surfaces: &DragSurfaces, animation_scale: f64) -> LiftStep {
        self.now = self.now.max(at);
        self.animation_scale = animation_scale;
        self.scale.advance(at, animation_scale);
        self.scale.land(REST_DISTANCE_SCALE, REST_SPEED_SCALE);
        let mut step = LiftStep::default();
        // A late frame scrolls no further than a frame or two would have.
        let elapsed = self
            .last_frame
            .map_or(Duration::ZERO, |last| at.saturating_sub(last))
            .min(MAX_FRAME_STEP);
        self.last_frame = Some(self.last_frame.map_or(at, |last| last.max(at)));
        if let Some(view) = surfaces.view(self.surface).filter(|_| self.speed != 0.0) {
            if let Some(layout) = view.layout {
                let max = layout.max_scroll(view.rows);
                let to = (view.scroll_px + self.speed * elapsed.as_secs_f64()).clamp(0.0, max);
                if to != view.scroll_px {
                    step.scroll_to = Some((self.surface, to));
                }
            }
        }
        self.track(surfaces);
        if let Some((Over::Place(surface, place), since)) = &self.over {
            let own = surfaces.place(self.surface, &self.key);
            if *surface == self.surface
                && own != Some(*place)
                && at.saturating_sub(*since) >= REORDER_DWELL
            {
                step.arrange = self.place_at(*place);
            }
        }
        step
    }

    /// Taking place `to` on its own surface.
    fn place_at(&self, to: usize) -> Option<DropAt> {
        match (self.surface, &self.key) {
            (Surface::Grid, key) => Some(DropAt::Grid {
                key: key.clone(),
                to,
            }),
            (Surface::Folder, CellKey::App(app)) => Some(DropAt::Folder {
                app: app.clone(),
                to,
            }),
            (Surface::Folder, CellKey::Folder(_)) => None,
        }
    }

    /// Where the finger lifting now drops the icon.
    fn drop_at(&self) -> DropAt {
        match (&self.over, &self.key) {
            (Some((Over::Icon(onto), since)), CellKey::App(app))
                if self.now.saturating_sub(*since) >= FOLDER_HOVER =>
            {
                DropAt::Combine {
                    app: app.clone(),
                    onto: onto.clone(),
                }
            }
            (Some((Over::Place(surface, place), _)), _) if *surface == self.surface => {
                self.place_at(*place).unwrap_or(DropAt::Back)
            }
            _ => DropAt::Back,
        }
    }

    /// An app dragged out of the open folder's panel comes out of the folder, at the grid place
    /// under its centre, and the drag goes on in the grid (FOLD-05).
    fn leave_folder(&mut self, surfaces: &DragSurfaces) -> Option<LiftEvent> {
        let (Surface::Folder, CellKey::App(app)) = (self.surface, &self.key) else {
            return None;
        };
        let bounds = surfaces.folder_bounds?;
        let centre = self.centre();
        if bounds.contains(centre) {
            return None;
        }
        let layout = surfaces.grid.layout?;
        let (left, top) = layout.origin(surfaces.grid.scroll_px);
        // One place past the last cell is the end.
        let count = surfaces.grid.cells.len() + 1;
        let to = layout.place_at(centre.0 - left, centre.1 - top, count, surfaces.rtl)?;
        let app = app.clone();
        self.surface = Surface::Grid;
        self.over = None;
        self.may_scroll = (false, false);
        Some(LiftEvent::LeftFolder { app, to })
    }

    /// Works out, from the finger and the surface now, what the icon's centre is over, and how
    /// fast it scrolls its surface.
    fn track(&mut self, surfaces: &DragSurfaces) {
        self.speed = 0.0;
        let lifting = !self.scale.at_rest(REST_DISTANCE_SCALE, REST_SPEED_SCALE);
        let found = surfaces
            .view(self.surface)
            .and_then(|view| Some((view, view.layout?)));
        let Some((view, layout)) = found else {
            self.over = None;
            self.waiting = lifting;
            return;
        };
        let (left, top) = layout.origin(view.scroll_px);
        let centre = self.centre();
        let content = (centre.0 - left, centre.1 - top);
        let own = surfaces.place(self.surface, &self.key);
        if let Some(place) = layout.place_at(content.0, content.1, view.cells.len(), surfaces.rtl) {
            let over = self.over_now(surfaces, &layout, view, place, own, content);
            if self.over.as_ref().map(|(over, _)| over) != Some(&over) {
                self.over = Some((over, self.now));
            }
        }
        self.speed = self.scroll_speed(&layout, view, content.1);
        self.waiting = lifting
            || match &self.over {
                Some((Over::Place(_, place), _)) => own != Some(*place),
                Some((Over::Icon(_), _)) => true,
                None => false,
            };
    }

    /// What the icon's centre at `content` px from the cells' top left, in place `place`, is
    /// over: another app's or folder's icon in the grid, for a dragged app, or else the place.
    /// The place it is over now stays while the centre is within [`HYSTERESIS`] of it.
    fn over_now(
        &self,
        surfaces: &DragSurfaces,
        layout: &GridLayout,
        view: &SurfaceView,
        place: usize,
        own: Option<usize>,
        content: (f64, f64),
    ) -> Over {
        let mergeable = self.surface == Surface::Grid
            && matches!(self.key, CellKey::App(_))
            && own != Some(place);
        if mergeable && layout.over_icon(place, surfaces.rtl, surfaces.scale, content) {
            if let Some(item) = view.cells.get(place) {
                return Over::Icon(item.key());
            }
        }
        if let Some((Over::Place(surface, kept), _)) = &self.over {
            if *surface == self.surface
                && *kept != place
                && near(layout, surfaces.rtl, *kept, content)
            {
                return Over::Place(*surface, *kept);
            }
        }
        Over::Place(self.surface, place)
    }

    /// How fast the surface scrolls, in px per second, with the icon's centre `y` px below its
    /// cells' top: 0 outside the zones at the visible top and bottom, up to
    /// [`AUTO_SCROLL_MAX_DP`] at its very edge, and 0 once it can go no further.
    fn scroll_speed(&mut self, layout: &GridLayout, view: &SurfaceView, y: f64) -> f64 {
        if layout.height_px <= 0.0 {
            return 0.0;
        }
        let zone = AUTO_SCROLL_ZONE_DP * layout.density;
        // From the top of the visible cells, below the safe area.
        let on_screen = y - view.scroll_px;
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
        let stuck = (speed < 0.0 && view.scroll_px <= 0.0)
            || (speed > 0.0 && view.scroll_px >= layout.max_scroll(view.rows));
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
mod tests;
