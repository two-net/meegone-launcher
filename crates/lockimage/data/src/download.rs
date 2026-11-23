//! Turning the feed's image into the lock-screen image (WALL-06 to WALL-09, ADR 0028).
//!
//! [`Downloader::download`] takes one [`FeedImage`] through every step, and caches the result
//! only when all of them succeed. On any failure the cache is untouched, so the current image,
//! and the lock-screen wallpaper set from it, stay as they were (WALL-08):
//! 1. Work out the size to crop to from the device's screen ([`lock_screen_target`]), and
//!    skip the download when storage is low, and say so (SRS 7.6).
//! 2. Pick the version that suits the lock screen ([`pick_version`], WALL-06).
//! 3. Download it over HTTPS, at most [`MAX_DOWNLOAD_BYTES`] (WALL-06, WALL-07).
//! 4. Check that it is a JPEG image of a usable size ([`check_image`], WALL-07, SEC-02).
//! 5. Have the platform cut the lock screen's proportions out of its middle, decoding no more
//!    than needed ([`plan_crop`], WALL-06, SEC-02), and check what comes back.
//! 6. Cache the cropped image with its details, as the current one (WALL-09).
//!
//! Sizing, picking, checking and planning are pure, so they are tested without a device
//! (MAINT-01). The updater, `ImageUpdater` in `meegone-lockimage-state`, decides when to
//! download (ADR 0029), and sets the wallpaper from the cache (ADR 0030).

use std::sync::Arc;

use meegone_core_data::{
    DataError, ImageCrop, NetworkAdapter, Problem, Screen, Size, SystemConfigAdapter,
    WallpaperAdapter,
};

use crate::feed::{FeedImage, ImageVersion};
use crate::{ImageCache, ImageRecord, jpeg};

/// The largest download accepted (WALL-06). The feed's largest image is about 4 MB.
pub const MAX_DOWNLOAD_BYTES: usize = 20_000_000;
/// An image must be at least this many pixels on its short side (WALL-07).
pub const MIN_SHORT_SIDE: u32 = 720;
/// An image may be at most this many pixels on either side, so decoding it stays bounded
/// (SEC-02). The feed's largest image is 3840 px wide.
pub const MAX_SIDE: u32 = 8192;
/// A progressive image may have at most this many pixels (SEC-02, ADR 0042). Its decoder keeps
/// every coefficient of the whole image while it crops, 2 to 6 bytes a pixel: up to 400 MB at
/// [`MAX_SIDE`] square, in the home's own process, but under 100 MB here. The feed's largest
/// image, 3840 × 2160, has about 8.3 million.
pub const MAX_PROGRESSIVE_PIXELS: u64 = 16_000_000;

/// Screens at least this many dp across their smallest side turn freely, as MeeGone lets them
/// (HOME-12): tablets, and foldables unfolded.
pub const LARGE_SCREEN_DP: u32 = 600;

/// The size to crop the lock-screen image to for `screen` (WALL-06, ADR 0030):
/// - On a phone, whose lock screen stays upright, the screen's size in its natural
///   orientation.
/// - On a screen [`LARGE_SCREEN_DP`] or more across, or a device that `folds`, the lock screen
///   may turn or change shape, so a square as wide as the screen's longest side. The system's
///   own centre crop then fills the lock screen whichever way it is turned, and on a
///   foldable's other screen.
///
/// Empty if the screen's size is unknown.
pub fn lock_screen_target(screen: Screen, folds: bool) -> Size {
    let Size { width, height } = screen.size;
    if screen.size.is_empty() {
        return Size::default();
    }
    // In dp, a length is its pixels × 160 / the density; an unknown density counts as a
    // phone's.
    let large = screen.density_dpi > 0
        && u64::from(width.min(height)) * 160
            >= u64::from(LARGE_SCREEN_DP) * u64::from(screen.density_dpi);
    if !large && !folds {
        return screen.size;
    }
    let side = width.max(height);
    Size {
        width: side,
        height: side,
    }
}

