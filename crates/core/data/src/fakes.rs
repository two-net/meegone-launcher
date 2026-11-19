//! Configurable fakes of every platform adapter and of storage, for tests (MAINT-01).
//!
//! Enable the `fakes` feature in `[dev-dependencies]` only (SEC-06); arch-check rejects it
//! anywhere else. Each fake's fields are public: set them to shape what the "system" reports,
//! and read them to see what was asked of it.
//!
//! Every fake has an `error` field. When it is set, every call that can fail fails with that
//! error, so each feature's failure handling can be tested (ARCH-09). The other fields
//! reproduce each capability's "if unavailable" case in SRS 8.1.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::accent;
use crate::adapters::{
    AppsAdapter, NetworkAdapter, NotificationsAdapter, Platform, SchedulerAdapter,
    SystemConfigAdapter, UsageAdapter, WallpaperAdapter,
};
use crate::model::{
    Access, App, AppId, DataError, ImageCrop, ImageJob, NetworkState, Notification, Profile,
    ProfileId, RecentUse, Rect, Screen, Shortcut, Size, TimeStyle, WallpaperMarker,
};
use crate::storage::{MAX_STORED_BYTES, StorageAdapter, StoredFile, check_budget};

/// Every fake, and the [`Platform`] built from them.
#[derive(Clone, Default)]
pub struct Fakes {
    pub apps: Arc<FakeApps>,
    pub notifications: Arc<FakeNotifications>,
    pub usage: Arc<FakeUsage>,
    pub wallpaper: Arc<FakeWallpaper>,
    pub network: Arc<FakeNetwork>,
    pub scheduler: Arc<FakeScheduler>,
    pub system: Arc<FakeSystem>,
    pub storage: Arc<FakeStorage>,
}

impl Fakes {
    pub fn platform(&self) -> Platform {
        Platform {
            apps: self.apps.clone(),
            notifications: self.notifications.clone(),
            usage: self.usage.clone(),
            wallpaper: self.wallpaper.clone(),
            network: self.network.clone(),
            scheduler: self.scheduler.clone(),
            system: self.system.clone(),
            storage: self.storage.clone(),
        }
    }
}

/// The id of an app in the personal profile.
pub fn app_id(package: &str) -> AppId {
    AppId {
        profile: ProfileId(0),
        package: package.to_owned(),
        activity: format!("{package}.Main"),
    }
}

/// The work profile that [`work_app`] puts apps in.
pub const WORK_PROFILE: ProfileId = ProfileId(10);

/// An available app in the personal profile that the user can uninstall. Its sort key is its
/// lowercased label, a simple stand-in for the platform's collation key, and its search key is
/// the label's [`fold`] (L10N-04).
pub fn app(package: &str, label: &str) -> App {
    App {
        id: app_id(package),
        label: label.to_owned(),
        sort_key: label.to_lowercase().into_bytes(),
        search_key: fold(label),
        accent: accent::NEUTRAL,
        available: true,
        work: false,
        can_uninstall: true,
        installed_at_ms: 0,
    }
}

/// An available app in the work profile, [`WORK_PROFILE`] (APPS-20).
pub fn work_app(package: &str, label: &str) -> App {
    let personal = app(package, label);
    App {
        id: AppId {
            profile: WORK_PROFILE,
            ..personal.id
        },
        work: true,
        ..personal
    }
}

/// A use of `package` in the personal profile at `at_ms`, in its launcher activity as
/// [`app_id`] names it (REC-03).
pub fn used(package: &str, at_ms: u64) -> RecentUse {
    let id = app_id(package);
    RecentUse {
        profile: id.profile,
        package: id.package,
        activity: id.activity,
        last_used_ms: at_ms,
    }
}

/// A clearable notification from `package` in the personal profile, titled "Title" with the
/// text "Text", posted at 1 ms. The system names its app by its package (EVT-04).
pub fn notification(key: &str, package: &str) -> Notification {
    Notification {
        key: key.to_owned(),
        profile: ProfileId(0),
        package: package.to_owned(),
        app_label: package.to_owned(),
        title: "Title".to_owned(),
        text: "Text".to_owned(),
        posted_at_ms: 1,
        clearable: true,
        ongoing: false,
        summary: false,
        auto_cancel: false,
    }
}

