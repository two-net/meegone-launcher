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

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meegone_core_data::storage::format::{Fields, Line};
use meegone_core_data::storage::{Decode, Encode, Format, Migration, Reader, Writer, load, save};
use meegone_core_data::{DataError, StorageAdapter, StoredFile};

pub use feed::{FeedImage, ImageFeed, ImageVersion};

/// Version of the saved image-details format (MAINT-04). Changing the format means a new
/// version, a migration in [`IMAGE_CACHE_MIGRATIONS`] and a fixture (ARCHITECTURE.md,
/// "Persistence").
pub const IMAGE_CACHE_VERSION: u32 = 1;

/// Brings image details saved by older versions up to date, one version at a time.
const IMAGE_CACHE_MIGRATIONS: [Migration; IMAGE_CACHE_VERSION as usize - 1] = [];

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

/// Keeps the details of the current and previous images in [`StoredFile::ImageDetails`]. The
/// images themselves join them with #27.
pub struct PersistentImageCache {
    storage: Arc<dyn StorageAdapter>,
    /// One change at a time, so two `put`s never lose each other's record.
    changing: Mutex<()>,
}

impl PersistentImageCache {
    pub fn new(storage: Arc<dyn StorageAdapter>) -> Self {
        Self {
            storage,
            changing: Mutex::new(()),
        }
    }

    fn cached(&self) -> Cached {
        load(&*self.storage, StoredFile::ImageDetails)
    }

    fn changing(&self) -> MutexGuard<'_, ()> {
        self.changing.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ImageCache for PersistentImageCache {
    fn current(&self) -> Option<ImageRecord> {
        self.cached().current
    }

    fn previous(&self) -> Option<ImageRecord> {
        self.cached().previous
    }

    fn put(&self, record: ImageRecord) -> Result<(), DataError> {
        let _changing = self.changing();
        let mut cached = self.cached();
        cached.previous = cached.current.replace(record);
        save(&*self.storage, StoredFile::ImageDetails, &cached)
    }

    fn clear(&self) -> Result<(), DataError> {
        let _changing = self.changing();
        self.storage.remove(StoredFile::ImageDetails)
    }
}

/// What the image cache keeps (WALL-09).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Cached {
    current: Option<ImageRecord>,
    previous: Option<ImageRecord>,
}

/// Two records, `current` and `previous`, each holding a record or none.
impl Format for Cached {
    const KIND: &'static str = "image-cache";
    const VERSION: u32 = IMAGE_CACHE_VERSION;
    const MIGRATIONS: &'static [Migration] = &IMAGE_CACHE_MIGRATIONS;

    fn write(&self, out: &mut Writer) {
        out.line("current", &self.current)
            .line("previous", &self.previous);
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            current: input.line("current")?,
            previous: input.line("previous")?,
        })
    }
}

impl Encode for ImageRecord {
    fn encode(&self, line: &mut Line<'_>) {
        self.date.encode(line);
        self.region.encode(line);
        self.title.encode(line);
        self.attribution.encode(line);
        self.info_link.encode(line);
        self.applied_at_ms.encode(line);
    }
}

impl Decode for ImageRecord {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        Some(Self {
            date: String::decode(fields)?,
            region: String::decode(fields)?,
            title: String::decode(fields)?,
            attribution: String::decode(fields)?,
            info_link: Option::decode(fields)?,
            applied_at_ms: Option::decode(fields)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::FakeStorage;
    use meegone_core_data::storage::format::seal;
    use meegone_core_data::storage::{decode, encode};

    use super::*;

    /// A saved file, and the value it must load as.
    type Fixture = (&'static str, fn() -> Cached);

    /// Every saved version, oldest first, with the details it must load as (MAINT-04). Never
    /// change or remove a fixture; add one for each new version.
    const FIXTURES: [Fixture; IMAGE_CACHE_VERSION as usize] =
        [(include_str!("../fixtures/image-cache-v1.txt"), sample)];

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

    fn sample() -> Cached {
        Cached {
            current: Some(ImageRecord {
                date: "20261116".into(),
                region: "en-GB".into(),
                title: "Puffins on a cliff, Skomer".into(),
                attribution: "© A Photographer / Agency".into(),
                info_link: Some("https://example.com/details?id=1 2".into()),
                applied_at_ms: Some(1_763_272_800_000),
            }),
            previous: Some(record("20261115")),
        }
    }

    fn cache() -> (Arc<FakeStorage>, PersistentImageCache) {
        let storage = Arc::new(FakeStorage::default());
        (storage.clone(), PersistentImageCache::new(storage))
    }

    #[test]
    fn keeps_the_current_and_the_previous_image_only() {
        // WALL-09
        let (storage, cache) = cache();
        assert_eq!(cache.current(), None);

        cache.put(record("20261113")).unwrap();
        cache.put(record("20261114")).unwrap();
        cache.put(record("20261115")).unwrap();

        let restarted = PersistentImageCache::new(storage);
        assert_eq!(restarted.current().unwrap().date, "20261115");
        assert_eq!(restarted.previous().unwrap().date, "20261114");
    }

    #[test]
    fn clearing_deletes_every_cached_image() {
        // WALL-12
        let (storage, cache) = cache();
        cache.put(record("20261115")).unwrap();

        cache.clear().unwrap();

        assert_eq!(cache.current(), None);
        assert!(storage.files.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_save_leaves_the_previous_details() {
        // REL-03
        let (storage, cache) = cache();
        cache.put(record("20261115")).unwrap();
        *storage.error.lock().unwrap() = Some(DataError::Failed("Disk error".into()));

        assert!(cache.put(record("20261116")).is_err());

        *storage.error.lock().unwrap() = None;
        assert_eq!(cache.current().unwrap().date, "20261115");
        assert_eq!(cache.previous(), None);
    }

    #[test]
    fn damaged_or_newer_details_load_as_empty() {
        // MAINT-04
        let saved = encode(&sample());
        let mut damaged = saved.clone();
        damaged[25] ^= 0x01;

        for bytes in [
            seal(
                "image-cache",
                IMAGE_CACHE_VERSION + 1,
                "current -\nprevious -\n",
            ),
            seal("image-cache", IMAGE_CACHE_VERSION, "current -\n"),
            seal(
                "image-cache",
                IMAGE_CACHE_VERSION,
                "current 1 2 3\nprevious -\n",
            ),
            damaged,
            b"garbage".to_vec(),
        ] {
            let (storage, cache) = cache();
            storage
                .files
                .lock()
                .unwrap()
                .insert(StoredFile::ImageDetails, bytes);

            assert_eq!(cache.current(), None);
            assert_eq!(cache.previous(), None);
        }
    }

    #[test]
    fn every_saved_version_still_loads() {
        // MAINT-04: one fixture per version, each brought up to today's details.
        for (version, (fixture, expected)) in (1..).zip(FIXTURES) {
            assert_eq!(
                decode::<Cached>(fixture.as_bytes()),
                Some(expected()),
                "image-cache-v{version}.txt"
            );
        }
    }

    #[test]
    fn the_latest_fixture_is_what_is_saved_today() {
        let (fixture, expected) = FIXTURES[FIXTURES.len() - 1];

        assert_eq!(String::from_utf8(encode(&expected())).unwrap(), fixture);
    }
}
