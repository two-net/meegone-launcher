//! A configurable fake of the image feed, for tests (MAINT-01).
//!
//! Enable the `fakes` feature in `[dev-dependencies]` only (SEC-06).

use std::sync::Mutex;

use meegone_core_data::DataError;

use crate::feed::{FORMAT_CHANGED, FeedImage, ImageFeed, ImageVersion};

/// An image with one portrait and one landscape version.
pub fn feed_image(id: &str, date: &str) -> FeedImage {
    FeedImage {
        id: id.to_owned(),
        date: date.to_owned(),
        title: format!("Image {id}"),
        attribution: "© Someone".to_owned(),
        info_link: Some(format!("https://example.com/{id}")),
        offered_as_wallpaper: true,
        versions: vec![
            ImageVersion {
                width: 1080,
                height: 1920,
                url: format!("https://example.com/{id}_1080x1920.jpg"),
            },
            ImageVersion {
                width: 1920,
                height: 1080,
                url: format!("https://example.com/{id}_1920x1080.jpg"),
            },
        ],
        preview: ImageVersion {
            width: 800,
            height: 480,
            url: format!("https://example.com/{id}_800x480.jpg"),
        },
    }
}

/// Offers `image` for every region until it is cleared; without one, `today` fails as if the
/// feed changed its format. Offers `week` as the past week; while it is empty, `past_week`
/// fails the same way.
pub struct FakeFeed {
    pub regions: Mutex<Vec<String>>,
    pub image: Mutex<Option<FeedImage>>,
    pub week: Mutex<Vec<FeedImage>>,
    /// The region of every request for today's image, including failed ones, to check call
    /// budgets (REL-04).
    pub requests: Mutex<Vec<String>>,
    /// The region of every request for the past week, including failed ones (WALL-17).
    pub week_requests: Mutex<Vec<String>>,
    /// When set, every request fails with this error.
    pub error: Mutex<Option<DataError>>,
}

impl Default for FakeFeed {
    fn default() -> Self {
        Self {
            regions: Mutex::new(vec!["en-US".into(), "en-GB".into(), "fi-FI".into()]),
            image: Mutex::default(),
            week: Mutex::default(),
            requests: Mutex::default(),
            week_requests: Mutex::default(),
            error: Mutex::default(),
        }
    }
}

impl ImageFeed for FakeFeed {
    fn regions(&self) -> Vec<String> {
        self.regions.lock().unwrap().clone()
    }

    fn today(&self, region: &str) -> Result<FeedImage, DataError> {
        self.requests.lock().unwrap().push(region.to_owned());
        if let Some(error) = self.error.lock().unwrap().clone() {
            return Err(error);
        }
        self.image
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| DataError::Invalid(FORMAT_CHANGED.into()))
    }

    fn past_week(&self, region: &str) -> Result<Vec<FeedImage>, DataError> {
        self.week_requests.lock().unwrap().push(region.to_owned());
        if let Some(error) = self.error.lock().unwrap().clone() {
            return Err(error);
        }
        let week = self.week.lock().unwrap().clone();
        if week.is_empty() {
            return Err(DataError::Invalid(FORMAT_CHANGED.into()));
        }
        Ok(week)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_reports_a_changed_format_as_a_distinct_failure() {
        // SRS 8.2: a format change is reported for the status line, never a crash.
        let feed: &dyn ImageFeed = &FakeFeed::default();

        assert!(matches!(feed.today("en-US"), Err(DataError::Invalid(_))));
    }

    #[test]
    fn every_request_is_recorded_for_the_call_budget() {
        // REL-04
        let feed = FakeFeed::default();
        *feed.image.lock().unwrap() = Some(feed_image("a1", "20261115"));
        *feed.error.lock().unwrap() = Some(DataError::Offline);
        assert_eq!(feed.today("fi-FI"), Err(DataError::Offline));

        *feed.error.lock().unwrap() = None;
        assert_eq!(feed.today("fi-FI").unwrap().id, "a1");

        assert_eq!(*feed.requests.lock().unwrap(), ["fi-FI", "fi-FI"]);
        assert!(feed.regions().contains(&"en-US".to_owned()));
    }

    #[test]
    fn the_past_week_is_offered_once_set() {
        // WALL-17
        let feed = FakeFeed::default();
        assert!(matches!(
            feed.past_week("en-US"),
            Err(DataError::Invalid(_))
        ));

        *feed.week.lock().unwrap() =
            vec![feed_image("b2", "20261116"), feed_image("a1", "20261115")];
        assert_eq!(feed.past_week("en-US").unwrap().len(), 2);
        assert_eq!(*feed.week_requests.lock().unwrap(), ["en-US", "en-US"]);
        assert!(feed.requests.lock().unwrap().is_empty());
    }
}
