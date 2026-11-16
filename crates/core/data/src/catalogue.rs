//! The app catalogue as Android's `Bridge` reports it (ADR 0010).
//!
//! `Bridge.launchableApps()` and `Bridge.profiles()` return flat `String[]` records, each a
//! fixed number of fields, in the order below. The fields and flags must stay in step with
//! `Bridge.java`, as the frame slots do with `HomeViews.java`. They are parsed here, apart from
//! JNI, so the parsing is tested without a device. A record that doesn't parse is left out,
//! never fatal.

use crate::accent;
use crate::model::{App, AppId, Profile, ProfileId, Rgb};

/// `launchableApps()`: package, activity, label, profile serial number, [`app_flag`]s, the
/// accent `AppAccents.java` derived from the icon as a decimal `0xRRGGBB`, empty until it has
/// (ADR 0011), the label's collation key in lowercase hex (L10N-04), and the first install time
/// in decimal milliseconds since the Unix epoch (ADR 0012).
pub(crate) const APP_FIELDS: usize = 8;
/// `profiles()`: serial number, [`profile_flag`]s.
pub(crate) const PROFILE_FIELDS: usize = 2;

/// The flags of one app.
pub(crate) mod app_flag {
    /// Its profile isn't paused, and it isn't suspended (APPS-09).
    pub const AVAILABLE: u32 = 1;
    /// It isn't a system app (APPS-13).
    pub const CAN_UNINSTALL: u32 = 2;
    /// It is in a work profile (APPS-20).
    pub const WORK: u32 = 4;
}

/// The flags of one profile.
pub(crate) mod profile_flag {
    pub const WORK: u32 = 1;
    pub const PAUSED: u32 = 2;
    /// Not unlocked since the device started (APPS-18).
    pub const LOCKED: u32 = 4;
}

/// The apps in `fields`, in order.
pub(crate) fn apps(fields: &[Option<String>]) -> Vec<App> {
    fields.chunks_exact(APP_FIELDS).filter_map(app).collect()
}

/// The profiles in `fields`, in order.
pub(crate) fn profiles(fields: &[Option<String>]) -> Vec<Profile> {
    fields
        .chunks_exact(PROFILE_FIELDS)
        .filter_map(profile)
        .collect()
}

fn app(record: &[Option<String>]) -> Option<App> {
    let [
        package,
        activity,
        label,
        serial,
        flags,
        accent,
        sort_key,
        installed,
    ] = record
    else {
        return None;
    };
    let package = package.as_deref().filter(|text| !text.is_empty())?;
    let activity = activity.as_deref().filter(|text| !text.is_empty())?;
    let flags: u32 = number(flags)?;
    // An app with no label of its own is named by its package, so its cell is never blank.
    let label = label
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(package);
    Some(App {
        id: AppId {
            profile: ProfileId(number(serial)?),
            package: package.to_owned(),
            activity: activity.to_owned(),
        },
        label: label.to_owned(),
        sort_key: hex(sort_key.as_deref()?)?,
        accent: accent_of(accent),
        available: flags & app_flag::AVAILABLE != 0,
        work: flags & app_flag::WORK != 0,
        can_uninstall: flags & app_flag::CAN_UNINSTALL != 0,
        installed_at_ms: number(installed)?,
    })
}

fn profile(record: &[Option<String>]) -> Option<Profile> {
    let [serial, flags] = record else {
        return None;
    };
    let flags: u32 = number(flags)?;
    Some(Profile {
        id: ProfileId(number(serial)?),
        work: flags & profile_flag::WORK != 0,
        paused: flags & profile_flag::PAUSED != 0,
        locked: flags & profile_flag::LOCKED != 0,
    })
}

fn number<T: std::str::FromStr>(field: &Option<String>) -> Option<T> {
    field.as_deref()?.parse().ok()
}

/// The bytes written as lowercase hex, two digits each.
fn hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    let digit = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    };
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

