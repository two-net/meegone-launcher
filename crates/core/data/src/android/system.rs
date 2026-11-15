//! [`SystemConfigAdapter`] on Android: the home-app role and the clock (HOME-01, ONB-04).
//!
//! Questions go to the Java `Bridge` object (`android/app/src/main/java/…/Bridge.java`), whose
//! method names and signatures must stay in step with the ones looked up here. The device
//! settings, the next alarm, haptics and the browser answer with their "if unavailable" values
//! until their issues build them.

use std::time::{SystemTime, UNIX_EPOCH};

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use crate::adapters::SystemConfigAdapter;
use crate::model::DataError;

pub struct AndroidSystem {
    vm: Vm,
    bridge: GlobalRef,
    /// `boolean isDefaultHome()`
    is_default_home: MethodId,
    /// `String requestDefaultHome()`: `null`, or written instructions when no system screen
    /// could be opened.
    request_default_home: MethodId,
    /// `long utcOffsetMs(long atMs)`
    utc_offset_ms: MethodId,
}

impl AndroidSystem {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            is_default_home: env.method_id(bridge, c"isDefaultHome", c"()Z")?,
            request_default_home: env.method_id(
                bridge,
                c"requestDefaultHome",
                c"()Ljava/lang/String;",
            )?,
            utc_offset_ms: env.method_id(bridge, c"utcOffsetMs", c"(J)J")?,
            bridge: env.global_ref(bridge)?,
        })
    }
}

fn not_yet() -> DataError {
    DataError::Unsupported("Not available yet".into())
}

impl SystemConfigAdapter for AndroidSystem {
    fn is_dark(&self) -> bool {
        false // The device's dark theme arrives with #32.
    }

    fn animation_scale(&self) -> f64 {
        1.0 // The system animation speed arrives with #33.
    }

    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            })
    }

    fn utc_offset_ms(&self, at_ms: u64) -> i64 {
        let at = JValue::long(i64::try_from(at_ms).unwrap_or(i64::MAX));
        self.vm
            .env()
            .and_then(|env| env.call_long(&self.bridge, self.utc_offset_ms, &[at]))
            .unwrap_or(0)
    }

    fn locale(&self) -> String {
        "en-US".into() // The device's locale arrives with #39.
    }

    fn is_default_home(&self) -> bool {
        self.vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.is_default_home, &[]))
            .unwrap_or(false)
    }

    fn request_default_home(&self) -> Result<(), DataError> {
        let env = self.vm.env()?;
        match env.call_string(&self.bridge, self.request_default_home, &[])? {
            None => Ok(()),
            Some(instructions) => Err(DataError::Unsupported(instructions)),
        }
    }

    fn next_alarm_ms(&self) -> Option<u64> {
        None // The next alarm arrives with #18.
    }

    fn open_next_alarm(&self) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn haptic_tick(&self) {
        // Haptics arrive with the long-press menu, #12.
    }

    fn has_browser(&self) -> bool {
        false // Links and web search arrive with #30 and #15.
    }

    fn open_in_browser(&self, _https_url: &str) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn web_search(&self, _query: &str) -> Result<(), DataError> {
        Err(not_yet())
    }
}
