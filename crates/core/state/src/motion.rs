//! The shared motion system (SRS 4.7, ADR 0034): every spring, duration, easing and scale that
//! MeeGone moves with, as named specs that every feature uses, and the physics that
//! gesture-driven motion runs on. The values are starting points for design tuning.
//!
//! A moving element's position and velocity are state, because the UI keeps none of its own
//! (ARCH-05, ARCH-06), so motion lives here in the shared core state rather than in the UI layer
//! (ADR 0006). Time is always passed in, never read from the clock (ARCH-04).
//!
//! - **Moments** of the catalogue (ANIM-07 to ANIM-22) are each a [`Spring`] or a [`Tween`]:
//!   a length with its [`Easing`]. [`CATALOGUE`] lists them, with what cues each one, so one
//!   test holds them all to ANIM-02 and ANIM-06.
//! - **The system animation speed** (ANIM-05) is cleaned up once by [`speed`]. Tweens last
//!   [`Tween::length_at`] it, and springs run on [`motion_time`]. At speed 0 every motion is
//!   instant.
//! - **Waits are not motion.** How long a finger must stay before something happens
//!   ([`REORDER_DWELL`], [`FOLDER_HOVER`]) and how long the view indicator stays
//!   ([`INDICATOR_HOLD`]) never scale with the speed, so removing animations loses no function.

use std::f64::consts::{PI, TAU};
use std::time::Duration;

/// A spring that motion settles with when a gesture ends (ANIM-02).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    /// 1.0 is critically damped; anything lower overshoots.
    pub damping_ratio: f64,
    /// Stiffness for a unit mass, in 1/s².
    pub stiffness: f64,
}

impl Spring {
    /// How far past its target the spring travels, as a fraction of the distance moved, when it
    /// starts at rest.
    pub fn overshoot(&self) -> f64 {
        let z = self.damping_ratio;
        if z >= 1.0 {
            0.0
        } else {
            (-z * PI / (1.0 - z * z).sqrt()).exp()
        }
    }

    /// The release velocity to hand this spring so that, starting `displacement` from its
    /// target, it travels at most `max_overshoot` past the target (ANIM-02).
    ///
    /// Motion continues at the finger's speed whenever the spring can absorb it; only a fast
    /// fling close to the target is slowed. Units are the caller's: a displacement in view
    /// widths takes a velocity in widths per second and an overshoot in widths.
    pub fn limit_velocity(&self, displacement: f64, velocity: f64, max_overshoot: f64) -> f64 {
        if self.overshoot_past_target(displacement, velocity) <= max_overshoot {
            return velocity;
        }
        // Overshoot grows steadily with the release velocity, so the largest velocity within
        // the limit is found by halving the interval.
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..32 {
            let middle = (low + high) / 2.0;
            if self.overshoot_past_target(displacement, velocity * middle) <= max_overshoot {
                low = middle;
            } else {
                high = middle;
            }
        }
        velocity * low
    }

    /// How far past its target the spring travels when released `displacement` from it at
    /// `velocity`: on the far side from where it started, or in the direction it was thrown
    /// when it starts on the target.
    pub fn overshoot_past_target(&self, displacement: f64, velocity: f64) -> f64 {
        let far_side = if displacement != 0.0 {
            -displacement.signum()
        } else {
            velocity.signum()
        };
        let mut motion = SpringMotion::new(*self, displacement, velocity);
        let mut peak: f64 = 0.0;
        // A millisecond is fine enough to find a peak, which is flat at its top. By ten time
        // constants of the slowest decay, what is left of the motion is far below a pixel.
        let z = self.damping_ratio;
        let decay = self.stiffness.sqrt() * if z < 1.0 { z } else { z - (z * z - 1.0).sqrt() };
        let steps = (10.0 / decay * 1000.0).ceil() as u32;
        for _ in 0..steps {
            motion.advance(Duration::from_millis(1));
            peak = peak.max(far_side * motion.displacement());
        }
        peak
    }
}

/// How a timed motion goes from its start to its end. `progress` runs from 0 to 1 and is
/// clamped to that range; anything that isn't a number counts as the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    /// Evenly. Only for opacity no gesture hands on, such as the view indicator's (ANIM-11):
    /// gesture endings never ease linearly (ANIM-02).
    Linear,
    /// Fast at first, slowing to a stop, as Android's `DecelerateInterpolator`.
    Decelerate,
    /// Pops in, as a ✕ badge does (ANIM-12): fast at first, a little past the end, about
    /// 2.3 %, then back to it, as Android's `OvershootInterpolator` with a tension of 0.8.
    Pop,
    /// Out and back: from 0 up to 1 half-way and down to 0 again, smoothly at both ends, as the
    /// icon an app is held over pulses (ANIM-14).
    Pulse,
}

/// How far [`Easing::Pop`] goes past its end.
const POP_TENSION: f64 = 0.8;

