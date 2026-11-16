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
    Access, App, AppId, DataError, ImageJob, NetworkState, Notification, Profile, ProfileId,
    RecentUse, Rect, Shortcut, Size,
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
    pub opened: Mutex<Vec<String>>,
    pub dismissed: Mutex<Vec<String>>,
    pub access_screen_opened: Mutex<u32>,
    pub app_settings_opened: Mutex<Vec<String>>,
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

    fn open(&self, key: &str) -> Result<(), DataError> {
        self.check_access()?;
        record(&self.opened, key.to_owned());
        Ok(())
    }

    /// Removes the notification from `active` if it is clearable, as the system does.
    fn dismiss(&self, key: &str) -> Result<(), DataError> {
        self.check_access()?;
        self.active
            .lock()
            .unwrap()
            .retain(|n| n.key != key || !n.clearable);
        record(&self.dismissed, key.to_owned());
        Ok(())
    }

    fn open_access_settings(&self) -> Result<(), DataError> {
        check(&self.error)?;
        *self.access_screen_opened.lock().unwrap() += 1;
        Ok(())
    }

    fn open_app_settings(&self, package: &str) -> Result<(), DataError> {
        check(&self.error)?;
        record(&self.app_settings_opened, package.to_owned());
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
    pub lock_screen_size: Mutex<Size>,
    /// The image last set as the lock-screen wallpaper; `None` after a reset.
    pub lock_screen: Mutex<Option<Vec<u8>>>,
    pub lock_screen_images_set: Mutex<u32>,
    pub settings_opened: Mutex<u32>,
    pub error: Mutex<Option<DataError>>,
}

impl Default for FakeWallpaper {
    fn default() -> Self {
        Self {
            unsupported: Mutex::default(),
            lock_screen_size: Mutex::new(Size {
                width: 1080,
                height: 2400,
            }),
            lock_screen: Mutex::default(),
            lock_screen_images_set: Mutex::default(),
            settings_opened: Mutex::default(),
            error: Mutex::default(),
        }
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

    fn lock_screen_size(&self) -> Size {
        *self.lock_screen_size.lock().unwrap()
    }

    fn set_lock_screen(&self, image: &[u8]) -> Result<(), DataError> {
        self.lock_screen_support()?;
        *self.lock_screen.lock().unwrap() = Some(image.to_vec());
        *self.lock_screen_images_set.lock().unwrap() += 1;
        Ok(())
    }

    fn reset_lock_screen(&self) -> Result<(), DataError> {
        self.lock_screen_support()?;
        *self.lock_screen.lock().unwrap() = None;
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
    pub error: Mutex<Option<DataError>>,
}

impl NetworkAdapter for FakeNetwork {
    fn state(&self) -> NetworkState {
        *self.state.lock().unwrap()
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
}
