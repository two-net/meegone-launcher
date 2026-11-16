//! Motion specifications (SRS 4.7) and the physics that gesture-driven motion runs on. The
//! values are starting points for design tuning.
//!
//! A moving element's position and velocity are state, because the UI keeps none of its own
//! (ARCH-05, ARCH-06), so motion lives here in the shared core state rather than in the UI layer
//! (ADR 0006). Time is always passed in, never read from the clock (ARCH-04).

use std::f64::consts::PI;
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

/// Lightly damped: overshoots by about 1.5 %, within the 3 % limit (ANIM-02).
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

/// Motion closer to its target than this, in pixels, and slower than [`REST_SPEED_PX`] has
/// finished: what is left would move less than half a pixel per 60 Hz frame.
pub const REST_DISTANCE_PX: f64 = 0.5;
/// See [`REST_DISTANCE_PX`]; in pixels per second.
pub const REST_SPEED_PX: f64 = 30.0;

/// ANIM-09
pub const RETURN_HOME_FROM_SCALE: f64 = 0.94;
/// ANIM-10
pub const ICON_PRESS_SCALE: f64 = 0.92;
/// ANIM-21: a new icon grows from this scale as it fades in, and a removed one shrinks to it as
/// it fades out.
pub const APP_INSTALL_REMOVE_SCALE: f64 = 0.5;
/// ANIM-12
pub const MODE_SCALE: f64 = 0.92;
/// ANIM-13
pub const DRAG_LIFT_SCALE: f64 = 1.10;
/// APPS-12: a dragged icon takes a new place once its centre has stayed over it this long, so
/// a quick drag across the grid doesn't shuffle every cell it passes.
pub const REORDER_DWELL: Duration = ms(150);
/// FOLD-01: a dragged app held over another's icon this long makes a folder when dropped.
pub const FOLDER_HOVER: Duration = ms(400);
/// ANIM-14: the icon an app is held over grows to this scale while it waits…
pub const FOLDER_HOVER_SCALE: f64 = 1.1;
/// …then pulses down to this one and back, once every [`FOLDER_PULSE`], until the drop.
pub const FOLDER_PULSE_SCALE: f64 = 1.05;
pub const FOLDER_PULSE: Duration = ms(600);
/// APPS-12: a dragged icon whose centre is this close to the top or bottom of the visible grid,
/// in dp, scrolls it…
pub const AUTO_SCROLL_ZONE_DP: f64 = 64.0;
/// …faster the deeper it is, up to this many dp per second at the edge itself.
pub const AUTO_SCROLL_MAX_DP: f64 = 640.0;

const fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// ANIM-08
pub const APP_LAUNCH: Duration = ms(300);
/// ANIM-09
pub const RETURN_HOME: Duration = ms(250);
/// ANIM-10
pub const ICON_PRESS: Duration = ms(90);
/// ANIM-11
pub const INDICATOR_IN: Duration = ms(100);
/// ANIM-11
pub const INDICATOR_OUT: Duration = ms(200);
/// ANIM-11: how long the indicator stays after the views settle.
pub const INDICATOR_HOLD: Duration = ms(600);
/// ANIM-12
pub const MODE_CHANGE: Duration = ms(150);
/// ANIM-12: the whole stagger of badges popping in.
pub const MODE_STAGGER: Duration = ms(120);
/// ANIM-14, ANIM-15
pub const FOLDER: Duration = ms(250);
/// ANIM-17
pub const RECENT_PINCH: Duration = ms(300);
/// ANIM-18
pub const CARD_REMOVE: Duration = ms(180);
/// ANIM-18
pub const CARDS_CLOSE_GAP: Duration = ms(220);
/// ANIM-18
pub const CLEAR_ALL_MAX: Duration = ms(400);
/// ANIM-19
pub const THEME_CROSSFADE: Duration = ms(250);
/// ANIM-20
pub const NOTIFICATIONS_CHANGE: Duration = ms(200);
/// ANIM-21
pub const APP_INSTALL_REMOVE: Duration = ms(200);
/// ANIM-22
pub const IMAGE_CROSSFADE: Duration = ms(300);

