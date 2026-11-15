//! State layer shared by every feature (SRS 7.3).
//!
//! - [`StateHolder`] and [`Store`]: one state holder per feature, publishing immutable
//!   snapshots and containing failures (ARCH-05, ARCH-09).
//! - [`ThemeHolder`]: theme resolution, used by the shell and by Settings (THEME-01, THEME-02).
//!
//! The data models a UI may render are re-exported here, so UI crates never depend on a data
//! crate (ARCH-02).

mod store;
mod theme;

pub use store::{Health, StateHolder, Store};
pub use theme::{Theme, ThemeHolder, ThemeIntent, ThemeState, resolve_theme};

pub use meegone_core_data::{
    Access, App, AppId, HomeTarget, Notification, ProfileId, RecentDensity, Rgb, ThemeChoice,
};
