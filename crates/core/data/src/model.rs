//! Models shared across features (SRS 7.4).

use std::fmt;

/// A user profile on the device, such as personal or work (APPS-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ProfileId(pub u32);

/// A profile that holds apps (SRS 8.1, "App catalogue").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    pub id: ProfileId,
    /// Its apps carry the system's work badge (APPS-20).
    pub work: bool,
    /// Its apps are shown dimmed while it is paused (APPS-09).
    pub paused: bool,
    /// Locked until the user unlocks it, such as a work profile with its own lock after a
    /// restart. The system lists none of its apps meanwhile, so the layout keeps their places
    /// (APPS-18).
    pub locked: bool,
}

/// Identifies one launchable app: its profile, package and launcher activity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AppId {
    pub profile: ProfileId,
    pub package: String,
    pub activity: String,
}

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// A colour from `0xRRGGBB`.
    pub const fn hex(rgb: u32) -> Self {
        Self {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
        }
    }
}

/// One launchable app. Rebuilt from the system at every start and kept in memory only
/// (SRS 7.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub id: AppId,
    pub label: String,
    /// The label's collation key in the device's locale: comparing keys byte by byte follows the
    /// locale's alphabetical order, ignoring case and accents (L10N-04). The platform makes it,
    /// because only it knows the locale's rules.
    pub sort_key: Vec<u8>,
    /// The label folded for search: its accents removed, its case folded and its spaces
    /// collapsed, so "Café" is `cafe` (SRCH-02, L10N-04). The platform makes it, and folds a
    /// query the same way with `AppsAdapter::search_key`, because only it knows Unicode's rules.
    pub search_key: String,
    /// Derived once from the icon and reused in Events and Recent (APPS-17).
    pub accent: Rgb,
    /// False while the profile is paused, the app is suspended or its storage is missing
    /// (APPS-09).
    pub available: bool,
    /// In a work profile: the system badges its icon and its name says so (APPS-20, A11Y-01).
    pub work: bool,
    /// False for apps the user can't uninstall, which show no ✕ in edit mode (APPS-13).
    pub can_uninstall: bool,
    /// When the app was first installed, in milliseconds since the Unix epoch. Apps that are new
    /// to the grid join its end in this order (APPS-06).
    pub installed_at_ms: u64,
}

/// One of an app's shortcuts, offered in its long-press menu (APPS-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub app: AppId,
    pub id: String,
    pub label: String,
}

/// Whether the user has granted a special access (C4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Access {
    #[default]
    Missing,
    Granted,
}

/// An active notification. Held in memory only and never stored, logged or transmitted
/// (EVT-13), so its `Debug` output leaves out the title and text.
#[derive(Clone, PartialEq, Eq)]
pub struct Notification {
    pub key: String,
    /// The profile of the app that posted it: the same app in the personal and the work profile
    /// is two apps (APPS-20).
    pub profile: ProfileId,
    pub package: String,
    /// The system's name for the app that posted it, for an app with no icon in the grid, such
    /// as Android System (EVT-04). Empty if the system has none.
    pub app_label: String,
    pub title: String,
    pub text: String,
    pub posted_at_ms: u64,
    pub clearable: bool,
    /// Media playback, navigation or a running service (EVT-05).
    pub ongoing: bool,
    /// A summary the app posts for a bundle of its notifications, which stand for themselves
    /// (EVT-04).
    pub summary: bool,
    /// Meant to disappear when opened: MeeGone removes it after opening it (EVT-06).
    pub auto_cancel: bool,
}

impl fmt::Debug for Notification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Notification")
            .field("key", &self.key)
            .field("profile", &self.profile)
            .field("package", &self.package)
            .field("app_label", &self.app_label)
            .field("title", &"<redacted>")
            .field("text", &"<redacted>")
            .field("posted_at_ms", &self.posted_at_ms)
            .field("clearable", &self.clearable)
            .field("ongoing", &self.ongoing)
            .field("summary", &self.summary)
            .field("auto_cancel", &self.auto_cancel)
            .finish()
    }
}

/// When an app was last used (REC-03). Memory only (PRIV-03).
///
/// The system reports the activity last resumed, which is rarely the app's launcher activity,
/// so this isn't an [`AppId`]: Recent finds the launchable app with the same profile and
/// package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentUse {
    pub profile: ProfileId,
    pub package: String,
    /// The class of the activity last resumed, or empty if the system didn't say.
    pub activity: String,
    /// The last time the app came to the front or left it, in milliseconds since the Unix
    /// epoch.
    pub last_used_ms: u64,
}

/// The kind of network the device is on (WALL-04, WALL-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkState {
    #[default]
    Offline,
    /// Mobile data or another network that may cost the user.
    Metered,
    Unmetered,
}

/// The network a scheduled image job waits for (WALL-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobNetwork {
    Unmetered,
    /// Any connection, when "Use mobile data" is on.
    Any,
}

