//! State layer shared by every feature (SRS 7.3).
//!
//! - [`StateHolder`] and [`Store`]: one state holder per feature, publishing immutable
//!   snapshots and containing failures (ARCH-05, ARCH-09).
//! - [`ThemeHolder`]: theme resolution, used by the shell and by Settings (THEME-01, THEME-02).
//! - [`motion`]: motion specifications and the spring physics that gesture-driven motion runs
//!   on (SRS 4.7, ADR 0006).
//!
//! The data models a UI may render are re-exported here, so UI crates never depend on a data
//! crate (ARCH-02).

// A failing feature is contained by catching its panic, which needs them to unwind (ARCH-09,
// ADR 0043). This holds however the strategy is set: a profile, `.cargo/config.toml`,
// `RUSTFLAGS` or `CARGO_PROFILE_*_PANIC`.
#[cfg(not(panic = "unwind"))]
compile_error!(
    "MeeGone contains a failing feature by catching its panic, so panics must unwind: never set \
     `panic = \"abort\"` (ARCH-09, REL-07, ADR 0043)"
);

pub mod motion;
mod store;
mod theme;

#[cfg(any(test, feature = "faults"))]
pub use store::faults;
pub use store::{Health, StateHolder, Store};
pub use theme::{Theme, ThemeHolder, ThemeIntent, ThemeState, resolve_theme};

pub use meegone_core_data::{
    Access, App, AppId, HomeTarget, Notification, Problem, ProfileId, RecentDensity, RecentWindow,
    Rect, Rgb, Shortcut, ThemeChoice,
};
