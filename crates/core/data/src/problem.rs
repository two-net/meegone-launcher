//! What went wrong, as a code the host words in the device's language (L10N-01, ADR 0040).
//!
//! Nothing in the Rust app writes words for the user. A failure is a [`Problem`]; the host
//! shows it through `Problems.java`, which turns each code into its `problem_<code>` string
//! resource. A code is also what the image cache saves (`IMAGE_CACHE_VERSION` 6), so a problem
//! saved in one language is shown in the next.
//!
//! The platform's own failures arrive as codes too: each `Bridge` method that can fail returns
//! one of `Problems.java`'s constants, read with [`Problem::from_host`].
//!
//! `cargo test -p arch-check` checks that every code here has its constant and case in
//! `Problems.java` and its string resource, and that there are no others.

use std::fmt;

/// A failure the user may be told about. Each one has a stable [`code`](Self::code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Problem {
    // The system.
    /// The user hasn't granted the special access this needs.
    AccessDenied,
    /// The system refused or failed, without saying why.
    SystemFailed,
    /// The app catalogue couldn't be read.
    AppListUnavailable,
    /// The system offers an app's shortcuts only to the default home app (APPS-11).
    ShortcutsNeedDefaultHome,
    /// The active notifications couldn't be read (EVT-04).
    NotificationsUnreadable,
    /// The apps used recently couldn't be read (REC-01).
    RecentUnreadable,

    // Opening things.
    /// A tapped app couldn't open, and the system said nothing itself (APPS-09).
    AppUnavailable,
    /// An app's shortcut couldn't open (APPS-11).
    ShortcutUnavailable,
    /// The system refused to uninstall an app (APPS-13).
    UninstallUnavailable,
    /// An app's App info couldn't open (APPS-13).
    AppInfoUnavailable,
    /// Neither a notification nor its app could open (EVT-06).
    NotificationUnavailable,
    /// The system refused to remove notifications (EVT-07, EVT-08).
    NotificationsDismissUnavailable,
    /// Neither an app's notification settings nor its App info could open (EVT-08).
    NotificationSettingsUnavailable,
    /// Neither the app that set the alarm nor a clock app could open (EVT-02).
    AlarmUnavailable,
    /// No browser could open a link (SEC-02).
    BrowserUnavailable,
    /// Nothing on the device can search the web (SRCH-05).
    WebSearchUnavailable,
    /// Only `https://` links open (SEC-02).
    HttpsLinksOnly,

    // Written instructions, where a system screen couldn't open (SRS 8.1).
    /// How to make MeeGone the default home app (HOME-01).
    DefaultHomeInstructions,
    /// How to turn on notification access (EVT-03).
    NotificationAccessInstructions,
    /// How to turn on usage access (REC-02).
    UsageAccessInstructions,
    /// How to change the wallpaper (WALL-12).
    WallpaperSettingsInstructions,

    // The network (SEC-02).
    /// There is no network connection.
    NoConnection,
    /// A request was redirected too many times.
    TooManyRedirects,
    /// The connection's certificate or protocol wasn't secure.
    ConnectionInsecure,
    /// A URL or redirect wasn't `https://`.
    HttpsOnly,
    /// A response was larger than allowed.
    DownloadTooLarge,
    /// A request failed for any other reason.
    DownloadFailed,

    // Storage (REL-03, PERF-08).
    /// There isn't enough storage space.
    StorageFull,
    /// MeeGone's own files couldn't be read or written.
    SavedDataUnusable,
    /// The write would take MeeGone's saved data past its budget (PERF-08).
    StorageBudget,

    // The daily image (WALL-*).
    /// The device can't set wallpapers (WALL-13).
    WallpaperNotSupported,
    /// The device's policy doesn't allow changing the wallpaper (WALL-13).
    WallpaperNotAllowed,
    /// The lock screen's wallpaper couldn't be set.
    WallpaperNotSet,
    /// The lock screen couldn't be reset to the home wallpaper (WALL-12).
    LockScreenResetFailed,
    /// The daily check couldn't be scheduled.
    ImageJobUnavailable,
    /// The lock screen's size is unknown, so there is nothing to crop for.
    ScreenSizeUnknown,
    /// The image didn't decode.
    ImageUnreadable,
    /// The image is too small for the lock screen.
    ImageTooSmall,
    /// The image is larger than MeeGone decodes (SEC-02).
    ImageTooLarge,
    /// A past image's preview is larger than allowed (SEC-02).
    PreviewTooLarge,
    /// The image feed's format has changed (SRS 8.2).
    FeedFormatChanged,
    /// The image feed doesn't list that region (WALL-05).
    NoSuchRegion,
    /// The feed no longer lists the image (WALL-17).
    ImageNoLongerListed,
    /// The feed doesn't offer the image as a wallpaper (WALL-17).
    ImageNotOffered,
    /// The image isn't in the cache.
    ImageNotCached,
}

