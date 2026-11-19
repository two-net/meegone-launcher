//! The icon press (ANIM-10): a touched icon shrinks to 92 % over about 90 ms, decelerating, and
//! springs back to full size when the finger lifts or leaves it.
//!
//! It is touch feedback only: a tap launches at once, and nothing waits for the motion (PERF-06,
//! ANIM-06). Like the carousel, it is pure: the host passes the time of each touch and frame
//! (ADR 0006). With animations removed, the icon changes size at once (ANIM-05).

use std::time::Duration;

use meegone_apps_state::Surface;
use meegone_core_state::motion::{
    ICON_PRESS, ICON_PRESS_SCALE, ICON_RELEASE, REST_DISTANCE_SCALE, REST_SPEED_SCALE,
    SpringMotion, motion_time,
};

/// The pressed item as it is drawn now: an immutable snapshot inside `ShellState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressState {
    /// The item that is pressed or springing back, by its place in the current view's cells,
    /// or the open folder's.
    pub item: Option<usize>,
    /// Whether it is in the open folder's panel, rather than the view's own cells (FOLD-04).
    pub surface: Surface,
    /// Its icon's scale, from 0.92 to 1, and briefly a little over 1 as it springs back.
    pub scale: f64,
    /// It is still moving, so the host should draw another frame.
    pub animating: bool,
}

