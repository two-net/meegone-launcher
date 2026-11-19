//! Lock-screen image state (SRS 4.5): whether the daily image is on or available, the current
//! image's details, the feed region (WALL-05), "Use mobile data" (WALL-04), and how the daily
//! checks are going (SRS 7.6).
//!
//! Its options live in the settings store (ADR 0002); the image details and the check log live
//! in this feature's image cache. [`ImageUpdater`] runs the daily check, off the main thread
//! (ADR 0029); [`checks`] decides when.

pub mod checks;
mod updater;

use std::sync::Arc;

use meegone_core_data::{
    DataError, ImageOptions, SchedulerAdapter, SettingsStore, SystemConfigAdapter, WallpaperAdapter,
};
use meegone_core_state::StateHolder;
use meegone_lockimage_data::{ImageCache, ImageFeed, region};

pub use checks::CheckStatus;
pub use meegone_lockimage_data::{ImageFile, ImageRecord};
pub use updater::{Checked, ImageUpdater, Wait};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ImageStatus {
    /// The user hasn't turned the daily image on (WALL-02).
    #[default]
    Off,
    On,
    /// The lock-screen wallpaper can't be set on this device; the text says why (WALL-13).
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LockImageState {
    pub status: ImageStatus,
    pub current: Option<ImageRecord>,
    /// Whether Events shows the image-of-the-day card (EVT-12).
    pub show_card: bool,
    /// The regions the feed supports, for the Region setting (WALL-05).
    pub regions: Vec<String>,
    /// The region the user chose, while the feed supports it; `None` is Automatic (WALL-05).
    pub region: Option<String>,
    /// The region Automatic stands for on this device (WALL-05).
    pub automatic_region: String,
    /// Whether automatic downloads may use mobile data (WALL-04).
    pub use_mobile_data: bool,
    /// How the daily checks are going (WALL-16, SRS 7.6).
    pub checks: CheckStatus,
    pub problem: Option<String>,
}

impl LockImageState {
    /// The region the feed is asked for: the one chosen, or Automatic's (WALL-05).
    pub fn region_in_use(&self) -> &str {
        self.region.as_deref().unwrap_or(&self.automatic_region)
    }
}

#[derive(Debug)]
pub enum LockImageIntent {
    Refresh,
    /// Turns the daily image on or off (WALL-02, WALL-12).
    SetEnabled(bool),
    /// Shows or hides the image-of-the-day card in Events (EVT-12).
    SetShowCard(bool),
    /// Chooses the feed region, or Automatic with `None` (WALL-05). A region the feed doesn't
    /// support is ignored.
    SetRegion(Option<String>),
    /// Lets automatic downloads use mobile data, or not (WALL-04).
    SetUseMobileData(bool),
    /// After days of failures, checks again as soon as the network and battery allow, within
    /// the day's budget (SRS 7.6, REL-04).
    TryAgain,
    /// Opens the image details screen, from the card in Events (EVT-12, WALL-15). The screen
    /// covers the home views, so the shell opens it (ADR 0022); the holder ignores it.
    OpenDetails,
}

pub struct LockImageHolder {
    settings: Arc<dyn SettingsStore>,
    cache: Arc<dyn ImageCache>,
    feed: Arc<dyn ImageFeed>,
    wallpaper: Arc<dyn WallpaperAdapter>,
    scheduler: Arc<dyn SchedulerAdapter>,
    /// The device's locale, from which Automatic is derived (WALL-05).
    system: Arc<dyn SystemConfigAdapter>,
    state: LockImageState,
}

impl LockImageHolder {
    pub fn new(
        settings: Arc<dyn SettingsStore>,
        cache: Arc<dyn ImageCache>,
        feed: Arc<dyn ImageFeed>,
        wallpaper: Arc<dyn WallpaperAdapter>,
        scheduler: Arc<dyn SchedulerAdapter>,
        system: Arc<dyn SystemConfigAdapter>,
    ) -> Self {
        Self {
            settings,
            cache,
            feed,
            wallpaper,
            scheduler,
            system,
            state: LockImageState::default(),
        }
    }

    fn refresh(&mut self) {
        let options = self.settings.get().image;
        self.state.show_card = options.show_card;
        // Reading the regions and the locale makes no request (WALL-05).
        let regions = self.feed.regions();
        self.state.automatic_region = region::automatic(&self.system.locale(), &regions);
        self.state.region = options.region.filter(|chosen| regions.contains(chosen));
        self.state.regions = regions;
        self.state.use_mobile_data = options.use_mobile_data;
        self.state.current = self.cache.current();
        self.state.checks = checks::status(&self.cache.checks(), self.system.now_ms());
        self.state.status = match self.wallpaper.lock_screen_support() {
            Err(reason) => ImageStatus::Unavailable(reason.to_string()),
            Ok(()) if options.enabled => ImageStatus::On,
            Ok(()) => ImageStatus::Off,
        };
    }

    fn set_enabled(&mut self, on: bool) {
        if matches!(self.state.status, ImageStatus::Unavailable(_)) {
            return;
        }
        let result = self
            .settings
            .update(&mut |settings| settings.image.enabled = on)
            .and_then(|settings| self.apply(&settings.image));
        self.state.problem = result.err().map(|error| error.to_string());
        self.refresh();
    }

    fn set_show_card(&mut self, on: bool) {
        let result = self
            .settings
            .update(&mut |settings| settings.image.show_card = on);
        self.state.problem = result.err().map(|error| error.to_string());
        self.refresh();
    }

    /// The lock-screen image is the one writer of the region (ARCH-06). The choice takes effect
    /// with the next image; a failed save keeps the region as it was.
    fn set_region(&mut self, choice: Option<String>) {
        if choice
            .as_ref()
            .is_some_and(|chosen| !self.state.regions.contains(chosen))
        {
            return;
        }
        let result = self
            .settings
            .update(&mut |settings| settings.image.region = choice.clone());
        self.state.problem = result.err().map(|error| error.to_string());
        self.refresh();
    }

    /// The lock-screen image is the one writer of "Use mobile data" (ARCH-06). The waiting job
    /// takes the new network at once (SET-03).
    fn set_use_mobile_data(&mut self, on: bool) {
        let result = self
            .settings
            .update(&mut |settings| settings.image.use_mobile_data = on)
            .and_then(|settings| self.arm(settings.image.enabled, settings.image.use_mobile_data));
        self.state.problem = result.err().map(|error| error.to_string());
        self.refresh();
    }

    /// Forgets the failures, so "Daily image unavailable" goes, and the job runs as soon as it
    /// may (SRS 7.6).
    fn try_again(&mut self) {
        let options = self.settings.get().image;
        let result = self
            .cache
            .update_checks(&mut checks::try_again)
            .and_then(|_| self.arm(options.enabled, options.use_mobile_data));
        self.state.problem = result.err().map(|error| error.to_string());
        self.refresh();
    }

    fn apply(&self, options: &ImageOptions) -> Result<(), DataError> {
        if options.enabled {
            self.arm(true, options.use_mobile_data)
        } else {
            // The current lock-screen wallpaper stays as it is (WALL-12).
            self.scheduler.cancel_image_job()?;
            self.cache.clear()
        }
    }

    /// Schedules the job for the next check, while the daily image is on (WALL-03, WALL-04).
    fn arm(&self, enabled: bool, use_mobile_data: bool) -> Result<(), DataError> {
        if !enabled || matches!(self.state.status, ImageStatus::Unavailable(_)) {
            return Ok(());
        }
        let now = self.system.now_ms();
        let offset = |at| self.system.utc_offset_ms(at);
        let job = checks::next_job(&self.cache.checks(), now, &offset, use_mobile_data);
        self.scheduler.schedule_image_job(job)
    }
}

impl StateHolder for LockImageHolder {
    type State = LockImageState;
    type Intent = LockImageIntent;

    fn state(&self) -> &LockImageState {
        &self.state
    }

    fn handle(&mut self, intent: LockImageIntent) {
        match intent {
            LockImageIntent::Refresh => self.refresh(),
            LockImageIntent::SetEnabled(on) => self.set_enabled(on),
            LockImageIntent::SetShowCard(on) => self.set_show_card(on),
            LockImageIntent::SetRegion(choice) => self.set_region(choice),
            LockImageIntent::SetUseMobileData(on) => self.set_use_mobile_data(on),
            LockImageIntent::TryAgain => self.try_again(),
            LockImageIntent::OpenDetails => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::Fakes;
    use meegone_core_data::{JobNetwork, PersistentSettingsStore};
    use meegone_lockimage_data::PersistentImageCache;
    use meegone_lockimage_data::fakes::FakeFeed;

    use super::*;

    struct Setup {
        fakes: Fakes,
        feed: Arc<FakeFeed>,
        settings: Arc<PersistentSettingsStore>,
        cache: Arc<PersistentImageCache>,
        holder: LockImageHolder,
    }

    fn setup() -> Setup {
        let fakes = Fakes::default();
        let feed = Arc::new(FakeFeed::default());
        let settings = Arc::new(PersistentSettingsStore::new(fakes.storage.clone()));
        let cache = Arc::new(PersistentImageCache::new(fakes.storage.clone()));
        let mut holder = holder(&fakes, &feed, &settings, &cache);
        holder.handle(LockImageIntent::Refresh);
        Setup {
            fakes,
            feed,
            settings,
            cache,
            holder,
        }
    }

    fn holder(
        fakes: &Fakes,
        feed: &Arc<FakeFeed>,
        settings: &Arc<PersistentSettingsStore>,
        cache: &Arc<PersistentImageCache>,
    ) -> LockImageHolder {
        LockImageHolder::new(
            settings.clone(),
            cache.clone(),
            feed.clone(),
            fakes.wallpaper.clone(),
            fakes.scheduler.clone(),
            fakes.system.clone(),
        )
    }

    #[test]
    fn the_daily_image_starts_off() {
        // WALL-02
        let Setup { fakes, holder, .. } = setup();

        assert_eq!(holder.state().status, ImageStatus::Off);
        assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
    }

    #[test]
    fn turning_it_on_schedules_the_daily_job_on_unmetered_networks() {
        // WALL-03, WALL-04, ARCH-07
        let Setup {
            fakes, mut holder, ..
        } = setup();

        holder.handle(LockImageIntent::SetEnabled(true));

        assert_eq!(holder.state().status, ImageStatus::On);
        assert_eq!(
            fakes.scheduler.job.lock().unwrap().map(|job| job.network),
            Some(JobNetwork::Unmetered)
        );
    }

    #[test]
    fn with_mobile_data_allowed_the_job_runs_on_any_network() {
        // WALL-04
        let Setup {
            fakes,
            settings,
            mut holder,
            ..
        } = setup();
        settings
            .update(&mut |s| s.image.use_mobile_data = true)
            .unwrap();

        holder.handle(LockImageIntent::SetEnabled(true));

        assert_eq!(
            fakes.scheduler.job.lock().unwrap().map(|job| job.network),
            Some(JobNetwork::Any)
        );
    }

    #[test]
    fn without_background_scheduling_the_feature_stays_on_and_says_why() {
        // SRS 8.1: without background scheduling, the image is checked at the next start.
        let Setup {
            fakes, mut holder, ..
        } = setup();
        *fakes.scheduler.error.lock().unwrap() = Some(DataError::Unsupported(
            "Background work is restricted".into(),
        ));

        holder.handle(LockImageIntent::SetEnabled(true));

        assert_eq!(holder.state().status, ImageStatus::On);
        assert_eq!(
            holder.state().problem.as_deref(),
            Some("Background work is restricted")
        );
    }

    #[test]
    fn use_mobile_data_is_saved_and_the_waiting_job_takes_it_at_once() {
        // WALL-04, SET-03
        let Setup {
            fakes,
            settings,
            mut holder,
            ..
        } = setup();
        holder.handle(LockImageIntent::SetEnabled(true));
        assert!(!holder.state().use_mobile_data);

        holder.handle(LockImageIntent::SetUseMobileData(true));

        assert!(holder.state().use_mobile_data);
        assert!(settings.get().image.use_mobile_data);
        assert_eq!(
            fakes.scheduler.job.lock().unwrap().map(|job| job.network),
            Some(JobNetwork::Any)
        );

        holder.handle(LockImageIntent::SetUseMobileData(false));

        assert!(!settings.get().image.use_mobile_data);
        assert_eq!(
            fakes.scheduler.job.lock().unwrap().map(|job| job.network),
            Some(JobNetwork::Unmetered)
        );
    }

    #[test]
    fn use_mobile_data_while_off_schedules_nothing() {
        // WALL-02
        let Setup {
            fakes,
            settings,
            mut holder,
            ..
        } = setup();

        holder.handle(LockImageIntent::SetUseMobileData(true));

        assert!(settings.get().image.use_mobile_data);
        assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
    }

    /// The checks failing since `since_ms`, the last with "No connection".
    fn failing_since(cache: &PersistentImageCache, since_ms: u64) {
        cache
            .update_checks(&mut |checks| {
                checks.day = since_ms / 86_400_000;
                checks.attempts = 6;
                checks.failures = 6;
                checks.last_attempt_ms = Some(since_ms);
                checks.failing_since_ms = Some(since_ms);
                checks.problem = Some("No connection".into());
            })
            .unwrap();
    }

    #[test]
    fn three_days_of_failures_show_daily_image_unavailable() {
        // SRS 7.6
        let Setup {
            fakes,
            cache,
            mut holder,
            ..
        } = setup();
        holder.handle(LockImageIntent::SetEnabled(true));
        let day = 86_400_000;
        failing_since(&cache, 10 * day);

        *fakes.system.now_ms.lock().unwrap() = 10 * day + 3_600_000;
        holder.handle(LockImageIntent::Refresh);
        assert_eq!(
            holder.state().checks,
            CheckStatus::Retrying("No connection".into())
        );

        *fakes.system.now_ms.lock().unwrap() = 13 * day;
        holder.handle(LockImageIntent::Refresh);
        assert_eq!(
            holder.state().checks,
            CheckStatus::Unavailable("No connection".into())
        );
    }

    #[test]
    fn try_again_clears_the_failures_and_runs_the_job_as_soon_as_it_may() {
        // SRS 7.6, REL-04
        let Setup {
            fakes,
            cache,
            mut holder,
            ..
        } = setup();
        holder.handle(LockImageIntent::SetEnabled(true));
        let day = 86_400_000;
        failing_since(&cache, 10 * day);
        *fakes.system.now_ms.lock().unwrap() = 13 * day + 3_600_000;

        holder.handle(LockImageIntent::TryAgain);

        assert_eq!(holder.state().checks, CheckStatus::Fine);
        assert_eq!(cache.checks().failing_since_ms, None);
        // A new day, so the job may run at once: since that day's midnight.
        assert_eq!(
            fakes
                .scheduler
                .job
                .lock()
                .unwrap()
                .map(|job| job.not_before_ms),
            Some(13 * day)
        );
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn turning_it_off_cancels_the_job_and_deletes_the_cache() {
        // WALL-12
        let Setup {
            fakes,
            cache,
            mut holder,
            ..
        } = setup();
        holder.handle(LockImageIntent::SetEnabled(true));
        cache
            .put(
                ImageRecord {
                    date: "20261115".into(),
                    region: "en-US".into(),
                    title: "Title".into(),
                    attribution: "© Someone".into(),
                    info_link: None,
                    applied_at_ms: Some(1),
                    image: None,
                    id: None,
                },
                b"image",
            )
            .unwrap();

        holder.handle(LockImageIntent::SetEnabled(false));

        assert_eq!(holder.state().status, ImageStatus::Off);
        assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
        assert_eq!(cache.current(), None);
    }

    #[test]
    fn an_unsupported_device_shows_the_reason_and_stays_off() {
        // WALL-13
        let Setup {
            fakes, mut holder, ..
        } = setup();
        *fakes.wallpaper.unsupported.lock().unwrap() = Some("Blocked by device policy".into());
        holder.handle(LockImageIntent::Refresh);

        holder.handle(LockImageIntent::SetEnabled(true));

        assert_eq!(
            holder.state().status,
            ImageStatus::Unavailable("Blocked by device policy".into())
        );
        assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
    }

    #[test]
    fn the_card_shows_until_the_user_hides_it() {
        // EVT-12, SET-03: the choice is saved and takes effect at once.
        let Setup {
            settings,
            mut holder,
            ..
        } = setup();
        assert!(holder.state().show_card);

        holder.handle(LockImageIntent::SetShowCard(false));

        assert!(!holder.state().show_card);
        assert!(!settings.get().image.show_card);
        assert_eq!(holder.state().problem, None);

        holder.handle(LockImageIntent::SetShowCard(true));

        assert!(holder.state().show_card);
        assert!(settings.get().image.show_card);
    }

    #[test]
    fn a_choice_that_cant_be_saved_is_the_problem() {
        // ARCH-09
        let Setup {
            fakes, mut holder, ..
        } = setup();
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed("Disk error".into()));

        holder.handle(LockImageIntent::SetShowCard(false));

        assert_eq!(holder.state().problem.as_deref(), Some("Disk error"));
    }

    #[test]
    fn the_region_starts_automatic_from_the_devices_language() {
        // WALL-05
        let Setup {
            fakes, mut holder, ..
        } = setup();
        assert_eq!(holder.state().regions, ["en-US", "en-GB", "fi-FI"]);
        assert_eq!(holder.state().region, None);
        assert_eq!(holder.state().region_in_use(), "en-US");

        *fakes.system.locale.lock().unwrap() = "en-IE".into();
        holder.handle(LockImageIntent::Refresh);
        assert_eq!(holder.state().automatic_region, "en-US");

        *fakes.system.locale.lock().unwrap() = "fi-FI".into();
        holder.handle(LockImageIntent::Refresh);
        assert_eq!(holder.state().region_in_use(), "fi-FI");
    }

    #[test]
    fn a_chosen_region_is_saved_and_used() {
        // WALL-05, SET-03: saved and in effect at once, across a restart.
        let Setup {
            fakes,
            feed,
            settings,
            cache,
            mut holder,
        } = setup();

        holder.handle(LockImageIntent::SetRegion(Some("en-GB".into())));

        assert_eq!(holder.state().region.as_deref(), Some("en-GB"));
        assert_eq!(holder.state().region_in_use(), "en-GB");
        assert_eq!(settings.get().image.region.as_deref(), Some("en-GB"));
        let mut restarted = holder_after_restart(&fakes, &feed, &cache);
        restarted.handle(LockImageIntent::Refresh);
        assert_eq!(restarted.state().region_in_use(), "en-GB");

        holder.handle(LockImageIntent::SetRegion(None));

        assert_eq!(holder.state().region, None);
        assert_eq!(holder.state().region_in_use(), "en-US");
        assert_eq!(settings.get().image.region, None);
        // The feed was never asked for an image.
        assert!(feed.requests.lock().unwrap().is_empty());
    }

    /// A new holder over the same files, as after the process restarts.
    fn holder_after_restart(
        fakes: &Fakes,
        feed: &Arc<FakeFeed>,
        cache: &Arc<PersistentImageCache>,
    ) -> LockImageHolder {
        let settings = Arc::new(PersistentSettingsStore::new(fakes.storage.clone()));
        holder(fakes, feed, &settings, cache)
    }

    #[test]
    fn a_region_the_feed_doesnt_support_is_ignored() {
        // WALL-05
        let Setup {
            settings,
            mut holder,
            ..
        } = setup();
        holder.handle(LockImageIntent::SetRegion(Some("en-GB".into())));

        holder.handle(LockImageIntent::SetRegion(Some("xx-XX".into())));

        assert_eq!(holder.state().region.as_deref(), Some("en-GB"));
        assert_eq!(settings.get().image.region.as_deref(), Some("en-GB"));
    }

    #[test]
    fn a_saved_region_the_feed_no_longer_lists_is_automatic() {
        // WALL-05
        let Setup {
            feed,
            settings,
            mut holder,
            ..
        } = setup();
        holder.handle(LockImageIntent::SetRegion(Some("fi-FI".into())));

        feed.regions
            .lock()
            .unwrap()
            .retain(|region| region != "fi-FI");
        holder.handle(LockImageIntent::Refresh);

        assert_eq!(holder.state().region, None);
        assert_eq!(holder.state().region_in_use(), "en-US");
        // The saved choice stays, in case the feed lists it again.
        assert_eq!(settings.get().image.region.as_deref(), Some("fi-FI"));
    }

    #[test]
    fn a_region_that_cant_be_saved_is_the_problem() {
        // ARCH-09
        let Setup {
            fakes, mut holder, ..
        } = setup();
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed("Disk error".into()));

        holder.handle(LockImageIntent::SetRegion(Some("en-GB".into())));

        assert_eq!(holder.state().problem.as_deref(), Some("Disk error"));
        assert_eq!(holder.state().region, None);
    }

    #[test]
    fn the_holder_leaves_opening_the_details_to_the_shell() {
        let Setup { mut holder, .. } = setup();
        let before = holder.state().clone();

        holder.handle(LockImageIntent::OpenDetails);

        assert_eq!(*holder.state(), before);
    }
}
