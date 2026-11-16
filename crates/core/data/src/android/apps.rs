//! [`AppsAdapter`] on Android: the app catalogue and launching (APPS-01, APPS-09, APPS-10,
//! ANIM-08, ADR 0010).
//!
//! Questions go to the Java `Bridge` object, whose method names and signatures must stay in
//! step with the ones looked up here. The catalogue arrives as flat records, parsed in
//! [`crate::catalogue`]. Shortcuts arrive with #12; uninstalling and App info with #13.

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use super::not_yet;
use crate::adapters::AppsAdapter;
use crate::catalogue;
use crate::model::{App, AppId, DataError, Profile, Rect, Shortcut};

pub struct AndroidApps {
    vm: Vm,
    bridge: GlobalRef,
    /// `String[] launchableApps()`: records of [`catalogue::APP_FIELDS`] fields.
    launchable_apps: MethodId,
    /// `String[] profiles()`: records of [`catalogue::PROFILE_FIELDS`] fields.
    profiles: MethodId,
    /// `String launch(String package, String activity, long profile, int left, int top,
    /// int right, int bottom)`: `null`, or a short message when the system couldn't start it.
    launch: MethodId,
}

impl AndroidApps {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            launchable_apps: env.method_id(bridge, c"launchableApps", c"()[Ljava/lang/String;")?,
            profiles: env.method_id(bridge, c"profiles", c"()[Ljava/lang/String;")?,
            launch: env.method_id(
                bridge,
                c"launch",
                c"(Ljava/lang/String;Ljava/lang/String;JIIII)Ljava/lang/String;",
            )?,
            bridge: env.global_ref(bridge)?,
        })
    }
}

impl AppsAdapter for AndroidApps {
    /// Fails when the system couldn't list the apps, so the saved layout keeps them (REL-02).
    fn launchable_apps(&self) -> Result<Vec<App>, DataError> {
        let env = self.vm.env()?;
        match env.call_strings(&self.bridge, self.launchable_apps, &[])? {
            Some(fields) => Ok(catalogue::apps(&fields)),
            None => Err(unlisted()),
        }
    }

    fn profiles(&self) -> Result<Vec<Profile>, DataError> {
        let env = self.vm.env()?;
        match env.call_strings(&self.bridge, self.profiles, &[])? {
            Some(fields) => Ok(catalogue::profiles(&fields)),
            None => Err(unlisted()),
        }
    }

    /// An empty rect, or none, launches with the system's default transition.
    fn launch(&self, app: &AppId, from: Option<Rect>) -> Result<(), DataError> {
        let env = self.vm.env()?;
        let package = env.new_string(&app.package)?;
        let activity = env.new_string(&app.activity)?;
        let from = from.filter(|rect| !rect.is_empty()).unwrap_or_default();
        let args = [
            JValue::object(&package),
            JValue::object(&activity),
            JValue::long(i64::from(app.profile.0)),
            JValue::int(from.left),
            JValue::int(from.top),
            JValue::int(from.right),
            JValue::int(from.bottom),
        ];
        match env.call_string(&self.bridge, self.launch, &args)? {
            None => Ok(()),
            Some(message) => Err(DataError::Failed(message)),
        }
    }

    fn shortcuts(&self, _app: &AppId) -> Result<Vec<Shortcut>, DataError> {
        Err(not_yet()) // #12
    }

    fn launch_shortcut(&self, _shortcut: &Shortcut) -> Result<(), DataError> {
        Err(not_yet()) // #12
    }

    fn request_uninstall(&self, _app: &AppId) -> Result<(), DataError> {
        Err(not_yet()) // #13
    }

    fn open_app_info(&self, _app: &AppId) -> Result<(), DataError> {
        Err(not_yet()) // #12
    }
}

/// The bridge returns `null` when the system couldn't answer.
fn unlisted() -> DataError {
    DataError::Failed("The app list isn't available".into())
}
