//! Lock-screen image data layer (SRS 7.3):
//!
//! - the image cache, the one store this feature owns;
//! - [`ImageFeed`], the interface to the image-of-the-day feed. Only its implementation knows
//!   the feed's format (ARCH-04).
//!
//! The Bing implementation of the feed (#26) and the daily updater (SRS 7.5) arrive with the
//! WALL requirements.

#[cfg(feature = "fakes")]
pub mod fakes;
mod feed;

use std::sync::{Mutex, MutexGuard, PoisonError};

use meegone_core_data::DataError;

pub use feed::{FeedImage, ImageFeed, ImageVersion};

/// The details of one image of the day (SRS 7.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRecord {
    /// Publication date as the feed gives it, for example `20261115`.
    pub date: String,
    pub region: String,
    pub title: String,
    /// Shown as the source provides it (LEGAL-03).
    pub attribution: String,
    pub info_link: Option<String>,
    /// When MeeGone set it as the lock-screen wallpaper.
    pub applied_at_ms: Option<u64>,
}

pub trait ImageCache: Send + Sync {
    fn current(&self) -> Option<ImageRecord>;
    fn previous(&self) -> Option<ImageRecord>;
    /// Makes `record` current and keeps the one it replaces; older ones are deleted (WALL-09).
    fn put(&self, record: ImageRecord) -> Result<(), DataError>;
    /// Deletes every cached image (WALL-12).
    fn clear(&self) -> Result<(), DataError>;
}

/// Keeps image details in memory. Used until the on-disk cache lands (WALL-09).
#[derive(Debug, Default)]
pub struct MemoryImageCache(Mutex<Cached>);

#[derive(Debug, Default)]
struct Cached {
    current: Option<ImageRecord>,
    previous: Option<ImageRecord>,
}

impl MemoryImageCache {
    fn lock(&self) -> MutexGuard<'_, Cached> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ImageCache for MemoryImageCache {
    fn current(&self) -> Option<ImageRecord> {
        self.lock().current.clone()
    }

    fn previous(&self) -> Option<ImageRecord> {
        self.lock().previous.clone()
    }

    fn put(&self, record: ImageRecord) -> Result<(), DataError> {
        let mut cached = self.lock();
        cached.previous = cached.current.replace(record);
        Ok(())
    }

    fn clear(&self) -> Result<(), DataError> {
        *self.lock() = Cached::default();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(date: &str) -> ImageRecord {
        ImageRecord {
            date: date.into(),
            region: "en-US".into(),
            title: "Title".into(),
            attribution: "© Someone".into(),
            info_link: None,
            applied_at_ms: None,
        }
    }

    #[test]
    fn keeps_the_current_and_the_previous_image_only() {
        // WALL-09
        let cache = MemoryImageCache::default();

        cache.put(record("20261113")).unwrap();
        cache.put(record("20261114")).unwrap();
        cache.put(record("20261115")).unwrap();

        assert_eq!(cache.current().unwrap().date, "20261115");
        assert_eq!(cache.previous().unwrap().date, "20261114");
    }
}
