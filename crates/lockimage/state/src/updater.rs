//! The daily image's updater (SRS 7.5, ADR 0029): MeeGone's one scheduled job (ARCH-07).
//!
//! [`ImageUpdater::check`] runs when the system's scheduler starts the job, and once at each
//! start of MeeGone, where background scheduling may not be available (SRS 8.1). Each run:
//! 1. Does nothing unless a check is due ([`checks::due_ms`]), and schedules the job for when
//!    it is.
//! 2. Waits, without counting an attempt, while battery saver is on or the battery is low, or
//!    while the only network is metered and "Use mobile data" is off or Data Saver is on
//!    (WALL-04, PWR-04).
//! 3. Counts the attempt and schedules its retry before any request, so the budget holds even
//!    if MeeGone stops part-way (REL-04).
//! 4. Asks the feed for today's image in the region in use. If it is the current one, or isn't
//!    offered as a wallpaper, nothing is downloaded (WALL-03, LEGAL-04); otherwise
//!    [`Downloader::download`] caches it (WALL-06 to WALL-09).
//! 5. Records how it went, for the next run and for Settings, and schedules the next run.
//!
//! Any failure leaves the cache, and the wallpaper set from it, as they were (WALL-08).

use std::sync::{Arc, Mutex, PoisonError};

use meegone_core_data::{
    DataError, ImageJob, ImageOptions, NetworkAdapter, NetworkState, SchedulerAdapter,
    SettingsStore, SystemConfigAdapter, WallpaperAdapter,
};
use meegone_lockimage_data::{Downloader, ImageCache, ImageFeed, ImageRecord, region};

use crate::checks::{self, WAIT_MS};

/// What a check did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// The daily image is off or can't be used on this device, or was turned off during the
    /// check: nothing is cached, and no job waits (WALL-12, WALL-13).
    Off,
    /// No check is due yet; the job waits until one is.
    NotDue,
    /// A check is due, but waits for the battery or the network (WALL-04, PWR-04).
    Waiting(Wait),
    /// The feed still offers the current image (WALL-03).
    Same,
    /// Today's image isn't offered as a wallpaper, so the current one stays (LEGAL-04).
    NotOffered,
    /// Today's image was cached as the current one (WALL-09). #29 sets it as the wallpaper.
    New(ImageRecord),
    /// The attempt failed; it is retried after a back-off (REL-04).
    Failed(DataError),
}

impl Checked {
    /// Whether the check made an attempt, so what Settings or the card in Events shows may
    /// have changed.
    pub fn attempted(&self) -> bool {
        matches!(
            self,
            Self::Same | Self::NotOffered | Self::New(_) | Self::Failed(_)
        )
    }
}

/// What a due check waits for (WALL-04, PWR-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// Battery saver is on, or the battery is low.
    Battery,
    /// The network is metered, and "Use mobile data" is off or Data Saver is on.
    UnmeteredNetwork,
}

/// The feed's answer.
enum Answer {
    Same,
    NotOffered,
    New(ImageRecord),
}

pub struct ImageUpdater {
    /// Read at each check, so a change of setting counts at once.
    settings: Arc<dyn SettingsStore>,
    cache: Arc<dyn ImageCache>,
    feed: Arc<dyn ImageFeed>,
    downloader: Downloader,
    network: Arc<dyn NetworkAdapter>,
    wallpaper: Arc<dyn WallpaperAdapter>,
    scheduler: Arc<dyn SchedulerAdapter>,
    system: Arc<dyn SystemConfigAdapter>,
    /// One check at a time: the job and the check at start never overlap.
    checking: Mutex<()>,
}

impl ImageUpdater {
    pub fn new(
        settings: Arc<dyn SettingsStore>,
        cache: Arc<dyn ImageCache>,
        feed: Arc<dyn ImageFeed>,
        network: Arc<dyn NetworkAdapter>,
        wallpaper: Arc<dyn WallpaperAdapter>,
        scheduler: Arc<dyn SchedulerAdapter>,
        system: Arc<dyn SystemConfigAdapter>,
    ) -> Self {
        Self {
            downloader: Downloader::new(
                network.clone(),
                wallpaper.clone(),
                system.clone(),
                cache.clone(),
            ),
            settings,
            cache,
            feed,
            network,
            wallpaper,
            scheduler,
            system,
            checking: Mutex::new(()),
        }
    }