impl Easing {
    pub fn at(self, progress: f64) -> f64 {
        let t = if progress.is_nan() {
            1.0
        } else {
            progress.clamp(0.0, 1.0)
        };
        match self {
            Self::Linear => t,
            Self::Decelerate => 1.0 - (1.0 - t) * (1.0 - t),
            Self::Pop => {
                let t = t - 1.0;
                t * t * ((POP_TENSION + 1.0) * t + POP_TENSION) + 1.0
            }
            Self::Pulse => (1.0 - (t * TAU).cos()) / 2.0,
        }
    }
}

/// A timed motion: how long it takes at normal speed, and how it eases.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
    pub length: Duration,
    pub easing: Easing,
}

impl Tween {
    const fn new(millis: u64, easing: Easing) -> Self {
        Self {
            length: ms(millis),
            easing,
        }
    }

    /// How long it lasts at the system animation `speed`: none at all with animations removed
    /// (ANIM-05).
    pub fn length_at(&self, speed: f64) -> Duration {
        scaled(self.length, speed)
    }

    /// How far it has come, from 0 to 1, without easing, `played` into it at normal speed.
    pub fn progress(&self, played: Duration) -> f64 {
        if self.length.is_zero() {
            1.0
        } else {
            played.as_secs_f64() / self.length.as_secs_f64()
        }
    }

    /// How far it has come, eased, `played` into it at normal speed.
    pub fn eased(&self, played: Duration) -> f64 {
        self.easing.at(self.progress(played))
    }

    /// How far it has come, eased, `played` into it at normal speed, starting again every
    /// length, as a pulse that repeats while an icon is held over another (ANIM-14).
    pub fn repeating(&self, played: Duration) -> f64 {
        self.easing.at(self.progress(played).fract())
    }
}

/// What cues a moment of the catalogue, which decides the limits it keeps (ANIM-02, ANIM-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// The finger, while it acts: an icon shrinking as it is pressed, or held over another.
    Touch,
    /// A gesture ending: it carries on from where the finger left it, never linearly (ANIM-02).
    Release,
    /// Anything else, which lasts 150 to 350 ms (ANIM-06).
    Transition,
}

/// A spring or a tween.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    Spring(Spring),
    Tween(Tween),
}

/// One moment of the motion catalogue (SRS 4.7.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moment {
    /// The requirement it meets, such as `ANIM-07`.
    pub id: &'static str,
    pub motion: Motion,
    pub cue: Cue,
}

const fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

// Springs.

/// Lightly damped: overshoots by about 1.5 %, within the 3 % limit (ANIM-02). The views settle
/// with it (ANIM-07), and so does the search field (ANIM-16).
pub const SETTLE: Spring = Spring {
    damping_ratio: 0.8,
    stiffness: 400.0,
};

/// A pressed icon springs back to full size on release, about twice as fast as a view settles,
/// overshooting by about 1.5 % (ANIM-10, ANIM-02).
pub const ICON_RELEASE: Spring = Spring {
    damping_ratio: 0.8,
    stiffness: 1600.0,
};

/// A picked-up icon lifts with this spring, and a dropped one settles into its cell with it, in
/// about 220 ms, overshooting by about 1.5 % (ANIM-13, ANIM-02).
pub const DROP: Spring = Spring {
    damping_ratio: 0.8,
    stiffness: 900.0,
};

// Tweens, one for each timed moment of the catalogue.

/// ANIM-08: the system's launch-from-bounds transition runs it, with its own timing, from the
/// tapped icon or card. MeeGone never waits for it (PERF-06).
pub const APP_LAUNCH: Tween = Tween::new(300, Easing::Decelerate);
/// ANIM-09
pub const RETURN_HOME: Tween = Tween::new(250, Easing::Decelerate);
/// ANIM-10: the touched icon shrinks; it springs back on [`ICON_RELEASE`].
pub const ICON_PRESS: Tween = Tween::new(90, Easing::Decelerate);
/// ANIM-11: the view indicator fades in as a drag starts…
pub const INDICATOR_IN: Tween = Tween::new(100, Easing::Linear);
/// …and out [`INDICATOR_HOLD`] after the views settle.
pub const INDICATOR_OUT: Tween = Tween::new(200, Easing::Linear);
/// ANIM-12: the cells or cards shrink into edit or close mode, and back out of it…
pub const MODE_CHANGE: Tween = Tween::new(150, Easing::Decelerate);
/// …while each row's ✕ badges pop in…
pub const MODE_BADGE: Tween = Tween::new(150, Easing::Pop);
/// …the last row's at most this long after the first's.
pub const MODE_STAGGER: Duration = ms(120);
/// ANIM-14: the icon an app is held over grows as the wait for a folder runs: it is at
/// [`FOLDER_HOVER_SCALE`] just as the wait ends. It marks a wait, so it never scales with the
/// system animation speed.
pub const FOLDER_HOVER_GROW: Tween = Tween {
    length: FOLDER_HOVER,
    easing: Easing::Decelerate,
};
/// ANIM-14: then it pulses down to [`FOLDER_PULSE_SCALE`] and back, once every length, until
/// the drop.
pub const FOLDER_PULSE: Tween = Tween::new(600, Easing::Pulse);
/// ANIM-14: a dropped app shrinks into the folder's preview.
pub const FOLDER_MERGE: Tween = Tween::new(250, Easing::Decelerate);
/// ANIM-15: the folder's panel grows from its icon, and shrinks back into it.
pub const FOLDER: Tween = Tween::new(250, Easing::Decelerate);
/// ANIM-17: Recent's cards reflow to their places after a pinch, and whenever the density
/// changes.
pub const RECENT_REFLOW: Tween = Tween::new(300, Easing::Decelerate);
/// ANIM-18: a card removed from Recent shrinks and fades this long, and the others close the
/// gap in the same time. With Clear all, each row of cards collapses this long…
pub const CARD_REMOVE: Tween = Tween::new(180, Easing::Decelerate);
/// …and the last row starts this long after the first…
pub const CLEAR_ALL_STAGGER: Duration = ms(220);
/// …so Clear all takes no longer than this.
pub const CLEAR_ALL_MAX: Duration = ms(400);
/// ANIM-19
pub const THEME_CROSSFADE: Tween = Tween::new(250, Easing::Decelerate);
/// ANIM-20
pub const NOTIFICATIONS_CHANGE: Tween = Tween::new(200, Easing::Decelerate);
/// ANIM-21
pub const APP_INSTALL_REMOVE: Tween = Tween::new(200, Easing::Decelerate);
/// ANIM-22
pub const IMAGE_CROSSFADE: Tween = Tween::new(300, Easing::Decelerate);

