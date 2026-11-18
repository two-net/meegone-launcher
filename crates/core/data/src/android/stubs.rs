//! The platform adapters whose Android implementations arrive with later issues.
//!
//! Each answers with its "if unavailable" value from SRS 8.1, so every feature shows its
//! reduced state meanwhile: no lock-screen image. The app catalogue (`apps.rs`), notifications
//! (`notifications.rs`), usage (`usage.rs`) and the network (`network.rs`) are built.

use super::not_yet;
use crate::adapters::{SchedulerAdapter, WallpaperAdapter};
use crate::model::{DataError, ImageJob, Size};

/// Every adapter not built yet.
pub struct NotBuiltYet;

/// The lock-screen wallpaper arrives with #29.
impl WallpaperAdapter for NotBuiltYet {
    fn lock_screen_support(&self) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn lock_screen_size(&self) -> Size {
        Size::default()
    }

    fn set_lock_screen(&self, _image: &[u8]) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn reset_lock_screen(&self) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn open_wallpaper_settings(&self) -> Result<(), DataError> {
        Err(not_yet())
    }
}

/// The daily image job arrives with #28.
impl SchedulerAdapter for NotBuiltYet {
    fn schedule_image_job(&self, _job: ImageJob) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn cancel_image_job(&self) -> Result<(), DataError> {
        Err(not_yet())
    }
}
