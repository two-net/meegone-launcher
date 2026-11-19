//! The past week's images, for browsing and pinning one (WALL-17, ADR 0032).
//!
//! They are kept in memory only, never on disk: the image cache keeps the current and the
//! previous image and nothing else (WALL-09), and stored data stays within its budget
//! (PERF-08). A browse fills [`PastImages`] on the image job's thread; the lock-screen image's
//! holder reads it on the main thread. Neither holds its lock across a request.
//!
//! Each preview is a small JPEG from the feed, about 35 KB, checked like the daily image but
//! with a preview's limits: at most [`MAX_PREVIEW_BYTES`], a JPEG `jpeg` reads, and no side
//! over [`MAX_PREVIEW_SIDE`], so decoding it stays bounded (SEC-02). The image that is pinned is
//! downloaded and checked as the daily image is (WALL-06, WALL-07).

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meegone_core_data::{DataError, Size};

use crate::download::NOT_AN_IMAGE;
use crate::feed::FeedImage;
use crate::jpeg;

/// A preview larger than this isn't read (SEC-02). The feed's are about 35 KB.
pub const MAX_PREVIEW_BYTES: usize = 256 * 1024;
/// A preview wider or taller than this is refused, so decoding it stays bounded (SEC-02). The
/// feed's are 800 × 480.
pub const MAX_PREVIEW_SIDE: u32 = 1920;
/// What a preview larger than [`MAX_PREVIEW_SIDE`] fails with.
pub const PREVIEW_TOO_LARGE: &str = "The preview is too large";

/// The size of the preview in `bytes`, if it may be shown: a JPEG as `jpeg` reads it, at most
/// [`MAX_PREVIEW_SIDE`] on either side (SEC-02).
pub fn check_preview(bytes: &[u8]) -> Result<Size, DataError> {
    let size = jpeg::size(bytes).ok_or_else(|| DataError::Invalid(NOT_AN_IMAGE.into()))?;
    if size.width.max(size.height) > MAX_PREVIEW_SIDE {
        Err(DataError::Invalid(PREVIEW_TOO_LARGE.into()))
    } else {
        Ok(size)
    }
}

/// A checked preview's bytes. Copies share them, and two previews are the same only if they
/// share them, so comparing states never compares pictures. Debug output gives only the size.
#[derive(Clone)]
pub struct Preview(Arc<[u8]>);

impl Preview {
    /// `bytes`, if [`check_preview`] accepts them.
    pub fn new(bytes: &[u8]) -> Result<Self, DataError> {
        check_preview(bytes)?;
        Ok(Self(bytes.into()))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl PartialEq for Preview {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Preview {}

impl fmt::Debug for Preview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Preview({} bytes)", self.0.len())
    }
}

/// One of the past week's images, with its preview when it has one. An image not offered as a
/// wallpaper has none, and so has one whose preview failed (LEGAL-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastImage {
    pub image: FeedImage,
    pub preview: Option<Preview>,
}

/// The past week's images for one region, as browsed on one local day (WALL-17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Week {
    /// The region the feed was asked for (WALL-05).
    pub region: String,
    /// The local day of the browse, in days since 1 January 1970.
    pub day: u64,
    /// Today's first.
    pub images: Vec<PastImage>,
}

impl Week {
    /// The image with the feed's `id`.
    pub fn image(&self, id: &str) -> Option<&FeedImage> {
        self.images
            .iter()
            .map(|past| &past.image)
            .find(|image| image.id == id)
    }
}

/// The last browse of the past week, and the last problem browsing or pinning had (WALL-17).
/// Memory only; clones of its `Arc` share it.
#[derive(Debug, Default)]
pub struct PastImages {
    browsed: Mutex<Browsed>,
}

#[derive(Debug, Default)]
struct Browsed {
    week: Option<Week>,
    problem: Option<String>,
}

impl PastImages {
    fn browsed(&self) -> MutexGuard<'_, Browsed> {
        self.browsed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn week(&self) -> Option<Week> {
        self.browsed().week.clone()
    }

    /// The last browse or pin that failed, in plain words (WALL-16).
    pub fn problem(&self) -> Option<String> {
        self.browsed().problem.clone()
    }

    /// Keeps `week`, and forgets the last problem.
    pub fn set_week(&self, week: Week) {
        *self.browsed() = Browsed {
            week: Some(week),
            problem: None,
        };
    }

    pub fn set_problem(&self, problem: Option<String>) {
        self.browsed().problem = problem;
    }

    /// Forgets the week and the problem, such as when the daily image is turned off (WALL-12)
    /// or a new image of the day comes in.
    pub fn clear(&self) {
        *self.browsed() = Browsed::default();
    }
}

