//! [`NotificationsAdapter`] on Android: whether MeeGone has notification access, the active
//! notifications its `NotificationListener` sees, opening and dismissing them, and the system's
//! access and notification-settings screens (EVT-03 to EVT-08, ADR 0020, ADR 0021).
//!
//! Questions go to the Java `Bridge` object, whose method names and signatures must stay in
//! step with the ones looked up here. The notifications arrive as flat records, parsed in
//! [`crate::notification_records`]. Their content stays in memory: nothing here logs it, and
//! no error carries it (EVT-13).

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use crate::adapters::NotificationsAdapter;
use crate::model::{Access, DataError, Notification, ProfileId, Rect};
use crate::notification_records;
use crate::problem::Problem;

pub struct AndroidNotifications {
    vm: Vm,
    bridge: GlobalRef,
    /// `boolean notificationAccess()`: whether the user has granted MeeGone's listener
    /// notification access.
    notification_access: MethodId,
    /// `String[] activeNotifications()`: records of
    /// [`notification_records::NOTIFICATION_FIELDS`] fields, none while the listener isn't
    /// connected yet, or `null` if the system couldn't list them.
    active_notifications: MethodId,
    /// `String openNotificationAccess()`: `null`, or a short message when no system screen
    /// could be opened.
    open_notification_access: MethodId,
    /// `String openNotification(String key, int left, int top, int right, int bottom)`: `null`,
    /// or a short message when nothing could open.
    open_notification: MethodId,
    /// `String dismissNotifications(String[] keys)`: `null`, or a short message when the system
    /// refused.
    dismiss_notifications: MethodId,
    /// `String openNotificationSettings(String package, long profile)`: `null`, or a short
    /// message when no screen could be opened.
    open_notification_settings: MethodId,
}

impl AndroidNotifications {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            notification_access: env.method_id(bridge, c"notificationAccess", c"()Z")?,
            active_notifications: env.method_id(
                bridge,
                c"activeNotifications",
                c"()[Ljava/lang/String;",
            )?,
            open_notification_access: env.method_id(
                bridge,
                c"openNotificationAccess",
                c"()Ljava/lang/String;",
            )?,
            open_notification: env.method_id(
                bridge,
                c"openNotification",
                c"(Ljava/lang/String;IIII)Ljava/lang/String;",
            )?,
            dismiss_notifications: env.method_id(
                bridge,
                c"dismissNotifications",
                c"([Ljava/lang/String;)Ljava/lang/String;",
            )?,
            open_notification_settings: env.method_id(
                bridge,
                c"openNotificationSettings",
                c"(Ljava/lang/String;J)Ljava/lang/String;",
            )?,
            bridge: env.global_ref(bridge)?,
        })
    }
}

impl NotificationsAdapter for AndroidNotifications {
    /// `Missing` if the bridge can't answer, so Events explains instead of failing (EVT-03).
    fn access(&self) -> Access {
        let granted = self
            .vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.notification_access, &[]))
            .unwrap_or(false);
        if granted {
            Access::Granted
        } else {
            Access::Missing
        }
    }

    fn active(&self) -> Result<Vec<Notification>, DataError> {
        let env = self.vm.env()?;
        match env.call_strings(&self.bridge, self.active_notifications, &[])? {
            Some(fields) => Ok(notification_records::notifications(&fields)),
            None => Err(DataError::Failed(Problem::NotificationsUnreadable)),
        }
    }

    /// The bridge sends the notification's content intent, or opens its app (EVT-06, ANIM-08).
    fn open(&self, key: &str, from: Option<Rect>) -> Result<(), DataError> {
        let env = self.vm.env()?;
        let key = env.new_string(key)?;
        let from = from.filter(|rect| !rect.is_empty()).unwrap_or_default();
        let args = [
            JValue::object(&key),
            JValue::int(from.left),
            JValue::int(from.top),
            JValue::int(from.right),
            JValue::int(from.bottom),
        ];
        match env.call_string(&self.bridge, self.open_notification, &args)? {
            None => Ok(()),
            Some(code) => Err(DataError::Failed(Problem::from_host(&code))),
        }
    }

    /// One request for all of them, as the listener's `cancelNotifications` takes (EVT-07).
    fn dismiss(&self, keys: &[String]) -> Result<(), DataError> {
        if keys.is_empty() {
            return Ok(());
        }
        let env = self.vm.env()?;
        let keys = env.new_string_array(keys.iter().map(String::as_str))?;
        match env.call_string(
            &self.bridge,
            self.dismiss_notifications,
            &[JValue::object(&keys)],
        )? {
            None => Ok(()),
            Some(code) => Err(DataError::Failed(Problem::from_host(&code))),
        }
    }

    /// Opens MeeGone's own page of the system's notification-access screen, or the screen
    /// itself (EVT-03).
    fn open_access_settings(&self) -> Result<(), DataError> {
        let env = self.vm.env()?;
        match env.call_string(&self.bridge, self.open_notification_access, &[])? {
            None => Ok(()),
            Some(code) => Err(DataError::Unsupported(Problem::from_host(&code))),
        }
    }

    /// The app's notification settings in its own profile, or its App info (EVT-08).
    fn open_app_settings(&self, profile: ProfileId, package: &str) -> Result<(), DataError> {
        let env = self.vm.env()?;
        let package = env.new_string(package)?;
        let args = [JValue::object(&package), JValue::long(i64::from(profile.0))];
        match env.call_string(&self.bridge, self.open_notification_settings, &args)? {
            None => Ok(()),
            Some(code) => Err(DataError::Unsupported(Problem::from_host(&code))),
        }
    }
}
