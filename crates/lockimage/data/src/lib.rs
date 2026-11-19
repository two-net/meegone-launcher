//! Lock-screen image data layer (SRS 7.3):
//!
//! - the image cache, the one store this feature owns: the current and previous images and
//!   their details;
//! - [`ImageFeed`], the interface to the image-of-the-day feed, and [`BingFeed`], its one
//!   implementation and the only code that knows the feed's format (ARCH-04, ADR 0027);
//! - [`region`]: the feed region, Automatic or chosen (WALL-05);
//! - [`Downloader`]: turns the feed's image into the lock-screen image. It picks a version,
//!   downloads it, checks it, has the platform crop it, and caches the result (WALL-06 to
//!   WALL-09, ADR 0028).
//! - [`Checks`]: what the daily image's checks have done, kept with the images so the updater
//!   keeps to its budget across restarts (WALL-03, REL-04, ADR 0029).
//! - [`Marker`]: how the lock screen took the image MeeGone set, so a change by someone else
//!   can be told (WALL-14, ADR 0030).
//! - [`ImageMemory`], kept in memory only: [`PastImages`], the past week's images and their
//!   previews, for browsing and pinning one (WALL-17), and [`ImageTasks`], what the user asked
//!   of the image job's thread: "Update now", browsing and pinning (ADR 0032).
//!
//! The updater that decides when to check (SRS 7.5) is in `meegone-lockimage-state`.

mod bing;
mod download;
#[cfg(feature = "fakes")]
pub mod fakes;
mod feed;
mod jpeg;
mod json;
mod past;
pub mod region;
mod tasks;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meegone_core_data::storage::format::{Fields, Line};
use meegone_core_data::storage::{Decode, Encode, Format, Migration, Reader, Writer, load, save};
use meegone_core_data::{DataError, StorageAdapter, StoredFile, WallpaperMarker};

pub use bing::BingFeed;
pub use download::{
    Downloader, IMAGE_TOO_LARGE, IMAGE_TOO_SMALL, LARGE_SCREEN_DP, LOW_STORAGE, MAX_DOWNLOAD_BYTES,
    MAX_SIDE, MIN_SHORT_SIDE, NOT_AN_IMAGE, check_image, lock_screen_target, pick_version,
    plan_crop,
};
pub use feed::{FORMAT_CHANGED, FeedImage, ImageFeed, ImageVersion, PAST_WEEK_IMAGES};
pub use past::{
    MAX_PREVIEW_BYTES, MAX_PREVIEW_SIDE, PREVIEW_TOO_LARGE, PastImage, PastImages, Preview, Week,
    check_preview,
};
pub use tasks::{ImageTask, ImageTasks};

/// What the lock-screen image keeps in memory only, which the home and the image job's thread
/// share (ADR 0032): the past week as last browsed, and what the user asked of the job's
/// thread. Nothing of it is stored (PERF-08).
#[derive(Debug, Default)]
pub struct ImageMemory {
    pub past: PastImages,
    pub tasks: ImageTasks,
}

/// Version of the saved image-details format (MAINT-04). Changing the format means a new
/// version, a migration in [`IMAGE_CACHE_MIGRATIONS`] and a fixture (ARCHITECTURE.md,
/// "Persistence").
pub const IMAGE_CACHE_VERSION: u32 = 5;

/// Brings image details saved by older versions up to date, one version at a time.
const IMAGE_CACHE_MIGRATIONS: [Migration; IMAGE_CACHE_VERSION as usize - 1] =
    [add_image_file, add_ids_and_checks, add_markers, add_pins];

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

/// Version 3 keeps the feed's id with each record, and the check log (ADR 0029). Records saved
/// before have no id, so the next check takes the feed's image as a new one; there were no
/// checks.
fn add_ids_and_checks(body: &str) -> Option<String> {
    let mut migrated = String::with_capacity(body.len() + 16);
    for line in body.lines() {
        let (key, value) = line.split_once(' ')?;
        migrated.push_str(line);
        if matches!(key, "current" | "previous") && value != "-" {
            migrated.push_str(" -");
        }
        migrated.push('\n');
    }
    migrated.push_str("checks -\n");
    Some(migrated)
}