impl Problem {
    /// Every problem, in declaration order.
    pub const ALL: [Self; 45] = [
        Self::AccessDenied,
        Self::SystemFailed,
        Self::AppListUnavailable,
        Self::ShortcutsNeedDefaultHome,
        Self::NotificationsUnreadable,
        Self::RecentUnreadable,
        Self::AppUnavailable,
        Self::ShortcutUnavailable,
        Self::UninstallUnavailable,
        Self::AppInfoUnavailable,
        Self::NotificationUnavailable,
        Self::NotificationsDismissUnavailable,
        Self::NotificationSettingsUnavailable,
        Self::AlarmUnavailable,
        Self::BrowserUnavailable,
        Self::WebSearchUnavailable,
        Self::HttpsLinksOnly,
        Self::DefaultHomeInstructions,
        Self::NotificationAccessInstructions,
        Self::UsageAccessInstructions,
        Self::WallpaperSettingsInstructions,
        Self::NoConnection,
        Self::TooManyRedirects,
        Self::ConnectionInsecure,
        Self::HttpsOnly,
        Self::DownloadTooLarge,
        Self::DownloadFailed,
        Self::StorageFull,
        Self::SavedDataUnusable,
        Self::StorageBudget,
        Self::WallpaperNotSupported,
        Self::WallpaperNotAllowed,
        Self::WallpaperNotSet,
        Self::LockScreenResetFailed,
        Self::ImageJobUnavailable,
        Self::ScreenSizeUnknown,
        Self::ImageUnreadable,
        Self::ImageTooSmall,
        Self::ImageTooLarge,
        Self::PreviewTooLarge,
        Self::FeedFormatChanged,
        Self::NoSuchRegion,
        Self::ImageNoLongerListed,
        Self::ImageNotOffered,
        Self::ImageNotCached,
    ];

