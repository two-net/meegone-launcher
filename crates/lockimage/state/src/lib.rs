//! Lock-screen image state (SRS 4.5): whether the daily image is on or available, the current
//! image's details, the feed region (WALL-05), "Use mobile data" (WALL-04), how the daily
//! checks are going (SRS 7.6), "Update now" (WALL-11), whether daily updates are paused
//! because someone else changed the lock screen (WALL-14), resetting the lock screen once the
//! image is off (WALL-12), what the image details screen shows and opens (WALL-15, WALL-16),
//! the past week's images, one of which can be pinned (WALL-17), and setup's lock-screen image
//! step: today's image, the switch, and what Continue applies (ONB-01, WALL-02, ADR 0036).
//!
//! Its options live in the settings store (ADR 0002); the image details, the pin and the check
//! log live in this feature's image cache, and the past week in memory. [`ImageUpdater`] runs
//! the daily check, browses, pins and sets the wallpaper, off the main thread (ADR 0029,
//! ADR 0030, ADR 0032); [`checks`] decides when. What the user asks of it waits in
//! [`ImageMemory`]'s tasks until the job's thread runs it.

pub mod checks;
mod updater;

use std::sync::Arc;

use meegone_core_data::{
    DataError, ImageOptions, SchedulerAdapter, SettingsStore, SystemConfigAdapter, TimeStyle,
    WallpaperAdapter,
};
use meegone_core_state::StateHolder;
use meegone_lockimage_data::{ImageCache, ImageFeed, region};

pub use checks::CheckStatus;
pub use meegone_core_state::Problem;
pub use meegone_lockimage_data::{
    ImageFile, ImageMemory, ImageRecord, ImageTask, ImageTasks, Marker, PAST_WEEK_IMAGES,
    PastImages, Preview, TodayImage,
};
pub use updater::{Checked, ImageUpdater, Wait};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ImageStatus {
    /// The user hasn't turned the daily image on (WALL-02).
    #[default]
    Off,
    On,
    /// The lock-screen wallpaper can't be set on this device; the problem says why (WALL-13).
    Unavailable(Problem),
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
    /// Whether "Update now", Resume or Unpin is on its way (WALL-11, WALL-14, WALL-17).
    pub updating: bool,
    /// Whether someone else changed the lock screen's wallpaper after MeeGone set it, so daily
    /// updates wait until the user resumes them (WALL-14).
    pub paused: bool,
    /// Whether the device didn't keep the image as the lock screen's wallpaper, so it may not
    /// appear there (SRS C5, ADR 0030).
    pub may_not_show: bool,
    /// The current image's date, written in the device's language, such as "15 November";
    /// empty when there is none or it isn't a date (WALL-15, L10N-03).
    pub date: String,
    /// When MeeGone last set an image on the lock screen, written in the device's language,
    /// such as "19 November, 07:00" (WALL-16, L10N-03).
    pub updated: Option<String>,
    /// Whether a web browser is installed, without which More info is hidden (SRS 8.1).
    pub has_browser: bool,
    /// Whether the current image is pinned, so daily updates wait until it is unpinned
    /// (WALL-17). Only while the daily image is on.
    pub pinned: bool,
    /// The past week's images, today's first, once browsed (WALL-17).
    pub past: Vec<PastEntry>,
    /// Whether the past week is being read.
    pub browsing: bool,
    /// The past week's image being pinned, by the feed's id.
    pub pinning: Option<String>,
    /// The last problem browsing or pinning had.
    pub past_problem: Option<Problem>,
    /// Setup's lock-screen image step, from when setup reaches it until Continue (ONB-01).
    pub setup: Option<ImageSetup>,
    pub problem: Option<Problem>,
}

/// Setup's lock-screen image step (ONB-01, ADR 0036). Nothing of it is saved, and nothing
/// reaches the lock screen, until Continue (WALL-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSetup {
    /// The step's switch: on until the user turns it off (WALL-02).
    pub on: bool,
    /// Today's image in the region in use, once read; never while offline.
    pub today: Option<TodayEntry>,
    /// Whether today's image is being read.
    pub loading: bool,
}

/// Today's image as setup's lock-screen image step shows it (ONB-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodayEntry {
    pub title: String,
    /// As the source provides it (LEGAL-03).
    pub attribution: String,
    /// Its preview, when it is offered as a wallpaper and the preview checked out (LEGAL-04).
    pub preview: Option<Preview>,
}

/// One of the past week's images, as browsing shows it (WALL-17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastEntry {
    /// The feed's id, which names it to pin.
    pub id: String,
    /// Its date, written in the device's language, such as "15 November" (L10N-03).
    pub date: String,
    pub title: String,
    /// As the source provides it (LEGAL-03).
    pub attribution: String,
    /// Whether it may be pinned: the source offers it as a wallpaper (LEGAL-04).
    pub offered: bool,
    /// Whether it is the current image.
    pub current: bool,
    /// Its preview, when it has one.
    pub preview: Option<Preview>,
}

impl LockImageState {
    /// The region the feed is asked for: the one chosen, or Automatic's (WALL-05).
    pub fn region_in_use(&self) -> &str {
        self.region.as_deref().unwrap_or(&self.automatic_region)
    }

    /// Whether the image details screen has something to show: the daily image is on and has
    /// an image (WALL-15, ADR 0031).
    pub fn has_details(&self) -> bool {
        self.status == ImageStatus::On
            && self
                .current
                .as_ref()
                .is_some_and(|current| current.image.is_some())
    }

    /// Whether the past week can be browsed: the daily image is on, with an image yet or not
    /// (WALL-17).
    pub fn past_offered(&self) -> bool {
        self.status == ImageStatus::On
    }