/// Version 4 keeps the marker of the wallpaper each image was set as (ADR 0030). Images saved
/// before were never set, so they have none.
fn add_markers(body: &str) -> Option<String> {
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

/// Version 5 keeps whether the current image is pinned (WALL-17, ADR 0032). Nothing was pinned
/// before.
fn add_pins(body: &str) -> Option<String> {
    let mut migrated = String::with_capacity(body.len() + 12);
    for line in body.lines() {
        let (key, value) = line.split_once(' ')?;
        migrated.push_str(line);
        if matches!(key, "current" | "previous") && value != "-" {
            migrated.push_str(" false");
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
    /// The feed's id for the image, which tells it apart from the next day's (WALL-03). A
    /// record saved before ids were kept has none.
    pub id: Option<String>,
    /// How the lock screen took the image when MeeGone set it (WALL-14). `None` until it is
    /// set, and where the platform can't tell wallpapers apart.
    pub marker: Option<Marker>,
    /// Whether the user pinned it, so daily updates wait until it is unpinned (WALL-17). Only
    /// the current image's counts; [`ImageCache::put`] and [`ImageCache::choose`] unpin the
    /// image they make previous.
    pub pinned: bool,
}

impl ImageRecord {
    /// Whether MeeGone has set it as the lock-screen wallpaper.
    pub fn applied(&self) -> bool {
        self.applied_at_ms.is_some()
    }
}

/// How the lock screen took an image MeeGone set as its wallpaper (WALL-14, SRS C5,
/// ADR 0030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// The system's marker for it, read back as the lock screen's own. While the lock screen
    /// has another, someone else changed it.
    Set(WallpaperMarker),
    /// Right after it was set, the lock screen had another wallpaper: the device didn't keep
    /// it, so the image may not appear there.
    NotKept,
}

/// What the daily image's checks have done (ADR 0029). The updater keeps it, so its budget
/// holds across restarts (WALL-03, REL-04), and Settings says how the checks went (SRS 7.6).
///
/// Counts are for one local day, [`Checks::day`]; the updater starts them again each day.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Checks {
    /// The local day the counts are for, in days since 1 January 1970.
    pub day: u64,
    /// Network attempts that day, at most 6 (REL-04).
    pub attempts: u32,
    /// Checks that day that found no new image (WALL-03).
    pub same: u32,
    /// Whether that day's new image was cached.
    pub updated: bool,
    /// Attempts in a row that failed that day, for the back-off (REL-04).
    pub failures: u32,
    /// When the last attempt started.
    pub last_attempt_ms: Option<u64>,
    /// When the first attempt failed since the feed last answered (SRS 7.6).
    pub failing_since_ms: Option<u64>,
    /// The last failure, in plain words for Settings (WALL-16).
    pub problem: Option<String>,
}

pub trait ImageCache: Send + Sync {
    fn current(&self) -> Option<ImageRecord>;
    fn previous(&self) -> Option<ImageRecord>;
    /// Makes `record` current with its `image`, and keeps the one it replaces; older ones are
    /// deleted (WALL-09). The cache picks the file the image goes in, whatever `record.image`
    /// says. Returns the record as cached, naming that file.
    fn put(&self, record: ImageRecord, image: &[u8]) -> Result<ImageRecord, DataError>;
    /// The bytes of `record`'s image, or `None` if it has none.
    fn image(&self, record: &ImageRecord) -> Result<Option<Vec<u8>>, DataError>;
    /// Records that the current image, in `file`, was set as the lock-screen wallpaper at
    /// `at_ms`, with its `marker` (SRS 7.5). Returns the record as saved, or `None` if the
    /// current image is no longer the one in `file`, which then stays as it is.
    fn applied(
        &self,
        file: ImageFile,
        at_ms: u64,
        marker: Option<Marker>,
    ) -> Result<Option<ImageRecord>, DataError>;
    /// Makes the image in `file` current, pinned or not (WALL-17), without touching any image:
    /// the current one stays, or the previous one and the current one change places. Returns
    /// the record as saved, or `None` if no record's image is in `file`.
    ///
    /// The previous image comes back as not yet set on the lock screen, with no time or
    /// marker: its old marker would look like someone else's wallpaper (WALL-14).
    fn choose(&self, file: ImageFile, pinned: bool) -> Result<Option<ImageRecord>, DataError>;
    /// Deletes every cached image and its details, the check log included (WALL-12).
    fn clear(&self) -> Result<(), DataError>;
    /// What the daily image's checks have done (ADR 0029).
    fn checks(&self) -> Checks;
    /// Changes the check log in one write that no `put` or `clear` interleaves with, and
    /// returns it as saved. A failed save leaves it as it was.
    fn update_checks(&self, change: &mut dyn FnMut(&mut Checks)) -> Result<Checks, DataError>;
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
    fn put(&self, mut record: ImageRecord, image: &[u8]) -> Result<ImageRecord, DataError> {
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
        cached.previous = cached.current.replace(record.clone());
        if let Some(previous) = cached.previous.as_mut() {
            previous.pinned = false;
        }
        self.save(&cached)?;
        Ok(record)
    }

    fn image(&self, record: &ImageRecord) -> Result<Option<Vec<u8>>, DataError> {
        match record.image {
            Some(file) => self.storage.read(file.stored()),
            None => Ok(None),
        }
    }

    fn applied(
        &self,
        file: ImageFile,
        at_ms: u64,
        marker: Option<Marker>,
    ) -> Result<Option<ImageRecord>, DataError> {
        let _changing = self.changing();
        let mut cached = self.cached();
        let Some(current) = cached
            .current
            .as_mut()
            .filter(|current| current.image == Some(file))
        else {
            return Ok(None);
        };
        current.applied_at_ms = Some(at_ms);
        current.marker = marker;
        let record = current.clone();
        self.save(&cached)?;
        Ok(Some(record))
    }

    fn choose(&self, file: ImageFile, pinned: bool) -> Result<Option<ImageRecord>, DataError> {
        let _changing = self.changing();
        let mut cached = self.cached();
        let holds = |record: &Option<ImageRecord>| {
            record
                .as_ref()
                .is_some_and(|record| record.image == Some(file))
        };
        if !holds(&cached.current) {
            if !holds(&cached.previous) {
                return Ok(None);
            }
            std::mem::swap(&mut cached.current, &mut cached.previous);
            if let Some(current) = cached.current.as_mut() {
                current.applied_at_ms = None;
                current.marker = None;
            }
            if let Some(previous) = cached.previous.as_mut() {
                previous.pinned = false;
            }
        }
        let Some(current) = cached.current.as_mut() else {
            return Ok(None);
        };
        current.pinned = pinned;
        let record = current.clone();
        self.save(&cached)?;
        Ok(Some(record))
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

    fn checks(&self) -> Checks {
        self.cached().checks
    }

    fn update_checks(&self, change: &mut dyn FnMut(&mut Checks)) -> Result<Checks, DataError> {
        let _changing = self.changing();
        let mut cached = self.cached();
        change(&mut cached.checks);
        self.save(&cached)?;
        Ok(cached.checks)
    }
}

/// What the image cache keeps (WALL-09), and the check log (ADR 0029).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Cached {
    current: Option<ImageRecord>,
    previous: Option<ImageRecord>,
    checks: Checks,
}

/// Three records: `current` and `previous`, each holding a record or none, and `checks`, none
/// until a check has been made.
impl Format for Cached {
    const KIND: &'static str = "image-cache";
    const VERSION: u32 = IMAGE_CACHE_VERSION;
    const MIGRATIONS: &'static [Migration] = &IMAGE_CACHE_MIGRATIONS;

    fn write(&self, out: &mut Writer) {
        let checks = (self.checks != Checks::default()).then_some(&self.checks);
        out.line("current", &self.current)
            .line("previous", &self.previous)
            .line("checks", checks);
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            current: input.line("current")?,
            previous: input.line("previous")?,
            checks: input.line::<Option<Checks>>("checks")?.unwrap_or_default(),
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
        self.id.encode(line);
        self.marker.encode(line);
        self.pinned.encode(line);
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
            id: Option::decode(fields)?,
            marker: Option::decode(fields)?,
            pinned: bool::decode(fields)?,
        })
    }
}

