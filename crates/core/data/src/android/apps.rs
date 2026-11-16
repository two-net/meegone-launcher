//! [`AppsAdapter`] on Android: the app catalogue and launching (APPS-01, APPS-09, APPS-10,
//! ANIM-08, ADR 0010), the default apps the first grid starts with (APPS-05, ADR 0012), and
//! what the app long-press menu offers: shortcuts, App info and uninstall requests (APPS-11,
//! APPS-13, ADR 0013).
//!
//! Questions go to the Java `Bridge` object, whose method names and signatures must stay in
//! step with the ones looked up here. The catalogue and the shortcuts arrive as flat records,
//! parsed in [`crate::catalogue`].

use super::jni::{Env, GlobalRef, JValue, Local, LocalRef, MethodId, Vm};
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
    /// `String[] defaultApps()`: the default phone, messaging, browser and camera packages, in
    /// that order, leaving out those the system doesn't identify.
    default_apps: MethodId,
    /// `String launch(String package, String activity, long profile, int left, int top,
    /// int right, int bottom)`: `null`, or a short message when the system couldn't start it.
    launch: MethodId,
    /// `String[] shortcuts(String package, String activity, long profile)`: records of
    /// [`catalogue::SHORTCUT_FIELDS`] fields, or `null` while MeeGone isn't the default home
    /// app or the system can't say.
    shortcuts: MethodId,
    /// `String launchShortcut(String package, String id, long profile)`: `null`, or a short
    /// message.
    launch_shortcut: MethodId,
    /// `String requestUninstall(String package, String activity, long profile)`: opens the
    /// system's uninstall confirmation. `null`, or a short message.
    request_uninstall: MethodId,
    /// `String openAppInfo(String package, String activity, long profile)`: `null`, or a short
    /// message.
    open_app_info: MethodId,
}

impl AndroidApps {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            launchable_apps: env.method_id(bridge, c"launchableApps", c"()[Ljava/lang/String;")?,
            profiles: env.method_id(bridge, c"profiles", c"()[Ljava/lang/String;")?,
            default_apps: env.method_id(bridge, c"defaultApps", c"()[Ljava/lang/String;")?,
            launch: env.method_id(
                bridge,
                c"launch",
                c"(Ljava/lang/String;Ljava/lang/String;JIIII)Ljava/lang/String;",
            )?,
            shortcuts: env.method_id(
                bridge,
                c"shortcuts",
                c"(Ljava/lang/String;Ljava/lang/String;J)[Ljava/lang/String;",
            )?,
            launch_shortcut: env.method_id(bridge, c"launchShortcut", APP_REQUEST)?,
            request_uninstall: env.method_id(bridge, c"requestUninstall", APP_REQUEST)?,
            open_app_info: env.method_id(bridge, c"openAppInfo", APP_REQUEST)?,
            bridge: env.global_ref(bridge)?,
        })
    }
}

/// The signature of a request about one app: two strings and a profile serial number, answered
/// with `null` or a short message.
const APP_REQUEST: &std::ffi::CStr = c"(Ljava/lang/String;Ljava/lang/String;J)Ljava/lang/String;";

impl AndroidApps {
    /// Calls a bridge method of [`APP_REQUEST`]'s signature. A message back means the system
    /// refused.
    fn request(
        &self,
        method: MethodId,
        first: &str,
        second: &str,
        profile: u32,
    ) -> Result<(), DataError> {
        let env = self.vm.env()?;
        let (first, second) = (env.new_string(first)?, env.new_string(second)?);
        let args = app_args(&first, &second, profile);
        match env.call_string(&self.bridge, method, &args)? {
            None => Ok(()),
            Some(message) => Err(DataError::Failed(message)),
        }
    }
}

fn app_args(first: &Local, second: &Local, profile: u32) -> [JValue; 3] {
    [
        JValue::object(first),
        JValue::object(second),
        JValue::long(i64::from(profile)),
    ]
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

    /// None if the system couldn't say: the grid then starts alphabetically (APPS-04).
    fn default_apps(&self) -> Vec<String> {
        let packages = self
            .vm
            .env()
            .and_then(|env| env.call_strings(&self.bridge, self.default_apps, &[]));
        match packages {
            Ok(Some(packages)) => packages
                .into_iter()
                .flatten()
                .filter(|package| !package.is_empty())
                .collect(),
            Ok(None) | Err(_) => Vec::new(),
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

    /// `Unsupported` while MeeGone isn't the default home app: the system offers shortcuts only
    /// to it, and the menu then shows none (APPS-11).
    fn shortcuts(&self, app: &AppId) -> Result<Vec<Shortcut>, DataError> {
        let env = self.vm.env()?;
        let (package, activity) = (
            env.new_string(&app.package)?,
            env.new_string(&app.activity)?,
        );
        let args = app_args(&package, &activity, app.profile.0);
        match env.call_strings(&self.bridge, self.shortcuts, &args)? {
            Some(fields) => Ok(catalogue::shortcuts(app, &fields)),
            None => Err(DataError::Unsupported(
                "Shortcuts need MeeGone as the default home app".into(),
            )),
        }
    }

    fn launch_shortcut(&self, shortcut: &Shortcut) -> Result<(), DataError> {
        self.request(
            self.launch_shortcut,
            &shortcut.app.package,
            &shortcut.id,
            shortcut.app.profile.0,
        )
    }

    /// The system asks the user to confirm; MeeGone never uninstalls anything itself (APPS-13).
    fn request_uninstall(&self, app: &AppId) -> Result<(), DataError> {
        self.request(
            self.request_uninstall,
            &app.package,
            &app.activity,
            app.profile.0,
        )
    }

    fn open_app_info(&self, app: &AppId) -> Result<(), DataError> {
        self.request(
            self.open_app_info,
            &app.package,
            &app.activity,
            app.profile.0,
        )
    }
}

/// The bridge returns `null` when the system couldn't answer.
fn unlisted() -> DataError {
    DataError::Failed("The app list isn't available".into())
}
