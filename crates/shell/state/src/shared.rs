//! What the home and the daily image's job share in one process (ADR 0029).
//!
//! The system may start the job with or without the home, and on another thread. Both use one
//! settings store and one image cache, so each sees the other's writes at once: a choice made
//! in Settings counts for the next check, and turning the daily image off while it downloads
//! leaves nothing cached (WALL-12).

use std::sync::Arc;

use meegone_core_data::{PersistentSettingsStore, Platform};
use meegone_lockimage_data::{BingFeed, PersistentImageCache};
use meegone_lockimage_state::ImageUpdater;

/// The platform adapters, the stores both the home and the job write, and the daily image's
/// updater. Built once per process; clones share everything.
#[derive(Clone)]
pub struct Shared {
    pub(crate) platform: Platform,
    pub(crate) settings: Arc<PersistentSettingsStore>,
    pub(crate) image_cache: Arc<PersistentImageCache>,
    /// The feed's only implementation, which alone knows its format (ADR 0027).
    pub(crate) feed: Arc<BingFeed>,
    updater: Arc<ImageUpdater>,
}

impl Shared {
    /// Does no I/O.
    pub fn new(platform: Platform) -> Self {
        let settings = Arc::new(PersistentSettingsStore::new(platform.storage.clone()));
        let image_cache = Arc::new(PersistentImageCache::new(platform.storage.clone()));
        let feed = Arc::new(BingFeed::new(platform.network.clone()));
        let updater = Arc::new(ImageUpdater::new(
            settings.clone(),
            image_cache.clone(),
            feed.clone(),
            platform.network.clone(),
            platform.wallpaper.clone(),
            platform.scheduler.clone(),
            platform.system.clone(),
        ));
        Self {
            platform,
            settings,
            image_cache,
            feed,
            updater,
        }
    }

    /// Runs the daily image's check if one is due, and schedules the job for the next one
    /// (ADR 0029). It blocks: on Android, call it from a JVM thread other than the main one.
    ///
    /// Returns whether it reached the feed, so the home should read the image and the checks'
    /// status again (`ShellIntent::ImageChecked`).
    pub fn check_image(&self) -> bool {
        self.updater.check().attempted()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_job_thread_can_hold_it() {
        fn send_and_sync<T: Send + Sync>() {}
        send_and_sync::<Shared>();
    }
}