/// Today's image and its preview, for setup's lock-screen image step (ONB-01, ADR 0036), as
/// read for one region on one local day. It is read while the daily image may still be off, and
/// is only ever shown: it never becomes the wallpaper (WALL-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Today {
    /// The region the feed was asked for (WALL-05).
    pub region: String,
    /// The local day it was read, in days since 1 January 1970.
    pub day: u64,
    pub image: PastImage,
}

/// The last [`Today`] read for setup. Memory only; clones of its `Arc` share it.
#[derive(Debug, Default)]
pub struct TodayImage {
    today: Mutex<Option<Today>>,
}

impl TodayImage {
    fn today(&self) -> MutexGuard<'_, Option<Today>> {
        self.today.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn get(&self) -> Option<Today> {
        self.today().clone()
    }

    pub fn set(&self, today: Today) {
        *self.today() = Some(today);
    }

    /// Forgets it, once setup's step is answered or the daily image is turned off.
    pub fn clear(&self) {
        *self.today() = None;
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes;

    use super::*;
    use crate::feed::ImageVersion;

    /// A JPEG header of `width` × `height`, enough for `jpeg` to read.
    fn header(width: u32, height: u32) -> Vec<u8> {
        fakes::jpeg(Size { width, height })
    }

    fn feed_image(id: &str, date: &str) -> FeedImage {
        let version = |width, height| ImageVersion {
            width,
            height,
            url: format!("https://www.bing.com/th?id={id}_{width}x{height}.jpg"),
        };
        FeedImage {
            id: id.into(),
            date: date.into(),
            title: format!("Image {id}"),
            attribution: "© Someone".into(),
            info_link: None,
            offered_as_wallpaper: true,
            versions: vec![version(1920, 1080)],
            preview: version(800, 480),
        }
    }

    #[test]
    fn a_preview_is_a_jpeg_of_bounded_size() {
        // SEC-02, issue #31: bounded decoding for previews too.
        assert_eq!(
            check_preview(&header(800, 480)),
            Ok(Size {
                width: 800,
                height: 480
            })
        );
        // Small is fine: a preview isn't the wallpaper (WALL-07 applies to the pinned image).
        assert!(check_preview(&header(320, 240)).is_ok());
        assert!(check_preview(&header(1920, 1080)).is_ok());
        assert_eq!(
            check_preview(&header(1921, 1080)),
            Err(DataError::Invalid(PREVIEW_TOO_LARGE.into()))
        );
        assert_eq!(
            check_preview(&header(480, 4000)),
            Err(DataError::Invalid(PREVIEW_TOO_LARGE.into()))
        );
        assert_eq!(
            check_preview(b"<html>"),
            Err(DataError::Invalid(NOT_AN_IMAGE.into()))
        );
        assert!(Preview::new(b"").is_err());
    }

    #[test]
    fn previews_compare_by_their_bytes_identity_and_print_only_their_size() {
        let preview = Preview::new(&header(800, 480)).unwrap();
        let same = preview.clone();
        let copy = Preview::new(preview.bytes()).unwrap();

        assert_eq!(preview, same);
        assert_ne!(preview, copy);
        assert_eq!(
            format!("{preview:?}"),
            format!("Preview({} bytes)", header(800, 480).len())
        );
    }

    #[test]
    fn the_week_is_kept_in_memory_until_cleared() {
        // WALL-17, PERF-08: nothing of it is stored.
        let past = PastImages::default();
        assert_eq!(past.week(), None);

        past.set_problem(Some("No connection".into()));
        let week = Week {
            region: "en-US".into(),
            day: 20_411,
            images: vec![PastImage {
                image: feed_image("b2", "20261119"),
                preview: None,
            }],
        };
        past.set_week(week.clone());
        assert_eq!(past.week(), Some(week.clone()));
        assert_eq!(past.problem(), None);
        assert_eq!(
            week.image("b2").map(|image| image.date.as_str()),
            Some("20261119")
        );
        assert_eq!(week.image("a1"), None);

        past.set_problem(Some("No connection".into()));
        assert_eq!(past.problem().as_deref(), Some("No connection"));
        past.clear();
        assert_eq!((past.week(), past.problem()), (None, None));
    }

    #[test]
    fn todays_image_is_kept_in_memory_until_cleared() {
        // ONB-01, ADR 0036
        let today = TodayImage::default();
        assert_eq!(today.get(), None);
        let read = Today {
            region: "en-GB".into(),
            day: 20_400,
            image: PastImage {
                image: feed_image("a1", "20251120"),
                preview: Some(Preview::new(&header(800, 480)).unwrap()),
            },
        };

        today.set(read.clone());
        assert_eq!(today.get(), Some(read));
        today.clear();
        assert_eq!(today.get(), None);
    }
}