/// The app's accent (APPS-17), or the neutral one until its icon has been read. An accent that
/// doesn't parse never drops the app.
fn accent_of(field: &Option<String>) -> Rgb {
    number::<u32>(field)
        .filter(|rgb| *rgb <= 0x00FF_FFFF)
        .map_or(accent::NEUTRAL, Rgb::hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(values: &[Option<&str>]) -> Vec<Option<String>> {
        values
            .iter()
            .map(|value| value.map(str::to_owned))
            .collect()
    }

    /// A record for an available personal app, with `field` set to `value`.
    fn with(field: usize, value: Option<&'static str>) -> Vec<Option<&'static str>> {
        let mut record = vec![
            Some("mail"),
            Some("mail.Main"),
            Some("Mail"),
            Some("0"),
            Some("1"),
            Some(""),
            Some("2b"),
            Some("0"),
        ];
        record[field] = value;
        record
    }

    #[test]
    fn apps_are_read_field_by_field() {
        // APPS-01, APPS-09, APPS-20, L10N-04
        let records = fields(&[
            Some("com.example.mail"),
            Some("com.example.mail.Inbox"),
            Some("Mail"),
            Some("0"),
            Some("3"),
            Some("15352629"),
            Some("2b3d00"),
            Some("1763251200000"),
            Some("com.example.docs"),
            Some("com.example.docs.Main"),
            Some("Docs"),
            Some("10"),
            Some("4"),
            Some(""),
            Some(""),
            Some("0"),
        ]);

        let apps = apps(&records);

        assert_eq!(
            apps,
            [
                App {
                    id: AppId {
                        profile: ProfileId(0),
                        package: "com.example.mail".into(),
                        activity: "com.example.mail.Inbox".into(),
                    },
                    label: "Mail".into(),
                    sort_key: vec![0x2B, 0x3D, 0x00],
                    // APPS-17: 0xEA4335, from its icon.
                    accent: Rgb::hex(0xEA4335),
                    available: true,
                    work: false,
                    can_uninstall: true,
                    installed_at_ms: 1_763_251_200_000,
                },
                App {
                    id: AppId {
                        profile: ProfileId(10),
                        package: "com.example.docs".into(),
                        activity: "com.example.docs.Main".into(),
                    },
                    label: "Docs".into(),
                    sort_key: Vec::new(),
                    // Its icon hasn't been read yet.
                    accent: accent::NEUTRAL,
                    available: false,
                    work: true,
                    can_uninstall: false,
                    installed_at_ms: 0,
                },
            ]
        );
    }

    #[test]
    fn an_app_without_a_label_is_named_by_its_package() {
        for label in [None, Some(""), Some("  ")] {
            let records = fields(&with(2, label));

            assert_eq!(apps(&records)[0].label, "mail");
        }
    }

    #[test]
    fn records_that_dont_parse_are_left_out() {
        let mut values: Vec<Option<&str>> = Vec::new();
        for (field, broken) in [
            (0, None),
            (1, Some("")),
            (3, Some("-1")),
            (4, Some("x")),
            (3, None),
            // A collation key that isn't lowercase hex, two digits a byte (L10N-04).
            (6, None),
            (6, Some("2b3")),
            (6, Some("2B")),
            (6, Some("zz")),
            // An install time that isn't a number of milliseconds (APPS-06).
            (7, None),
            (7, Some("-5")),
            (7, Some("soon")),
        ] {
            values.extend(with(field, broken));
        }
        values.extend(with(2, Some("Mail")));
        // An incomplete record at the end.
        values.extend([Some("b"), Some("b.Main")]);

        let apps = apps(&fields(&values));

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].label, "Mail");
    }

    #[test]
    fn an_accent_that_doesnt_parse_is_neutral_and_keeps_the_app() {
        // APPS-17
        for accent in [None, Some(""), Some("x"), Some("-1"), Some("16777216")] {
            let records = fields(&with(5, accent));

            let apps = apps(&records);

            assert_eq!(apps.len(), 1);
            assert_eq!(apps[0].accent, accent::NEUTRAL, "{accent:?}");
        }
        let white = fields(&with(5, Some("16777215")));
        assert_eq!(apps(&white)[0].accent, Rgb::WHITE);
    }

    #[test]
    fn collation_keys_keep_their_byte_order() {
        // L10N-04: comparing the keys byte by byte is the locale's order.
        assert_eq!(hex("00ff7f80"), Some(vec![0x00, 0xFF, 0x7F, 0x80]));
        assert_eq!(hex(""), Some(Vec::new()));
        assert!(hex("0a").unwrap() < hex("0a01").unwrap());
        assert!(hex("0aff").unwrap() < hex("0b").unwrap());
    }

    #[test]
    fn profiles_say_whether_they_are_work_paused_and_locked() {
        // APPS-09, APPS-18, APPS-20
        let records = fields(&[
            Some("0"),
            Some("0"),
            Some("10"),
            Some("3"),
            Some("11"),
            Some("5"),
            Some("x"),
            Some("1"),
        ]);

        assert_eq!(
            profiles(&records),
            [
                Profile {
                    id: ProfileId(0),
                    work: false,
                    paused: false,
                    locked: false,
                },
                Profile {
                    id: ProfileId(10),
                    work: true,
                    paused: true,
                    locked: false,
                },
                Profile {
                    id: ProfileId(11),
                    work: true,
                    paused: false,
                    locked: true,
                },
            ]
        );
    }
}