    /// Whether the past week's entry `id` may be pinned now: it is listed, offered as a
    /// wallpaper, and not pinned already, and nothing else is being pinned (WALL-17,
    /// LEGAL-04).
    pub fn may_pin(&self, id: &str) -> bool {
        self.past_offered()
            && self.pinning.is_none()
            && self
                .past
                .iter()
                .any(|entry| entry.id == id && entry.offered && !(entry.current && self.pinned))
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
    /// Fetches today's image at once, over any connection, after letting go of a pin and
    /// putting the current image back on the lock screen, which resumes paused updates
    /// (WALL-11, WALL-14). Resume and Unpin are this too (WALL-17). Only while the daily image
    /// is on; the update runs on the job's thread, and [`Self::Checked`] reports it.
    UpdateNow,
    /// A check, or a task the user asked for, has run on the job's thread: reads the image,
    /// the checks, the past week and the tasks again (ADR 0029, ADR 0032).
    Checked,
    /// Opens the past week's images, from the image details screen or Settings (WALL-17). The
    /// screen covers the home views, so the shell opens it, and asks for [`Self::Browse`]; the
    /// holder ignores it.
    OpenPastImages,
    /// Reads the past week's images and their previews, on the job's thread, unless they are
    /// read already today. Only while the daily image is on (WALL-17).
    Browse,
    /// Pins the past week's image with the feed's id, on the job's thread: it becomes the
    /// lock-screen wallpaper, and daily updates wait until it is unpinned. Only an image
    /// [`LockImageState::may_pin`] allows (WALL-17).
    Pin(String),
    /// Makes the lock screen show the home wallpaper again. Only while the daily image is off
    /// (WALL-12).
    ResetLockScreen,
    /// Opens the system's wallpaper settings (WALL-12).
    OpenWallpaperSettings,
    /// Opens the image details screen, from the card in Events or the home menu (EVT-12,
    /// WALL-15). The screen covers the home views, so the shell opens it (ADR 0022, ADR 0031);
    /// the holder ignores it.
    OpenDetails,
    /// More info: opens the current image's info link in the browser. Only while the daily image
    /// is on, and only an `https://` link (WALL-15, SEC-02).
    OpenInfoLink,
    /// Opens Settings at its Lock-screen image group, from the details screen (WALL-15).
    /// Settings covers the home views, so the shell opens it (ADR 0031); the holder ignores it.
    OpenSettings,
    /// Setup has reached its lock-screen image step (ONB-01): the step's switch starts on, and
    /// today's image is read on the job's thread, for the region in use. Nothing is saved or
    /// scheduled, while the daily image may still be off (WALL-02). The shell sends it.
    OpenSetup,
    /// The step's switch: only the step changes, nothing is saved (WALL-02).
    SetupSwitched(bool),
    /// Continue on the step: the daily image is turned on, if the switch is on, and today's
    /// check runs at once on the job's thread, within its rules, so the image is on the lock
    /// screen within a minute on Wi-Fi (ONB-03, WALL-04). With the switch off, the daily image
    /// stays off. The step closes either way.
    ConfirmSetup,
    /// Settings → Reset launcher (SET-06): the options go back to their defaults in one save.
    /// The daily image is off, as switching it off leaves it, with nothing cached or scheduled;
    /// the region is Automatic, mobile data isn't used and the card shows. The lock screen keeps
    /// its picture (WALL-12).
    Reset,
}

pub struct LockImageHolder {
    settings: Arc<dyn SettingsStore>,
    cache: Arc<dyn ImageCache>,
    feed: Arc<dyn ImageFeed>,
    wallpaper: Arc<dyn WallpaperAdapter>,
    scheduler: Arc<dyn SchedulerAdapter>,
    /// The device's locale, from which Automatic is derived (WALL-05).
    system: Arc<dyn SystemConfigAdapter>,
    /// The past week the job's thread read (WALL-17), and what waits for that thread or runs
    /// on it.
    memory: Arc<ImageMemory>,
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
        memory: Arc<ImageMemory>,
    ) -> Self {
        Self {
            settings,
            cache,
            feed,
            wallpaper,
            scheduler,
            system,
            memory,
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
            Err(reason) => ImageStatus::Unavailable(reason.problem()),
            Ok(()) if options.enabled => ImageStatus::On,
            Ok(()) => ImageStatus::Off,
        };
        let current = self
            .state
            .current
            .as_ref()
            .filter(|_| self.state.status == ImageStatus::On);
        self.state.pinned = current.is_some_and(|current| current.pinned);
        // While pinned, the daily updates wait anyway (WALL-17).
        self.state.paused = !self.state.pinned
            && current
                .is_some_and(|current| updater::changed_by_someone_else(current, &*self.wallpaper));
        self.state.may_not_show =
            current.is_some_and(|current| current.marker == Some(Marker::NotKept));
        self.write_times();
        self.state.has_browser = self.system.has_browser();
        self.read_past();
        self.read_tasks();
    }