/// Every moment of the motion catalogue, with what cues it.
pub const CATALOGUE: &[Moment] = &[
    moment("ANIM-07", Motion::Spring(SETTLE), Cue::Release),
    moment("ANIM-08", Motion::Tween(APP_LAUNCH), Cue::Transition),
    moment("ANIM-09", Motion::Tween(RETURN_HOME), Cue::Transition),
    moment("ANIM-10", Motion::Tween(ICON_PRESS), Cue::Touch),
    moment("ANIM-10", Motion::Spring(ICON_RELEASE), Cue::Release),
    // The indicator fades in as a drag starts, under the finger.
    moment("ANIM-11", Motion::Tween(INDICATOR_IN), Cue::Touch),
    moment("ANIM-11", Motion::Tween(INDICATOR_OUT), Cue::Transition),
    moment("ANIM-12", Motion::Tween(MODE_CHANGE), Cue::Transition),
    moment("ANIM-12", Motion::Tween(MODE_BADGE), Cue::Transition),
    moment("ANIM-13", Motion::Spring(DROP), Cue::Release),
    moment("ANIM-14", Motion::Tween(FOLDER_HOVER_GROW), Cue::Touch),
    moment("ANIM-14", Motion::Tween(FOLDER_PULSE), Cue::Touch),
    moment("ANIM-14", Motion::Tween(FOLDER_MERGE), Cue::Release),
    moment("ANIM-15", Motion::Tween(FOLDER), Cue::Transition),
    moment("ANIM-16", Motion::Spring(SETTLE), Cue::Release),
    moment("ANIM-17", Motion::Tween(RECENT_REFLOW), Cue::Release),
    moment("ANIM-18", Motion::Tween(CARD_REMOVE), Cue::Transition),
    moment("ANIM-19", Motion::Tween(THEME_CROSSFADE), Cue::Transition),
    moment(
        "ANIM-20",
        Motion::Tween(NOTIFICATIONS_CHANGE),
        Cue::Transition,
    ),
    moment(
        "ANIM-21",
        Motion::Tween(APP_INSTALL_REMOVE),
        Cue::Transition,
    ),
    moment("ANIM-22", Motion::Tween(IMAGE_CROSSFADE), Cue::Transition),
];

const fn moment(id: &'static str, motion: Motion, cue: Cue) -> Moment {
    Moment { id, motion, cue }
}

// Scales.

/// ANIM-09
pub const RETURN_HOME_FROM_SCALE: f64 = 0.94;
/// ANIM-10
pub const ICON_PRESS_SCALE: f64 = 0.92;
/// ANIM-21: a new icon grows from this scale as it fades in, and a removed one shrinks to it as
/// it fades out.
pub const APP_INSTALL_REMOVE_SCALE: f64 = 0.5;
/// ANIM-18: a card removed from Recent shrinks to this scale as it fades, as an icon removed
/// from the grid does.
pub const CARD_REMOVE_SCALE: f64 = APP_INSTALL_REMOVE_SCALE;
/// ANIM-12
pub const MODE_SCALE: f64 = 0.92;
/// ANIM-13
pub const DRAG_LIFT_SCALE: f64 = 1.10;
/// ANIM-14: the icon an app is held over grows to this scale while it waits…
pub const FOLDER_HOVER_SCALE: f64 = 1.1;
/// …then pulses down to this one and back, once every [`FOLDER_PULSE`], until the drop.
pub const FOLDER_PULSE_SCALE: f64 = 1.05;
/// ANIM-14: an app dropped into a folder whose preview is full shrinks to this fraction of its
/// size as it fades into the folder's icon.
pub const MERGE_AWAY_SCALE: f64 = 0.3;