    /// Checks for today's image if a check is due, and schedules the job for the next one. It
    /// blocks: on Android, call it from a JVM thread other than the main one.
    pub fn check(&self) -> Checked {
        let _checking = self.checking.lock().unwrap_or_else(PoisonError::into_inner);
        let options = self.settings.get().image;
        if !options.enabled {
            // A job left from before the feature was turned off has nothing to do.
            let _ = self.scheduler.cancel_image_job();
            return Checked::Off;
        }
        if self.wallpaper.lock_screen_support().is_err() {
            return Checked::Off;
        }
        let now = self.system.now_ms();
        let offset = |at| self.system.utc_offset_ms(at);
        let job = checks::next_job(&self.cache.checks(), now, &offset, options.use_mobile_data);
        if now < job.not_before_ms {
            self.schedule(job);
            return Checked::NotDue;
        }
        if let Some(wait) = self.wait(&options) {
            self.schedule(ImageJob {
                not_before_ms: now.saturating_add(WAIT_MS),
                ..job
            });
            return Checked::Waiting(wait);
        }
        // Counted before any request; without a record of it, no request is made (REL-04).
        let begun = self
            .cache
            .update_checks(&mut |saved| checks::begin(saved, now, &offset));
        let begun = match begun {
            Ok(begun) => begun,
            Err(error) => {
                self.schedule(ImageJob {
                    not_before_ms: now.saturating_add(WAIT_MS),
                    ..job
                });
                return Checked::Failed(error);
            }
        };
        self.schedule(checks::next_job(
            &begun,
            now,
            &offset,
            options.use_mobile_data,
        ));
        let answer = self.ask(&options);
        let recorded = self.cache.update_checks(&mut |saved| match &answer {
            Ok(answer) => checks::answered(saved, matches!(answer, Answer::New(_))),
            Err(error) => checks::failed(saved, error.to_string()),
        });
        if let Ok(recorded) = recorded {
            self.schedule(checks::next_job(
                &recorded,
                now,
                &offset,
                options.use_mobile_data,
            ));
        }
        // The feature may have been turned off while the image downloaded: nothing stays
        // (WALL-12).
        if !self.settings.get().image.enabled {
            let _ = self.cache.clear();
            let _ = self.scheduler.cancel_image_job();
            return Checked::Off;
        }
        match answer {
            Ok(Answer::Same) => Checked::Same,
            Ok(Answer::NotOffered) => Checked::NotOffered,
            Ok(Answer::New(record)) => Checked::New(record),
            Err(error) => Checked::Failed(error),
        }
    }

    /// What a due check waits for, if anything (WALL-04, PWR-04). Offline isn't one: the
    /// attempt fails as "No connection", and counts (AC-16).
    fn wait(&self, options: &ImageOptions) -> Option<Wait> {
        if self.system.saves_battery() {
            Some(Wait::Battery)
        } else if self.network.state() == NetworkState::Metered
            && (!options.use_mobile_data || self.network.saves_data())
        {
            Some(Wait::UnmeteredNetwork)
        } else {
            None
        }
    }

    /// Asks the feed for today's image, and downloads it if it is new and may be used.
    fn ask(&self, options: &ImageOptions) -> Result<Answer, DataError> {
        let region = region::in_use(
            options.region.as_deref(),
            &self.system.locale(),
            &self.feed.regions(),
        );
        let image = self.feed.today(&region)?;
        let current = self.cache.current();
        let same = current.is_some_and(|current| {
            current.id.as_deref() == Some(image.id.as_str()) && current.region == region
        });
        if same {
            Ok(Answer::Same)
        } else if !image.offered_as_wallpaper {
            Ok(Answer::NotOffered)
        } else {
            self.downloader.download(&image, &region).map(Answer::New)
        }
    }

