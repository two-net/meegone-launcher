//! Lock-screen image data layer (SRS 7.3):
//!
//! - the image cache, the one store this feature owns: the current and previous images and
//!   their details;
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
pub const IMAGE_CACHE_VERSION: u32 = 2;

/// Brings image details saved by older versions up to date, one version at a time.
const IMAGE_CACHE_MIGRATIONS: [Migration; IMAGE_CACHE_VERSION as usize - 1] = [add_image_file];

/// Version 2 names the file that holds each record's image (ADR 0022). Records saved before
/// had no image.
fn add_image_file(body: &str) -> Option<String> {
    let mut migrated = String::with_capacity(body.len() + 4);
    for line in body.lines() {
        let (key, value) = line.split_once(' ')?;
        migrated.push_str(line);
        if matches!(key, "current" | "previous") && value != "-" {
            migrated.push_str(" -");
        }
        migrated.push('\n');
    }
    Some(migrated)
}

/// One of the two files that hold the cached images (WALL-09).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFile {
    A,
    B,
}

impl ImageFile {
    pub const fn stored(self) -> StoredFile {
        match self {
            Self::A => StoredFile::ImageA,
            Self::B => StoredFile::ImageB,
        }
    }

    /// Where the file lives, relative to the storage root, so the host can draw the image from
    /// it (ADR 0022).
    pub const fn path(self) -> &'static str {
        self.stored().path()
    }

    fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

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
    /// The file that holds the image. [`ImageCache::put`] picks it; a record saved before
    /// images were kept has none.
    pub image: Option<ImageFile>,
}

pub trait ImageCache: Send + Sync {
    fn current(&self) -> Option<ImageRecord>;
    fn previous(&self) -> Option<ImageRecord>;
    /// Makes `record` current with its `image`, and keeps the one it replaces; older ones are
    /// deleted (WALL-09). The cache picks the file the image goes in, whatever `record.image`
    /// says.
    fn put(&self, record: ImageRecord, image: &[u8]) -> Result<(), DataError>;
    /// The bytes of `record`'s image, or `None` if it has none.
    fn image(&self, record: &ImageRecord) -> Result<Option<Vec<u8>>, DataError>;
    /// Deletes every cached image and its details (WALL-12).
    fn clear(&self) -> Result<(), DataError>;
}

/// Keeps the details of the current and previous images in [`StoredFile::ImageDetails`], and
/// the images themselves in [`StoredFile::ImageA`] and [`StoredFile::ImageB`].
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

    fn save(&self, cached: &Cached) -> Result<(), DataError> {
        save(&*self.storage, StoredFile::ImageDetails, cached)
    }
}

impl ImageCache for PersistentImageCache {
    fn current(&self) -> Option<ImageRecord> {
        self.cached().current
    }

    fn previous(&self) -> Option<ImageRecord> {
        self.cached().previous
    }

    /// Each step is one atomic write, so whenever it stops, every record that names a file
    /// names the one that holds its image (REL-03):
    /// 1. The new image goes in the file the current image doesn't use.
    /// 2. If the previous image is in that file, its record lets go of it first.
    /// 3. The image is written.
    /// 4. The details make it current, and the current one previous. The oldest record is gone,
    ///    and its file is the one the next image goes in.
    fn put(&self, mut record: ImageRecord, image: &[u8]) -> Result<(), DataError> {
        let _changing = self.changing();
        let mut cached = self.cached();
        let file = cached
            .current
            .as_ref()
            .and_then(|current| current.image)
            .map_or(ImageFile::A, ImageFile::other);
        if let Some(previous) = cached
            .previous
            .as_mut()
            .filter(|previous| previous.image == Some(file))
        {
            previous.image = None;
            self.save(&cached)?;
        }
        self.storage.write(file.stored(), image)?;
        record.image = Some(file);
        cached.previous = cached.current.replace(record);
        self.save(&cached)
    }

    fn image(&self, record: &ImageRecord) -> Result<Option<Vec<u8>>, DataError> {
        match record.image {
            Some(file) => self.storage.read(file.stored()),
            None => Ok(None),
        }
    }

    fn clear(&self) -> Result<(), DataError> {
        let _changing = self.changing();
        // The details go first, so nothing names an image while its file goes.
        for file in [
            StoredFile::ImageDetails,
            ImageFile::A.stored(),
            ImageFile::B.stored(),
        ] {
            self.storage.remove(file)?;
        }
        Ok(())
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
        self.image.encode(line);
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
            image: Option::decode(fields)?,
        })
    }
}

/// Saved as `a` or `b`.
impl Encode for ImageFile {
    fn encode(&self, line: &mut Line<'_>) {
        line.token(match self {
            Self::A => "a",
            Self::B => "b",
        });
    }
}

impl Decode for ImageFile {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        match fields.token()? {
            "a" => Some(Self::A),
            "b" => Some(Self::B),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::FakeStorage;
    use meegone_core_data::storage::format::seal;
    use meegone_core_data::storage::{MAX_STORED_BYTES, decode, encode};

    use super::*;

