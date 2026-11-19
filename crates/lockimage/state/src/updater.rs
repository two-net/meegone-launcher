//! The daily image's updater (SRS 7.5, ADR 0029, ADR 0030): MeeGone's one scheduled job
//! (ARCH-07).
//!
//! [`ImageUpdater::check`] runs when the system's scheduler starts the job, and once at each
//! start of MeeGone, where background scheduling may not be available (SRS 8.1). Each run:
//! 1. Does nothing while someone else has changed the lock screen's wallpaper since MeeGone set
//!    it: daily updates are paused until the user resumes them (WALL-14).
//! 2. Does nothing unless a check is due ([`checks::due_ms`]), and schedules the job for when
//!    it is.
//! 3. Waits, without counting an attempt, while battery saver is on or the battery is low, or
//!    while the only network is metered and "Use mobile data" is off or Data Saver is on
//!    (WALL-04, PWR-04).
//! 4. Counts the attempt and schedules its retry before any request, so the budget holds even
//!    if MeeGone stops part-way (REL-04).
//! 5. Asks the feed for today's image in the region in use. If it is the current one, or isn't
//!    offered as a wallpaper, nothing is downloaded (WALL-03, LEGAL-04); otherwise
//!    [`Downloader::download`] caches it (WALL-06 to WALL-09).
//! 6. Sets the image as the lock-screen wallpaper only once it is cached, and records when, and
//!    the marker the system gave it (WALL-01, WALL-08, SRS 7.5). A cached image that isn't on
//!    the lock screen yet, because setting it failed, is set at the next attempt, without
//!    downloading it again.
//! 7. Records how it went, for the next run and for Settings, and schedules the next run.
//!
//! Any failure leaves the cache, and the wallpaper set from it, as they were (WALL-08). Nothing
//! is ever set while the daily image is off (WALL-02, WALL-12), and with no image there is
//! nothing to set (WALL-10).
//!
//! [`ImageUpdater::update_now`] is "Update now": the same run at once, whenever the user asks,
//! over any connection (WALL-11). It puts the current image back on the lock screen first,
//! which resumes paused updates (WALL-14).

use std::sync::{Arc, Mutex, PoisonError};

use meegone_core_data::{
    DataError, ImageJob, ImageOptions, NetworkAdapter, NetworkState, SchedulerAdapter,
    SettingsStore, SystemConfigAdapter, WallpaperAdapter,
};
use meegone_lockimage_data::{Downloader, ImageCache, ImageFeed, ImageRecord, Marker, region};

use crate::checks::{self, WAIT_MS};

/// What a check did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// The daily image is off or can't be used on this device, or was turned off during the
    /// check: nothing is cached, and no job waits (WALL-12, WALL-13).
    Off,
    /// Someone else changed the lock screen's wallpaper after MeeGone set it: daily updates
    /// are paused, and no job waits, until the user resumes them (WALL-14).
    Paused,
    /// No check is due yet; the job waits until one is.
    NotDue,
    /// A check is due, but waits for the battery or the network (WALL-04, PWR-04).
    Waiting(Wait),
    /// The feed still offers the current image (WALL-03).
    Same,
    /// Today's image isn't offered as a wallpaper, so the current one stays (LEGAL-04).
    NotOffered,
    /// Today's image is cached as the current one, and set as the lock-screen wallpaper
    /// (WALL-01, WALL-09).
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
    /// One check at a time: the job, the check at start and "Update now" never overlap.
    checking: Mutex<()>,
}

