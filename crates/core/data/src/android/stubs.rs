//! The platform adapters whose Android implementations arrive with later issues.
//!
//! Each answers with its "if unavailable" value from SRS 8.1, so every feature shows its
//! reduced state meanwhile. The app catalogue (`apps.rs`), notifications (`notifications.rs`),
//! usage (`usage.rs`) and the network (`network.rs`) are built, and so is part of the wallpaper
//! (`wallpaper.rs`).

use super::not_yet;
use crate::adapters::SchedulerAdapter;
use crate::model::{DataError, ImageJob};

/// Every adapter not built yet.
pub struct NotBuiltYet;

/// The daily image job arrives with #28.
impl SchedulerAdapter for NotBuiltYet {
    fn schedule_image_job(&self, _job: ImageJob) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn cancel_image_job(&self) -> Result<(), DataError> {
        Err(not_yet())
    }
}