/// The version to download for a lock screen of `screen`'s size (WALL-06): the smallest
/// portrait version at least as large as the screen, or else the largest version. Of two
/// equally large, the one that keeps more once cropped to the screen's proportions. Only
/// `https://` versions count (WALL-07). `None` if there are none.
pub fn pick_version(versions: &[ImageVersion], screen: Size) -> Option<&ImageVersion> {
    let usable = || {
        versions.iter().filter(|version| {
            version.url.starts_with("https://") && version.width > 0 && version.height > 0
        })
    };
    let pixels = |size: Size| u64::from(size.width) * u64::from(size.height);
    let size = |version: &ImageVersion| Size {
        width: version.width,
        height: version.height,
    };
    let kept = |version: &ImageVersion| {
        if screen.is_empty() {
            return 0;
        }
        let crop = plan_crop(size(version), screen);
        u64::from(crop.width) * u64::from(crop.height)
    };
    usable()
        .filter(|version| {
            version.height > version.width
                && version.width >= screen.width
                && version.height >= screen.height
        })
        // The smallest that covers the screen downloads the least (PWR-03).
        .min_by_key(|version| pixels(size(version)))
        .or_else(|| usable().max_by_key(|version| (pixels(size(version)), kept(version))))
}

/// The size of the JPEG image in `bytes`, if it may be used (WALL-07, SEC-02): a JPEG as
/// `jpeg` reads it, at least [`MIN_SHORT_SIDE`] on its short side and at most [`MAX_SIDE`] on
/// either side, and if progressive, at most [`MAX_PROGRESSIVE_PIXELS`] in all.
pub fn check_image(bytes: &[u8]) -> Result<Size, DataError> {
    let frame = jpeg::read_frame(bytes).ok_or_else(|| invalid(Problem::ImageUnreadable))?;
    let size = frame.size;
    let pixels = u64::from(size.width) * u64::from(size.height);
    if size.width.min(size.height) < MIN_SHORT_SIDE {
        Err(invalid(Problem::ImageTooSmall))
    } else if size.width.max(size.height) > MAX_SIDE
        || (frame.progressive && pixels > MAX_PROGRESSIVE_PIXELS)
    {
        Err(invalid(Problem::ImageTooLarge))
    } else {
        Ok(size)
    }
}

/// How to cut an `image`-sized picture down to a lock screen of `screen`'s size (WALL-06):
/// - The region kept is the image's middle, in the screen's proportions, as large as fits.
/// - The result is the screen's size, or the region's if that is smaller: an image is never
///   scaled up, as the system scales the wallpaper to the screen anyway.
/// - The region is decoded at 1/`sample` of its size, the largest power of two that still
///   gives the result's size or more, so the decoded picture is less than twice the result on
///   each side (SEC-02).
///
/// Both sizes must be non-empty.
pub fn plan_crop(image: Size, screen: Size) -> ImageCrop {
    let (image_width, image_height) = (u64::from(image.width), u64::from(image.height));
    let (screen_width, screen_height) = (u64::from(screen.width), u64::from(screen.height));
    // Rounded to the nearest pixel, which never leaves the image.
    let (width, height) = if image_width * screen_height > image_height * screen_width {
        // Wider than the screen: all of its height, the middle of its width.
        let width = (image_height * screen_width + screen_height / 2) / screen_height;
        (width.clamp(1, image_width), image_height)
    } else {
        let height = (image_width * screen_height + screen_width / 2) / screen_width;
        (image_width, height.clamp(1, image_height))
    };
    let pixels = |length: u64| u32::try_from(length).unwrap_or(u32::MAX);
    let (width, height) = (pixels(width), pixels(height));
    let size = if width >= screen.width && height >= screen.height {
        screen
    } else {
        Size { width, height }
    };
    let mut sample = 1;
    while width / (sample * 2) >= size.width && height / (sample * 2) >= size.height {
        sample *= 2;
    }
    ImageCrop {
        image,
        left: (image.width - width) / 2,
        top: (image.height - height) / 2,
        width,
        height,
        sample,
        size,
    }
}

/// Downloads, checks, crops and caches the feed's images, one at a time.
pub struct Downloader {
    network: Arc<dyn NetworkAdapter>,
    /// The lock screen's size, and the platform's image decoder.
    wallpaper: Arc<dyn WallpaperAdapter>,
    /// How much free storage there is.
    system: Arc<dyn SystemConfigAdapter>,
    cache: Arc<dyn ImageCache>,
}

impl Downloader {
    pub fn new(
        network: Arc<dyn NetworkAdapter>,
        wallpaper: Arc<dyn WallpaperAdapter>,
        system: Arc<dyn SystemConfigAdapter>,
        cache: Arc<dyn ImageCache>,
    ) -> Self {
        Self {
            network,
            wallpaper,
            system,
            cache,
        }
    }