    /// A saved file, and the value it must load as.
    type Fixture = (&'static str, fn() -> Cached);

    /// Every saved version, oldest first, with the details it must load as (MAINT-04). Never
    /// change or remove a fixture; add one for each new version.
    const FIXTURES: [Fixture; IMAGE_CACHE_VERSION as usize] = [
        (include_str!("../fixtures/image-cache-v1.txt"), sample_v1),
        (include_str!("../fixtures/image-cache-v2.txt"), sample),
    ];

    fn record(date: &str) -> ImageRecord {
        ImageRecord {
            date: date.into(),
            region: "en-US".into(),
            title: "Title".into(),
            attribution: "© Someone".into(),
            info_link: None,
            applied_at_ms: None,
            image: None,
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
                image: Some(ImageFile::B),
            }),
            previous: Some(ImageRecord {
                image: Some(ImageFile::A),
                ..record("20261115")
            }),
        }
    }

    /// Version 1 kept no images.
    fn sample_v1() -> Cached {
        let mut cached = sample();
        for record in [&mut cached.current, &mut cached.previous]
            .into_iter()
            .flatten()
        {
            record.image = None;
        }
        cached
    }

    fn cache() -> (Arc<FakeStorage>, PersistentImageCache) {
        let storage = Arc::new(FakeStorage::default());
        (storage.clone(), PersistentImageCache::new(storage))
    }

    fn stored(storage: &FakeStorage) -> Vec<StoredFile> {
        storage.files.lock().unwrap().keys().copied().collect()
    }

    #[test]
    fn keeps_the_current_and_the_previous_image_only() {
        // WALL-09
        let (storage, cache) = cache();
        assert_eq!(cache.current(), None);

        cache.put(record("20261113"), b"monday").unwrap();
        cache.put(record("20261114"), b"tuesday").unwrap();
        cache.put(record("20261115"), b"wednesday").unwrap();

        let restarted = PersistentImageCache::new(storage.clone());
        let current = restarted.current().unwrap();
        let previous = restarted.previous().unwrap();
        assert_eq!(current.date, "20261115");
        assert_eq!(previous.date, "20261114");
        assert_eq!(restarted.image(&current), Ok(Some(b"wednesday".to_vec())));
        assert_eq!(restarted.image(&previous), Ok(Some(b"tuesday".to_vec())));
        // The oldest image is gone: its file holds the newest.
        assert_eq!(
            stored(&storage),
            [
                StoredFile::ImageDetails,
                StoredFile::ImageA,
                StoredFile::ImageB
            ]
        );
    }

    #[test]
    fn each_new_image_goes_in_the_file_the_current_one_doesnt_use() {
        // WALL-09
        let (_, cache) = cache();
        let mut files = Vec::new();

        for date in ["20261113", "20261114", "20261115"] {
            cache
                .put(
                    ImageRecord {
                        // Whatever the caller says, the cache picks the file.
                        image: Some(ImageFile::B),
                        ..record(date)
                    },
                    date.as_bytes(),
                )
                .unwrap();
            files.push(cache.current().unwrap().image.unwrap());
        }

        assert_eq!(files, [ImageFile::A, ImageFile::B, ImageFile::A]);
    }

    #[test]
    fn a_record_without_an_image_has_no_bytes() {
        let (storage, cache) = cache();
        storage
            .files
            .lock()
            .unwrap()
            .insert(StoredFile::ImageDetails, encode(&sample_v1()));

        let current = cache.current().unwrap();

        assert_eq!(cache.image(&current), Ok(None));
        // An image after it goes in the first file.
        cache.put(record("20261117"), b"new").unwrap();
        assert_eq!(cache.current().unwrap().image, Some(ImageFile::A));
    }

    #[test]
    fn clearing_deletes_every_cached_image() {
        // WALL-12
        let (storage, cache) = cache();
        cache.put(record("20261115"), b"one").unwrap();
        cache.put(record("20261116"), b"two").unwrap();

        cache.clear().unwrap();

        assert_eq!(cache.current(), None);
        assert!(storage.files.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_save_leaves_the_previous_details() {
        // REL-03
        let (storage, cache) = cache();
        cache.put(record("20261115"), b"one").unwrap();
        *storage.error.lock().unwrap() = Some(DataError::Failed("Disk error".into()));

        assert!(cache.put(record("20261116"), b"two").is_err());

        *storage.error.lock().unwrap() = None;
        let current = cache.current().unwrap();
        assert_eq!(current.date, "20261115");
        assert_eq!(cache.image(&current), Ok(Some(b"one".to_vec())));
        assert_eq!(cache.previous(), None);
    }

    #[test]
    fn an_image_that_cant_be_written_never_mixes_up_the_images() {
        // REL-03, PERF-08: the new image would go in the previous image's file, so that record
        // lets go of it first. The current image stays as it was.
        let (_, cache) = cache();
        cache.put(record("20261114"), b"tuesday").unwrap();
        cache.put(record("20261115"), b"wednesday").unwrap();

        let too_big = vec![0; MAX_STORED_BYTES as usize + 1];
        assert!(cache.put(record("20261116"), &too_big).is_err());

        let current = cache.current().unwrap();
        let previous = cache.previous().unwrap();
        assert_eq!(current.date, "20261115");
        assert_eq!(cache.image(&current), Ok(Some(b"wednesday".to_vec())));
        assert_eq!(previous.date, "20261114");
        assert_eq!(previous.image, None);
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
            seal(
                "image-cache",
                IMAGE_CACHE_VERSION,
                "current 1 en-US T A - - c\nprevious -\n",
            ),
            seal("image-cache", 1, "current\nprevious -\n"),
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