    /// Without background scheduling, the next start checks instead (SRS 8.1).
    fn schedule(&self, job: ImageJob) {
        let _ = self.scheduler.schedule_image_job(job);
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::{self, Fakes};
    use meegone_core_data::{JobNetwork, PersistentSettingsStore, Size};
    use meegone_lockimage_data::fakes::{FakeFeed, feed_image};
    use meegone_lockimage_data::{Checks, FeedImage, PersistentImageCache};

    use super::*;
    use crate::checks::{CheckStatus, FIRST_BACK_OFF_MS, SAME_IMAGE_GAP_MS};

    const HOUR_MS: u64 = 3_600_000;
    /// 19 November 2026, 00:00 UTC.
    const NOV_19: u64 = 1_795_046_400_000;
    const NOV_20: u64 = NOV_19 + 24 * HOUR_MS;

    struct Setup {
        fakes: Fakes,
        feed: Arc<FakeFeed>,
        settings: Arc<PersistentSettingsStore>,
        cache: Arc<PersistentImageCache>,
        updater: ImageUpdater,
    }

    /// The daily image on, on Wi-Fi, at 08:00 UTC on 19 November, with `id` in the feed.
    fn setup(id: &str) -> Setup {
        setup_with(id, |feed| feed)
    }

    fn setup_with(id: &str, wrap: impl FnOnce(Arc<FakeFeed>) -> Arc<dyn ImageFeed>) -> Setup {
        let fakes = Fakes::default();
        *fakes.network.state.lock().unwrap() = NetworkState::Unmetered;
        *fakes.system.now_ms.lock().unwrap() = NOV_19 + 8 * HOUR_MS;
        let feed = Arc::new(FakeFeed::default());
        offer(&fakes, &feed, feed_image(id, "20261119"));
        let settings = Arc::new(PersistentSettingsStore::new(fakes.storage.clone()));
        settings.update(&mut |s| s.image.enabled = true).unwrap();
        let cache = Arc::new(PersistentImageCache::new(fakes.storage.clone()));
        let updater = ImageUpdater::new(
            settings.clone(),
            cache.clone(),
            wrap(feed.clone()),
            fakes.network.clone(),
            fakes.wallpaper.clone(),
            fakes.scheduler.clone(),
            fakes.system.clone(),
        );
        Setup {
            fakes,
            feed,
            settings,
            cache,
            updater,
        }
    }

    /// The feed offers `image`, and its portrait version downloads as a JPEG of its size.
    fn offer(fakes: &Fakes, feed: &FakeFeed, image: FeedImage) {
        let mut responses = fakes.network.responses.lock().unwrap();
        for version in &image.versions {
            let size = Size {
                width: version.width,
                height: version.height,
            };
            responses.insert(version.url.clone(), fakes::jpeg(size));
        }
        *feed.image.lock().unwrap() = Some(image);
    }

    fn at(fakes: &Fakes, now_ms: u64) {
        *fakes.system.now_ms.lock().unwrap() = now_ms;
    }

    fn job(fakes: &Fakes) -> Option<ImageJob> {
        *fakes.scheduler.job.lock().unwrap()
    }

    fn feed_requests(feed: &FakeFeed) -> usize {
        feed.requests.lock().unwrap().len()
    }

    #[test]
    fn the_first_check_caches_todays_image_and_waits_for_tomorrow() {
        // WALL-03, WALL-09
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");

        let Checked::New(record) = updater.check() else {
            panic!("expected a new image");
        };

        assert_eq!(record.id.as_deref(), Some("puffins"));
        assert_eq!(record.region, "en-US");
        assert_eq!(cache.current(), Some(record));
        assert_eq!(*feed.requests.lock().unwrap(), ["en-US"]);
        assert_eq!(
            *fakes.network.requests.lock().unwrap(),
            ["https://example.com/puffins_1080x1920.jpg"]
        );
        assert_eq!(
            job(&fakes),
            Some(ImageJob {
                not_before_ms: NOV_20,
                network: JobNetwork::Unmetered,
            })
        );
        assert_eq!(cache.checks().attempts, 1);
    }

    #[test]
    fn a_check_before_its_time_only_schedules_the_job() {
        // WALL-03: the job may run early, or MeeGone start before it is due.
        let Setup {
            fakes,
            feed,
            updater,
            ..
        } = setup("puffins");
        updater.check();

        at(&fakes, NOV_19 + 20 * HOUR_MS);
        assert_eq!(updater.check(), Checked::NotDue);

        assert_eq!(feed_requests(&feed), 1);
        assert_eq!(job(&fakes).unwrap().not_before_ms, NOV_20);
    }

    #[test]
    fn the_same_image_isnt_downloaded_again_and_is_checked_two_hours_later() {
        // WALL-03
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");
        updater.check();
        let current = cache.current();

        at(&fakes, NOV_20 + HOUR_MS);
        assert_eq!(updater.check(), Checked::Same);

        assert_eq!(feed_requests(&feed), 2);
        assert_eq!(fakes.network.requests.lock().unwrap().len(), 1);
        assert_eq!(cache.current(), current);
        assert_eq!(
            job(&fakes).unwrap().not_before_ms,
            NOV_20 + HOUR_MS + SAME_IMAGE_GAP_MS
        );

        // A new image later that day is downloaded, and the day is done.
        offer(&fakes, &feed, feed_image("lighthouse", "20261120"));
        at(&fakes, NOV_20 + HOUR_MS + SAME_IMAGE_GAP_MS);
        assert!(matches!(updater.check(), Checked::New(_)));
        assert_eq!(cache.current().unwrap().id.as_deref(), Some("lighthouse"));
        assert_eq!(job(&fakes).unwrap().not_before_ms, NOV_20 + 24 * HOUR_MS);
    }

    #[test]
    fn an_image_not_offered_as_a_wallpaper_is_left_alone() {
        // LEGAL-04, ADR 0027
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");
        offer(
            &fakes,
            &feed,
            FeedImage {
                offered_as_wallpaper: false,
                ..feed_image("puffins", "20261119")
            },
        );

        assert_eq!(updater.check(), Checked::NotOffered);

        assert_eq!(cache.current(), None);
        assert!(fakes.network.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn on_mobile_data_with_use_mobile_data_off_nothing_is_requested() {
        // Issue #28: "Given mobile data only and 'Use mobile data' off, then no automatic
        // download happens." WALL-04
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");
        *fakes.network.state.lock().unwrap() = NetworkState::Metered;

        assert_eq!(updater.check(), Checked::Waiting(Wait::UnmeteredNetwork));

        assert_eq!(feed_requests(&feed), 0);
        assert!(fakes.network.requests.lock().unwrap().is_empty());
        assert_eq!(cache.checks(), Checks::default());
        // The system runs the job again on Wi-Fi, and not before 15 minutes from now.
        assert_eq!(
            job(&fakes),
            Some(ImageJob {
                not_before_ms: NOV_19 + 8 * HOUR_MS + WAIT_MS,
                network: JobNetwork::Unmetered,
            })
        );
    }

    #[test]
    fn with_use_mobile_data_on_it_downloads_on_mobile_data_unless_data_saver_is_on() {
        // WALL-04, PWR-04
        let Setup {
            fakes,
            feed,
            settings,
            updater,
            ..
        } = setup("puffins");
        *fakes.network.state.lock().unwrap() = NetworkState::Metered;
        settings
            .update(&mut |s| s.image.use_mobile_data = true)
            .unwrap();
        *fakes.network.saves_data.lock().unwrap() = true;

        assert_eq!(updater.check(), Checked::Waiting(Wait::UnmeteredNetwork));
        assert_eq!(feed_requests(&feed), 0);

        *fakes.network.saves_data.lock().unwrap() = false;
        assert!(matches!(updater.check(), Checked::New(_)));
        assert_eq!(job(&fakes).unwrap().network, JobNetwork::Any);
    }

    #[test]
    fn battery_saver_or_a_low_battery_waits() {
        // WALL-04, PWR-04
        let Setup {
            fakes,
            feed,
            updater,
            ..
        } = setup("puffins");
        *fakes.system.saves_battery.lock().unwrap() = true;

        assert_eq!(updater.check(), Checked::Waiting(Wait::Battery));

        assert_eq!(feed_requests(&feed), 0);
        assert_eq!(
            job(&fakes).unwrap().not_before_ms,
            NOV_19 + 8 * HOUR_MS + WAIT_MS
        );
    }

    #[test]
    fn offline_all_day_makes_six_attempts_and_keeps_the_current_image() {
        // AC-16, REL-04, WALL-08
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");
        updater.check();
        let current = cache.current();
        *feed.error.lock().unwrap() = Some(DataError::Offline);
        let mut runs = Vec::new();

        // The system runs the job whenever it is due, all through the next day.
        at(&fakes, NOV_20);
        while let Some(job) = job(&fakes).filter(|job| job.not_before_ms < NOV_20 + 24 * HOUR_MS) {
            at(&fakes, job.not_before_ms.max(NOV_20));
            runs.push(updater.check());
        }

        assert_eq!(feed_requests(&feed), 1 + 6);
        assert!(
            runs.iter()
                .all(|run| *run == Checked::Failed(DataError::Offline))
        );
        assert_eq!(cache.current(), current);
        assert_eq!(
            checks::status(&cache.checks(), NOV_20 + 12 * HOUR_MS),
            CheckStatus::Retrying("No connection".into())
        );
        assert_eq!(job(&fakes).unwrap().not_before_ms, NOV_20 + 24 * HOUR_MS);
    }

    #[test]
    fn a_failure_is_retried_after_15_minutes() {
        // REL-04
        let Setup {
            fakes,
            feed,
            updater,
            ..
        } = setup("puffins");
        *feed.error.lock().unwrap() =
            Some(DataError::Invalid("The feed's format has changed".into()));

        assert!(matches!(updater.check(), Checked::Failed(_)));

        assert_eq!(
            job(&fakes).unwrap().not_before_ms,
            NOV_19 + 8 * HOUR_MS + FIRST_BACK_OFF_MS
        );
    }

    /// Runs `during` whenever the feed is asked for today's image, then answers as `inner`.
    struct During<F> {
        inner: Arc<FakeFeed>,
        during: F,
    }

    impl<F: Fn() + Send + Sync> ImageFeed for During<F> {
        fn regions(&self) -> Vec<String> {
            self.inner.regions()
        }

        fn today(&self, region: &str) -> Result<FeedImage, DataError> {
            (self.during)();
            self.inner.today(region)
        }
    }

    #[test]
    fn the_attempt_is_saved_before_the_feed_is_asked() {
        // REL-04
        let shared = Arc::new(Mutex::new(None::<Arc<PersistentImageCache>>));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (cache_for_feed, seen_by_feed) = (shared.clone(), seen.clone());
        let Setup { cache, updater, .. } = setup_with("puffins", move |inner| {
            Arc::new(During {
                inner,
                during: move || {
                    if let Some(cache) = &*cache_for_feed.lock().unwrap() {
                        let checks = cache.checks();
                        seen_by_feed
                            .lock()
                            .unwrap()
                            .push((checks.attempts, checks.failures));
                    }
                },
            })
        });
        *shared.lock().unwrap() = Some(cache.clone());

        updater.check();

        // Counted, and as failed until the feed answered.
        assert_eq!(*seen.lock().unwrap(), [(1, 1)]);
        assert_eq!((cache.checks().attempts, cache.checks().failures), (1, 0));
    }

    #[test]
    fn turning_it_off_during_the_check_leaves_nothing_cached() {
        // WALL-12
        let settings_for_feed = Arc::new(Mutex::new(None::<Arc<PersistentSettingsStore>>));
        let off = settings_for_feed.clone();
        let Setup {
            fakes,
            settings,
            cache,
            updater,
            ..
        } = setup_with("puffins", move |inner| {
            Arc::new(During {
                inner,
                during: move || {
                    if let Some(settings) = &*off.lock().unwrap() {
                        settings.update(&mut |s| s.image.enabled = false).unwrap();
                    }
                },
            })
        });
        *settings_for_feed.lock().unwrap() = Some(settings.clone());

        assert_eq!(updater.check(), Checked::Off);

        // The image was downloaded, but nothing of it stays, and no job waits.
        assert_eq!(fakes.network.requests.lock().unwrap().len(), 1);
        assert_eq!(cache.current(), None);
        assert_eq!(cache.checks(), Checks::default());
        assert!(fakes.storage.files.lock().unwrap().keys().all(|file| {
            !matches!(
                file,
                meegone_core_data::StoredFile::ImageA | meegone_core_data::StoredFile::ImageB
            )
        }));
        assert_eq!(job(&fakes), None);
    }

    #[test]
    fn nothing_is_checked_while_the_daily_image_is_off() {
        // WALL-02, WALL-12
        let Setup {
            fakes,
            feed,
            settings,
            updater,
            ..
        } = setup("puffins");
        settings.update(&mut |s| s.image.enabled = false).unwrap();
        *fakes.scheduler.job.lock().unwrap() = Some(ImageJob {
            not_before_ms: 0,
            network: JobNetwork::Unmetered,
        });

        assert_eq!(updater.check(), Checked::Off);

        assert_eq!(feed_requests(&feed), 0);
        assert_eq!(job(&fakes), None);
    }

    #[test]
    fn nothing_is_checked_where_the_wallpaper_cant_be_set() {
        // WALL-13
        let Setup {
            fakes,
            feed,
            updater,
            ..
        } = setup("puffins");
        *fakes.wallpaper.unsupported.lock().unwrap() = Some("Blocked by device policy".into());

        assert_eq!(updater.check(), Checked::Off);

        assert_eq!(feed_requests(&feed), 0);
    }

    #[test]
    fn without_background_scheduling_the_check_still_runs() {
        // SRS 8.1: MeeGone's start checks instead.
        let Setup { fakes, updater, .. } = setup("puffins");
        *fakes.scheduler.error.lock().unwrap() = Some(DataError::Unsupported(
            "Background work is restricted".into(),
        ));

        assert!(matches!(updater.check(), Checked::New(_)));
    }

    #[test]
    fn without_a_record_of_the_attempt_nothing_is_requested() {
        // REL-04: the budget can't be kept without it.
        let Setup {
            fakes,
            feed,
            updater,
            ..
        } = setup("puffins");
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed("Disk error".into()));

        assert_eq!(
            updater.check(),
            Checked::Failed(DataError::Failed("Disk error".into()))
        );

        assert_eq!(feed_requests(&feed), 0);
        assert_eq!(
            job(&fakes).unwrap().not_before_ms,
            NOV_19 + 8 * HOUR_MS + WAIT_MS
        );
    }

    #[test]
    fn the_region_chosen_is_the_one_asked_for() {
        // WALL-05
        let Setup {
            feed,
            settings,
            cache,
            updater,
            ..
        } = setup("puffins");
        settings
            .update(&mut |s| s.image.region = Some("fi-FI".into()))
            .unwrap();

        updater.check();

        assert_eq!(*feed.requests.lock().unwrap(), ["fi-FI"]);
        assert_eq!(cache.current().unwrap().region, "fi-FI");
    }

    #[test]
    fn only_checks_that_reached_the_feed_count_as_attempted() {
        assert!(Checked::Same.attempted());
        assert!(Checked::Failed(DataError::Offline).attempted());
        assert!(!Checked::NotDue.attempted());
        assert!(!Checked::Waiting(Wait::Battery).attempted());
        assert!(!Checked::Off.attempted());
    }
}