    /// Makes `image`, from the feed's `region`, the cached current image, pinned or not
    /// (WALL-17), and returns its record as cached. The previous image stays, and the one
    /// before it is deleted (WALL-09). Only then may it be set as the wallpaper (WALL-08).
    ///
    /// It makes at most one request and never retries; the updater keeps to the call budget
    /// (REL-04). It blocks: on Android, call it from a JVM thread other than the main one.
    ///
    /// Fails without caching anything, each failure in plain words for the status line
    /// (WALL-16):
    /// - [`Problem::StorageFull`] without a request, when free storage is under [`MAX_DOWNLOAD_BYTES`];
    /// - [`Problem::FeedFormatChanged`] without a request, if the feed offers no `https://` version;
    /// - [`DataError::Offline`], or "Too large" for a download over [`MAX_DOWNLOAD_BYTES`],
    ///   from the network;
    /// - [`Problem::ImageUnreadable`], [`Problem::ImageTooSmall`] or [`Problem::ImageTooLarge`] for an image that isn't
    ///   usable;
    /// - the cache's error if it can't be saved.
    pub fn download(
        &self,
        image: &FeedImage,
        region: &str,
        pinned: bool,
    ) -> Result<ImageRecord, DataError> {
        let screen = lock_screen_target(self.wallpaper.screen(), self.wallpaper.folds());
        if screen.is_empty() {
            return Err(DataError::Failed(Problem::ScreenSizeUnknown));
        }
        let max_bytes = u64::try_from(MAX_DOWNLOAD_BYTES).unwrap_or(u64::MAX);
        if self
            .system
            .free_storage_bytes()
            .is_some_and(|free| free < max_bytes)
        {
            return Err(DataError::Failed(Problem::StorageFull));
        }
        let version = pick_version(&image.versions, screen).ok_or_else(|| {
            // The feed's one implementation offers only `https://` versions.
            invalid(Problem::FeedFormatChanged)
        })?;
        let download = self.network.get(&version.url, MAX_DOWNLOAD_BYTES)?;
        let crop = plan_crop(check_image(&download)?, screen);
        let cropped = self
            .wallpaper
            .crop_image(&download, &crop)
            .map_err(|error| match error {
                DataError::Invalid(_) => invalid(Problem::ImageUnreadable),
                error => error,
            })?;
        drop(download);
        // What comes back is checked too: a JPEG of exactly the size asked for.
        if jpeg::size(&cropped) != Some(crop.size) {
            return Err(invalid(Problem::ImageUnreadable));
        }
        let record = ImageRecord {
            date: image.date.clone(),
            region: region.to_owned(),
            title: image.title.clone(),
            attribution: image.attribution.clone(),
            info_link: image.info_link.clone(),
            applied_at_ms: None,
            image: None,
            id: Some(image.id.clone()),
            marker: None,
            pinned,
        };
        self.cache.put(record, &cropped)
    }
}

