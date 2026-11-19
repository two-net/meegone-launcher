//! [`SystemConfigAdapter`] on Android: the home-app role, the clock (HOME-01, ONB-04), whether
//! the system animates the return home (ANIM-09), the haptic tick (ANIM-24), whether a browser
//! is installed, opening links in it and web search (SRCH-05, WALL-15, ADR 0016, ADR 0031), the
//! Events header's dates, times and next alarm (EVT-01, EVT-02, L10N-03, ADR 0019), the
//! device's locale (WALL-05, ADR 0027), how much free storage there is (SRS 7.6, ADR 0028), and
//! whether the device is saving battery (WALL-04, PWR-04, ADR 0029).
//!
//! Questions go to the Java `Bridge` object (`android/app/src/main/java/…/Bridge.java`), whose
//! method names and signatures must stay in step with the ones looked up here. The other device
//! settings answer with their "if unavailable" values until their issues build them.

use std::time::{SystemTime, UNIX_EPOCH};

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use crate::adapters::SystemConfigAdapter;
use crate::model::{DataError, TimeStyle};

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
    /// `String openInBrowser(String url)`: `null`, or a short message when no browser could
    /// open it.
    open_in_browser: MethodId,
    /// `String webSearch(String query)`: `null`, or a short message when nothing could search.
    web_search: MethodId,
    /// `String formatTime(long atMs, int style)`: `at_ms` in the style numbered by
    /// [`style_code`], or `null`.
    format_time: MethodId,
    /// `String locale()`: the device's first language as a BCP 47 tag, or `null`.
    locale: MethodId,
    /// `long nextAlarmMs()`: when the next alarm rings, or -1 if none is set.
    next_alarm_ms: MethodId,
    /// `long freeStorageBytes()`: the bytes MeeGone can still store, or -1 if the system can't
    /// say.
    free_storage_bytes: MethodId,
    /// `boolean savesBattery()`: battery saver is on or the battery is low.
    saves_battery: MethodId,
    /// `String openNextAlarm()`: `null`, or a short message when nothing could be opened.
    open_next_alarm: MethodId,
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
            open_in_browser: env.method_id(
                bridge,
                c"openInBrowser",
                c"(Ljava/lang/String;)Ljava/lang/String;",
            )?,
            web_search: env.method_id(
                bridge,
                c"webSearch",
                c"(Ljava/lang/String;)Ljava/lang/String;",
            )?,
            format_time: env.method_id(bridge, c"formatTime", c"(JI)Ljava/lang/String;")?,
            locale: env.method_id(bridge, c"locale", c"()Ljava/lang/String;")?,
            next_alarm_ms: env.method_id(bridge, c"nextAlarmMs", c"()J")?,
            free_storage_bytes: env.method_id(bridge, c"freeStorageBytes", c"()J")?,
            saves_battery: env.method_id(bridge, c"savesBattery", c"()Z")?,
            open_next_alarm: env.method_id(bridge, c"openNextAlarm", c"()Ljava/lang/String;")?,
            bridge: env.global_ref(bridge)?,
        })
    }
}

/// The number `Bridge.formatTime` knows each style by; `Bridge.java` mirrors them.
fn style_code(style: TimeStyle) -> i32 {
    match style {
        TimeStyle::Weekday => 0,
        TimeStyle::DayMonth => 1,
        TimeStyle::Time => 2,
        TimeStyle::WeekdayTime => 3,
        TimeStyle::DayMonthTime => 4,
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

    /// The first of the device's languages, such as `de-AT`. United States English if the
    /// bridge can't answer, the region the feed falls back to (WALL-05).
    fn locale(&self) -> String {
        self.vm
            .env()
            .and_then(|env| env.call_string(&self.bridge, self.locale, &[]))
            .ok()
            .flatten()
            .filter(|tag| !tag.is_empty())
            .unwrap_or_else(|| "en-US".into())
    }

    /// `StorageManager.getAllocatableBytes` for MeeGone's files, which leaves out the space the
    /// system keeps for itself when storage runs low. `None` if the bridge can't answer.
    fn free_storage_bytes(&self) -> Option<u64> {
        self.vm
            .env()
            .and_then(|env| env.call_long(&self.bridge, self.free_storage_bytes, &[]))
            .ok()
            .and_then(|bytes| u64::try_from(bytes).ok())
    }

    /// `PowerManager.isPowerSaveMode`, or the battery's own "low" flag. False if the bridge
    /// can't answer, so the daily image isn't held back for good; the job's scheduler still
    /// waits while the battery is low (WALL-04).
    fn saves_battery(&self) -> bool {
        self.vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.saves_battery, &[]))
            .unwrap_or(false)
    }

    /// Written by `android.text.format.DateFormat` in the default locale and time zone, so it
    /// follows the device's language and 12- or 24-hour setting (L10N-03). Empty if the bridge
    /// can't answer, and the header then leaves that line out.
    fn format_time(&self, at_ms: u64, style: TimeStyle) -> String {
        let at = JValue::long(i64::try_from(at_ms).unwrap_or(i64::MAX));
        let style = JValue::int(style_code(style));
        self.vm
            .env()
            .and_then(|env| env.call_string(&self.bridge, self.format_time, &[at, style]))
            .ok()
            .flatten()
            .unwrap_or_default()
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

    /// `AlarmManager.getNextAlarmClock()`, which needs no permission. `None`, which hides the
    /// alarm row, if none is set or the bridge can't answer (SRS 8.1).
    fn next_alarm_ms(&self) -> Option<u64> {
        self.vm
            .env()
            .and_then(|env| env.call_long(&self.bridge, self.next_alarm_ms, &[]))
            .ok()
            .and_then(|ms| u64::try_from(ms).ok())
    }

    /// Sends the alarm's show intent, which the app that set it provides, or opens the clock
    /// app's alarms when there is none (EVT-02).
    fn open_next_alarm(&self) -> Result<(), DataError> {
        let env = self.vm.env()?;
        match env.call_string(&self.bridge, self.open_next_alarm, &[])? {
            None => Ok(()),
            Some(message) => Err(DataError::Unsupported(message)),
        }
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

    /// Opens the link in the device's browser, never inside MeeGone, and only over HTTPS: the
    /// bridge checks the scheme again (SEC-02, WALL-15).
    fn open_in_browser(&self, https_url: &str) -> Result<(), DataError> {
        if !https_url.starts_with("https://") {
            return Err(DataError::Invalid("Only HTTPS links open".into()));
        }
        let env = self.vm.env()?;
        let url = env.new_string(https_url)?;
        match env.call_string(&self.bridge, self.open_in_browser, &[JValue::object(&url)])? {
            None => Ok(()),
            Some(message) => Err(DataError::Unsupported(message)),
        }
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