// Gestures.

/// No settle overshoots its target by more than this fraction of the distance between views
/// (ANIM-02).
pub const MAX_OVERSHOOT: f64 = 0.03;

/// A view swipe settles on the next view once dragged past this fraction of the width
/// (ANIM-07).
pub const VIEW_SWIPE_THRESHOLD: f64 = 0.35;
/// A view swipe released at least this fast, in dp per second, is a fling and settles on the
/// next view whatever its distance (ANIM-07).
pub const VIEW_FLING_VELOCITY_DP: f64 = 500.0;
/// Past the last view, with wrap-around off, the row follows the finger this much (HOME-04)…
pub const VIEW_EDGE_RESISTANCE: f64 = 1.0 / 3.0;
/// …and never stretches further than this fraction of the width.
pub const VIEW_EDGE_STRETCH: f64 = 0.12;
/// Pulled past the whole search field, the field follows the finger this much (ANIM-16)…
pub const SEARCH_PULL_RESISTANCE: f64 = 1.0 / 3.0;
/// …and never stretches further than this fraction of the field's band.
pub const SEARCH_PULL_STRETCH: f64 = 0.5;

/// A pinch on Recent switches its density once the cards have gone past this fraction of the
/// way to the other density's size (REC-05, ANIM-17).
pub const RECENT_PINCH_THRESHOLD: f64 = 0.35;
/// Pinched past either density's size, the cards follow the fingers this much (ANIM-17)…
pub const RECENT_PINCH_RESISTANCE: f64 = 1.0 / 3.0;
/// …and never stretch further than this fraction of the way between the two sizes.
pub const RECENT_PINCH_STRETCH: f64 = 0.25;

/// A touch catches a settling element only while it is further than this fraction of the touch
/// slop from where it is going, as Android's paged launchers do (ANIM-03). Closer, it is as good
/// as there: it lands at once and the touch goes to what is under it, so the last sliver of a
/// settle never swallows a tap.
pub const CATCH_SLOP_FRACTION: f64 = 1.0 / 3.0;

/// Motion closer to its target than this, in pixels, and slower than [`REST_SPEED_PX`] has
/// finished: what is left would move less than half a pixel per 60 Hz frame.
pub const REST_DISTANCE_PX: f64 = 0.5;
/// See [`REST_DISTANCE_PX`]; in pixels per second.
pub const REST_SPEED_PX: f64 = 30.0;
/// The same for a scale, which is about 1: closer than this and slower than
/// [`REST_SPEED_SCALE`] per second is well under a pixel on any icon.
pub const REST_DISTANCE_SCALE: f64 = 0.001;
/// See [`REST_DISTANCE_SCALE`].
pub const REST_SPEED_SCALE: f64 = 0.05;

// Waits: how long a finger stays, or something is shown, never scaled with the speed.

/// ANIM-11: how long the view indicator stays after the views settle, before it fades out.
pub const INDICATOR_HOLD: Duration = ms(600);
/// APPS-12: a dragged icon takes a new place once its centre has stayed over it this long, so
/// a quick drag across the grid doesn't shuffle every cell it passes.
pub const REORDER_DWELL: Duration = ms(150);
/// FOLD-01: a dragged app held over another's icon this long makes a folder when dropped.
pub const FOLDER_HOVER: Duration = ms(400);
/// APPS-12: a dragged icon whose centre is this close to the top or bottom of the visible grid,
/// in dp, scrolls it…
pub const AUTO_SCROLL_ZONE_DP: f64 = 64.0;
/// …faster the deeper it is, up to this many dp per second at the edge itself.
pub const AUTO_SCROLL_MAX_DP: f64 = 640.0;

// Limits.

/// Nothing decorative lasts longer (ANIM-06).
pub const DECORATIVE_MAX: Duration = ms(400);
/// With animations removed, a transition is at most a fade this long (ANIM-05). Every motion is
/// instant instead.
pub const REDUCED_MOTION_FADE_MAX: Duration = ms(100);
/// The most one frame advances a timed motion: two frames at 60 Hz.
///
/// Taking on new content costs the host a frame or two: on a Galaxy S22, the frame after the
/// grid took on new cells came 75 to 92 ms late. So a late frame advances a tween by at most
/// this much, and it is seen from its start instead of popping in part-way. Springs are exact
/// at any frame time instead, so a late frame simply lands further along (ANIM-04).
pub const MAX_FRAME_STEP: Duration = ms(34);

// The system animation speed.

/// The system animation speed as motion takes it (ANIM-05): 1 is normal, and 0 means
/// animations are removed. Android offers speeds from 0 to 10. Anything that isn't a number
/// counts as normal, and a speed too slow or too fast to time with counts as off or as 10.
pub fn speed(raw: f64) -> f64 {
    match raw {
        raw if !raw.is_finite() => 1.0,
        raw if raw < 0.01 => 0.0,
        raw => raw.min(10.0),
    }
}