/// A request to run the one scheduled background job, the daily image (ARCH-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageJob {
    /// The job runs no earlier than this, in milliseconds since the Unix epoch. 0 means as soon
    /// as its conditions allow.
    pub not_before_ms: u64,
    pub network: JobNetwork,
}

/// A rectangle on the screen, in physical pixels from its top left, such as where a launch
/// grows from (ANIM-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// Whether it covers no pixels.
    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
}

/// How [`SystemConfigAdapter::format_time`](crate::SystemConfigAdapter::format_time) writes a
/// moment: in the device's language, time zone and 12- or 24-hour setting (L10N-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeStyle {
    /// The weekday in full, such as "Tuesday" (EVT-01).
    Weekday,
    /// The day of the month and the month in full, in the language's order, such as
    /// "17 November" or "November 17" (EVT-01).
    DayMonth,
    /// Hours and minutes, such as "07:00" on a 24-hour device or "7:00 AM" (EVT-02).
    Time,
    /// The short weekday and the time, such as "Wed 07:00" (EVT-02).
    WeekdayTime,
    /// The day of the month, the month in full and the time, in the language's order, such as
    /// "19 November, 07:00" or "November 19, 7:00 AM": when the daily image last updated
    /// (WALL-16).
    DayMonthTime,
}

/// A size in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    /// Whether it covers no pixels.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// One of the device's screens, as the lock-screen image is cropped for it (WALL-06,
/// ADR 0030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Screen {
    /// Its size in its natural orientation, so turning the device doesn't change it.
    pub size: Size,
    /// Its density in dots per inch: 160 is one pixel per dp.
    pub density_dpi: u32,
}

/// The system's name for a wallpaper it holds, so MeeGone can tell whether the lock screen
/// still has the one it set (WALL-14, ADR 0030). It means nothing else: markers are only
/// compared. On Android it is `WallpaperManager`'s wallpaper id, negative when the lock screen
/// has no wallpaper of its own and shows the home wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WallpaperMarker(pub i32);

/// How to turn a downloaded image into the lock-screen image (WALL-06): keep the region at
/// `left`, `top`, `width` × `height` of an `image`-sized picture, decode it at 1/`sample` of its
/// size, and scale the result to `size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageCrop {
    /// The whole image's size, as its header gives it. The decoder must find the same.
    pub image: Size,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    /// A power of two: the region is decoded at 1/`sample` of its size, so no more is decoded
    /// than the result needs (SEC-02).
    pub sample: u32,
    /// The result's size.
    pub size: Size,
}

/// An error from a platform adapter or a store. Each feature turns it into its own error state
/// (ARCH-09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    /// The user hasn't granted the special access this needs (C4).
    AccessDenied,
    /// The device or its policy doesn't allow it; the text says why (WALL-13).
    Unsupported(String),
    /// There is no network connection (WALL-16).
    Offline,
    /// Data from outside was rejected: not HTTPS, too large, malformed, or in a format that
    /// changed (SEC-02). The text says why.
    Invalid(String),
    /// Anything else, in plain words for a status line.
    Failed(String),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessDenied => f.write_str("Access not granted"),
            Self::Offline => f.write_str("No connection"),
            Self::Unsupported(reason) | Self::Invalid(reason) | Self::Failed(reason) => {
                f.write_str(reason)
            }
        }
    }
}

impl std::error::Error for DataError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_debug_output_leaves_out_its_content() {
        let notification = Notification {
            key: "k1".into(),
            profile: ProfileId(0),
            package: "com.example.chat".into(),
            app_label: "Chat".into(),
            title: "Anna".into(),
            text: "See you at eight".into(),
            posted_at_ms: 1,
            clearable: true,
            ongoing: false,
            summary: false,
            auto_cancel: true,
        };

        let printed = format!("{notification:?}");

        // EVT-13
        assert!(printed.contains("com.example.chat"));
        assert!(!printed.contains("Anna"));
        assert!(!printed.contains("See you at eight"));
    }

    #[test]
    fn errors_read_as_plain_words_for_a_status_line() {
        // WALL-16
        assert_eq!(DataError::AccessDenied.to_string(), "Access not granted");
        assert_eq!(DataError::Offline.to_string(), "No connection");
        assert_eq!(
            DataError::Invalid("Too large".into()).to_string(),
            "Too large"
        );
        assert_eq!(
            DataError::Unsupported("Blocked by device policy".into()).to_string(),
            "Blocked by device policy"
        );
    }

    #[test]
    fn a_rect_without_area_is_empty() {
        let icon = Rect {
            left: 10,
            top: 20,
            right: 66,
            bottom: 76,
        };

        assert!(!icon.is_empty());
        assert!(Rect::default().is_empty());
        assert!(Rect { right: 10, ..icon }.is_empty());
        assert!(Rect { bottom: 0, ..icon }.is_empty());
    }

    #[test]
    fn hex_colours_split_into_channels() {
        assert_eq!(
            Rgb::hex(0x3DA5FF),
            Rgb {
                r: 0x3D,
                g: 0xA5,
                b: 0xFF
            }
        );
    }
}
