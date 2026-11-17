//! [`NotificationsAdapter`] on Android: whether MeeGone has notification access, the active
//! notifications its `NotificationListener` sees, and the system's access screen (EVT-03,
//! EVT-04, EVT-05, ADR 0020).
//!
//! Questions go to the Java `Bridge` object, whose method names and signatures must stay in
//! step with the ones looked up here. The notifications arrive as flat records, parsed in
//! [`crate::notification_records`]. Their content stays in memory: nothing here logs it, and
//! no error carries it (EVT-13).

use super::jni::{Env, GlobalRef, LocalRef, MethodId, Vm};
use super::not_yet;
use crate::adapters::NotificationsAdapter;
use crate::model::{Access, DataError, Notification};
use crate::notification_records;

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
            None => Err(DataError::Failed(
                "Notifications can't be read right now".into(),
            )),
        }
    }

    fn open(&self, _key: &str) -> Result<(), DataError> {
        Err(not_yet()) // Opening a notification arrives with #20.
    }

    fn dismiss(&self, _key: &str) -> Result<(), DataError> {
        Err(not_yet()) // Clear and Dismiss arrive with #20.
    }

    /// Opens MeeGone's own page of the system's notification-access screen, or the screen
    /// itself (EVT-03).
    fn open_access_settings(&self) -> Result<(), DataError> {
        let env = self.vm.env()?;
        match env.call_string(&self.bridge, self.open_notification_access, &[])? {
            None => Ok(()),
            Some(message) => Err(DataError::Unsupported(message)),
        }
    }

    fn open_app_settings(&self, _package: &str) -> Result<(), DataError> {
        Err(not_yet()) // An app's notification settings arrive with #20.
    }
}