/// A stand-in for the platform's search folding (SRCH-02, L10N-04): lowercase, the accents of
/// Latin-1 letters removed, and runs of spaces collapsed to one, without spaces at either end.
/// The platform folds every script; this is enough for tests.
pub fn fold(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'à'..='å' => folded.push('a'),
            'ç' => folded.push('c'),
            'è'..='ë' => folded.push('e'),
            'ì'..='ï' => folded.push('i'),
            'ñ' => folded.push('n'),
            'ò'..='ö' => folded.push('o'),
            'ù'..='ü' => folded.push('u'),
            'ý' | 'ÿ' => folded.push('y'),
            // Case folding, as the platform's, turns ß into ss.
            'ß' => folded.push_str("ss"),
            c => folded.push(c),
        }
    }
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The header of a baseline JPEG image of `size`, with no pixels: what an image's size is read
/// from, without anything to decode. The fake wallpaper's crops are these (ADR 0028).
///
/// # Panics
///
/// If a side is longer than a JPEG can be: 65 535 px.
pub fn jpeg(size: Size) -> Vec<u8> {
    let side = |pixels: u32| u16::try_from(pixels).expect("a JPEG side fits in 16 bits");
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
    bytes.extend(side(size.height).to_be_bytes());
    bytes.extend(side(size.width).to_be_bytes());
    // Three components: Y, Cb and Cr.
    bytes.extend([0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    bytes.extend([0xFF, 0xD9]);
    bytes
}

/// Fails with the error a fake was set to fail with, if any.
fn check(error: &Mutex<Option<DataError>>) -> Result<(), DataError> {
    error.lock().unwrap().clone().map_or(Ok(()), Err)
}

fn record<T>(log: &Mutex<Vec<T>>, item: T) {
    log.lock().unwrap().push(item);
}

pub struct FakeApps {
    pub apps: Mutex<Vec<App>>,
    /// The personal profile only, unless set.
    pub profiles: Mutex<Vec<Profile>>,
    /// The packages of the default phone, messaging, browser and camera apps (APPS-05). None
    /// unless set.
    pub default_apps: Mutex<Vec<String>>,
    pub shortcuts: Mutex<BTreeMap<AppId, Vec<Shortcut>>>,
    /// The system offers shortcuts only while MeeGone is the default home app (APPS-11).
    pub shortcuts_allowed: Mutex<bool>,
    pub launched: Mutex<Vec<AppId>>,
    /// Where each launch in `launched` grew from (ANIM-08).
    pub launched_from: Mutex<Vec<Option<Rect>>>,
    pub shortcuts_launched: Mutex<Vec<Shortcut>>,
    pub uninstall_requested: Mutex<Vec<AppId>>,
    pub app_info_opened: Mutex<Vec<AppId>>,
    /// Every text folded with `search_key`, such as each query typed (SRCH-02).
    pub search_keys: Mutex<Vec<String>>,
    pub error: Mutex<Option<DataError>>,
}

impl Default for FakeApps {
    fn default() -> Self {
        Self {
            apps: Mutex::default(),
            profiles: Mutex::new(vec![Profile {
                id: ProfileId(0),
                work: false,
                paused: false,
                locked: false,
            }]),
            default_apps: Mutex::default(),
            shortcuts: Mutex::default(),
            shortcuts_allowed: Mutex::default(),
            launched: Mutex::default(),
            launched_from: Mutex::default(),
            shortcuts_launched: Mutex::default(),
            uninstall_requested: Mutex::default(),
            app_info_opened: Mutex::default(),
            search_keys: Mutex::default(),
            error: Mutex::default(),
        }
    }
}

impl AppsAdapter for FakeApps {
    fn launchable_apps(&self) -> Result<Vec<App>, DataError> {
        check(&self.error)?;
        Ok(self.apps.lock().unwrap().clone())
    }

    fn profiles(&self) -> Result<Vec<Profile>, DataError> {
        check(&self.error)?;
        Ok(self.profiles.lock().unwrap().clone())
    }

    fn default_apps(&self) -> Vec<String> {
        self.default_apps.lock().unwrap().clone()
    }

    fn search_key(&self, text: &str) -> String {
        record(&self.search_keys, text.to_owned());
        fold(text)
    }

    fn launch(&self, app: &AppId, from: Option<Rect>) -> Result<(), DataError> {
        check(&self.error)?;
        record(&self.launched, app.clone());
        record(&self.launched_from, from);
        Ok(())
    }

    fn shortcuts(&self, app: &AppId) -> Result<Vec<Shortcut>, DataError> {
        check(&self.error)?;
        if !*self.shortcuts_allowed.lock().unwrap() {
            return Err(DataError::Unsupported(
                "Shortcuts need MeeGone as the default home app".into(),
            ));
        }
        Ok(self
            .shortcuts
            .lock()
            .unwrap()
            .get(app)
            .cloned()
            .unwrap_or_default())
    }

    fn launch_shortcut(&self, shortcut: &Shortcut) -> Result<(), DataError> {
        check(&self.error)?;
        record(&self.shortcuts_launched, shortcut.clone());
        Ok(())
    }

    fn request_uninstall(&self, app: &AppId) -> Result<(), DataError> {
        check(&self.error)?;
        let apps = self.apps.lock().unwrap();
        if apps.iter().any(|a| a.id == *app && !a.can_uninstall) {
            return Err(DataError::Unsupported(
                "This app can't be uninstalled".into(),
            ));
        }
        record(&self.uninstall_requested, app.clone());
        Ok(())
    }

    fn open_app_info(&self, app: &AppId) -> Result<(), DataError> {
        check(&self.error)?;
        record(&self.app_info_opened, app.clone());
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeNotifications {
    pub access: Mutex<Access>,
    pub active: Mutex<Vec<Notification>>,
    /// The key of every notification opened, in order.
    pub opened: Mutex<Vec<String>>,
    /// Where each opening in `opened` grew from (ANIM-08).
    pub opened_from: Mutex<Vec<Option<Rect>>>,
    /// Every key asked to be dismissed, in order, whether it was clearable or not.
    pub dismissed: Mutex<Vec<String>>,
    /// How many requests `dismissed` came in: Clear sends one for all (EVT-07).
    pub dismiss_requests: Mutex<u32>,
    pub access_screen_opened: Mutex<u32>,
    /// The app of each notification-settings screen opened (EVT-08).
    pub app_settings_opened: Mutex<Vec<(ProfileId, String)>>,
    pub error: Mutex<Option<DataError>>,
}

impl FakeNotifications {
    fn check_access(&self) -> Result<(), DataError> {
        check(&self.error)?;
        match self.access() {
            Access::Granted => Ok(()),
            Access::Missing => Err(DataError::AccessDenied),
        }
    }
}

impl NotificationsAdapter for FakeNotifications {
    fn access(&self) -> Access {
        *self.access.lock().unwrap()
    }

    fn active(&self) -> Result<Vec<Notification>, DataError> {
        self.check_access()?;
        Ok(self.active.lock().unwrap().clone())
    }

    /// Records the opening and removes nothing, as the system does.
    fn open(&self, key: &str, from: Option<Rect>) -> Result<(), DataError> {
        self.check_access()?;
        record(&self.opened, key.to_owned());
        record(&self.opened_from, from);
        Ok(())
    }

    /// Removes the clearable ones from `active`, as the system does.
    fn dismiss(&self, keys: &[String]) -> Result<(), DataError> {
        self.check_access()?;
        self.active
            .lock()
            .unwrap()
            .retain(|n| !keys.contains(&n.key) || !n.clearable);
        self.dismissed.lock().unwrap().extend_from_slice(keys);
        *self.dismiss_requests.lock().unwrap() += 1;
        Ok(())
    }

    fn open_access_settings(&self) -> Result<(), DataError> {
        check(&self.error)?;
        *self.access_screen_opened.lock().unwrap() += 1;
        Ok(())
    }

    fn open_app_settings(&self, profile: ProfileId, package: &str) -> Result<(), DataError> {
        check(&self.error)?;
        record(&self.app_settings_opened, (profile, package.to_owned()));
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeUsage {
    pub access: Mutex<Access>,
    pub uses: Mutex<Vec<RecentUse>>,
    pub access_screen_opened: Mutex<u32>,
    pub error: Mutex<Option<DataError>>,
}

impl UsageAdapter for FakeUsage {
    fn access(&self) -> Access {
        *self.access.lock().unwrap()
    }

    fn used_since(&self, since_ms: u64) -> Result<Vec<RecentUse>, DataError> {
        check(&self.error)?;
        if self.access() == Access::Missing {
            return Err(DataError::AccessDenied);
        }
        let uses = self.uses.lock().unwrap();
        Ok(uses
            .iter()
            .filter(|used| used.last_used_ms >= since_ms)
            .cloned()
            .collect())
    }

    fn open_access_settings(&self) -> Result<(), DataError> {
        check(&self.error)?;
        *self.access_screen_opened.lock().unwrap() += 1;
        Ok(())
    }
}

pub struct FakeWallpaper {
    /// When set, the lock-screen wallpaper can't be set, for this reason (WALL-13).
    pub unsupported: Mutex<Option<String>>,
    /// A typical phone screen, unless set.
    pub screen: Mutex<Screen>,
    /// Whether the device folds: false unless set.
    pub folds: Mutex<bool>,
    /// The image last set as the lock-screen wallpaper; `None` after a reset.
    pub lock_screen: Mutex<Option<Vec<u8>>>,
    pub lock_screen_images_set: Mutex<u32>,
    /// The marker of the lock screen's wallpaper now. It starts as the home wallpaper's, which
    /// the lock screen shows until a wallpaper of its own is set (WALL-14).
    pub lock_screen_marker: Mutex<WallpaperMarker>,
    /// The marker the next wallpaper set gets; each one set takes the next.
    pub next_marker: Mutex<i32>,
    /// Whether the platform can tell wallpapers apart: true unless set (WALL-14).
    pub tracks: Mutex<bool>,
    /// Whether the system keeps the image as the lock screen's wallpaper: true unless set. When
    /// false, the lock screen keeps the wallpaper it had, as a device that overrides it may
    /// (SRS C5).
    pub keeps: Mutex<bool>,
    /// When set, setting a wallpaper fails with it, as when the system refuses the image.
    pub refuses: Mutex<Option<DataError>>,
    pub resets: Mutex<u32>,
    pub settings_opened: Mutex<u32>,
    /// Every crop asked for, in order.
    pub crops: Mutex<Vec<ImageCrop>>,
    /// When set, every crop returns these bytes. Otherwise a crop is the [`jpeg`] header of
    /// its size.
    pub cropped: Mutex<Option<Vec<u8>>>,
    /// When set, every call that can fail fails, cropping included: an image that doesn't
    /// decode is `Invalid`.
    pub error: Mutex<Option<DataError>>,
}

/// The marker of the home wallpaper, which the lock screen shows until it has its own.
pub const HOME_WALLPAPER: WallpaperMarker = WallpaperMarker(-1);

/// A phone screen 1080 × 2400 pixels at 420 dpi: about 411 × 914 dp.
pub const PHONE_SCREEN: Screen = Screen {
    size: Size {
        width: 1080,
        height: 2400,
    },
    density_dpi: 420,
};

impl Default for FakeWallpaper {
    fn default() -> Self {
        Self {
            unsupported: Mutex::default(),
            screen: Mutex::new(PHONE_SCREEN),
            folds: Mutex::default(),
            lock_screen: Mutex::default(),
            lock_screen_images_set: Mutex::default(),
            lock_screen_marker: Mutex::new(HOME_WALLPAPER),
            next_marker: Mutex::new(100),
            tracks: Mutex::new(true),
            keeps: Mutex::new(true),
            refuses: Mutex::default(),
            resets: Mutex::default(),
            settings_opened: Mutex::default(),
            crops: Mutex::default(),
            cropped: Mutex::default(),
            error: Mutex::default(),
        }
    }
}

impl FakeWallpaper {
    /// Someone other than MeeGone sets a lock-screen wallpaper of their own (WALL-14).
    pub fn changed_by_someone_else(&self) {
        let marker = self.take_marker();
        *self.lock_screen.lock().unwrap() = Some(b"someone else's".to_vec());
        *self.lock_screen_marker.lock().unwrap() = marker;
    }

    fn take_marker(&self) -> WallpaperMarker {
        let mut next = self.next_marker.lock().unwrap();
        *next += 1;
        WallpaperMarker(*next - 1)
    }
}

impl WallpaperAdapter for FakeWallpaper {
    fn lock_screen_support(&self) -> Result<(), DataError> {
        check(&self.error)?;
        match self.unsupported.lock().unwrap().clone() {
            Some(reason) => Err(DataError::Unsupported(reason)),
            None => Ok(()),
        }
    }

    fn screen(&self) -> Screen {
        *self.screen.lock().unwrap()
    }

    fn folds(&self) -> bool {
        *self.folds.lock().unwrap()
    }

    fn crop_image(&self, _jpeg: &[u8], crop: &ImageCrop) -> Result<Vec<u8>, DataError> {
        record(&self.crops, *crop);
        check(&self.error)?;
        Ok(self
            .cropped
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| jpeg(crop.size)))
    }

    fn set_lock_screen(&self, image: &[u8]) -> Result<Option<WallpaperMarker>, DataError> {
        self.lock_screen_support()?;
        check(&self.refuses)?;
        *self.lock_screen_images_set.lock().unwrap() += 1;
        let marker = self.take_marker();
        if *self.keeps.lock().unwrap() {
            *self.lock_screen.lock().unwrap() = Some(image.to_vec());
            *self.lock_screen_marker.lock().unwrap() = marker;
        }
        Ok(self.tracks.lock().unwrap().then_some(marker))
    }

    fn lock_screen_marker(&self) -> Option<WallpaperMarker> {
        self.tracks
            .lock()
            .unwrap()
            .then(|| *self.lock_screen_marker.lock().unwrap())
    }

    fn reset_lock_screen(&self) -> Result<(), DataError> {
        self.lock_screen_support()?;
        *self.lock_screen.lock().unwrap() = None;
        *self.lock_screen_marker.lock().unwrap() = HOME_WALLPAPER;
        *self.resets.lock().unwrap() += 1;
        Ok(())
    }

    fn open_wallpaper_settings(&self) -> Result<(), DataError> {
        check(&self.error)?;
        *self.settings_opened.lock().unwrap() += 1;
        Ok(())
    }
}

/// Offline until `state` is set. Online, it serves `responses` and records every request.
#[derive(Default)]
pub struct FakeNetwork {
    pub state: Mutex<NetworkState>,
    pub responses: Mutex<BTreeMap<String, Vec<u8>>>,
    /// Every URL asked for, including failed attempts, to check call budgets (REL-04).
    pub requests: Mutex<Vec<String>>,
    /// Whether Data Saver is on: false unless set (PWR-04).
    pub saves_data: Mutex<bool>,
    pub error: Mutex<Option<DataError>>,
}

impl NetworkAdapter for FakeNetwork {
    fn state(&self) -> NetworkState {
        *self.state.lock().unwrap()
    }

    fn saves_data(&self) -> bool {
        *self.saves_data.lock().unwrap()
    }

    fn get(&self, https_url: &str, max_bytes: usize) -> Result<Vec<u8>, DataError> {
        record(&self.requests, https_url.to_owned());
        check(&self.error)?;
        if !https_url.starts_with("https://") {
            return Err(DataError::Invalid("Only HTTPS is allowed".into()));
        }
        if self.state() == NetworkState::Offline {
            return Err(DataError::Offline);
        }
        match self.responses.lock().unwrap().get(https_url) {
            Some(body) if body.len() > max_bytes => Err(DataError::Invalid("Too large".into())),
            Some(body) => Ok(body.clone()),
            None => Err(DataError::Failed("Not found".into())),
        }
    }
}

/// Set `error` to `Unsupported` to model a device without background scheduling.
#[derive(Default)]
pub struct FakeScheduler {
    /// The job waiting to run, if any (ARCH-07: there is only one).
    pub job: Mutex<Option<ImageJob>>,
    /// How many times an update was asked to run at once (WALL-11). The test runs it, with
    /// `Shared::update_image_now`, as the job's thread would.
    pub updates_now: Mutex<u32>,
    pub error: Mutex<Option<DataError>>,
}

impl SchedulerAdapter for FakeScheduler {
    fn schedule_image_job(&self, job: ImageJob) -> Result<(), DataError> {
        check(&self.error)?;
        *self.job.lock().unwrap() = Some(job);
        Ok(())
    }

    fn cancel_image_job(&self) -> Result<(), DataError> {
        check(&self.error)?;
        *self.job.lock().unwrap() = None;
        Ok(())
    }

    /// It runs on its own thread, without background scheduling, so `error` doesn't stop it.
    fn update_image_now(&self) -> Result<(), DataError> {
        *self.updates_now.lock().unwrap() += 1;
        Ok(())
    }
}

pub struct FakeSystem {
    pub dark: Mutex<bool>,
    /// 1.0 unless set; 0.0 means animations are removed.
    pub animation_scale: Mutex<f64>,
    /// False unless set, so MeeGone plays its own return motion (ANIM-09).
    pub animates_return_home: Mutex<bool>,
    pub now_ms: Mutex<u64>,
    /// When this copy of MeeGone was installed: 0 unless set, the same on every start. A test
    /// that restores a backup on "another device" gives that device another time (APPS-21).
    pub installed_ms: Mutex<u64>,
    /// UTC unless set.
    pub utc_offset_ms: Mutex<i64>,
    /// `en-US` unless set.
    pub locale: Mutex<String>,
    /// 1 GB unless set; `None` when the system can't say.
    pub free_storage_bytes: Mutex<Option<u64>>,
    /// Whether battery saver is on or the battery is low: false unless set (WALL-04).
    pub saves_battery: Mutex<bool>,
    /// The device's 24-hour setting: false unless set, as in `en-US` (L10N-03).
    pub hour_24: Mutex<bool>,
    pub default_home: Mutex<bool>,
    pub default_home_requested: Mutex<u32>,
    pub next_alarm_ms: Mutex<Option<u64>>,
    pub alarm_opened: Mutex<u32>,
    pub haptic_ticks: Mutex<u32>,
    /// True unless set.
    pub browser: Mutex<bool>,
    pub opened_urls: Mutex<Vec<String>>,
    pub web_searches: Mutex<Vec<String>>,
    pub error: Mutex<Option<DataError>>,
}

impl Default for FakeSystem {
    fn default() -> Self {
        Self {
            dark: Mutex::default(),
            animation_scale: Mutex::new(1.0),
            animates_return_home: Mutex::default(),
            now_ms: Mutex::default(),
            installed_ms: Mutex::default(),
            utc_offset_ms: Mutex::default(),
            locale: Mutex::new("en-US".into()),
            free_storage_bytes: Mutex::new(Some(1_000_000_000)),
            saves_battery: Mutex::default(),
            hour_24: Mutex::default(),
            default_home: Mutex::default(),
            default_home_requested: Mutex::default(),
            next_alarm_ms: Mutex::default(),
            alarm_opened: Mutex::default(),
            haptic_ticks: Mutex::default(),
            browser: Mutex::new(true),
            opened_urls: Mutex::default(),
            web_searches: Mutex::default(),
            error: Mutex::default(),
        }
    }
}

impl FakeSystem {
    fn check_browser(&self) -> Result<(), DataError> {
        check(&self.error)?;
        if self.has_browser() {
            Ok(())
        } else {
            Err(DataError::Unsupported("No web browser is installed".into()))
        }
    }
}

impl SystemConfigAdapter for FakeSystem {
    fn is_dark(&self) -> bool {
        *self.dark.lock().unwrap()
    }

    fn animation_scale(&self) -> f64 {
        *self.animation_scale.lock().unwrap()
    }

    fn animates_return_home(&self) -> bool {
        *self.animates_return_home.lock().unwrap()
    }

    fn now_ms(&self) -> u64 {
        *self.now_ms.lock().unwrap()
    }

    fn installed_ms(&self) -> u64 {
        *self.installed_ms.lock().unwrap()
    }

    fn utc_offset_ms(&self, _at_ms: u64) -> i64 {
        *self.utc_offset_ms.lock().unwrap()
    }

    fn locale(&self) -> String {
        self.locale.lock().unwrap().clone()
    }

    fn free_storage_bytes(&self) -> Option<u64> {
        *self.free_storage_bytes.lock().unwrap()
    }

    fn saves_battery(&self) -> bool {
        *self.saves_battery.lock().unwrap()
    }

    /// English in every locale, day before month as in the SRS's layout mock: "Tuesday",
    /// "17 November", "07:00" or "7:00 AM", "Wed 07:00", "17 November, 07:00". Local time is
    /// `utc_offset_ms` from UTC.
    fn format_time(&self, at_ms: u64, style: TimeStyle) -> String {
        let local = i64::try_from(at_ms)
            .unwrap_or(i64::MAX)
            .saturating_add(self.utc_offset_ms(at_ms));
        let days = local.div_euclid(DAY_MS);
        let minutes = local.rem_euclid(DAY_MS) / 60_000;
        let (hour, minute) = (minutes / 60, minutes % 60);
        // 1 January 1970 was a Thursday.
        let weekday = WEEKDAYS[(days + 4).rem_euclid(7) as usize];
        let time = if *self.hour_24.lock().unwrap() {
            format!("{hour:02}:{minute:02}")
        } else {
            let half = if hour < 12 { "AM" } else { "PM" };
            let hour = if hour % 12 == 0 { 12 } else { hour % 12 };
            format!("{hour}:{minute:02} {half}")
        };
        let day_month = || {
            let (month, day) = month_and_day(days);
            format!("{day} {}", MONTHS[month - 1])
        };
        match style {
            TimeStyle::Weekday => weekday.to_owned(),
            TimeStyle::DayMonth => day_month(),
            TimeStyle::Time => time,
            TimeStyle::WeekdayTime => format!("{} {time}", &weekday[..3]),
            TimeStyle::DayMonthTime => format!("{}, {time}", day_month()),
        }
    }

    fn is_default_home(&self) -> bool {
        *self.default_home.lock().unwrap()
    }

    fn request_default_home(&self) -> Result<(), DataError> {
        check(&self.error)?;
        *self.default_home_requested.lock().unwrap() += 1;
        Ok(())
    }

    fn next_alarm_ms(&self) -> Option<u64> {
        *self.next_alarm_ms.lock().unwrap()
    }

    fn open_next_alarm(&self) -> Result<(), DataError> {
        check(&self.error)?;
        if self.next_alarm_ms().is_none() {
            return Err(DataError::Unsupported("No alarm is set".into()));
        }
        *self.alarm_opened.lock().unwrap() += 1;
        Ok(())
    }

    fn haptic_tick(&self) {
        *self.haptic_ticks.lock().unwrap() += 1;
    }

    fn has_browser(&self) -> bool {
        *self.browser.lock().unwrap()
    }

    fn open_in_browser(&self, https_url: &str) -> Result<(), DataError> {
        self.check_browser()?;
        if !https_url.starts_with("https://") {
            return Err(DataError::Invalid("Only HTTPS links open".into()));
        }
        record(&self.opened_urls, https_url.to_owned());
        Ok(())
    }

    fn web_search(&self, query: &str) -> Result<(), DataError> {
        self.check_browser()?;
        record(&self.web_searches, query.to_owned());
        Ok(())
    }
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The month (1 to 12) and day of the month of the day `days` after 1 January 1970, in the
/// proleptic Gregorian calendar (Howard Hinnant's `civil_from_days`).
fn month_and_day(days: i64) -> (usize, i64) {
    let z = days + 719_468;
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (month as usize, day)
}

/// MeeGone's files, kept in memory. Writes are atomic and stay within the same budget as
/// `FileStorage` (PERF-08).
#[derive(Default)]
pub struct FakeStorage {
    pub files: Mutex<BTreeMap<StoredFile, Vec<u8>>>,
    /// Every file read, in order.
    pub reads: Mutex<Vec<StoredFile>>,
    /// Every file written, in order.
    pub writes: Mutex<Vec<StoredFile>>,
    pub error: Mutex<Option<DataError>>,
}

impl FakeStorage {
    /// Every stored byte, for checking what was and wasn't stored (PRIV-03).
    pub fn stored_text(&self) -> String {
        let files = self.files.lock().unwrap();
        files
            .values()
            .map(|bytes| String::from_utf8_lossy(bytes))
            .collect()
    }
}

impl StorageAdapter for FakeStorage {
    fn read(&self, file: StoredFile) -> Result<Option<Vec<u8>>, DataError> {
        record(&self.reads, file);
        check(&self.error)?;
        Ok(self.files.lock().unwrap().get(&file).cloned())
    }

    fn write(&self, file: StoredFile, bytes: &[u8]) -> Result<(), DataError> {
        check(&self.error)?;
        let mut files = self.files.lock().unwrap();
        let others = files
            .iter()
            .filter(|(stored, _)| **stored != file)
            .map(|(_, bytes)| bytes.len() as u64)
            .sum();
        check_budget(others, bytes.len(), MAX_STORED_BYTES)?;
        files.insert(file, bytes.to_vec());
        record(&self.writes, file);
        Ok(())
    }

    fn remove(&self, file: StoredFile) -> Result<(), DataError> {
        check(&self.error)?;
        self.files.lock().unwrap().remove(&file);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_folding_ignores_case_accents_and_extra_spaces() {
        // SRCH-02, L10N-04, as the platform's folding does.
        assert_eq!(fold("  Café  Crème "), "cafe creme");
        assert_eq!(fold("STRASSE Straße"), "strasse strasse");
        assert_eq!(app("cafe", "Café").search_key, "cafe");
        assert_eq!(FakeApps::default().search_key("CAFÉ"), "cafe");
    }

    #[test]
    fn the_fake_clock_writes_english_in_the_local_time_zone() {
        // L10N-03, as the platform writes it. 17 November 2026, 07:05 UTC, was a Tuesday.
        let at = 1_794_873_600_000 + 7 * 3_600_000 + 5 * 60_000;
        let system = FakeSystem::default();
        assert_eq!(system.format_time(at, TimeStyle::Weekday), "Tuesday");
        assert_eq!(system.format_time(at, TimeStyle::DayMonth), "17 November");
        assert_eq!(system.format_time(at, TimeStyle::Time), "7:05 AM");
        assert_eq!(
            system.format_time(at, TimeStyle::WeekdayTime),
            "Tue 7:05 AM"
        );
        assert_eq!(
            system.format_time(at, TimeStyle::DayMonthTime),
            "17 November, 7:05 AM"
        );

        *system.hour_24.lock().unwrap() = true;
        assert_eq!(system.format_time(at, TimeStyle::Time), "07:05");
        *system.utc_offset_ms.lock().unwrap() = -8 * 3_600_000;
        assert_eq!(system.format_time(at, TimeStyle::WeekdayTime), "Mon 23:05");
        assert_eq!(system.format_time(at, TimeStyle::DayMonth), "16 November");
        assert_eq!(
            system.format_time(at, TimeStyle::DayMonthTime),
            "16 November, 23:05"
        );

        // Leap days and the turn of the year.
        assert_eq!(month_and_day(0), (1, 1));
        assert_eq!(month_and_day(11_016), (2, 29)); // 2000-02-29
        assert_eq!(month_and_day(20_818), (12, 31)); // 2026-12-31
    }
}