/// Whether someone else changed the lock screen's wallpaper after MeeGone set `record` as it
/// (WALL-14): the lock screen has another wallpaper now. Never where the platform can't tell,
/// nor for an image the device didn't keep in the first place. It asks the system only about
/// an image MeeGone set.
pub(crate) fn changed_by_someone_else(
    record: &ImageRecord,
    wallpaper: &dyn WallpaperAdapter,
) -> bool {
    match record.marker {
        Some(Marker::Set(set)) => wallpaper.lock_screen_marker().is_some_and(|now| now != set),
        _ => false,
    }
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
        self.run(false)
    }

    /// "Update now" (WALL-11): puts the current image back on the lock screen if it isn't
    /// there, which resumes paused updates (WALL-14), then asks the feed for today's image at
    /// once, over any available connection. Neither the time of day, the battery, the network
    /// nor the day's budget of automatic attempts holds it back, though its attempt counts
    /// towards that budget (REL-04). It blocks, as [`check`](Self::check) does.
    pub fn update_now(&self) -> Checked {
        self.run(true)
    }

    fn run(&self, now_asked: bool) -> Checked {
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
        let current = self.cache.current();
        if now_asked {
            // Without the network, so it works offline too. If it fails, the feed's answer
            // tries again below.
            if let Some(current) = current.filter(|current| !self.shows(current)) {
                let _ = self.apply(&current, now);
            }
        } else if current.is_some_and(|current| changed_by_someone_else(&current, &*self.wallpaper))
        {
            let _ = self.scheduler.cancel_image_job();
            return Checked::Paused;
        }
        let job = checks::next_job(&self.cache.checks(), now, &offset, options.use_mobile_data);
        if !now_asked {
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
        let answer = self.ask(&options, now);
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

    /// Asks the feed for today's image, and downloads it if it is new and may be used. Today's
    /// image is in only once it is on the lock screen.
    fn ask(&self, options: &ImageOptions, now: u64) -> Result<Answer, DataError> {
        let region = region::in_use(
            options.region.as_deref(),
            &self.system.locale(),
            &self.feed.regions(),
        );
        let image = self.feed.today(&region)?;
        let same = self.cache.current().filter(|current| {
            current.id.as_deref() == Some(image.id.as_str())
                && current.region == region
                && current.image.is_some()
        });
        if let Some(current) = same {
            if self.shows(&current) {
                return Ok(Answer::Same);
            }
            // Cached, but setting it failed, or the user resumes: no need to download it again.
            let applied = self.apply(&current, now)?;
            return Ok(Answer::New(applied.unwrap_or(current)));
        }
        if !image.offered_as_wallpaper {
            return Ok(Answer::NotOffered);
        }
        let record = self.downloader.download(&image, &region)?;
        let applied = self.apply(&record, now)?;
        Ok(Answer::New(applied.unwrap_or(record)))
    }

    /// Whether the lock screen shows `record` as far as MeeGone can tell: it was set, and
    /// nobody has changed it since (WALL-14).
    fn shows(&self, record: &ImageRecord) -> bool {
        record.applied() && !changed_by_someone_else(record, &*self.wallpaper)
    }

    /// Sets `record`'s cached image as the lock-screen wallpaper, only the lock screen's
    /// (WALL-01), and records when, with the marker the system gave it (SRS 7.5). If the
    /// lock screen has another wallpaper right after, the device didn't keep it, and the image
    /// may not appear there (SRS C5).
    ///
    /// Nothing is set once the daily image is off (WALL-12): then, or if a newer image was
    /// cached meanwhile, it answers `None`.
    fn apply(&self, record: &ImageRecord, now: u64) -> Result<Option<ImageRecord>, DataError> {
        if !self.settings.get().image.enabled {
            return Ok(None);
        }
        let (Some(file), Some(image)) = (record.image, self.cache.image(record)?) else {
            return Err(DataError::Failed("The image isn't cached".into()));
        };
        let set = self.wallpaper.set_lock_screen(&image)?;
        drop(image);
        let marker = set.map(|set| match self.wallpaper.lock_screen_marker() {
            Some(now) if now != set => Marker::NotKept,
            _ => Marker::Set(set),
        });
        self.cache.applied(file, now, marker)
    }

    /// Without background scheduling, the next start checks instead (SRS 8.1).
    fn schedule(&self, job: ImageJob) {
        let _ = self.scheduler.schedule_image_job(job);
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::{self, Fakes, HOME_WALLPAPER};
    use meegone_core_data::{JobNetwork, PersistentSettingsStore, Size, WallpaperMarker};
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

        // The image was downloaded, but nothing of it stays, the lock screen is as it was, and
        // no job waits.
        assert_eq!(fakes.network.requests.lock().unwrap().len(), 1);
        assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 0);
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

    fn lock_screen(fakes: &Fakes) -> Option<Vec<u8>> {
        fakes.wallpaper.lock_screen.lock().unwrap().clone()
    }

    #[test]
    fn a_new_image_is_set_as_the_lock_screen_wallpaper_once_it_is_cached() {
        // WALL-01, WALL-08, SRS 7.5: cached, then set, then its time and marker recorded.
        let Setup {
            fakes,
            cache,
            updater,
            ..
        } = setup("puffins");

        let Checked::New(record) = updater.check() else {
            panic!("expected a new image");
        };

        assert_eq!(lock_screen(&fakes), cache.image(&record).unwrap());
        assert!(lock_screen(&fakes).is_some());
        assert_eq!(record.applied_at_ms, Some(NOV_19 + 8 * HOUR_MS));
        assert_eq!(record.marker, Some(Marker::Set(WallpaperMarker(100))));
        assert_eq!(cache.current(), Some(record));
        assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 1);
        assert!(cache.checks().updated);
    }

    #[test]
    fn an_image_that_fails_any_step_sets_nothing() {
        // WALL-08, WALL-10: with no image ever cached, the lock screen stays as it is.
        for broken in [
            |fakes: &Fakes| *fakes.network.state.lock().unwrap() = NetworkState::Offline,
            |fakes: &Fakes| *fakes.wallpaper.cropped.lock().unwrap() = Some(b"garbage".to_vec()),
            |fakes: &Fakes| {
                *fakes.storage.error.lock().unwrap() = Some(DataError::Failed("Full".into()));
            },
        ] {
            let Setup {
                fakes,
                cache,
                updater,
                ..
            } = setup("puffins");
            broken(&fakes);

            assert!(matches!(updater.check(), Checked::Failed(_)));

            assert_eq!(lock_screen(&fakes), None);
            assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 0);
            assert_eq!(cache.current(), None);
        }
    }

    #[test]
    fn a_wallpaper_the_system_refuses_is_set_at_the_retry_without_downloading_again() {
        // WALL-08, REL-04
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");
        *fakes.wallpaper.refuses.lock().unwrap() = Some(DataError::Failed("Refused".into()));

        assert_eq!(
            updater.check(),
            Checked::Failed(DataError::Failed("Refused".into()))
        );
        // Cached, but not on the lock screen, and retried after the back-off.
        assert!(!cache.current().unwrap().applied());
        assert_eq!(lock_screen(&fakes), None);
        assert_eq!(cache.checks().problem.as_deref(), Some("Refused"));
        let retry = job(&fakes).unwrap().not_before_ms;
        assert_eq!(retry, NOV_19 + 8 * HOUR_MS + FIRST_BACK_OFF_MS);

        *fakes.wallpaper.refuses.lock().unwrap() = None;
        at(&fakes, retry);
        let Checked::New(record) = updater.check() else {
            panic!("expected the image set");
        };

        assert!(record.applied());
        assert_eq!(lock_screen(&fakes), cache.image(&record).unwrap());
        assert_eq!(feed_requests(&feed), 2);
        assert_eq!(fakes.network.requests.lock().unwrap().len(), 1);
        // Today's image is in: the next check is tomorrow's.
        assert_eq!(job(&fakes).unwrap().not_before_ms, NOV_20);
    }

    #[test]
    fn the_same_image_already_on_the_lock_screen_isnt_set_again() {
        // WALL-03
        let Setup { fakes, updater, .. } = setup("puffins");
        updater.check();

        at(&fakes, NOV_20 + HOUR_MS);
        assert_eq!(updater.check(), Checked::Same);

        assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 1);
    }

    #[test]
    fn a_wallpaper_someone_else_sets_pauses_the_daily_checks() {
        // WALL-14
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");
        updater.check();
        fakes.wallpaper.changed_by_someone_else();
        offer(&fakes, &feed, feed_image("lighthouse", "20261120"));

        at(&fakes, NOV_20 + HOUR_MS);
        assert_eq!(updater.check(), Checked::Paused);

        // Nothing is asked, nothing is set, and no job waits.
        assert_eq!(feed_requests(&feed), 1);
        assert_eq!(lock_screen(&fakes).as_deref(), Some(&b"someone else's"[..]));
        assert_eq!(job(&fakes), None);
        assert_eq!(cache.current().unwrap().id.as_deref(), Some("puffins"));
        assert!(!Checked::Paused.attempted());
    }

    #[test]
    fn update_now_resumes_by_putting_the_image_back_even_offline() {
        // WALL-11, WALL-14
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");
        let Checked::New(set) = updater.check() else {
            panic!("expected a new image");
        };
        fakes.wallpaper.changed_by_someone_else();
        *feed.error.lock().unwrap() = Some(DataError::Offline);
        at(&fakes, NOV_20 + HOUR_MS);

        assert_eq!(updater.update_now(), Checked::Failed(DataError::Offline));

        assert_eq!(lock_screen(&fakes), cache.image(&set).unwrap());
        let current = cache.current().unwrap();
        assert_eq!(current.applied_at_ms, Some(NOV_20 + HOUR_MS));
        assert_eq!(current.marker, Some(Marker::Set(WallpaperMarker(102))));
        // No longer paused: the daily checks go on, after the back-off.
        assert_eq!(
            job(&fakes).unwrap().not_before_ms,
            NOV_20 + HOUR_MS + FIRST_BACK_OFF_MS
        );
        *feed.error.lock().unwrap() = None;
        at(&fakes, NOV_20 + HOUR_MS + FIRST_BACK_OFF_MS);
        assert_eq!(updater.check(), Checked::Same);
    }

    #[test]
    fn update_now_fetches_at_once_on_any_network_past_the_days_budget() {
        // WALL-11: not held back by the time, the battery, the network or the budget, but
        // counted.
        let Setup {
            fakes,
            feed,
            cache,
            updater,
            ..
        } = setup("puffins");
        *fakes.network.state.lock().unwrap() = NetworkState::Metered;
        *fakes.system.saves_battery.lock().unwrap() = true;
        *fakes.network.saves_data.lock().unwrap() = true;
        cache
            .update_checks(&mut |checks| {
                checks.day = checks::local_day(NOV_19, &|_| 0);
                checks.attempts = 6;
                checks.failures = 6;
                checks.last_attempt_ms = Some(NOV_19 + 7 * HOUR_MS);
                checks.failing_since_ms = Some(NOV_19);
            })
            .unwrap();
        assert!(matches!(updater.check(), Checked::NotDue));

        let Checked::New(record) = updater.update_now() else {
            panic!("expected a new image");
        };

        assert_eq!(record.id.as_deref(), Some("puffins"));
        assert_eq!(lock_screen(&fakes), cache.image(&record).unwrap());
        assert_eq!(feed_requests(&feed), 1);
        assert_eq!(cache.checks().attempts, 7);
        assert_eq!(job(&fakes).unwrap().not_before_ms, NOV_20);
    }

    #[test]
    fn update_now_with_the_image_on_the_lock_screen_only_asks_the_feed() {
        // WALL-11
        let Setup {
            fakes,
            feed,
            updater,
            ..
        } = setup("puffins");
        updater.check();

        assert_eq!(updater.update_now(), Checked::Same);

        assert_eq!(feed_requests(&feed), 2);
        assert_eq!(*fakes.wallpaper.lock_screen_images_set.lock().unwrap(), 1);
    }

    #[test]
    fn update_now_does_nothing_while_the_daily_image_is_off() {
        // WALL-02
        let Setup {
            fakes,
            feed,
            settings,
            updater,
            ..
        } = setup("puffins");
        settings.update(&mut |s| s.image.enabled = false).unwrap();

        assert_eq!(updater.update_now(), Checked::Off);

        assert_eq!(feed_requests(&feed), 0);
        assert_eq!(lock_screen(&fakes), None);
    }

    #[test]
    fn an_image_the_device_doesnt_keep_is_marked_and_pauses_nothing() {
        // SRS C5, ADR 0030: the lock screen still has the home wallpaper right after.
        let Setup {
            fakes,
            cache,
            updater,
            ..
        } = setup("puffins");
        *fakes.wallpaper.keeps.lock().unwrap() = false;

        let Checked::New(record) = updater.check() else {
            panic!("expected a new image");
        };

        assert_eq!(record.marker, Some(Marker::NotKept));
        assert_eq!(
            *fakes.wallpaper.lock_screen_marker.lock().unwrap(),
            HOME_WALLPAPER
        );
        at(&fakes, NOV_20 + HOUR_MS);
        assert_eq!(updater.check(), Checked::Same);
        assert_eq!(cache.current().unwrap().marker, Some(Marker::NotKept));
    }

    #[test]
    fn where_wallpapers_cant_be_told_apart_nothing_pauses() {
        // WALL-14: "where the platform makes it detectable".
        let Setup {
            fakes,
            cache,
            updater,
            ..
        } = setup("puffins");
        *fakes.wallpaper.tracks.lock().unwrap() = false;
        updater.check();
        assert_eq!(cache.current().unwrap().marker, None);
        assert!(cache.current().unwrap().applied());

        fakes.wallpaper.changed_by_someone_else();
        at(&fakes, NOV_20 + HOUR_MS);

        assert_eq!(updater.check(), Checked::Same);
    }
}