impl Encode for Checks {
    fn encode(&self, line: &mut Line<'_>) {
        self.day.encode(line);
        self.attempts.encode(line);
        self.same.encode(line);
        self.updated.encode(line);
        self.failures.encode(line);
        self.last_attempt_ms.encode(line);
        self.failing_since_ms.encode(line);
        self.problem.encode(line);
    }
}

impl Decode for Checks {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        Some(Self {
            day: u64::decode(fields)?,
            attempts: u32::decode(fields)?,
            same: u32::decode(fields)?,
            updated: bool::decode(fields)?,
            failures: u32::decode(fields)?,
            last_attempt_ms: Option::decode(fields)?,
            failing_since_ms: Option::decode(fields)?,
            problem: Option::decode(fields)?,
        })
    }
}

/// Saved as the system's number for it, or `not-kept`.
impl Encode for Marker {
    fn encode(&self, line: &mut Line<'_>) {
        match self {
            Self::Set(marker) => line.token(&marker.0.to_string()),
            Self::NotKept => line.token(NOT_KEPT),
        }
    }
}

impl Decode for Marker {
    fn decode(fields: &mut Fields<'_>) -> Option<Self> {
        match fields.token()? {
            NOT_KEPT => Some(Self::NotKept),
            // Only as written: `+1` or `01` isn't a marker.
            token => token
                .parse()
                .ok()
                .filter(|number: &i32| number.to_string() == token)
                .map(|number| Self::Set(WallpaperMarker(number))),
        }
    }
}