/// Scales a duration by the system animation speed. A speed of 0 means animations are off
/// (ANIM-05); one past what a duration can hold counts as the longest.
pub fn scaled(duration: Duration, speed: f64) -> Duration {
    Duration::try_from_secs_f64(duration.as_secs_f64() * speed.max(0.0)).unwrap_or(Duration::MAX)
}

/// How much motion `real` time plays at the system animation speed: less when it is slower. A
/// spring's clock runs on it. With animations removed, any time at all plays every motion to
/// its end.
pub fn motion_time(real: Duration, speed: f64) -> Duration {
    if speed > 0.0 {
        Duration::try_from_secs_f64(real.as_secs_f64() / speed).unwrap_or(Duration::MAX)
    } else if real.is_zero() {
        Duration::ZERO
    } else {
        Duration::MAX
    }
}

/// How much of a tween a frame `real` after the last one plays at the system animation speed:
/// at most [`MAX_FRAME_STEP`] of real time.
pub fn frame_step(real: Duration, speed: f64) -> Duration {
    motion_time(real.min(MAX_FRAME_STEP), speed)
}

/// When the `rank`th of `count` staggered things starts, from 0 for the first to `total` for the
/// last, evenly between: rows of ✕ badges popping in (ANIM-12), or rows of cards leaving
/// Recent (ANIM-18).
pub fn stagger(rank: usize, count: usize, total: Duration) -> Duration {
    if count > 1 {
        total.mul_f64(rank.min(count - 1) as f64 / (count - 1) as f64)
    } else {
        Duration::ZERO
    }
}

/// One element settling with a [`Spring`] towards a target at displacement 0.
///
/// The motion is the spring's exact solution, so it is the same at any frame rate and a late
/// frame simply lands further along (ANIM-04).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringMotion {
    spring: Spring,
    /// Where the motion started and how fast, relative to the target.
    start: (f64, f64),
    /// Time since the start, in seconds.
    elapsed: f64,
}

impl SpringMotion {
    pub fn new(spring: Spring, displacement: f64, velocity: f64) -> Self {
        Self {
            spring,
            start: (displacement, velocity),
            elapsed: 0.0,
        }
    }

    pub fn advance(&mut self, by: Duration) {
        self.elapsed += by.as_secs_f64();
    }

    /// The distance from the target now.
    pub fn displacement(&self) -> f64 {
        self.now().0
    }

    /// The speed now, per second.
    pub fn velocity(&self) -> f64 {
        self.now().1
    }

    /// Whether the motion is within `distance` of its target and slower than `speed`.
    pub fn is_at_rest(&self, distance: f64, speed: f64) -> bool {
        let (displacement, velocity) = self.now();
        displacement.abs() < distance && velocity.abs() < speed
    }

    /// The damped harmonic oscillator's displacement and velocity at `elapsed`. Long after the
    /// start, which is how a motion run to its end with animations removed looks, it is at
    /// rest on its target.
    fn now(&self) -> (f64, f64) {
        let (x0, v0) = self.start;
        let t = self.elapsed;
        let w = self.spring.stiffness.sqrt();
        let z = self.spring.damping_ratio;
        if !t.is_finite() || t > 1e6 {
            return (0.0, 0.0);
        }
        if z < 1.0 {
            let decay = z * w;
            let wd = w * (1.0 - z * z).sqrt();
            let b = (v0 + decay * x0) / wd;
            let (sin, cos) = (wd * t).sin_cos();
            let envelope = (-decay * t).exp();
            (
                envelope * (x0 * cos + b * sin),
                envelope * (v0 * cos - (x0 * wd + decay * b) * sin),
            )
        } else if z == 1.0 {
            let b = v0 + w * x0;
            let envelope = (-w * t).exp();
            (envelope * (x0 + b * t), envelope * (v0 - w * b * t))
        } else {
            let root = (z * z - 1.0).sqrt();
            let (r1, r2) = (-w * (z - root), -w * (z + root));
            let c2 = (v0 - r1 * x0) / (r2 - r1);
            let c1 = x0 - c2;
            let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
            (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
        }
    }
}

/// Estimates how fast a finger moves from its recent positions, for flings (ANIM-02, ANIM-07).
///
/// It fits a straight line through the positions of the last [`VelocityTracker::WINDOW`]. A
/// finger that stopped for [`VelocityTracker::STOPPED`] before lifting has no velocity, as on
/// Android.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VelocityTracker {
    /// Times and positions, oldest first.
    samples: Vec<(Duration, f64)>,
}

impl VelocityTracker {
    /// Positions older than this, before the latest one, are ignored.
    pub const WINDOW: Duration = ms(100);
    /// A finger that hasn't moved for this long has stopped.
    pub const STOPPED: Duration = ms(40);
    /// Enough positions for any touch rate within the window.
    const MAX_SAMPLES: usize = 32;

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn add(&mut self, at: Duration, position: f64) {
        if self.samples.len() == Self::MAX_SAMPLES {
            self.samples.remove(0);
        }
        self.samples.push((at, position));
    }

