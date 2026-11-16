//! [`SystemConfigAdapter`] on Android: the home-app role, the clock (HOME-01, ONB-04), whether
//! the system animates the return home (ANIM-09), the haptic tick (ANIM-24), whether a browser
//! is installed and web search (SRCH-05, ADR 0016).
//!
//! Questions go to the Java `Bridge` object (`android/app/src/main/java/…/Bridge.java`), whose
//! method names and signatures must stay in step with the ones looked up here. The device
//! settings, the next alarm and opening links answer with their "if unavailable" values until
//! their issues build them.

use std::time::{SystemTime, UNIX_EPOCH};

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use super::not_yet;
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
    /// `long installedMs()`: `PackageInfo.firstInstallTime`, or 0.
    installed_ms: MethodId,
    /// `boolean animatesReturnHome()`
    animates_return_home: MethodId,
    /// `boolean hapticTick()`: whether a view could play it.
    haptic_tick: MethodId,
    /// `boolean hasBrowser()`: whether anything opens an `https:` link.
    has_browser: MethodId,
    /// `String webSearch(String query)`: `null`, or a short message when nothing could search.
    web_search: MethodId,
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
            installed_ms: env.method_id(bridge, c"installedMs", c"()J")?,
            animates_return_home: env.method_id(bridge, c"animatesReturnHome", c"()Z")?,
            haptic_tick: env.method_id(bridge, c"hapticTick", c"()Z")?,
            has_browser: env.method_id(bridge, c"hasBrowser", c"()Z")?,
            web_search: env.method_id(
                bridge,
                c"webSearch",
                c"(Ljava/lang/String;)Ljava/lang/String;",
            )?,
            bridge: env.global_ref(bridge)?,
        })
    }
}

impl SystemConfigAdapter for AndroidSystem {
    fn is_dark(&self) -> bool {
        false // The device's dark theme arrives with #32.
    }

    fn animation_scale(&self) -> f64 {
        1.0 // The system animation speed arrives with #33.
    }

    /// If the bridge can't answer, the system is taken to animate, so the two never double up.
    fn animates_return_home(&self) -> bool {
        self.vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.animates_return_home, &[]))
            .unwrap_or(true)
    }

    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            })
    }

    /// 0 if the bridge can't answer, so a restore is told apart only when the time is known.
    fn installed_ms(&self) -> u64 {
        self.vm
            .env()
            .and_then(|env| env.call_long(&self.bridge, self.installed_ms, &[]))
            .map_or(0, |ms| u64::try_from(ms).unwrap_or(0))
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

    /// Played by a view, so it follows the system's touch-feedback setting and needs no
    /// permission (SEC-03). Nothing happens if it can't be played.
    fn haptic_tick(&self) {
        let _played = self
            .vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.haptic_tick, &[]));
    }

    /// If the bridge can't answer, there is taken to be none, so nothing offers a link that
    /// can't open.
    fn has_browser(&self) -> bool {
        self.vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.has_browser, &[]))
            .unwrap_or(false)
    }

    fn open_in_browser(&self, _https_url: &str) -> Result<(), DataError> {
        Err(not_yet()) // Links arrive with #30.
    }

    /// Hands the query to the device's default search handler, `ACTION_WEB_SEARCH` (SRCH-05).
    fn web_search(&self, query: &str) -> Result<(), DataError> {
        let env = self.vm.env()?;
        let query = env.new_string(query)?;
        match env.call_string(&self.bridge, self.web_search, &[JValue::object(&query)])? {
            None => Ok(()),
            Some(message) => Err(DataError::Unsupported(message)),
        }
    }
}
