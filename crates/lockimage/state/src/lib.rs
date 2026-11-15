//! Lock-screen image state (SRS 4.5): whether the daily image is on or available, and the
//! current image's details.
//!
//! Its options live in the settings store (ADR 0002); the image details live in this feature's
//! image cache.

use std::sync::Arc;

use meegone_core_data::{DataError, SchedulerAdapter, SettingsStore, WallpaperAdapter};
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
            .and_then(|_| self.apply(on));
        self.state.problem = result.err().map(|error| error.to_string());
        self.refresh();
    }

    fn apply(&self, on: bool) -> Result<(), DataError> {
        if on {
            self.scheduler.ensure_daily_job()
        } else {
            // The current lock-screen wallpaper stays as it is (WALL-12).
            self.scheduler.cancel_daily_job()?;
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
    use meegone_core_data::MemorySettingsStore;
    use meegone_core_data::fakes::Fakes;
    use meegone_lockimage_data::MemoryImageCache;

    use super::*;

    fn setup() -> (Fakes, Arc<MemoryImageCache>, LockImageHolder) {
        let fakes = Fakes::default();
        let cache = Arc::new(MemoryImageCache::default());
        let mut holder = LockImageHolder::new(
            Arc::new(MemorySettingsStore::default()),
            cache.clone(),
            fakes.wallpaper.clone(),
            fakes.scheduler.clone(),
        );
        holder.handle(LockImageIntent::Refresh);
        (fakes, cache, holder)
    }

    #[test]
    fn the_daily_image_starts_off() {
        // WALL-02
        let (fakes, _, holder) = setup();

        assert_eq!(holder.state().status, ImageStatus::Off);
        assert!(!*fakes.scheduler.daily_job_scheduled.lock().unwrap());
    }

    #[test]
    fn turning_it_on_schedules_the_daily_job() {
        // WALL-03, ARCH-07
        let (fakes, _, mut holder) = setup();

        holder.handle(LockImageIntent::SetEnabled(true));

        assert_eq!(holder.state().status, ImageStatus::On);
        assert!(*fakes.scheduler.daily_job_scheduled.lock().unwrap());
    }

    #[test]
    fn turning_it_off_cancels_the_job_and_deletes_the_cache() {
        // WALL-12
        let (fakes, cache, mut holder) = setup();
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
        assert!(!*fakes.scheduler.daily_job_scheduled.lock().unwrap());
        assert_eq!(cache.current(), None);
    }

    #[test]
    fn an_unsupported_device_shows_the_reason_and_stays_off() {
        // WALL-13
        let (fakes, _, mut holder) = setup();
        *fakes.wallpaper.unsupported.lock().unwrap() = Some("Blocked by device policy".into());
        holder.handle(LockImageIntent::Refresh);

        holder.handle(LockImageIntent::SetEnabled(true));

        assert_eq!(
            holder.state().status,
            ImageStatus::Unavailable("Blocked by device policy".into())
        );
        assert!(!*fakes.scheduler.daily_job_scheduled.lock().unwrap());
    }
}