    /// The velocity at `now`, per second.
    pub fn velocity(&self, now: Duration) -> f64 {
        let Some(&(last, _)) = self.samples.last() else {
            return 0.0;
        };
        if now.saturating_sub(last) > Self::STOPPED {
            return 0.0;
        }
        let recent: Vec<(f64, f64)> = self
            .samples
            .iter()
            .filter(|(at, _)| last.saturating_sub(*at) <= Self::WINDOW)
            .map(|&(at, position)| (at.as_secs_f64() - last.as_secs_f64(), position))
            .collect();
        let count = recent.len() as f64;
        let mean_t = recent.iter().map(|(t, _)| t).sum::<f64>() / count;
        let mean_x = recent.iter().map(|(_, x)| x).sum::<f64>() / count;
        let (mut covariance, mut variance) = (0.0, 0.0);
        for (t, x) in &recent {
            covariance += (t - mean_t) * (x - mean_x);
            variance += (t - mean_t) * (t - mean_t);
        }
        if variance > 0.0 {
            covariance / variance
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The last time, in seconds, that `spring` settling from `displacement` was further than
    /// `band` from its target.
    fn settle_time(spring: Spring, displacement: f64, band: f64) -> f64 {
        let mut motion = SpringMotion::new(spring, displacement, 0.0);
        let mut last_outside = 0.0;
        for step in 1..=2000 {
            motion.advance(ms(1));
            if motion.displacement().abs() > band {
                last_outside = f64::from(step) / 1000.0;
            }
        }
        last_outside
    }

    #[test]
    fn the_settle_spring_overshoots_by_at_most_3_percent() {
        // ANIM-02
        assert!(SETTLE.overshoot() > 0.0);
        assert!(SETTLE.overshoot() <= MAX_OVERSHOOT);
    }

    #[test]
    fn a_view_settles_in_about_250_to_350_ms() {
        // ANIM-07: released at 35 % or half-way, still; within 0.5 % of the width is about
        // 5 px on a phone.
        for displacement in [-0.65, -0.5] {
            let time = settle_time(SETTLE, displacement, 0.005);
            assert!((0.25..=0.35).contains(&time), "{displacement}: {time} s");
        }
    }

    #[test]
    fn a_pressed_icon_springs_back_quickly_with_little_overshoot() {
        // ANIM-10, ANIM-02: from 92 % to within a hundredth of a percent of full size.
        assert!(ICON_RELEASE.overshoot() > 0.0);
        assert!(ICON_RELEASE.overshoot() <= MAX_OVERSHOOT);
        let time = settle_time(ICON_RELEASE, -(1.0 - ICON_PRESS_SCALE), 0.0001);
        assert!((0.1..=0.25).contains(&time), "{time} s");
    }

    #[test]
    fn a_dropped_icon_settles_in_about_220_ms_with_little_overshoot() {
        // ANIM-13, ANIM-02: within 0.5 % of the distance, as a view settle is measured.
        assert!(DROP.overshoot() > 0.0);
        assert!(DROP.overshoot() <= MAX_OVERSHOOT);
        let time = settle_time(DROP, 1.0, 0.005);
        assert!((0.18..=0.27).contains(&time), "{time} s");
    }

    #[test]
    fn every_easing_starts_at_0_and_ends_at_1() {
        for easing in [Easing::Linear, Easing::Decelerate, Easing::Pop] {
            assert_eq!(easing.at(0.0), 0.0, "{easing:?}");
            assert_eq!(easing.at(1.0), 1.0, "{easing:?}");
            assert_eq!(easing.at(-1.0), 0.0, "{easing:?}");
            assert_eq!(easing.at(2.0), 1.0, "{easing:?}");
            assert_eq!(easing.at(f64::NAN), 1.0, "{easing:?}");
        }
        assert_eq!(Easing::Linear.at(0.25), 0.25);
    }

    #[test]
    fn a_badge_pops_a_little_past_full_size_and_ends_there() {
        // ANIM-12
        let peak = (0..=1000)
            .map(|i| Easing::Pop.at(f64::from(i) / 1000.0))
            .fold(0.0, f64::max);
        assert!((1.01..=1.03).contains(&peak), "{peak}");
        // Fast at first.
        assert!(Easing::Pop.at(0.25) > 0.5);
    }

    #[test]
    fn decelerating_motion_starts_fast_and_ends_at_rest() {
        // ANIM-09
        assert!(Easing::Decelerate.at(0.5) > 0.5);
        let steps: Vec<f64> = (0..=10)
            .map(|step| Easing::Decelerate.at(f64::from(step) / 10.0))
            .collect();
        assert!(steps.windows(2).all(|pair| pair[1] > pair[0]));
    }

    #[test]
    fn a_pulse_goes_out_and_back_and_repeats() {
        // ANIM-14
        assert_eq!(Easing::Pulse.at(0.0), 0.0);
        assert!((Easing::Pulse.at(0.5) - 1.0).abs() < 1e-12);
        assert!(Easing::Pulse.at(1.0).abs() < 1e-12);
        assert!((FOLDER_PULSE.repeating(ms(300)) - 1.0).abs() < 1e-12);
        assert!(FOLDER_PULSE.repeating(ms(600)).abs() < 1e-12);
        let twice = FOLDER_PULSE.repeating(ms(750));
        assert!((twice - FOLDER_PULSE.repeating(ms(150))).abs() < 1e-12);
    }

    #[test]
    fn a_tween_eases_over_its_length() {
        assert_eq!(FOLDER.eased(Duration::ZERO), 0.0);
        assert_eq!(FOLDER.eased(ms(125)), Easing::Decelerate.at(0.5));
        assert_eq!(FOLDER.eased(ms(250)), 1.0);
        assert_eq!(FOLDER.eased(ms(400)), 1.0);
        assert_eq!(FOLDER.progress(ms(125)), 0.5);
    }

    #[test]
    fn the_spring_overshoot_matches_its_analytic_value() {
        let measured = SETTLE.overshoot_past_target(-1.0, 0.0);
        assert!((measured - SETTLE.overshoot()).abs() < 1e-4, "{measured}");
    }

    #[test]
    fn no_release_overshoots_by_more_than_3_percent_of_the_width() {
        // ANIM-02: a fast fling close to its target would otherwise fly far past it.
        for displacement in [-1.0, -0.65, -0.5, -0.2, -0.05, 0.0, 0.3] {
            for velocity in [0.0, 1.0, 3.0, 6.0, 10.0, 20.0, -4.0] {
                let limited = SETTLE.limit_velocity(displacement, velocity, MAX_OVERSHOOT);
                let overshoot = SETTLE.overshoot_past_target(displacement, limited);
                assert!(
                    overshoot <= MAX_OVERSHOOT + 1e-9,
                    "{displacement} at {velocity}: {overshoot}"
                );
                assert!(limited.abs() <= velocity.abs());
            }
        }
    }

    #[test]
    fn a_release_the_spring_can_absorb_keeps_the_fingers_speed() {
        // ANIM-02: motion continues at the finger's speed.
        assert_eq!(SETTLE.limit_velocity(-0.65, 3.0, MAX_OVERSHOOT), 3.0);
        assert!(SETTLE.limit_velocity(-0.05, 20.0, MAX_OVERSHOOT) < 20.0);
    }

    #[test]
    fn a_spring_motion_comes_to_rest_on_its_target() {
        for spring in [
            SETTLE,
            Spring {
                damping_ratio: 1.0,
                stiffness: 400.0,
            },
            Spring {
                damping_ratio: 1.5,
                stiffness: 400.0,
            },
        ] {
            let mut motion = SpringMotion::new(spring, -0.5, 2.0);
            assert!(!motion.is_at_rest(1e-3, 1e-2));
            assert!((motion.velocity() - 2.0).abs() < 1e-9);
            motion.advance(Duration::from_secs(2));
            assert!(motion.is_at_rest(1e-3, 1e-2), "{spring:?}");
            // Run to its end with animations removed.
            motion.advance(Duration::MAX);
            assert_eq!(motion.displacement(), 0.0, "{spring:?}");
            assert_eq!(motion.velocity(), 0.0, "{spring:?}");
        }
    }

    #[test]
    fn the_spring_is_the_same_at_any_frame_rate() {
        // ANIM-04
        let mut at_60_hz = SpringMotion::new(SETTLE, -0.65, 1.0);
        let mut at_120_hz = at_60_hz;
        for _ in 0..12 {
            at_60_hz.advance(Duration::from_micros(16_667));
            at_120_hz.advance(Duration::from_micros(8_333));
            at_120_hz.advance(Duration::from_micros(8_334));
        }
        assert!((at_60_hz.displacement() - at_120_hz.displacement()).abs() < 1e-9);
    }

    #[test]
    fn the_velocity_of_a_steady_finger_is_its_speed() {
        let mut tracker = VelocityTracker::default();
        for step in 0..10 {
            tracker.add(ms(1000 + step * 8), 100.0 + 4.0 * step as f64);
        }
        let velocity = tracker.velocity(ms(1072));
        assert!((velocity - 500.0).abs() < 1e-6, "{velocity}");
    }

    #[test]
    fn only_the_last_100_ms_count() {
        let mut tracker = VelocityTracker::default();
        tracker.add(ms(0), 0.0);
        tracker.add(ms(10), 1000.0);
        for step in 0..=10 {
            tracker.add(ms(200 + step * 10), 2000.0 - step as f64);
        }
        let velocity = tracker.velocity(ms(300));
        assert!((velocity + 100.0).abs() < 1e-6, "{velocity}");
    }

    #[test]
    fn a_finger_that_stopped_or_barely_moved_has_no_velocity() {
        let mut tracker = VelocityTracker::default();
        assert_eq!(tracker.velocity(ms(0)), 0.0);
        tracker.add(ms(0), 0.0);
        assert_eq!(tracker.velocity(ms(0)), 0.0);
        tracker.add(ms(10), 50.0);
        assert!(tracker.velocity(ms(10)) > 0.0);
        assert_eq!(tracker.velocity(ms(60)), 0.0);
        tracker.clear();
        assert_eq!(tracker.velocity(ms(60)), 0.0);
    }

    #[test]
    fn the_tracker_keeps_a_bounded_history() {
        let mut tracker = VelocityTracker::default();
        for step in 0..100 {
            tracker.add(ms(step), step as f64);
        }
        assert_eq!(tracker.samples.len(), VelocityTracker::MAX_SAMPLES);
        assert!((tracker.velocity(ms(99)) - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn the_catalogue_names_every_moment() {
        // SRS 4.7.2: ANIM-07 to ANIM-22 each have a spec.
        for number in 7..=22 {
            let id = format!("ANIM-{number:02}");
            assert!(
                CATALOGUE.iter().any(|moment| moment.id == id),
                "{id} has no spec"
            );
        }
    }

    #[test]
    fn transitions_last_150_to_350_ms_and_nothing_decorative_more_than_400() {
        // ANIM-06
        for moment in CATALOGUE {
            let Motion::Tween(tween) = moment.motion else {
                continue;
            };
            if moment.cue == Cue::Transition {
                assert!(
                    (ms(150)..=ms(350)).contains(&tween.length),
                    "{}: {:?}",
                    moment.id,
                    tween.length
                );
            }
            // The pulse repeats for as long as the finger holds the icon; each beat is
            // short enough to read as one.
            if tween != FOLDER_PULSE {
                assert!(tween.length <= DECORATIVE_MAX, "{}", moment.id);
            }
        }
        // ANIM-12: the badges' stagger fits within the mode's change.
        assert!(MODE_STAGGER <= ms(120));
    }

    #[test]
    fn gesture_endings_spring_or_ease_never_linearly() {
        // ANIM-02
        for moment in CATALOGUE.iter().filter(|moment| moment.cue == Cue::Release) {
            match moment.motion {
                Motion::Spring(spring) => {
                    assert!(spring.overshoot() <= MAX_OVERSHOOT, "{}", moment.id);
                }
                Motion::Tween(tween) => {
                    assert_ne!(tween.easing, Easing::Linear, "{}", moment.id);
                }
            }
        }
    }

    #[test]
    fn clear_all_ends_within_its_limit() {
        // ANIM-18: the last row starts after the stagger and collapses as one card does.
        assert!(CARD_REMOVE.length + CLEAR_ALL_STAGGER <= CLEAR_ALL_MAX);
        assert!(CLEAR_ALL_MAX <= DECORATIVE_MAX);
    }

    #[test]
    fn removed_animations_make_transitions_instant() {
        // ANIM-05
        assert_eq!(FOLDER.length_at(0.0), Duration::ZERO);
        assert_eq!(FOLDER.length_at(2.0), ms(500));
        assert_eq!(scaled(ms(250), f64::INFINITY), Duration::MAX);
        assert_eq!(scaled(ms(250), -1.0), Duration::ZERO);
        assert!(REDUCED_MOTION_FADE_MAX <= ms(100));
    }

    #[test]
    fn a_slower_speed_plays_less_motion_in_the_same_time() {
        // ANIM-05
        assert_eq!(motion_time(ms(100), 1.0), ms(100));
        assert_eq!(motion_time(ms(100), 2.0), ms(50));
        assert_eq!(motion_time(ms(100), 0.5), ms(200));
        assert_eq!(motion_time(ms(100), 0.0), Duration::MAX);
        assert_eq!(motion_time(Duration::ZERO, 0.0), Duration::ZERO);
        assert_eq!(frame_step(ms(100), 1.0), MAX_FRAME_STEP);
        assert_eq!(frame_step(ms(8), 2.0), ms(4));
    }

    #[test]
    fn the_system_speed_is_cleaned_up() {
        // ANIM-05: Android offers 0 to 10.
        assert_eq!(speed(1.0), 1.0);
        assert_eq!(speed(0.5), 0.5);
        assert_eq!(speed(0.0), 0.0);
        assert_eq!(speed(f64::NAN), 1.0);
        assert_eq!(speed(f64::INFINITY), 1.0);
        assert_eq!(speed(1e30), 10.0);
        assert_eq!(speed(1e-17), 0.0);
        assert_eq!(speed(-1.0), 0.0);
    }

    #[test]
    fn staggered_rows_spread_evenly_from_the_first_to_the_last() {
        // ANIM-12, ANIM-18
        assert_eq!(stagger(0, 1, MODE_STAGGER), Duration::ZERO);
        assert_eq!(stagger(0, 3, MODE_STAGGER), Duration::ZERO);
        assert_eq!(stagger(1, 3, MODE_STAGGER), ms(60));
        assert_eq!(stagger(2, 3, MODE_STAGGER), ms(120));
        assert_eq!(stagger(5, 3, MODE_STAGGER), ms(120));
    }

    #[test]
    fn waits_are_not_motion() {
        // ANIM-05: they keep their length whatever the speed; the hover's grow marks its wait.
        assert_eq!(FOLDER_HOVER_GROW.length, FOLDER_HOVER);
        assert!(REORDER_DWELL < FOLDER_HOVER);
        assert!(INDICATOR_HOLD > INDICATOR_OUT.length);
    }
}
