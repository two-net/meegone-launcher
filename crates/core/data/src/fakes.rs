//! Configurable fakes of every platform adapter, for tests (MAINT-01).
//!
//! Enable the `fakes` feature in `[dev-dependencies]` only. Each fake's fields are public:
//! set them to shape what the "system" reports, and read them to see what was asked of it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::adapters::{
    AppsAdapter, NetworkAdapter, NotificationsAdapter, Platform, SchedulerAdapter,
    SystemConfigAdapter, UsageAdapter, WallpaperAdapter,
};
use crate::model::{Access, App, AppId, DataError, Notification, ProfileId, RecentUse, Rgb};

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

/// An available app in the personal profile.
pub fn app(package: &str, label: &str) -> App {
    App {
        id: app_id(package),
        label: label.to_owned(),
        accent: Rgb::hex(0x808080),
        available: true,
        installed_at_ms: 0,
    }
}

#[derive(Default)]
pub struct FakeApps {
    pub apps: Mutex<Vec<App>>,
    pub launched: Mutex<Vec<AppId>>,
    /// When set, every call fails with this error.
    pub error: Mutex<Option<DataError>>,
}

impl FakeApps {
    fn check(&self) -> Result<(), DataError> {
        self.error.lock().unwrap().clone().map_or(Ok(()), Err)
    }
}

impl AppsAdapter for FakeApps {
    fn launchable_apps(&self) -> Result<Vec<App>, DataError> {
        self.check()?;
        Ok(self.apps.lock().unwrap().clone())
    }

    fn launch(&self, app: &AppId) -> Result<(), DataError> {
        self.check()?;
        self.launched.lock().unwrap().push(app.clone());
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeNotifications {
    pub access: Mutex<Access>,
    pub active: Mutex<Vec<Notification>>,
    pub access_screen_opened: Mutex<u32>,
}

impl NotificationsAdapter for FakeNotifications {
    fn access(&self) -> Access {
        *self.access.lock().unwrap()
    }

    fn active(&self) -> Result<Vec<Notification>, DataError> {
        match self.access() {
            Access::Granted => Ok(self.active.lock().unwrap().clone()),
            Access::Missing => Err(DataError::AccessDenied),
        }
    }

    fn open_access_settings(&self) -> Result<(), DataError> {
        *self.access_screen_opened.lock().unwrap() += 1;
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeUsage {
    pub access: Mutex<Access>,
    pub uses: Mutex<Vec<RecentUse>>,
    pub access_screen_opened: Mutex<u32>,
}

impl UsageAdapter for FakeUsage {
    fn access(&self) -> Access {
        *self.access.lock().unwrap()
    }

    fn used_since(&self, since_ms: u64) -> Result<Vec<RecentUse>, DataError> {
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
        *self.access_screen_opened.lock().unwrap() += 1;
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeWallpaper {
    /// When set, the lock-screen wallpaper can't be set, for this reason.
    pub unsupported: Mutex<Option<String>>,
    pub lock_screen_images_set: Mutex<u32>,
}

impl WallpaperAdapter for FakeWallpaper {
    fn lock_screen_support(&self) -> Result<(), DataError> {
        match self.unsupported.lock().unwrap().clone() {
            Some(reason) => Err(DataError::Unsupported(reason)),
            None => Ok(()),
        }
    }

    fn set_lock_screen(&self, _image: &[u8]) -> Result<(), DataError> {
        self.lock_screen_support()?;
        *self.lock_screen_images_set.lock().unwrap() += 1;
        Ok(())
    }
}

/// Offline unless a response is added for a URL.
#[derive(Default)]
pub struct FakeNetwork {
    pub unmetered: Mutex<bool>,
    pub responses: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl NetworkAdapter for FakeNetwork {
    fn is_unmetered(&self) -> bool {
        *self.unmetered.lock().unwrap()
    }

    fn get(&self, https_url: &str, max_bytes: usize) -> Result<Vec<u8>, DataError> {
        if !https_url.starts_with("https://") {
            return Err(DataError::Failed("Only HTTPS is allowed".into()));
        }
        match self.responses.lock().unwrap().get(https_url) {
            Some(body) if body.len() > max_bytes => Err(DataError::Failed("Too large".into())),
            Some(body) => Ok(body.clone()),
            None => Err(DataError::Failed("No connection".into())),
        }
    }
}

#[derive(Default)]
pub struct FakeScheduler {
    pub daily_job_scheduled: Mutex<bool>,
}

impl SchedulerAdapter for FakeScheduler {
    fn ensure_daily_job(&self) -> Result<(), DataError> {
        *self.daily_job_scheduled.lock().unwrap() = true;
        Ok(())
    }

    fn cancel_daily_job(&self) -> Result<(), DataError> {
        *self.daily_job_scheduled.lock().unwrap() = false;
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeSystem {
    pub dark: Mutex<bool>,
    pub default_home: Mutex<bool>,
    pub now_ms: Mutex<u64>,
}

impl SystemConfigAdapter for FakeSystem {
    fn is_dark(&self) -> bool {
        *self.dark.lock().unwrap()
    }

    fn is_default_home(&self) -> bool {
        *self.default_home.lock().unwrap()
    }

    fn now_ms(&self) -> u64 {
        *self.now_ms.lock().unwrap()
    }
}
