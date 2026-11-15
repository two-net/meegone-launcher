//! UI layer shared by every feature (SRS 7.3): colour tokens (SRS 5.3), motion specifications
//! (SRS 4.7) and the view contracts every feature's UI implements.
//!
//! No UI toolkit has been chosen yet (ADR 0002), so [`View`] and [`Slot`] are toolkit-agnostic
//! placeholders.

pub mod motion;
pub mod tokens;
mod view;

pub use tokens::{Tokens, tokens};
pub use view::{Dispatch, Latest, Slot, View};