impl Default for PressState {
    /// Nothing pressed.
    fn default() -> Self {
        Self {
            item: None,
            surface: Surface::Grid,
            scale: 1.0,
            animating: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Shrinking from `from` towards 92 %, from `start`, over `length`.
    Shrinking {
        from: f64,
        start: Duration,
        length: Duration,
    },
    /// At 92 % while the finger stays.
    Held,
    /// Springing back to full size; `at` is the time the motion has reached, and its clock
    /// runs at the system animation `speed` (ANIM-05).
    Releasing {
        motion: SpringMotion,
        at: Duration,
        speed: f64,
    },
}

/// Runs the icon press. `HomeShell` owns it and publishes its [`PressState`].
#[derive(Debug)]
pub(crate) struct Press {
    /// The pressed item and where its motion is.
    pressed: Option<(usize, Phase)>,
    /// Its icon's scale now.
    scale: f64,
    surface: Surface,
}

impl Default for Press {
    fn default() -> Self {
        Self {
            pressed: None,
            scale: 1.0,
            surface: Surface::Grid,
        }
    }
}

impl Press {
    /// A finger went down on `item` on `surface` at `at`. Any other item that was pressed lets
    /// go at once.
    pub fn press(&mut self, surface: Surface, item: usize, at: Duration, animation_scale: f64) {
        let from = match self.pressed {
            Some((pressed, _)) if pressed == item && surface == self.surface => self.scale,
            _ => 1.0,
        };
        self.surface = surface;
        let length = ICON_PRESS.length_at(animation_scale);
        let phase = if length.is_zero() {
            self.scale = ICON_PRESS_SCALE;
            Phase::Held
        } else {
            self.scale = from;
            Phase::Shrinking {
                from,
                start: at,
                length,
            }
        };
        self.pressed = Some((item, phase));
    }

    /// The finger lifted or left the item at `at`, so it springs back (ANIM-10). With
    /// animations removed, it is full size at once (ANIM-05).
    pub fn release(&mut self, at: Duration, animation_scale: f64) {
        let Some((item, _)) = self.pressed else {
            return;
        };
        if ICON_PRESS.length_at(animation_scale).is_zero() {
            self.finish();
            return;
        }
        let motion = SpringMotion::new(ICON_RELEASE, self.scale - 1.0, 0.0);
        self.pressed = Some((
            item,
            Phase::Releasing {
                motion,
                at,
                speed: animation_scale,
            },
        ));
    }

    /// Ends its motion at once, where it was going: a held icon at 92 %, a released one at full
    /// size (ANIM-05).
    pub fn land(&mut self) {
        match self.pressed {
            Some((item, Phase::Shrinking { .. })) => {
                self.scale = ICON_PRESS_SCALE;
                self.pressed = Some((item, Phase::Held));
            }
            Some((_, Phase::Releasing { .. })) => self.finish(),
            Some((_, Phase::Held)) | None => {}
        }
    }

    /// Shows the item at full size at once, such as when Settings covers it.
    pub fn finish(&mut self) {
        self.pressed = None;
        self.scale = 1.0;
    }

    /// Advances the motion to the display's frame time (ANIM-04).
    pub fn frame(&mut self, at: Duration) {
        let Some((_, phase)) = &mut self.pressed else {
            return;
        };
        match phase {
            Phase::Shrinking {
                from,
                start,
                length,
            } => {
                let progress = at.saturating_sub(*start).as_secs_f64() / length.as_secs_f64();
                if progress >= 1.0 {
                    self.scale = ICON_PRESS_SCALE;
                    *phase = Phase::Held;
                } else {
                    self.scale =
                        *from + (ICON_PRESS_SCALE - *from) * ICON_PRESS.easing.at(progress);
                }
            }
            Phase::Held => {}
            Phase::Releasing {
                motion,
                at: reached,
                speed,
            } => {
                motion.advance(motion_time(at.saturating_sub(*reached), *speed));
                *reached = (*reached).max(at);
                if motion.is_at_rest(REST_DISTANCE_SCALE, REST_SPEED_SCALE * *speed) {
                    self.finish();
                } else {
                    self.scale = 1.0 + motion.displacement();
                }
            }
        }
    }

    pub fn state(&self) -> PressState {
        match self.pressed {
            None => PressState::default(),
            Some((item, phase)) => PressState {
                item: Some(item),
                surface: self.surface,
                scale: self.scale,
                animating: phase != Phase::Held,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn a_pressed_icon_shrinks_to_92_percent_in_90_ms() {
        // ANIM-10
        let mut press = Press::default();
        assert_eq!(press.state(), PressState::default());

        press.press(Surface::Grid, 3, ms(1_000), 1.0);
        let first = press.state();
        assert_eq!(
            (first.item, first.scale, first.animating),
            (Some(3), 1.0, true)
        );

        press.frame(ms(1_045));
        let half = press.state().scale;
        // Decelerating: more than half-way at half the time.
        assert!(half < 0.96 && half > ICON_PRESS_SCALE, "{half}");

        press.frame(ms(1_090));
        assert_eq!(
            press.state(),
            PressState {
                surface: Surface::Grid,
                item: Some(3),
                scale: ICON_PRESS_SCALE,
                animating: false,
            }
        );
    }

    #[test]
    fn a_frame_from_before_the_touch_doesnt_move_it() {
        // The display's frame time can come just before the touch's time.
        let mut press = Press::default();
        press.press(Surface::Grid, 0, ms(1_000), 1.0);

        press.frame(ms(995));

        assert_eq!(press.state().scale, 1.0);
    }

    #[test]
    fn released_it_springs_back_with_little_overshoot() {
        // ANIM-10, ANIM-02
        let mut press = Press::default();
        press.press(Surface::Grid, 3, ms(0), 1.0);
        press.frame(ms(90));
        press.release(ms(500), 1.0);
        assert!(press.state().animating);

        let mut largest: f64 = 0.0;
        let mut at = 500;
        while press.state().animating {
            at += 8;
            press.frame(ms(at));
            largest = largest.max(press.state().scale);
            assert!(at < 1_000, "still moving at {at} ms");
        }

        assert!(largest > 1.0, "{largest}");
        assert!(
            largest - 1.0 <= 0.03 * (1.0 - ICON_PRESS_SCALE),
            "{largest}"
        );
        assert!(at <= 750, "{at} ms");
        assert_eq!(press.state(), PressState::default());
    }

    #[test]
    fn a_quick_tap_springs_back_from_where_it_got_to() {
        // ANIM-03: the release catches the shrink where it is.
        let mut press = Press::default();
        press.press(Surface::Grid, 1, ms(0), 1.0);
        press.frame(ms(30));
        let reached = press.state().scale;

        press.release(ms(40), 1.0);
        press.frame(ms(40));

        assert_eq!(press.state().scale, reached);
        press.frame(ms(48));
        assert!(press.state().scale > reached);
    }

    #[test]
    fn pressing_another_item_lets_the_first_go() {
        let mut press = Press::default();
        press.press(Surface::Grid, 1, ms(0), 1.0);
        press.frame(ms(90));

        press.press(Surface::Grid, 2, ms(100), 1.0);

        let state = press.state();
        assert_eq!((state.item, state.scale), (Some(2), 1.0));
    }

    #[test]
    fn it_follows_the_system_animation_speed() {
        // ANIM-05
        let mut slow = Press::default();
        slow.press(Surface::Grid, 0, ms(0), 2.0);
        slow.frame(ms(90));
        assert!(slow.state().scale > ICON_PRESS_SCALE);
        slow.frame(ms(180));
        assert_eq!(slow.state().scale, ICON_PRESS_SCALE);

        let mut removed = Press::default();
        removed.press(Surface::Grid, 0, ms(0), 0.0);
        assert_eq!(
            removed.state(),
            PressState {
                surface: Surface::Grid,
                item: Some(0),
                scale: ICON_PRESS_SCALE,
                animating: false,
            }
        );
        removed.release(ms(10), 0.0);
        assert_eq!(removed.state(), PressState::default());
    }

    #[test]
    fn it_springs_back_at_the_system_animation_speed() {
        // ANIM-05: at half speed, the spring back takes twice as long.
        let back_in = |speed: f64| {
            let mut press = Press::default();
            press.press(Surface::Grid, 0, ms(0), speed);
            press.frame(ms(1_000));
            press.release(ms(1_000), speed);
            let mut at = 1_000;
            while press.state().animating {
                at += 4;
                press.frame(ms(at));
            }
            (at - 1_000) as f64
        };
        let (normal, slow) = (back_in(1.0), back_in(2.0));
        assert!(
            (1.8..=2.2).contains(&(slow / normal)),
            "{normal} ms, then {slow} ms"
        );
    }

    #[test]
    fn finishing_shows_it_at_full_size_at_once() {
        let mut press = Press::default();
        press.press(Surface::Grid, 0, ms(0), 1.0);
        press.frame(ms(50));
        assert_eq!(press.state().item, Some(0));

        press.finish();

        assert_eq!(press.state(), PressState::default());
        // A release with nothing pressed does nothing.
        press.release(ms(60), 1.0);
        assert_eq!(press.state(), PressState::default());
    }
}