/// Nothing decorative lasts longer (ANIM-06).
pub const DECORATIVE_MAX: Duration = ms(400);
/// With animations removed, a transition is at most a fade this long (ANIM-05).
pub const REDUCED_MOTION_FADE_MAX: Duration = ms(100);

/// Scales a duration by the system animation speed. A scale of 0 means animations are off
/// (ANIM-05).
pub fn scaled(duration: Duration, system_scale: f64) -> Duration {
    duration.mul_f64(system_scale.max(0.0))
}

/// Decelerating easing, as Android's `DecelerateInterpolator`: fast at first, slowing to a stop.
/// `progress` runs from 0 to 1 and is clamped to that range.
pub fn decelerate(progress: f64) -> f64 {
    let progress = if progress.is_nan() {
        1.0
    } else {
        progress.clamp(0.0, 1.0)
    };
    1.0 - (1.0 - progress) * (1.0 - progress)
}

/// How far [`pop`] goes past its end, as Android's `OvershootInterpolator` with this tension.
const POP_TENSION: f64 = 0.8;

/// Easing that pops in, as a ✕ badge does (ANIM-12): fast at first, a little past the end, about
/// 2.3 %, then back to it. `progress` runs from 0 to 1 and is clamped to that range.
pub fn pop(progress: f64) -> f64 {
    let progress = if progress.is_nan() {
        1.0
    } else {
        progress.clamp(0.0, 1.0)
    };
    let t = progress - 1.0;
    t * t * ((POP_TENSION + 1.0) * t + POP_TENSION) + 1.0
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

    /// The damped harmonic oscillator's displacement and velocity at `elapsed`.
    fn now(&self) -> (f64, f64) {
        let (x0, v0) = self.start;
        let t = self.elapsed;
        let w = self.spring.stiffness.sqrt();
        let z = self.spring.damping_ratio;
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
    fn a_badge_pops_a_little_past_full_size_and_ends_there() {
        // ANIM-12
        assert_eq!(pop(0.0), 0.0);
        assert_eq!(pop(1.0), 1.0);
        assert_eq!(pop(-1.0), 0.0);
        assert_eq!(pop(f64::NAN), 1.0);
        let peak = (0..=1000)
            .map(|i| pop(f64::from(i) / 1000.0))
            .fold(0.0, f64::max);
        assert!((1.01..=1.03).contains(&peak), "{peak}");
        // Fast at first.
        assert!(pop(0.25) > 0.5);
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
    fn no_transition_is_longer_than_the_decorative_limit() {
        // ANIM-06
        for duration in [
            APP_LAUNCH,
            RETURN_HOME,
            ICON_PRESS,
            INDICATOR_IN,
            INDICATOR_OUT,
            MODE_CHANGE,
            MODE_STAGGER,
            FOLDER,
            RECENT_PINCH,
            CARD_REMOVE,
            CARDS_CLOSE_GAP,
            CLEAR_ALL_MAX,
            THEME_CROSSFADE,
            NOTIFICATIONS_CHANGE,
            APP_INSTALL_REMOVE,
            IMAGE_CROSSFADE,
        ] {
            assert!(duration <= DECORATIVE_MAX, "{duration:?}");
        }
    }

    #[test]
    fn removed_animations_make_transitions_instant() {
        // ANIM-05
        assert_eq!(scaled(FOLDER, 0.0), Duration::ZERO);
        assert_eq!(scaled(FOLDER, 2.0), ms(500));
    }

    #[test]
    fn decelerating_motion_starts_fast_and_ends_at_rest() {
        // ANIM-09
        assert_eq!(decelerate(0.0), 0.0);
        assert_eq!(decelerate(1.0), 1.0);
        assert!(decelerate(0.5) > 0.5);
        let steps: Vec<f64> = (0..=10)
            .map(|step| decelerate(f64::from(step) / 10.0))
            .collect();
        assert!(steps.windows(2).all(|pair| pair[1] > pair[0]));
        assert_eq!(decelerate(-1.0), 0.0);
        assert_eq!(decelerate(2.0), 1.0);
        assert_eq!(decelerate(f64::NAN), 1.0);
    }
}
