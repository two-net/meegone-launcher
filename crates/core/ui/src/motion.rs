//! Motion specifications (SRS 4.7). The values are starting points for design tuning.

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
    /// How far past its target the spring travels, as a fraction of the distance moved.
    pub fn overshoot(&self) -> f64 {
        let z = self.damping_ratio;
        if z >= 1.0 {
            0.0
        } else {
            (-z * PI / (1.0 - z * z).sqrt()).exp()
        }
    }
}

/// Lightly damped: overshoots by about 1.5 %, within the 3 % limit (ANIM-02).
pub const SETTLE: Spring = Spring {
    damping_ratio: 0.8,
    stiffness: 400.0,
};

/// A view swipe settles on the next view once dragged past this fraction of the width
/// (ANIM-07).
pub const VIEW_SWIPE_THRESHOLD: f64 = 0.35;
/// ANIM-09
pub const RETURN_HOME_FROM_SCALE: f64 = 0.94;
/// ANIM-10
pub const ICON_PRESS_SCALE: f64 = 0.92;
/// ANIM-12
pub const MODE_SCALE: f64 = 0.92;
/// ANIM-13
pub const DRAG_LIFT_SCALE: f64 = 1.10;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settle_spring_overshoots_by_at_most_3_percent() {
        // ANIM-02
        assert!(SETTLE.overshoot() > 0.0);
        assert!(SETTLE.overshoot() <= 0.03);
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
}
