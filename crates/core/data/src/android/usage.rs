//! [`UsageAdapter`] on Android: whether MeeGone has usage access, the apps used recently, and
//! the system's usage-access screen (REC-02, REC-03, ADR 0023).
//!
//! Questions go to the Java `Bridge` object, whose method names and signatures must stay in
//! step with the ones looked up here. The uses arrive as flat records, parsed in
//! [`crate::usage_records`]. Usage history stays in memory: nothing here logs it, and no error
//! carries it (PRIV-03).

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use crate::adapters::UsageAdapter;
use crate::model::{Access, DataError, RecentUse};
use crate::usage_records;

pub struct AndroidUsage {
    vm: Vm,
    bridge: GlobalRef,
    /// `boolean usageAccess()`: whether the user has granted MeeGone usage access.
    usage_access: MethodId,
    /// `String[] recentUses(long since)`: records of [`usage_records::USE_FIELDS`] fields, one
    /// per app used at or after `since`, or `null` if the system couldn't list them.
    recent_uses: MethodId,
    /// `String openUsageAccess()`: `null`, or a short message when no system screen could be
    /// opened.
    open_usage_access: MethodId,
}

impl AndroidUsage {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            usage_access: env.method_id(bridge, c"usageAccess", c"()Z")?,
            recent_uses: env.method_id(bridge, c"recentUses", c"(J)[Ljava/lang/String;")?,
            open_usage_access: env.method_id(
                bridge,
                c"openUsageAccess",
                c"()Ljava/lang/String;",
            )?,
            bridge: env.global_ref(bridge)?,
        })
    }
}

impl UsageAdapter for AndroidUsage {
    /// `Missing` if the bridge can't answer, so Recent explains instead of failing (REC-02).
    fn access(&self) -> Access {
        let granted = self
            .vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.usage_access, &[]))
            .unwrap_or(false);
        if granted {
            Access::Granted
        } else {
            Access::Missing
        }
    }

    /// Without access the system would answer with nothing, which Recent would read as "No
    /// recent apps", so this says so instead (REC-02).
    fn used_since(&self, since_ms: u64) -> Result<Vec<RecentUse>, DataError> {
        if self.access() == Access::Missing {
            return Err(DataError::AccessDenied);
        }
        let env = self.vm.env()?;
        let since = i64::try_from(since_ms).unwrap_or(i64::MAX);
        match env.call_strings(&self.bridge, self.recent_uses, &[JValue::long(since)])? {
            Some(fields) => Ok(usage_records::uses(&fields)),
            None => Err(DataError::Failed(
                "Recent apps can't be read right now".into(),
            )),
        }
    }

    /// Opens MeeGone's own page of the system's usage-access screen, or the screen itself
    /// (REC-02).
    fn open_access_settings(&self) -> Result<(), DataError> {
        let env = self.vm.env()?;
        match env.call_string(&self.bridge, self.open_usage_access, &[])? {
            None => Ok(()),
            Some(message) => Err(DataError::Unsupported(message)),
        }
    }
}