const NOT_KEPT: &str = "not-kept";

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
        (include_str!("../fixtures/image-cache-v2.txt"), sample_v2),
        (include_str!("../fixtures/image-cache-v3.txt"), sample_v3),
        (include_str!("../fixtures/image-cache-v4.txt"), sample_v4),
        (include_str!("../fixtures/image-cache-v5.txt"), sample),
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
            id: None,
            marker: None,
            pinned: false,
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
                id: Some("OHR.Puffins_EN-GB0123456789".into()),
                marker: Some(Marker::Set(WallpaperMarker(57))),
                pinned: true,
            }),
            previous: Some(ImageRecord {
                image: Some(ImageFile::A),
                ..record("20261115")
            }),
            checks: Checks {
                day: 20_408,
                attempts: 3,
                same: 1,
                updated: false,
                failures: 2,
                last_attempt_ms: Some(1_763_283_600_000),
                failing_since_ms: Some(1_763_280_000_000),
                problem: Some("No connection".into()),
            },
        }
    }

    /// Version 4 kept no pins.
    fn sample_v4() -> Cached {
        let mut cached = sample();
        for record in [&mut cached.current, &mut cached.previous]
            .into_iter()
            .flatten()
        {
            record.pinned = false;
        }
        cached
    }

    /// Version 3 kept no markers.
    fn sample_v3() -> Cached {
        let mut cached = sample_v4();
        for record in [&mut cached.current, &mut cached.previous]
            .into_iter()
            .flatten()
        {
            record.marker = None;
        }
        cached
    }

    /// Version 2 kept no ids and no checks.
    fn sample_v2() -> Cached {
        let mut cached = sample_v3();
        for record in [&mut cached.current, &mut cached.previous]
            .into_iter()
            .flatten()
        {
            record.id = None;
        }
        cached.checks = Checks::default();
        cached
    }

    /// Version 1 kept no images either.
    fn sample_v1() -> Cached {
        let mut cached = sample_v2();
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
            let cached = cache
                .put(
                    ImageRecord {
                        // Whatever the caller says, the cache picks the file.
                        image: Some(ImageFile::B),
                        ..record(date)
                    },
                    date.as_bytes(),
                )
                .unwrap();
            // It says which it picked.
            assert_eq!(cache.current(), Some(cached.clone()));
            files.push(cached.image.unwrap());
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
    fn the_check_log_is_kept_beside_the_images_and_cleared_with_them() {
        // ADR 0029, WALL-12
        let (storage, cache) = cache();
        assert_eq!(cache.checks(), Checks::default());

        let saved = cache
            .update_checks(&mut |checks| {
                checks.day = 20_408;
                checks.attempts = 1;
            })
            .unwrap();
        // An image cached after it keeps it.
        cache.put(record("20261116"), b"one").unwrap();

        let restarted = PersistentImageCache::new(storage.clone());
        assert_eq!(restarted.checks(), saved);
        assert_eq!(restarted.checks().attempts, 1);
        assert_eq!(restarted.current().unwrap().date, "20261116");

        restarted.clear().unwrap();
        assert_eq!(restarted.checks(), Checks::default());
        assert!(storage.files.lock().unwrap().is_empty());
    }

    #[test]
    fn a_check_log_that_cant_be_saved_stays_as_it_was() {
        // REL-03
        let (storage, cache) = cache();
        cache
            .update_checks(&mut |checks| checks.attempts = 1)
            .unwrap();
        *storage.error.lock().unwrap() = Some(DataError::Failed("Disk error".into()));

        assert!(
            cache
                .update_checks(&mut |checks| checks.attempts = 2)
                .is_err()
        );

        *storage.error.lock().unwrap() = None;
        assert_eq!(cache.checks().attempts, 1);
    }

    #[test]
    fn setting_the_current_image_records_its_time_and_marker() {
        // SRS 7.5: "Record time and applied marker".
        let (storage, cache) = cache();
        let cached = cache.put(record("20261116"), b"one").unwrap();
        assert!(!cached.applied());

        let applied = cache
            .applied(
                cached.image.unwrap(),
                1_000,
                Some(Marker::Set(WallpaperMarker(7))),
            )
            .unwrap()
            .unwrap();

        assert!(applied.applied());
        assert_eq!(applied.applied_at_ms, Some(1_000));
        assert_eq!(applied.marker, Some(Marker::Set(WallpaperMarker(7))));
        let restarted = PersistentImageCache::new(storage.clone());
        assert_eq!(restarted.current(), Some(applied));
        assert_eq!(restarted.image(&cached), Ok(Some(b"one".to_vec())));
    }

    #[test]
    fn an_image_no_longer_current_isnt_recorded_as_set() {
        // A newer image was cached meanwhile: the record is the newer one's.
        let (_, cache) = cache();
        let older = cache.put(record("20261115"), b"one").unwrap();
        cache.put(record("20261116"), b"two").unwrap();

        assert_eq!(
            cache.applied(older.image.unwrap(), 1_000, Some(Marker::NotKept)),
            Ok(None)
        );

        assert_eq!(cache.current().unwrap().applied_at_ms, None);
        assert_eq!(cache.previous().unwrap().applied_at_ms, None);
        assert_eq!(cache.applied(ImageFile::A, 1_000, None), Ok(None));
    }

    #[test]
    fn every_kind_of_marker_is_saved_as_it_was() {
        // ADR 0030: the system's number, negative included, and an image the device didn't
        // keep.
        let (storage, cache) = cache();
        for marker in [
            None,
            Some(Marker::Set(WallpaperMarker(-1))),
            Some(Marker::Set(WallpaperMarker(i32::MAX))),
            Some(Marker::NotKept),
        ] {
            let cached = cache.put(record("20261116"), b"one").unwrap();
            cache.applied(cached.image.unwrap(), 1, marker).unwrap();

            let restarted = PersistentImageCache::new(storage.clone());
            assert_eq!(restarted.current().unwrap().marker, marker);
        }
    }

    #[test]
    fn a_marker_written_any_other_way_doesnt_load() {
        // MAINT-04
        for (marker, loads) in [
            ("1", true),
            ("+1", false),
            ("01", false),
            ("x", false),
            ("-0", false),
            ("2147483648", false),
        ] {
            let body =
                format!("current 1 en-US T A - 1 a - {marker} false\nprevious -\nchecks -\n");
            let (storage, cache) = cache();
            storage.files.lock().unwrap().insert(
                StoredFile::ImageDetails,
                seal("image-cache", IMAGE_CACHE_VERSION, &body),
            );

            assert_eq!(cache.current().is_some(), loads, "{marker}");
        }
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
            seal(
                "image-cache",
                IMAGE_CACHE_VERSION,
                "current -\nprevious -\nchecks 1 2\n",
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
    fn a_pin_stays_with_the_current_image_only() {
        // WALL-17: the pin is written with the image, in one write.
        let (storage, cache) = cache();
        cache
            .put(
                ImageRecord {
                    pinned: true,
                    ..record("20261113")
                },
                b"pinned",
            )
            .unwrap();
        let restarted = PersistentImageCache::new(storage.clone());
        assert!(restarted.current().unwrap().pinned);

        // A newer image makes it previous, and unpinned.
        cache.put(record("20261119"), b"today").unwrap();
        assert!(!cache.current().unwrap().pinned);
        assert!(!cache.previous().unwrap().pinned);
    }

    #[test]
    fn choosing_the_current_image_pins_or_unpins_it() {
        // WALL-17
        let (storage, cache) = cache();
        let current = cache.put(record("20261119"), b"today").unwrap();
        let file = current.image.unwrap();
        let set = cache
            .applied(file, 1_000, Some(Marker::Set(WallpaperMarker(7))))
            .unwrap()
            .unwrap();

        let pinned = cache.choose(file, true).unwrap().unwrap();
        assert_eq!(
            pinned,
            ImageRecord {
                pinned: true,
                ..set.clone()
            }
        );
        assert_eq!(
            PersistentImageCache::new(storage.clone()).current(),
            Some(pinned)
        );

        // Still on the lock screen: its time and marker stay.
        assert_eq!(cache.choose(file, false), Ok(Some(set)));
        assert_eq!(cache.image(&current), Ok(Some(b"today".to_vec())));
    }

    #[test]
    fn choosing_the_previous_image_brings_it_back_as_not_yet_set() {
        // WALL-17, WALL-14: the previous image's old marker isn't the lock screen's any more.
        let (storage, cache) = cache();
        let older = cache.put(record("20261118"), b"yesterday").unwrap();
        cache
            .applied(
                older.image.unwrap(),
                1_000,
                Some(Marker::Set(WallpaperMarker(7))),
            )
            .unwrap();
        let newer = cache
            .put(
                ImageRecord {
                    pinned: true,
                    ..record("20261113")
                },
                b"pinned",
            )
            .unwrap();
        let writes = storage.writes.lock().unwrap().len();

        let chosen = cache.choose(older.image.unwrap(), false).unwrap().unwrap();

        assert_eq!(chosen, older);
        assert_eq!((chosen.applied_at_ms, chosen.marker), (None, None));
        let restarted = PersistentImageCache::new(storage.clone());
        assert_eq!(restarted.current(), Some(chosen.clone()));
        assert_eq!(
            restarted.previous(),
            Some(ImageRecord {
                pinned: false,
                ..newer.clone()
            })
        );
        // Only the details are written; both images stay where they were.
        assert_eq!(storage.writes.lock().unwrap().len(), writes + 1);
        assert_eq!(restarted.image(&chosen), Ok(Some(b"yesterday".to_vec())));
        assert_eq!(restarted.image(&newer), Ok(Some(b"pinned".to_vec())));
    }

    #[test]
    fn choosing_a_file_no_record_holds_changes_nothing() {
        let (storage, cache) = cache();
        assert_eq!(cache.choose(ImageFile::A, true), Ok(None));
        cache.put(record("20261119"), b"today").unwrap();
        let before = storage.files.lock().unwrap().clone();

        assert_eq!(cache.choose(ImageFile::B, true), Ok(None));
        assert_eq!(*storage.files.lock().unwrap(), before);
    }

    /// Storage that reads, but fails every write while `failing` is set.
    struct WritesFail {
        storage: FakeStorage,
        failing: std::sync::atomic::AtomicBool,
    }

    impl StorageAdapter for WritesFail {
        fn read(&self, file: StoredFile) -> Result<Option<Vec<u8>>, DataError> {
            self.storage.read(file)
        }

        fn write(&self, file: StoredFile, bytes: &[u8]) -> Result<(), DataError> {
            if self.failing.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(DataError::Failed("Disk error".into()));
            }
            self.storage.write(file, bytes)
        }

        fn remove(&self, file: StoredFile) -> Result<(), DataError> {
            self.storage.remove(file)
        }
    }

    #[test]
    fn a_choice_that_cant_be_saved_changes_nothing() {
        // REL-03
        let storage = Arc::new(WritesFail {
            storage: FakeStorage::default(),
            failing: false.into(),
        });
        let cache = PersistentImageCache::new(storage.clone());
        let older = cache.put(record("20261118"), b"yesterday").unwrap();
        cache.put(record("20261119"), b"today").unwrap();
        storage
            .failing
            .store(true, std::sync::atomic::Ordering::Relaxed);

        assert!(cache.choose(older.image.unwrap(), true).is_err());

        let current = cache.current().unwrap();
        assert_eq!(current.date, "20261119");
        assert!(!current.pinned);
        assert_eq!(cache.previous().unwrap().date, "20261118");
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
