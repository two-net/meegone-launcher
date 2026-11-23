//! The image-feed interface (SRS 8.2).
//!
//! The feed is public but undocumented and may change (A2), so only the one implementation of
//! [`ImageFeed`], [`BingFeed`](crate::BingFeed), knows its format (ARCH-04, ADR 0027).
//! Everything else sees the format-free [`FeedImage`]. It fetches through the `NetworkAdapter`.

use meegone_core_data::DataError;

/// One size of an image that the feed offers (WALL-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageVersion {
    pub width: u32,
    pub height: u32,
    /// Where to download it; always `https://` (WALL-07).
    pub url: String,
}

/// The details of one image of the day, as the feed gives them (SRS 8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedImage {
    /// Tells images apart, so the same image isn't downloaded twice (WALL-03).
    pub id: String,
    /// Publication date, for example `20261115`.
    pub date: String,
    pub title: String,
    /// Shown as the source provides it, as plain text (LEGAL-03, SEC-02).
    pub attribution: String,
    /// An `https://` link to more about the image, when the feed gives one (WALL-15).
    pub info_link: Option<String>,
    /// Whether the source offers the image for use as a wallpaper. The updater leaves one that
    /// isn't, and keeps the current wallpaper (LEGAL-04, ADR 0027).
    pub offered_as_wallpaper: bool,
    /// Every size on offer, in no particular order.
    pub versions: Vec<ImageVersion>,
    /// A small size, only for browsing the past week (WALL-17, ADR 0032) and for setup's
    /// lock-screen image step (ONB-01, ADR 0036). It is never one of
    /// [`versions`](Self::versions), so it never becomes the wallpaper.
    pub preview: ImageVersion,
}

/// How many images [`ImageFeed::past_week`] gives at most: today's and the six before it
/// (WALL-17).
pub const PAST_WEEK_IMAGES: usize = 7;

/// The image-of-the-day feed.
pub trait ImageFeed: Send + Sync {
    /// The regions the feed supports, for the Region setting, as BCP 47 `language-REGION` tags
    /// such as `en-GB` (WALL-05). Makes no request.
    fn regions(&self) -> Vec<String>;
    /// Today's image for `region`, one of [`regions`](Self::regions), in exactly one request.
    /// Never retries: the updater keeps to the call budget (REL-04).
    ///
    /// Fails with [`DataError::Offline`] without a connection, and with
    /// [`DataError::Invalid`] for a region the feed doesn't list, without a request, or with
    /// [`Problem::FeedFormatChanged`] if the response can't be read.
    fn today(&self, region: &str) -> Result<FeedImage, DataError>;
    /// The images of the past seven days for `region`, today's first, at most
    /// [`PAST_WEEK_IMAGES`], in exactly one request (WALL-17, ADR 0032). It fails as
    /// [`today`](Self::today) does.
    fn past_week(&self, region: &str) -> Result<Vec<FeedImage>, DataError>;
}
