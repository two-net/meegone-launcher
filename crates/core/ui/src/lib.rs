//! UI layer shared by every feature (SRS 7.3): colour tokens (SRS 5.3), motion specifications
//! (SRS 4.7) and the view contracts every feature's UI implements.
//!
//! Android's own views draw the home screen (ADR 0006). [`View`] and [`Slot`] stay
//! toolkit-agnostic: the Java host asks the Rust app what to draw.

pub mod tokens;
mod view;

pub use tokens::{Tokens, tokens};
pub use view::{Dispatch, Latest, Slot, View};

pub use meegone_core_state::Rgb;

/// The motion specifications live in the shared core state, which moving elements need (ADR 0006).
pub use meegone_core_state::motion;
