//! Models shared across features (SRS 7.4).

use std::fmt;

/// A user profile on the device, such as personal or work (APPS-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ProfileId(pub u32);

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
    /// Derived once from the icon and reused in Events and Recent (APPS-17).
    pub accent: Rgb,
    /// False while the profile is paused, the app is suspended or its storage is missing
    /// (APPS-09).
    pub available: bool,
    pub installed_at_ms: u64,
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
    pub package: String,
    pub title: String,
    pub text: String,
    pub posted_at_ms: u64,
    pub clearable: bool,
    /// Media playback, navigation or a running service (EVT-05).
    pub ongoing: bool,
}

impl fmt::Debug for Notification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Notification")
            .field("key", &self.key)
            .field("package", &self.package)
            .field("title", &"<redacted>")
            .field("text", &"<redacted>")
            .field("posted_at_ms", &self.posted_at_ms)
            .field("clearable", &self.clearable)
            .field("ongoing", &self.ongoing)
            .finish()
    }
}

/// When an app was last used (REC-03). Memory only (PRIV-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentUse {
    pub app: AppId,
    pub last_used_ms: u64,
}

/// An error from a platform adapter or a store. Each feature turns it into its own error state
/// (ARCH-09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    /// The user hasn't granted the special access this needs (C4).
    AccessDenied,
    /// The device or its policy doesn't allow it; the text says why (WALL-13).
    Unsupported(String),
    /// Anything else, in plain words for a status line.
    Failed(String),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessDenied => f.write_str("Access not granted"),
            Self::Unsupported(reason) | Self::Failed(reason) => f.write_str(reason),
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
            package: "com.example.chat".into(),
            title: "Anna".into(),
            text: "See you at eight".into(),
            posted_at_ms: 1,
            clearable: true,
            ongoing: false,
        };

        let printed = format!("{notification:?}");

        assert!(printed.contains("com.example.chat"));
        assert!(!printed.contains("Anna"));
        assert!(!printed.contains("See you at eight"));
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