    /// The past week as last browsed, while the daily image is on (WALL-17).
    fn read_past(&mut self) {
        let on = self.state.status == ImageStatus::On;
        let week = self.memory.past.week().filter(|_| on);
        let current = self.state.current.as_ref();
        self.state.past = week
            .map(|week| {
                week.images
                    .iter()
                    .map(|past| PastEntry {
                        id: past.image.id.clone(),
                        date: self.written_date(&past.image.date),
                        title: past.image.title.clone(),
                        attribution: past.image.attribution.clone(),
                        offered: past.image.offered_as_wallpaper,
                        current: current.is_some_and(|current| {
                            current.id.as_deref() == Some(past.image.id.as_str())
                                && current.region == week.region
                        }),
                        preview: past.preview.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.state.past_problem = self.memory.past.problem().filter(|_| on);
    }

    /// What waits for the job's thread or runs on it, whatever has reported back meanwhile.
    fn read_tasks(&mut self) {
        let pending = self.memory.tasks.pending();
        self.state.updating = pending.contains(&ImageTask::UpdateNow);
        self.state.browsing = pending.contains(&ImageTask::Browse);
        self.state.pinning = pending.into_iter().find_map(|task| match task {
            ImageTask::Pin(id) => Some(id),
            _ => None,
        });
        self.show_today();
    }

    /// The feed's `yyyymmdd` written in the device's language, at noon so a time zone far from
    /// UTC doesn't move it; empty if it isn't a date (WALL-15, L10N-03).
    fn written_date(&self, date: &str) -> String {
        let offset = |at| self.system.utc_offset_ms(at);
        checks::day_of(date)
            .map(|day| {
                let noon = checks::local_noon_ms(day, &offset);
                self.system.format_time(noon, TimeStyle::DayMonth)
            })
            .unwrap_or_default()
    }

    /// Writes the current image's date and when it was set, in the device's language. The date
    /// is written at noon, so a time zone far from UTC doesn't move it (WALL-15, WALL-16).
    fn write_times(&mut self) {
        let current = self.state.current.as_ref();
        self.state.date = current
            .map(|current| self.written_date(&current.date))
            .unwrap_or_default();
        self.state.updated = current
            .and_then(|current| current.applied_at_ms)
            .map(|at| self.system.format_time(at, TimeStyle::DayMonthTime));
    }

    fn set_enabled(&mut self, on: bool) {
        if matches!(self.state.status, ImageStatus::Unavailable(_)) {
            return;
        }
        let result = self
            .settings
            .update(&mut |settings| settings.image.enabled = on)
            .and_then(|settings| self.apply(&settings.image));
        self.state.problem = result.err().map(|error| error.problem());
        self.refresh();
    }

    /// The lock-screen image is the one writer of its options (ARCH-06), so it restores them
    /// for Reset launcher (SET-06). Off is applied as switching it off is, even where the
    /// device can't set the lock screen, so nothing is left cached or scheduled.
    fn reset(&mut self) {
        let result = self
            .settings
            .update(&mut |settings| settings.image = ImageOptions::default())
            .and_then(|settings| self.apply(&settings.image));
        self.state.problem = result.err().map(|error| error.problem());
        self.refresh();
    }

    fn set_show_card(&mut self, on: bool) {
        let result = self
            .settings
            .update(&mut |settings| settings.image.show_card = on);
        self.state.problem = result.err().map(|error| error.problem());
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
        let saved = result.is_ok();
        self.state.problem = result.err().map(|error| error.problem());
        self.refresh();
        // Setup's step shows today's image in the region chosen (ONB-01).
        if saved {
            self.read_today();
        }
    }

    /// The lock-screen image is the one writer of "Use mobile data" (ARCH-06). The waiting job
    /// takes the new network at once (SET-03).
    fn set_use_mobile_data(&mut self, on: bool) {
        let result = self
            .settings
            .update(&mut |settings| settings.image.use_mobile_data = on)
            .and_then(|settings| self.arm(settings.image.enabled, settings.image.use_mobile_data));
        self.state.problem = result.err().map(|error| error.problem());
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
        self.state.problem = result.err().map(|error| error.problem());
        self.refresh();
    }

    /// The update runs on the job's thread, so this only asks for it; one at a time.
    fn update_now(&mut self) {
        if self.state.status == ImageStatus::On {
            self.ask(ImageTask::UpdateNow);
        }
    }

    /// Reading the past week runs on the job's thread; one at a time (WALL-17).
    fn browse(&mut self) {
        if self.state.past_offered() {
            self.ask(ImageTask::Browse);
        }
    }

    /// Setup has reached its step: a switch that starts on, kept if the user comes back to the
    /// step, and today's image (ONB-01, WALL-02).
    fn open_setup(&mut self) {
        if self.state.setup.is_none() {
            self.state.setup = Some(ImageSetup {
                on: true,
                today: None,
                loading: false,
            });
        }
        self.read_today();
    }

    fn setup_switched(&mut self, on: bool) {
        if let Some(setup) = &mut self.state.setup {
            setup.on = on;
        }
    }

    /// Continue: applies the step's switch, the user's confirmation (WALL-02). On, the daily
    /// check runs at once rather than whenever the system's scheduler gets to it (ONB-03).
    fn confirm_setup(&mut self) {
        let Some(setup) = self.state.setup.take() else {
            return;
        };
        self.memory.today.clear();
        if setup.on {
            self.set_enabled(true);
            if self.state.status == ImageStatus::On && self.state.problem.is_none() {
                self.ask(ImageTask::Check);
            }
        } else if self.state.status == ImageStatus::On {
            // On already, as restored from a backup (ADR 0018): the switch turns it off.
            self.set_enabled(false);
        } else {
            self.refresh();
        }
    }

    /// Reads today's image for setup's step on the job's thread, where the device can set the
    /// lock screen (ONB-01, WALL-13).
    fn read_today(&mut self) {
        if self.state.setup.is_some() && !matches!(self.state.status, ImageStatus::Unavailable(_)) {
            self.ask(ImageTask::PreviewToday);
        }
    }

    /// Today's image for setup's step, as the job's thread read it for the region in use, and
    /// whether it is being read.
    fn show_today(&mut self) {
        let loading = self
            .memory
            .tasks
            .pending()
            .contains(&ImageTask::PreviewToday);
        let region = self.state.region_in_use().to_owned();
        let today = self
            .memory
            .today
            .get()
            .filter(|today| today.region == region)
            .map(|today| TodayEntry {
                title: today.image.image.title,
                attribution: today.image.image.attribution,
                preview: today.image.preview,
            });
        if let Some(setup) = &mut self.state.setup {
            setup.loading = loading;
            setup.today = today;
        }
    }

    /// Pinning runs on the job's thread; one at a time, and only an image that may be pinned
    /// (WALL-17, LEGAL-04).
    fn pin(&mut self, id: String) {
        if self.state.may_pin(&id) {
            self.ask(ImageTask::Pin(id));
        }
    }

    /// Queues `task` for the job's thread, unless it is already on its way, and asks for the
    /// queue to run. If the platform can't run it, the task is taken back and the reason is the
    /// problem.
    fn ask(&mut self, task: ImageTask) {
        if self.memory.tasks.push(task.clone()) {
            let result = self.scheduler.run_image_tasks();
            if result.is_err() {
                self.memory.tasks.withdraw(&task);
            }
            self.state.problem = result.err().map(|error| error.problem());
        }
        self.read_tasks();
    }

    fn checked(&mut self) {
        self.refresh();
    }

    /// While the daily image is on, the next image would replace the home wallpaper on the lock
    /// screen anyway, and the reset would look like someone else's change (WALL-14).
    fn reset_lock_screen(&mut self) {
        if self.state.status != ImageStatus::Off {
            return;
        }
        let result = self.wallpaper.reset_lock_screen();
        self.state.problem = result.err().map(|error| error.problem());
    }

    fn open_wallpaper_settings(&mut self) {
        let result = self.wallpaper.open_wallpaper_settings();
        self.state.problem = result.err().map(|error| error.problem());
    }

    /// Only a link the details screen offers opens: the daily image is on, the current image
    /// has one, it is `https://`, and a browser is installed (WALL-15, SEC-02, SRS 8.1).
    fn open_info_link(&mut self) {
        if self.state.status != ImageStatus::On || !self.state.has_browser {
            return;
        }
        let Some(link) = self
            .state
            .current
            .as_ref()
            .and_then(|current| current.info_link.as_deref())
            .filter(|link| link.starts_with("https://"))
        else {
            return;
        };
        let result = self.system.open_in_browser(link);
        self.state.problem = result.err().map(|error| error.problem());
    }

    fn apply(&self, options: &ImageOptions) -> Result<(), DataError> {
        if options.enabled {
            self.arm(true, options.use_mobile_data)
        } else {
            // The current lock-screen wallpaper stays as it is (WALL-12). Nothing browsed stays
            // either.
            self.memory.past.clear();
            self.memory.today.clear();
            self.scheduler.cancel_image_job()?;
            self.cache.clear()
        }
    }

    /// Schedules the job for the next check, while the daily image is on and nothing is
    /// pinned (WALL-03, WALL-04, WALL-17).
    fn arm(&self, enabled: bool, use_mobile_data: bool) -> Result<(), DataError> {
        if !enabled
            || matches!(self.state.status, ImageStatus::Unavailable(_))
            || self.cache.current().is_some_and(|current| current.pinned)
        {
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
            LockImageIntent::UpdateNow => self.update_now(),
            LockImageIntent::Checked => self.checked(),
            LockImageIntent::ResetLockScreen => self.reset_lock_screen(),
            LockImageIntent::OpenWallpaperSettings => self.open_wallpaper_settings(),
            LockImageIntent::OpenInfoLink => self.open_info_link(),
            LockImageIntent::Browse => self.browse(),
            LockImageIntent::Pin(id) => self.pin(id),
            LockImageIntent::Reset => self.reset(),
            LockImageIntent::OpenSetup => self.open_setup(),
            LockImageIntent::SetupSwitched(on) => self.setup_switched(on),
            LockImageIntent::ConfirmSetup => self.confirm_setup(),
            LockImageIntent::OpenDetails
            | LockImageIntent::OpenSettings
            | LockImageIntent::OpenPastImages => {}
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
        memory: Arc<ImageMemory>,
        holder: LockImageHolder,
    }

    fn setup() -> Setup {
        let fakes = Fakes::default();
        let feed = Arc::new(FakeFeed::default());
        let settings = Arc::new(PersistentSettingsStore::new(fakes.storage.clone()));
        let cache = Arc::new(PersistentImageCache::new(fakes.storage.clone()));
        let memory = Arc::new(ImageMemory::default());
        let mut holder = LockImageHolder::new(
            settings.clone(),
            cache.clone(),
            feed.clone(),
            fakes.wallpaper.clone(),
            fakes.scheduler.clone(),
            fakes.system.clone(),
            memory.clone(),
        );
        holder.handle(LockImageIntent::Refresh);
        Setup {
            fakes,
            feed,
            settings,
            cache,
            memory,
            holder,
        }
    }

    /// A holder of its own, with nothing browsed or queued.
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
            Arc::default(),
        )
    }

    /// The updater the job's thread runs, on the same stores.
    fn updater(setup: &Setup) -> ImageUpdater {
        ImageUpdater::new(
            setup.settings.clone(),
            setup.cache.clone(),
            setup.feed.clone(),
            setup.fakes.network.clone(),
            setup.fakes.wallpaper.clone(),
            setup.fakes.scheduler.clone(),
            setup.fakes.system.clone(),
        )
    }

    /// Runs the queued tasks as the job's thread would, then tells the holder.
    fn run_tasks(setup: &mut Setup) {
        let updater = updater(setup);
        let past = &setup.memory.past;
        setup.memory.tasks.run_each(&mut |task| {
            match task {
                ImageTask::UpdateNow => updater.update_now(),
                ImageTask::Browse => {
                    updater.browse(past);
                    Checked::NotDue
                }
                ImageTask::Pin(id) => updater.pin(id, past),
                ImageTask::PreviewToday => {
                    updater.preview_today(&setup.memory.today);
                    Checked::NotDue
                }
                ImageTask::Check => updater.check(),
            };
        });
        setup.holder.handle(LockImageIntent::Checked);
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
        *fakes.scheduler.error.lock().unwrap() =
            Some(DataError::Unsupported(Problem::ImageJobUnavailable));

        holder.handle(LockImageIntent::SetEnabled(true));

        assert_eq!(holder.state().status, ImageStatus::On);
        assert_eq!(holder.state().problem, Some(Problem::ImageJobUnavailable));
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
                checks.problem = Some(Problem::NoConnection);
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
            CheckStatus::Retrying(Problem::NoConnection)
        );

        *fakes.system.now_ms.lock().unwrap() = 13 * day;
        holder.handle(LockImageIntent::Refresh);
        assert_eq!(
            holder.state().checks,
            CheckStatus::Unavailable(Problem::NoConnection)
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
                    marker: None,
                    pinned: false,
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
    fn reset_restores_the_options_and_turns_the_daily_image_off_leaving_the_lock_screen() {
        // SET-06, WALL-12
        let mut setup = setup();
        with_an_image_set(&mut setup);
        setup
            .holder
            .handle(LockImageIntent::SetRegion(Some("en-GB".into())));
        setup.holder.handle(LockImageIntent::SetUseMobileData(true));
        setup.holder.handle(LockImageIntent::SetShowCard(false));
        assert!(setup.fakes.scheduler.job.lock().unwrap().is_some());

        setup.holder.handle(LockImageIntent::Reset);

        assert_eq!(setup.settings.get().image, ImageOptions::default());
        assert_eq!(setup.holder.state().status, ImageStatus::Off);
        assert_eq!(setup.holder.state().region, None);
        assert!(!setup.holder.state().use_mobile_data);
        assert!(setup.holder.state().show_card);
        assert_eq!(*setup.fakes.scheduler.job.lock().unwrap(), None);
        assert_eq!(setup.cache.current(), None);
        assert!(setup.fakes.wallpaper.lock_screen.lock().unwrap().is_some());
        assert_eq!(setup.holder.state().problem, None);
    }

    #[test]
    fn a_reset_that_cant_be_saved_says_why() {
        // SET-06, ARCH-09
        let mut setup = setup();
        setup.holder.handle(LockImageIntent::SetShowCard(false));
        *setup.fakes.storage.error.lock().unwrap() = Some(DataError::Failed(Problem::StorageFull));

        setup.holder.handle(LockImageIntent::Reset);

        assert_eq!(setup.holder.state().problem, Some(Problem::StorageFull));
        assert!(!setup.holder.state().show_card);
    }

    #[test]
    fn an_unsupported_device_shows_the_reason_and_stays_off() {
        // WALL-13
        let Setup {
            fakes, mut holder, ..
        } = setup();
        *fakes.wallpaper.unsupported.lock().unwrap() = Some(Problem::WallpaperNotAllowed);
        holder.handle(LockImageIntent::Refresh);

        holder.handle(LockImageIntent::SetEnabled(true));

        assert_eq!(
            holder.state().status,
            ImageStatus::Unavailable(Problem::WallpaperNotAllowed)
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
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed(Problem::SavedDataUnusable));

        holder.handle(LockImageIntent::SetShowCard(false));

        assert_eq!(holder.state().problem, Some(Problem::SavedDataUnusable));
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
            ..
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
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed(Problem::SavedDataUnusable));

        holder.handle(LockImageIntent::SetRegion(Some("en-GB".into())));

        assert_eq!(holder.state().problem, Some(Problem::SavedDataUnusable));
        assert_eq!(holder.state().region, None);
    }

    #[test]
    fn the_holder_leaves_opening_the_details_and_settings_to_the_shell() {
        let Setup { mut holder, .. } = setup();
        let before = holder.state().clone();

        holder.handle(LockImageIntent::OpenDetails);
        holder.handle(LockImageIntent::OpenSettings);

        assert_eq!(*holder.state(), before);
    }

    /// 19 November 2026, 00:00 UTC.
    const NOV_19: u64 = 1_795_046_400_000;
    const HOUR: u64 = 3_600_000;

    /// The daily image on, with a current image from `date` with `info_link`, set on the lock
    /// screen at `applied_at_ms`.
    fn with_a_record(
        setup: &mut Setup,
        date: &str,
        info_link: Option<&str>,
        applied_at_ms: Option<u64>,
    ) {
        setup.holder.handle(LockImageIntent::SetEnabled(true));
        setup
            .cache
            .put(
                ImageRecord {
                    date: date.into(),
                    region: "en-GB".into(),
                    title: "Puffins on a cliff".into(),
                    attribution: "© A Photographer".into(),
                    info_link: info_link.map(Into::into),
                    applied_at_ms,
                    image: None,
                    id: None,
                    marker: None,
                    pinned: false,
                },
                b"image",
            )
            .unwrap();
        setup.holder.handle(LockImageIntent::Refresh);
    }

    #[test]
    fn the_images_date_and_when_it_was_set_are_written_in_the_devices_language() {
        // WALL-15, WALL-16, L10N-03: eight hours behind UTC, the image was set late on the
        // 18th; its date is still the 15th.
        let mut setup = setup();
        *setup.fakes.system.utc_offset_ms.lock().unwrap() = -8 * HOUR as i64;

        with_a_record(&mut setup, "20261115", None, Some(NOV_19 + 7 * HOUR));

        let state = setup.holder.state();
        assert_eq!(state.date, "15 November");
        assert_eq!(state.updated.as_deref(), Some("18 November, 11:00 PM"));
        assert!(state.has_browser);
    }

    #[test]
    fn an_image_not_yet_set_or_without_a_date_writes_neither() {
        // WALL-16
        let mut setup = setup();
        assert_eq!(setup.holder.state().date, "");
        assert_eq!(setup.holder.state().updated, None);

        with_a_record(&mut setup, "2026-11-15", None, None);

        assert_eq!(setup.holder.state().date, "");
        assert_eq!(setup.holder.state().updated, None);
    }

    #[test]
    fn more_info_opens_the_images_link_in_the_browser() {
        // WALL-15, SEC-02
        let mut setup = setup();
        with_a_record(
            &mut setup,
            "20261115",
            Some("https://example.com/puffins"),
            None,
        );

        setup.holder.handle(LockImageIntent::OpenInfoLink);

        assert_eq!(
            *setup.fakes.system.opened_urls.lock().unwrap(),
            ["https://example.com/puffins"]
        );
        assert_eq!(setup.holder.state().problem, None);
    }

    #[test]
    fn more_info_opens_nothing_without_a_link_a_browser_https_or_the_daily_image() {
        // WALL-15, SEC-02, SRS 8.1
        let opened = |link: Option<&str>, change: &dyn Fn(&mut Setup)| {
            let mut setup = setup();
            with_a_record(&mut setup, "20261115", link, None);
            change(&mut setup);
            setup.holder.handle(LockImageIntent::OpenInfoLink);
            setup.fakes.system.opened_urls.lock().unwrap().len()
        };
        let link = Some("https://example.com/puffins");
        assert_eq!(opened(link, &|_| {}), 1);

        assert_eq!(opened(None, &|_| {}), 0);
        assert_eq!(opened(Some("http://example.com/puffins"), &|_| {}), 0);
        assert_eq!(
            opened(link, &|setup| {
                *setup.fakes.system.browser.lock().unwrap() = false;
                setup.holder.handle(LockImageIntent::Refresh);
            }),
            0
        );
        assert_eq!(
            opened(link, &|setup| {
                setup
                    .settings
                    .update(&mut |s| s.image.enabled = false)
                    .unwrap();
                setup.holder.handle(LockImageIntent::Refresh);
            }),
            0
        );
    }

    #[test]
    fn a_link_the_browser_cant_open_says_why() {
        // WALL-15, ARCH-09
        let mut setup = setup();
        with_a_record(
            &mut setup,
            "20261115",
            Some("https://example.com/puffins"),
            None,
        );
        *setup.fakes.system.error.lock().unwrap() =
            Some(DataError::Unsupported(Problem::BrowserUnavailable));

        setup.holder.handle(LockImageIntent::OpenInfoLink);

        assert_eq!(
            setup.holder.state().problem,
            Some(Problem::BrowserUnavailable)
        );
    }

    /// The daily image on, with an image the updater has set as the lock-screen wallpaper.
    fn with_an_image_set(setup: &mut Setup) {
        setup.holder.handle(LockImageIntent::SetEnabled(true));
        *setup.fakes.network.state.lock().unwrap() = meegone_core_data::NetworkState::Unmetered;
        let image = meegone_lockimage_data::fakes::feed_image("puffins", "20261119");
        for version in &image.versions {
            let size = meegone_core_data::Size {
                width: version.width,
                height: version.height,
            };
            setup
                .fakes
                .network
                .responses
                .lock()
                .unwrap()
                .insert(version.url.clone(), meegone_core_data::fakes::jpeg(size));
        }
        *setup.feed.image.lock().unwrap() = Some(image);
        assert!(matches!(updater(setup).check(), Checked::New(_)));
        setup.holder.handle(LockImageIntent::Checked);
    }

    #[test]
    fn update_now_runs_on_the_jobs_thread_once_until_it_has_checked() {
        // WALL-11
        let mut setup = setup();
        setup.holder.handle(LockImageIntent::SetEnabled(true));

        setup.holder.handle(LockImageIntent::UpdateNow);
        setup.holder.handle(LockImageIntent::UpdateNow);

        assert!(setup.holder.state().updating);
        assert_eq!(*setup.fakes.scheduler.task_runs.lock().unwrap(), 1);
        assert_eq!(setup.memory.tasks.pending(), [ImageTask::UpdateNow]);
        // Other reads don't end it, nor does an automatic check's report: only its own run
        // does (ADR 0032).
        setup.holder.handle(LockImageIntent::Refresh);
        setup.holder.handle(LockImageIntent::Checked);
        assert!(setup.holder.state().updating);

        run_tasks(&mut setup);

        assert!(!setup.holder.state().updating);
        setup.holder.handle(LockImageIntent::UpdateNow);
        assert_eq!(*setup.fakes.scheduler.task_runs.lock().unwrap(), 2);
    }

    #[test]
    fn update_now_does_nothing_while_the_image_is_off_or_unavailable() {
        // WALL-02, WALL-13
        let mut setup = setup();
        setup.holder.handle(LockImageIntent::UpdateNow);

        *setup.fakes.wallpaper.unsupported.lock().unwrap() = Some(Problem::WallpaperNotAllowed);
        setup.holder.handle(LockImageIntent::Refresh);
        setup.holder.handle(LockImageIntent::UpdateNow);

        assert!(!setup.holder.state().updating);
        assert_eq!(*setup.fakes.scheduler.task_runs.lock().unwrap(), 0);
    }

    #[test]
    fn an_image_on_the_lock_screen_is_neither_paused_nor_in_doubt() {
        let mut setup = setup();
        with_an_image_set(&mut setup);

        let state = setup.holder.state();
        assert!(state.current.as_ref().unwrap().applied());
        assert!(!state.paused);
        assert!(!state.may_not_show);
    }

    #[test]
    fn someone_elses_wallpaper_pauses_the_daily_image_until_update_now() {
        // WALL-14
        let mut setup = setup();
        with_an_image_set(&mut setup);

        setup.fakes.wallpaper.changed_by_someone_else();
        setup.holder.handle(LockImageIntent::Refresh);

        assert!(setup.holder.state().paused);
        assert_eq!(setup.holder.state().status, ImageStatus::On);

        // Resuming is "Update now", which puts the image back.
        setup.holder.handle(LockImageIntent::UpdateNow);
        assert_eq!(*setup.fakes.scheduler.task_runs.lock().unwrap(), 1);
    }

    #[test]
    fn turning_it_off_ends_the_pause() {
        // WALL-12, WALL-14: nothing is cached, so nothing waits to resume.
        let mut setup = setup();
        with_an_image_set(&mut setup);
        setup.fakes.wallpaper.changed_by_someone_else();
        setup.holder.handle(LockImageIntent::Refresh);

        setup.holder.handle(LockImageIntent::SetEnabled(false));

        assert!(!setup.holder.state().paused);
    }

    #[test]
    fn an_image_the_device_didnt_keep_may_not_show() {
        // SRS C5
        let mut setup = setup();
        *setup.fakes.wallpaper.keeps.lock().unwrap() = false;

        with_an_image_set(&mut setup);

        assert!(setup.holder.state().may_not_show);
        assert!(!setup.holder.state().paused);
    }

    #[test]
    fn once_off_the_lock_screen_can_be_reset_to_the_home_wallpaper() {
        // WALL-12, AC-17
        let mut setup = setup();
        with_an_image_set(&mut setup);
        assert!(setup.fakes.wallpaper.lock_screen.lock().unwrap().is_some());

        // Not while the daily image is on.
        setup.holder.handle(LockImageIntent::ResetLockScreen);
        assert_eq!(*setup.fakes.wallpaper.resets.lock().unwrap(), 0);

        // Turning it off leaves the wallpaper as it is.
        setup.holder.handle(LockImageIntent::SetEnabled(false));
        assert!(setup.fakes.wallpaper.lock_screen.lock().unwrap().is_some());

        setup.holder.handle(LockImageIntent::ResetLockScreen);

        assert_eq!(*setup.fakes.wallpaper.resets.lock().unwrap(), 1);
        assert_eq!(*setup.fakes.wallpaper.lock_screen.lock().unwrap(), None);
        assert_eq!(setup.holder.state().problem, None);
    }

    #[test]
    fn a_reset_or_wallpaper_settings_that_fail_say_why() {
        // WALL-12, ARCH-09
        let Setup {
            fakes, mut holder, ..
        } = setup();
        *fakes.wallpaper.error.lock().unwrap() = Some(DataError::Unsupported(
            Problem::WallpaperSettingsInstructions,
        ));

        holder.handle(LockImageIntent::OpenWallpaperSettings);
        assert_eq!(
            holder.state().problem,
            Some(Problem::WallpaperSettingsInstructions)
        );

        *fakes.wallpaper.error.lock().unwrap() = None;
        holder.handle(LockImageIntent::OpenWallpaperSettings);
        assert_eq!(holder.state().problem, None);
        assert_eq!(*fakes.wallpaper.settings_opened.lock().unwrap(), 1);

        *fakes.wallpaper.error.lock().unwrap() = Some(DataError::Failed(Problem::SystemFailed));
        holder.handle(LockImageIntent::ResetLockScreen);
        assert_eq!(holder.state().problem, Some(Problem::SystemFailed));
    }
    /// The feed offers `images` as the past week, and serves each one's sizes and preview.
    fn offer_week(setup: &Setup, images: Vec<meegone_lockimage_data::FeedImage>) {
        let mut responses = setup.fakes.network.responses.lock().unwrap();
        for image in &images {
            for version in image.versions.iter().chain([&image.preview]) {
                let size = meegone_core_data::Size {
                    width: version.width,
                    height: version.height,
                };
                responses.insert(version.url.clone(), meegone_core_data::fakes::jpeg(size));
            }
        }
        *setup.feed.week.lock().unwrap() = images;
    }

    /// Today's puffins, set by the daily check, barn owls, and a sculpture not offered as a
    /// wallpaper.
    fn with_a_week(setup: &mut Setup) {
        with_an_image_set(setup);
        let image = meegone_lockimage_data::fakes::feed_image;
        offer_week(
            setup,
            vec![
                image("puffins", "20261119"),
                image("owls", "20261118"),
                meegone_lockimage_data::FeedImage {
                    offered_as_wallpaper: false,
                    ..image("sculpture", "20261117")
                },
            ],
        );
    }

    #[test]
    fn browsing_runs_on_the_jobs_thread_and_lists_the_week() {
        // WALL-17
        let mut setup = setup();
        with_a_week(&mut setup);

        setup.holder.handle(LockImageIntent::Browse);
        setup.holder.handle(LockImageIntent::Browse);
        assert!(setup.holder.state().browsing);
        assert_eq!(*setup.fakes.scheduler.task_runs.lock().unwrap(), 1);

        run_tasks(&mut setup);

        let state = setup.holder.state();
        assert!(!state.browsing);
        assert_eq!(
            state
                .past
                .iter()
                .map(|entry| (entry.id.as_str(), entry.offered, entry.current))
                .collect::<Vec<_>>(),
            [
                ("puffins", true, true),
                ("owls", true, false),
                ("sculpture", false, false)
            ]
        );
        assert_eq!(state.past[1].date, setup.holder.written_date("20261118"));
        assert!(!state.past[1].date.is_empty());
        assert_eq!(state.past[1].title, "Image owls");
        assert!(state.past[1].preview.is_some());
        // No preview for an image not offered as a wallpaper (LEGAL-04).
        assert_eq!(state.past[2].preview, None);
        assert!(state.may_pin("owls") && state.may_pin("puffins"));
        assert!(!state.may_pin("sculpture") && !state.may_pin("lighthouse"));
    }

    #[test]
    fn pinning_and_unpinning_run_on_the_jobs_thread() {
        // WALL-17, issue #31
        let mut setup = setup();
        with_a_week(&mut setup);
        setup.holder.handle(LockImageIntent::Browse);
        run_tasks(&mut setup);

        // Only an image that may be pinned, and one at a time.
        setup
            .holder
            .handle(LockImageIntent::Pin("sculpture".into()));
        setup.holder.handle(LockImageIntent::Pin("owls".into()));
        setup.holder.handle(LockImageIntent::Pin("puffins".into()));
        assert_eq!(setup.holder.state().pinning.as_deref(), Some("owls"));
        assert_eq!(
            setup.memory.tasks.pending(),
            [ImageTask::Pin("owls".into())]
        );

        run_tasks(&mut setup);

        let state = setup.holder.state();
        assert_eq!(state.pinning, None);
        assert!(state.pinned);
        assert_eq!(state.current.as_ref().unwrap().id.as_deref(), Some("owls"));
        assert!(state.past[1].current && !state.past[0].current);
        assert!(!state.may_pin("owls"));
        assert_eq!(*setup.fakes.scheduler.job.lock().unwrap(), None);

        // Unpin is "Update now".
        setup.holder.handle(LockImageIntent::UpdateNow);
        run_tasks(&mut setup);

        let state = setup.holder.state();
        assert!(!state.pinned);
        assert_eq!(
            state.current.as_ref().unwrap().id.as_deref(),
            Some("puffins")
        );
        assert!(setup.fakes.scheduler.job.lock().unwrap().is_some());
    }

    #[test]
    fn a_pin_that_fails_says_why() {
        // WALL-16, WALL-17
        let mut setup = setup();
        with_a_week(&mut setup);
        setup.holder.handle(LockImageIntent::Browse);
        run_tasks(&mut setup);
        *setup.fakes.network.state.lock().unwrap() = meegone_core_data::NetworkState::Offline;

        setup.holder.handle(LockImageIntent::Pin("owls".into()));
        run_tasks(&mut setup);

        let state = setup.holder.state();
        assert_eq!(state.past_problem, Some(Problem::NoConnection));
        assert!(!state.pinned);
        assert_eq!(
            state.current.as_ref().unwrap().id.as_deref(),
            Some("puffins")
        );
    }

    #[test]
    fn while_pinned_nothing_pauses_and_nothing_schedules_the_job() {
        // WALL-14, WALL-17: daily updates wait until unpinned.
        let mut setup = setup();
        with_a_week(&mut setup);
        setup.holder.handle(LockImageIntent::Browse);
        run_tasks(&mut setup);
        setup.holder.handle(LockImageIntent::Pin("owls".into()));
        run_tasks(&mut setup);

        setup.fakes.wallpaper.changed_by_someone_else();
        setup.holder.handle(LockImageIntent::Refresh);
        assert!(!setup.holder.state().paused);

        setup.holder.handle(LockImageIntent::SetUseMobileData(true));
        setup.holder.handle(LockImageIntent::TryAgain);
        assert_eq!(*setup.fakes.scheduler.job.lock().unwrap(), None);
    }

    #[test]
    fn nothing_is_browsed_or_pinned_while_the_image_is_off_and_turning_it_off_forgets_the_week() {
        // WALL-02, WALL-12
        let mut setup = setup();
        setup.holder.handle(LockImageIntent::Browse);
        setup.holder.handle(LockImageIntent::Pin("owls".into()));
        assert!(setup.memory.tasks.pending().is_empty());
        assert!(!setup.holder.state().past_offered());

        with_a_week(&mut setup);
        assert!(setup.holder.state().past_offered());
        setup.holder.handle(LockImageIntent::Browse);
        run_tasks(&mut setup);
        assert_eq!(setup.holder.state().past.len(), 3);

        setup.holder.handle(LockImageIntent::SetEnabled(false));

        assert!(setup.holder.state().past.is_empty());
        assert_eq!(setup.memory.past.week(), None);
    }

    /// On Wi-Fi, with today's `id` in the feed, its preview and its full-size versions served.
    fn todays_image_served(setup: &Setup, id: &str) {
        *setup.fakes.network.state.lock().unwrap() = meegone_core_data::NetworkState::Unmetered;
        let image = meegone_lockimage_data::fakes::feed_image(id, "20261119");
        let mut responses = setup.fakes.network.responses.lock().unwrap();
        for version in image.versions.iter().chain([&image.preview]) {
            let size = meegone_core_data::Size {
                width: version.width,
                height: version.height,
            };
            responses.insert(version.url.clone(), meegone_core_data::fakes::jpeg(size));
        }
        *setup.feed.image.lock().unwrap() = Some(image);
    }

    #[test]
    fn setups_step_starts_on_and_saves_nothing() {
        // ONB-01, WALL-02: the switch starts on, but nothing is applied before Continue.
        let Setup {
            fakes,
            settings,
            memory,
            mut holder,
            ..
        } = setup();

        holder.handle(LockImageIntent::OpenSetup);
        holder.handle(LockImageIntent::SetupSwitched(false));
        holder.handle(LockImageIntent::SetupSwitched(true));

        let step = holder.state().setup.clone().unwrap();
        assert!(step.on);
        assert!(step.loading, "today's image is on its way");
        assert_eq!(memory.tasks.pending(), [ImageTask::PreviewToday]);
        assert_eq!(*fakes.scheduler.task_runs.lock().unwrap(), 1);
        assert_eq!(holder.state().status, ImageStatus::Off);
        assert!(!settings.get().image.enabled);
        assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
        assert!(fakes.storage.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn setups_step_shows_todays_image_once_read() {
        // ONB-01, LEGAL-03
        let mut setup = setup();
        todays_image_served(&setup, "puffins");
        setup.holder.handle(LockImageIntent::OpenSetup);

        run_tasks(&mut setup);

        let step = setup.holder.state().setup.clone().unwrap();
        let today = step.today.unwrap();
        assert!(!step.loading);
        assert_eq!(today.title, "Image puffins");
        assert_eq!(today.attribution, "© Someone");
        assert!(today.preview.is_some());
        assert_eq!(setup.holder.state().status, ImageStatus::Off);
        assert!(setup.fakes.wallpaper.lock_screen.lock().unwrap().is_none());
    }

    #[test]
    fn choosing_a_region_at_setups_step_shows_its_image() {
        // ONB-01, WALL-05
        let mut setup = setup();
        todays_image_served(&setup, "puffins");
        setup.holder.handle(LockImageIntent::OpenSetup);
        run_tasks(&mut setup);

        setup
            .holder
            .handle(LockImageIntent::SetRegion(Some("fi-FI".into())));
        assert_eq!(setup.holder.state().setup.as_ref().unwrap().today, None);
        run_tasks(&mut setup);

        assert!(setup.holder.state().setup.as_ref().unwrap().today.is_some());
        assert_eq!(*setup.feed.requests.lock().unwrap(), ["en-US", "fi-FI"]);
        assert!(!setup.settings.get().image.enabled);
    }

    #[test]
    fn continue_turns_the_daily_image_on_and_checks_at_once() {
        // ONB-03, WALL-02: Continue is the confirmation; the check runs within its rules.
        let mut setup = setup();
        todays_image_served(&setup, "puffins");
        setup.holder.handle(LockImageIntent::OpenSetup);
        run_tasks(&mut setup);

        setup.holder.handle(LockImageIntent::ConfirmSetup);

        assert_eq!(setup.holder.state().setup, None);
        assert_eq!(setup.holder.state().status, ImageStatus::On);
        assert!(setup.settings.get().image.enabled);
        assert!(setup.fakes.scheduler.job.lock().unwrap().is_some());
        assert_eq!(setup.memory.tasks.pending(), [ImageTask::Check]);
        assert_eq!(setup.memory.today.get(), None);

        run_tasks(&mut setup);

        assert!(setup.fakes.wallpaper.lock_screen.lock().unwrap().is_some());
        let current = setup.holder.state().current.clone().unwrap();
        assert_eq!(current.id.as_deref(), Some("puffins"));
    }

    #[test]
    fn continue_with_the_switch_off_leaves_the_daily_image_off() {
        // WALL-02
        let Setup {
            fakes,
            settings,
            memory,
            mut holder,
            ..
        } = setup();
        holder.handle(LockImageIntent::OpenSetup);
        holder.handle(LockImageIntent::SetupSwitched(false));

        holder.handle(LockImageIntent::ConfirmSetup);

        assert_eq!(holder.state().setup, None);
        assert_eq!(holder.state().status, ImageStatus::Off);
        assert!(!settings.get().image.enabled);
        assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
        assert!(!memory.tasks.pending().contains(&ImageTask::Check));
        assert!(fakes.storage.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn where_the_lock_screen_cant_be_set_setups_step_reads_and_applies_nothing() {
        // WALL-13
        let Setup {
            fakes,
            settings,
            memory,
            mut holder,
            ..
        } = setup();
        *fakes.wallpaper.unsupported.lock().unwrap() = Some(Problem::WallpaperNotAllowed);
        holder.handle(LockImageIntent::Refresh);

        holder.handle(LockImageIntent::OpenSetup);
        assert!(memory.tasks.pending().is_empty());
        holder.handle(LockImageIntent::ConfirmSetup);

        assert!(matches!(holder.state().status, ImageStatus::Unavailable(_)));
        assert!(!settings.get().image.enabled);
        assert_eq!(*fakes.scheduler.job.lock().unwrap(), None);
    }
}