    /// The problem's code: lower case with underscores, the same in `Problems.java` and, after
    /// `problem_`, in the string resource's name. Saved to disk, so never change one.
    pub const fn code(self) -> &'static str {
        match self {
            Self::AccessDenied => "access_denied",
            Self::SystemFailed => "system_failed",
            Self::AppListUnavailable => "app_list_unavailable",
            Self::ShortcutsNeedDefaultHome => "shortcuts_need_default_home",
            Self::NotificationsUnreadable => "notifications_unreadable",
            Self::RecentUnreadable => "recent_unreadable",
            Self::AppUnavailable => "app_unavailable",
            Self::ShortcutUnavailable => "shortcut_unavailable",
            Self::UninstallUnavailable => "uninstall_unavailable",
            Self::AppInfoUnavailable => "app_info_unavailable",
            Self::NotificationUnavailable => "notification_unavailable",
            Self::NotificationsDismissUnavailable => "notifications_dismiss_unavailable",
            Self::NotificationSettingsUnavailable => "notification_settings_unavailable",
            Self::AlarmUnavailable => "alarm_unavailable",
            Self::BrowserUnavailable => "browser_unavailable",
            Self::WebSearchUnavailable => "web_search_unavailable",
            Self::HttpsLinksOnly => "https_links_only",
            Self::DefaultHomeInstructions => "default_home_instructions",
            Self::NotificationAccessInstructions => "notification_access_instructions",
            Self::UsageAccessInstructions => "usage_access_instructions",
            Self::WallpaperSettingsInstructions => "wallpaper_settings_instructions",
            Self::NoConnection => "no_connection",
            Self::TooManyRedirects => "too_many_redirects",
            Self::ConnectionInsecure => "connection_insecure",
            Self::HttpsOnly => "https_only",
            Self::DownloadTooLarge => "download_too_large",
            Self::DownloadFailed => "download_failed",
            Self::StorageFull => "storage_full",
            Self::SavedDataUnusable => "saved_data_unusable",
            Self::StorageBudget => "storage_budget",
            Self::WallpaperNotSupported => "wallpaper_not_supported",
            Self::WallpaperNotAllowed => "wallpaper_not_allowed",
            Self::WallpaperNotSet => "wallpaper_not_set",
            Self::LockScreenResetFailed => "lock_screen_reset_failed",
            Self::ImageJobUnavailable => "image_job_unavailable",
            Self::ScreenSizeUnknown => "screen_size_unknown",
            Self::ImageUnreadable => "image_unreadable",
            Self::ImageTooSmall => "image_too_small",
            Self::ImageTooLarge => "image_too_large",
            Self::PreviewTooLarge => "preview_too_large",
            Self::FeedFormatChanged => "feed_format_changed",
            Self::NoSuchRegion => "no_such_region",
            Self::ImageNoLongerListed => "image_no_longer_listed",
            Self::ImageNotOffered => "image_not_offered",
            Self::ImageNotCached => "image_not_cached",
        }
    }

    /// The problem with `code`, or `None` if there is none.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|problem| problem.code() == code)
    }

    /// The problem a `Bridge` method returned. A code this app doesn't know, which only a
    /// mismatched host could send, is [`SystemFailed`](Self::SystemFailed).
    pub fn from_host(code: &str) -> Self {
        Self::from_code(code).unwrap_or(Self::SystemFailed)
    }
}

impl fmt::Display for Problem {
    /// The code, for logs. The user sees the host's words for it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_problem_is_listed_once() {
        let listed: HashSet<Problem> = Problem::ALL.into_iter().collect();
        assert_eq!(listed.len(), Problem::ALL.len());
        // `code` has an arm for every variant, which the compiler checks, so `ALL` lists every
        // one if it has as many.
        let arms = include_str!("problem.rs")
            .lines()
            .filter(|line| line.trim_start().starts_with("Self::") && line.contains(" => \""))
            .count();
        assert_eq!(arms, Problem::ALL.len());
    }

    #[test]
    fn every_code_is_unique_lower_case_and_reads_back() {
        let mut codes = HashSet::new();
        for problem in Problem::ALL {
            let code = problem.code();
            assert!(codes.insert(code), "{code} is used twice");
            assert!(
                !code.is_empty()
                    && !code.starts_with('_')
                    && !code.ends_with('_')
                    && code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{code} isn't lower case with underscores"
            );
            assert_eq!(Problem::from_code(code), Some(problem));
        }
    }

    #[test]
    fn an_unknown_code_from_the_host_is_a_system_failure() {
        assert_eq!(Problem::from_code("no_such_thing"), None);
        assert_eq!(Problem::from_host("no_such_thing"), Problem::SystemFailed);
        assert_eq!(
            Problem::from_host("app_unavailable"),
            Problem::AppUnavailable
        );
    }

    #[test]
    fn a_problem_displays_as_its_code() {
        assert_eq!(Problem::NoConnection.to_string(), "no_connection");
    }
}