fn invalid(problem: Problem) -> DataError {
    DataError::Invalid(problem)
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::{self, Fakes};
    use meegone_core_data::{NetworkState, StoredFile};

    use super::*;
    use crate::{ImageFile, PersistentImageCache};

    /// The Galaxy S22's screen, the reference phone the device checks used.
    const S22: Size = px(1080, 2340);

    const fn px(width: u32, height: u32) -> Size {
        Size { width, height }
    }

    const fn screen(size: Size, density_dpi: u32) -> Screen {
        Screen { size, density_dpi }
    }

    #[test]
    fn a_phone_crops_to_its_screen_upright() {
        // WALL-06: a phone's lock screen stays upright (HOME-12).
        for (phone, dpi) in [(S22, 425), (px(720, 1280), 320), (px(1440, 3200), 600)] {
            assert_eq!(
                lock_screen_target(screen(phone, dpi), false),
                phone,
                "{phone:?}"
            );
        }
    }

    #[test]
    fn a_screen_that_turns_crops_to_a_square_of_its_longest_side() {
        // ADR 0030: a tablet, whatever its natural orientation.
        assert_eq!(
            lock_screen_target(screen(px(2560, 1600), 320), false),
            px(2560, 2560)
        );
        assert_eq!(
            lock_screen_target(screen(px(1200, 1920), 240), false),
            px(1920, 1920)
        );
        // Exactly 600 dp across turns too; just under doesn't.
        assert_eq!(
            lock_screen_target(screen(px(1200, 1600), 320), false),
            px(1600, 1600)
        );
        assert_eq!(
            lock_screen_target(screen(px(1199, 1600), 320), false),
            px(1199, 1600)
        );
    }

    #[test]
    fn a_foldable_crops_to_a_square_on_either_screen() {
        // ADR 0030: folded, its cover screen is a phone's, but the lock screen may show on the
        // inner one too, which turns.
        let cover = screen(px(904, 2316), 420);
        let inner = screen(px(1812, 2176), 420);

        assert_eq!(lock_screen_target(cover, true), px(2316, 2316));
        assert_eq!(lock_screen_target(inner, true), px(2176, 2176));
        assert_eq!(lock_screen_target(inner, false), px(2176, 2176));
    }

    #[test]
    fn an_unknown_screen_gives_no_size() {
        assert!(lock_screen_target(Screen::default(), false).is_empty());
        assert!(lock_screen_target(screen(Size::default(), 420), true).is_empty());
        // A screen of unknown density is taken as a phone's.
        assert_eq!(lock_screen_target(screen(S22, 0), false), S22);
    }

    #[test]
    fn a_square_crop_keeps_the_middle_and_never_scales_up() {
        // WALL-06 on a tablet: the feed's UHD image gives its full height, square.
        let target = lock_screen_target(screen(px(2560, 1600), 320), false);
        let versions = bing_versions();
        let version = pick_version(&versions, target).unwrap();
        assert_eq!((version.width, version.height), (3840, 2160));

        let crop = plan_crop(px(3840, 2160), target);

        assert_eq!((crop.left, crop.top), (840, 0));
        assert_eq!((crop.width, crop.height), (2160, 2160));
        assert_eq!(crop.size, px(2160, 2160));
    }

    fn version(width: u32, height: u32) -> ImageVersion {
        ImageVersion {
            width,
            height,
            url: format!("https://www.bing.com/th?id=OHR.Test_{width}x{height}.jpg"),
        }
    }

    /// The sizes the feed offers every image in (ADR 0027).
    fn bing_versions() -> Vec<ImageVersion> {
        vec![
            version(3840, 2160),
            version(1920, 1080),
            version(1080, 1920),
        ]
    }

    #[test]
    fn a_portrait_version_is_picked_when_it_covers_the_screen() {
        // WALL-06
        let versions = bing_versions();
        for screen in [px(1080, 1920), px(720, 1280), px(1080, 1800)] {
            assert_eq!(
                pick_version(&versions, screen),
                Some(&versions[2]),
                "{screen:?}"
            );
        }
    }

    #[test]
    fn otherwise_the_largest_version_is_picked() {
        // WALL-06: the S22's screen is taller than the portrait version, and a landscape
        // tablet's is wider.
        let versions = bing_versions();
        for screen in [S22, px(1440, 3200), px(2560, 1600), px(1200, 1920)] {
            assert_eq!(
                pick_version(&versions, screen),
                Some(&versions[0]),
                "{screen:?}"
            );
        }
    }

    #[test]
    fn the_smallest_portrait_version_that_covers_the_screen_is_picked() {
        // PWR-03: no more data than the screen needs.
        let versions = vec![
            version(2160, 3840),
            version(1080, 1920),
            version(1440, 2560),
        ];

        assert_eq!(pick_version(&versions, S22), Some(&versions[2]));
        assert_eq!(pick_version(&versions, px(720, 1280)), Some(&versions[1]));
    }

    #[test]
    fn only_https_versions_are_picked() {
        // WALL-07, SEC-01
        let mut versions = bing_versions();
        versions[0].url = versions[0].url.replace("https://", "http://");

        // Of the two equally large left, the portrait one keeps more of a portrait screen.
        assert_eq!(pick_version(&versions, S22), Some(&versions[2]));
        assert_eq!(pick_version(&versions, px(2560, 1600)), Some(&versions[1]));

        for version in &mut versions {
            version.url = version.url.replace("https://", "http://");
        }
        assert_eq!(pick_version(&versions, S22), None);
        assert_eq!(pick_version(&[], S22), None);
        assert_eq!(pick_version(&[version(0, 0)], S22), None);
    }

    #[test]
    fn an_image_must_be_at_least_720_px_on_its_short_side() {
        // WALL-07
        for (size, expected) in [
            (px(720, 1280), Ok(px(720, 1280))),
            (px(1280, 720), Ok(px(1280, 720))),
            (px(719, 1280), Err(invalid(Problem::ImageTooSmall))),
            (px(3840, 719), Err(invalid(Problem::ImageTooSmall))),
            (px(600, 800), Err(invalid(Problem::ImageTooSmall))),
        ] {
            assert_eq!(check_image(&fakes::jpeg(size)), expected, "{size:?}");
        }
    }

    #[test]
    fn an_image_may_be_at_most_8192_px_on_a_side() {
        // SEC-02: bounded decoding.
        for (size, expected) in [
            (px(8192, 8192), Ok(px(8192, 8192))),
            (px(8193, 4320), Err(invalid(Problem::ImageTooLarge))),
            (px(720, 8193), Err(invalid(Problem::ImageTooLarge))),
        ] {
            assert_eq!(check_image(&fakes::jpeg(size)), expected, "{size:?}");
        }
    }

    #[test]
    fn a_progressive_image_may_have_at_most_16_million_pixels() {
        // SEC-02, ADR 0042: its decoder holds the whole image's coefficients.
        let progressive = |size: Size| {
            let mut bytes = fakes::jpeg(size);
            bytes[3] = 0xC2;
            bytes
        };
        for (size, expected) in [
            (px(3840, 2160), Ok(px(3840, 2160))),
            (px(5000, 3200), Ok(px(5000, 3200))),
            (px(5000, 3201), Err(invalid(Problem::ImageTooLarge))),
            (px(8192, 8192), Err(invalid(Problem::ImageTooLarge))),
        ] {
            assert_eq!(check_image(&progressive(size)), expected, "{size:?}");
        }
        // A baseline image of the same size decodes only the rows it keeps.
        assert_eq!(
            check_image(&fakes::jpeg(px(8192, 8192))),
            Ok(px(8192, 8192))
        );
    }

    #[test]
    fn content_that_isnt_an_image_is_refused() {
        // WALL-07, SEC-02
        for bytes in [
            &b""[..],
            b"<html><body>Not found</body></html>",
            b"\x89PNG\r\n\x1a\n",
        ] {
            assert_eq!(check_image(bytes), Err(invalid(Problem::ImageUnreadable)));
        }
    }

    #[test]
    fn a_landscape_image_keeps_its_middle_at_full_height() {
        // WALL-06: the feed's UHD image on the S22. The region is smaller than the screen, so
        // it isn't scaled up.
        assert_eq!(
            plan_crop(px(3840, 2160), S22),
            ImageCrop {
                image: px(3840, 2160),
                left: 1421,
                top: 0,
                width: 997,
                height: 2160,
                sample: 1,
                size: px(997, 2160),
            }
        );
    }

    #[test]
    fn a_larger_region_is_scaled_down_to_the_screen() {
        // WALL-06: a 4K portrait image on the S22.
        assert_eq!(
            plan_crop(px(2160, 3840), S22),
            ImageCrop {
                image: px(2160, 3840),
                left: 194,
                top: 0,
                width: 1772,
                height: 3840,
                sample: 1,
                size: S22,
            }
        );
        // A tall image keeps its middle at full width.
        assert_eq!(
            plan_crop(px(1080, 4000), S22),
            ImageCrop {
                image: px(1080, 4000),
                left: 0,
                top: 830,
                width: 1080,
                height: 2340,
                sample: 1,
                size: S22,
            }
        );
    }

    #[test]
    fn a_region_twice_the_screen_or_more_is_decoded_at_a_fraction() {
        // SEC-02: bounded decoding.
        let at = |image, screen| {
            let crop = plan_crop(image, screen);
            (crop.sample, crop.size)
        };
        assert_eq!(at(px(2160, 4680), S22), (2, S22));
        assert_eq!(at(px(4319, 8192), S22), (2, S22));
        assert_eq!(at(px(4320, 8192), px(1080, 2048)), (4, px(1080, 2048)));
        assert_eq!(at(px(8192, 8192), px(720, 1280)), (4, px(720, 1280)));
    }

    #[test]
    fn an_image_in_the_screens_proportions_is_kept_whole() {
        assert_eq!(
            plan_crop(S22, S22),
            ImageCrop {
                image: S22,
                left: 0,
                top: 0,
                width: 1080,
                height: 2340,
                sample: 1,
                size: S22,
            }
        );
        let square = plan_crop(px(1000, 1000), px(2000, 2000));
        assert_eq!((square.width, square.size), (1000, px(1000, 1000)));
    }

    #[test]
    fn every_crop_stays_inside_the_image_and_decodes_no_more_than_needed() {
        // WALL-06, SEC-02: over a sweep of images and screens, the region is inside the image,
        // the decoded region covers the result, and is less than twice it on each side.
        let sides = [
            720, 721, 1000, 1079, 1080, 1920, 2160, 2340, 3840, 5000, 8192,
        ];
        let screens = [
            S22,
            px(720, 1280),
            px(1440, 3200),
            px(2560, 1600),
            px(1812, 2176),
            px(1, 1),
            px(3000, 3000),
        ];
        for &width in &sides {
            for &height in &sides {
                for screen in screens {
                    let image = px(width, height);
                    let crop = plan_crop(image, screen);
                    let context = format!("{image:?} on {screen:?}: {crop:?}");
                    assert!(crop.width > 0 && crop.height > 0, "{context}");
                    assert!(crop.left + crop.width <= width, "{context}");
                    assert!(crop.top + crop.height <= height, "{context}");
                    assert!(crop.sample.is_power_of_two(), "{context}");
                    let decoded = px(crop.width / crop.sample, crop.height / crop.sample);
                    assert!(decoded.width >= crop.size.width, "{context}");
                    assert!(decoded.height >= crop.size.height, "{context}");
                    assert!(decoded.width <= 2 * crop.size.width + 1, "{context}");
                    assert!(decoded.height <= 2 * crop.size.height + 1, "{context}");
                    assert!(
                        crop.size.width <= screen.width && crop.size.height <= screen.height,
                        "{context}"
                    );
                }
            }
        }
    }

    /// The feed's image for `date`, offered in the feed's three sizes.
    fn feed_image(date: &str) -> FeedImage {
        FeedImage {
            id: format!("OHR.Test{date}"),
            date: date.to_owned(),
            title: format!("Image of {date}"),
            attribution: "Puffins (© A Photographer/Agency)".to_owned(),
            info_link: Some(format!("https://www.bing.com/search?q={date}")),
            offered_as_wallpaper: true,
            versions: bing_versions(),
            preview: ImageVersion {
                width: 800,
                height: 480,
                url: format!("https://www.bing.com/th?id=OHR.Test{date}_800x480.jpg"),
            },
        }
    }

    const UHD: &str = "https://www.bing.com/th?id=OHR.Test_3840x2160.jpg";

    struct Setup {
        fakes: Fakes,
        cache: Arc<PersistentImageCache>,
        downloader: Downloader,
    }

    /// Online, on an S22, with the feed's UHD image as `download`.
    fn setup(download: Vec<u8>) -> Setup {
        let fakes = Fakes::default();
        *fakes.network.state.lock().unwrap() = NetworkState::Unmetered;
        *fakes.wallpaper.screen.lock().unwrap() = screen(S22, 425);
        serve(&fakes, download);
        let cache = Arc::new(PersistentImageCache::new(fakes.storage.clone()));
        let downloader = Downloader::new(
            fakes.network.clone(),
            fakes.wallpaper.clone(),
            fakes.system.clone(),
            cache.clone(),
        );
        Setup {
            fakes,
            cache,
            downloader,
        }
    }

    fn serve(fakes: &Fakes, download: Vec<u8>) {
        fakes
            .network
            .responses
            .lock()
            .unwrap()
            .insert(UHD.to_owned(), download);
    }

    fn uhd() -> Vec<u8> {
        fakes::jpeg(px(3840, 2160))
    }

    /// What the cache holds: the current and previous records, and their images.
    fn cached(cache: &PersistentImageCache) -> Vec<(ImageRecord, Option<Vec<u8>>)> {
        [cache.current(), cache.previous()]
            .into_iter()
            .flatten()
            .map(|record| {
                let image = cache.image(&record).unwrap();
                (record, image)
            })
            .collect()
    }

    #[test]
    fn the_cropped_image_is_cached_with_its_details() {
        // WALL-06, WALL-08, WALL-09
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());

        let record = downloader
            .download(&feed_image("20261119"), "en-GB", false)
            .unwrap();

        assert_eq!(
            record,
            ImageRecord {
                date: "20261119".into(),
                region: "en-GB".into(),
                title: "Image of 20261119".into(),
                attribution: "Puffins (© A Photographer/Agency)".into(),
                info_link: Some("https://www.bing.com/search?q=20261119".into()),
                applied_at_ms: None,
                image: Some(ImageFile::A),
                id: Some("OHR.Test20261119".into()),
                marker: None,
                pinned: false,
            }
        );
        assert_eq!(cache.current(), Some(record.clone()));
        // One request, for the version picked.
        assert_eq!(*fakes.network.requests.lock().unwrap(), [UHD]);
        // The crop planned for the S22, decoded by the platform.
        let crop = plan_crop(px(3840, 2160), S22);
        assert_eq!(*fakes.wallpaper.crops.lock().unwrap(), [crop]);
        // The crop is cached, never the download (PERF-08).
        assert_eq!(cache.image(&record), Ok(Some(fakes::jpeg(crop.size))));
        // Nothing was set as the wallpaper: the updater does that (ADR 0030).
        assert_eq!(*fakes.wallpaper.lock_screen.lock().unwrap(), None);
    }

    #[test]
    fn a_download_over_20_mb_is_rejected() {
        // Issue #27: "Given a 25 MB download, then it is rejected."
        let mut huge = uhd();
        huge.resize(25_000_000, 0);
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());
        downloader
            .download(&feed_image("20261118"), "en-GB", false)
            .unwrap();
        let before = cached(&cache);
        serve(&fakes, huge);

        let result = downloader.download(&feed_image("20261119"), "en-GB", false);

        assert_eq!(result, Err(invalid(Problem::DownloadTooLarge)));
        assert_eq!(cached(&cache), before);
        assert_eq!(fakes.wallpaper.crops.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_small_image_or_content_that_isnt_one_is_discarded() {
        // Issue #27: "Given an image 600 px on its short side, or content that isn't an image,
        // then it is discarded and the current wallpaper is kept."
        for (download, problem) in [
            (fakes::jpeg(px(1066, 600)), Problem::ImageTooSmall),
            (
                b"<html><body>Moved</body></html>".to_vec(),
                Problem::ImageUnreadable,
            ),
            (
                b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec(),
                Problem::ImageUnreadable,
            ),
        ] {
            let Setup {
                fakes,
                cache,
                downloader,
            } = setup(uhd());
            downloader
                .download(&feed_image("20261118"), "en-GB", false)
                .unwrap();
            *fakes.wallpaper.lock_screen.lock().unwrap() = Some(b"current".to_vec());
            let before = cached(&cache);
            serve(&fakes, download);

            let result = downloader.download(&feed_image("20261119"), "en-GB", false);

            assert_eq!(result, Err(invalid(problem)));
            assert_eq!(cached(&cache), before, "{problem}");
            // Only the first image was ever decoded, and the wallpaper is as it was.
            assert_eq!(fakes.wallpaper.crops.lock().unwrap().len(), 1);
            assert_eq!(
                *fakes.wallpaper.lock_screen.lock().unwrap(),
                Some(b"current".to_vec())
            );
        }
    }

    #[test]
    fn the_third_days_image_deletes_the_oldest() {
        // Issue #27: "Given a third day's image, then the oldest cached image is deleted."
        // WALL-09
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());
        let mut sizes = Vec::new();
        for (date, size) in [
            ("20261117", px(3840, 2160)),
            ("20261118", px(2160, 3840)),
            ("20261119", px(1920, 1080)),
        ] {
            serve(&fakes, fakes::jpeg(size));
            downloader
                .download(&feed_image(date), "en-GB", false)
                .unwrap();
            sizes.push(plan_crop(size, S22).size);
        }

        let images = cached(&cache);
        let [(current, current_image), (previous, previous_image)] = &images[..] else {
            panic!("two images are cached: {images:?}");
        };
        assert_eq!(current.date, "20261119");
        assert_eq!(current_image, &Some(fakes::jpeg(sizes[2])));
        assert_eq!(previous.date, "20261118");
        assert_eq!(previous_image, &Some(fakes::jpeg(sizes[1])));
        // The first day's image and its details are gone: only two images are kept.
        assert!(!fakes.storage.stored_text().contains("20261117"));
        let files = fakes.storage.files.lock().unwrap();
        assert_eq!(
            files.keys().copied().collect::<Vec<_>>(),
            [
                StoredFile::ImageDetails,
                StoredFile::ImageA,
                StoredFile::ImageB
            ]
        );
        assert!(!files.values().any(|bytes| *bytes == fakes::jpeg(sizes[0])));
    }

    #[test]
    fn an_image_that_doesnt_decode_is_discarded() {
        // WALL-07, WALL-08: the header was fine, but the platform couldn't decode the pixels.
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());
        *fakes.wallpaper.error.lock().unwrap() = Some(invalid(Problem::SystemFailed));

        let result = downloader.download(&feed_image("20261119"), "en-GB", false);

        assert_eq!(result, Err(invalid(Problem::ImageUnreadable)));
        assert_eq!(cache.current(), None);
        assert!(fakes.storage.files.lock().unwrap().is_empty());
    }

    #[test]
    fn a_crop_that_isnt_what_was_asked_for_is_discarded() {
        // WALL-08: what the platform returns is checked too.
        for cropped in [
            fakes::jpeg(px(1080, 2340)),
            b"not an image".to_vec(),
            Vec::new(),
        ] {
            let Setup {
                fakes,
                cache,
                downloader,
            } = setup(uhd());
            *fakes.wallpaper.cropped.lock().unwrap() = Some(cropped);

            let result = downloader.download(&feed_image("20261119"), "en-GB", false);

            assert_eq!(result, Err(invalid(Problem::ImageUnreadable)));
            assert_eq!(cache.current(), None);
        }
    }

    #[test]
    fn a_platform_failure_while_cropping_is_reported_as_it_is() {
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());
        *fakes.wallpaper.error.lock().unwrap() = Some(DataError::Failed(Problem::SystemFailed));

        let result = downloader.download(&feed_image("20261119"), "en-GB", false);

        assert_eq!(result, Err(DataError::Failed(Problem::SystemFailed)));
        assert_eq!(cache.current(), None);
    }

    #[test]
    fn images_come_only_over_https() {
        // WALL-07, SEC-01: a feed image with only plain-HTTP versions is never requested.
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());
        let mut image = feed_image("20261119");
        for version in &mut image.versions {
            version.url = version.url.replace("https://", "http://");
        }

        let result = downloader.download(&image, "en-GB", false);

        assert_eq!(result, Err(invalid(Problem::FeedFormatChanged)));
        assert!(fakes.network.requests.lock().unwrap().is_empty());
        assert_eq!(cache.current(), None);
    }

    #[test]
    fn low_storage_skips_the_download_and_says_so() {
        // SRS 7.6: "Low storage: skip the download and show the status."
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());
        *fakes.system.free_storage_bytes.lock().unwrap() = Some(19_999_999);

        let result = downloader.download(&feed_image("20261119"), "en-GB", false);

        assert_eq!(result, Err(DataError::Failed(Problem::StorageFull)));
        assert!(fakes.network.requests.lock().unwrap().is_empty());
        assert_eq!(cache.current(), None);

        // Just enough space, or a system that can't say, and the download goes ahead.
        for free in [Some(20_000_000), None] {
            *fakes.system.free_storage_bytes.lock().unwrap() = free;
            assert!(
                downloader
                    .download(&feed_image("20261119"), "en-GB", false)
                    .is_ok()
            );
        }
    }

    #[test]
    fn without_the_lock_screens_size_nothing_is_requested() {
        let Setup {
            fakes, downloader, ..
        } = setup(uhd());
        *fakes.wallpaper.screen.lock().unwrap() = Screen::default();

        let result = downloader.download(&feed_image("20261119"), "en-GB", false);

        assert_eq!(result, Err(DataError::Failed(Problem::ScreenSizeUnknown)));
        assert!(fakes.network.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn offline_nothing_changes() {
        // WALL-08, WALL-16
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());
        downloader
            .download(&feed_image("20261118"), "en-GB", false)
            .unwrap();
        let before = cached(&cache);
        *fakes.network.state.lock().unwrap() = NetworkState::Offline;

        let result = downloader.download(&feed_image("20261119"), "en-GB", false);

        assert_eq!(result, Err(DataError::Offline));
        assert_eq!(cached(&cache), before);
    }

    #[test]
    fn an_image_that_cant_be_cached_keeps_the_current_one() {
        // WALL-08, REL-03: the error is the status.
        let Setup {
            fakes,
            cache,
            downloader,
        } = setup(uhd());
        downloader
            .download(&feed_image("20261118"), "en-GB", false)
            .unwrap();
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed(Problem::StorageFull));

        let result = downloader.download(&feed_image("20261119"), "en-GB", false);

        assert_eq!(result, Err(DataError::Failed(Problem::StorageFull)));
        *fakes.storage.error.lock().unwrap() = None;
        assert_eq!(cache.current().unwrap().date, "20261118");
    }
}
