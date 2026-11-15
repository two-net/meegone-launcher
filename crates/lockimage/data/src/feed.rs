//! The image-feed interface (SRS 8.2).
//!
//! The feed is public but undocumented and may change (A2), so only the one implementation of
//! [`ImageFeed`] knows its format (ARCH-04). Everything else sees the format-free
//! [`FeedImage`]. That implementation, for Bing's image of the day, arrives with #26 and fetches
//! through the `NetworkAdapter`.

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
    pub info_link: Option<String>,
    /// Every size on offer, in no particular order.
    pub versions: Vec<ImageVersion>,
}

/// The image-of-the-day feed.
pub trait ImageFeed: Send + Sync {
    /// The region codes the feed supports, for the Region setting (WALL-05).
    fn regions(&self) -> Vec<String>;
    /// Today's image for `region`.
    ///
    /// Fails with [`DataError::Offline`] without a connection, and with
    /// [`DataError::Invalid`] if the response is malformed or its format has changed.
    fn today(&self, region: &str) -> Result<FeedImage, DataError>;
}
