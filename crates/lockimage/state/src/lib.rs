//! Lock-screen image state (SRS 4.5): whether the daily image is on or available, and the
//! current image's details.
//!
//! Its options live in the settings store (ADR 0002); the image details live in this feature's
//! image cache.

use std::sync::Arc;

use meegone_core_data::{
    DataError, ImageJob, ImageOptions, JobNetwork, SchedulerAdapter, SettingsStore,
    WallpaperAdapter,
};
use meegone_core_state::StateHolder;
use meegone_lockimage_data::ImageCache;

pub use meegone_lockimage_data::ImageRecord;

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
    pub problem: Option<String>,
}

#[derive(Debug)]
pub enum LockImageIntent {
    Refresh,
    /// Turns the daily image on or off (WALL-02, WALL-12).
    SetEnabled(bool),
}

pub struct LockImageHolder {
    settings: Arc<dyn SettingsStore>,
    cache: Arc<dyn ImageCache>,
    wallpaper: Arc<dyn WallpaperAdapter>,
    scheduler: Arc<dyn SchedulerAdapter>,
    state: LockImageState,
}

impl LockImageHolder {
    pub fn new(
        settings: Arc<dyn SettingsStore>,
        cache: Arc<dyn ImageCache>,
        wallpaper: Arc<dyn WallpaperAdapter>,
        scheduler: Arc<dyn SchedulerAdapter>,
    ) -> Self {
        Self {
            settings,
            cache,
            wallpaper,
            scheduler,
            state: LockImageState::default(),
        }
    }

    fn refresh(&mut self) {
        let options = self.settings.get().image;
        self.state.show_card = options.show_card;
        self.state.current = self.cache.current();
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

    fn apply(&self, options: &ImageOptions) -> Result<(), DataError> {
        if options.enabled {
            // When the job runs again after this is decided by the updater (WALL-03, #28).
            self.scheduler.schedule_image_job(ImageJob {
                not_before_ms: 0,
                network: if options.use_mobile_data {
                    JobNetwork::Any
                } else {
                    JobNetwork::Unmetered
                },
            })
        } else {
            // The current lock-screen wallpaper stays as it is (WALL-12).
            self.scheduler.cancel_image_job()?;
            self.cache.clear()
        }
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
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::PersistentSettingsStore;
    use meegone_core_data::fakes::Fakes;
    use meegone_lockimage_data::PersistentImageCache;

    use super::*;

    struct Setup {
        fakes: Fakes,
        settings: Arc<PersistentSettingsStore>,
        cache: Arc<PersistentImageCache>,
        holder: LockImageHolder,
    }

    fn setup() -> Setup {
        let fakes = Fakes::default();
        let settings = Arc::new(PersistentSettingsStore::new(fakes.storage.clone()));
        let cache = Arc::new(PersistentImageCache::new(fakes.storage.clone()));
        let mut holder = LockImageHolder::new(
            settings.clone(),
            cache.clone(),
            fakes.wallpaper.clone(),
            fakes.scheduler.clone(),
        );
        holder.handle(LockImageIntent::Refresh);
        Setup {
            fakes,
            settings,
            cache,
            holder,
        }
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
            .put(ImageRecord {
                date: "20261115".into(),
                region: "en-US".into(),
                title: "Title".into(),
                attribution: "© Someone".into(),
                info_link: None,
                applied_at_ms: Some(1),
            })
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
}
