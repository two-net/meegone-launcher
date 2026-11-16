//! The app catalogue as Android's `Bridge` reports it (ADR 0010).
//!
//! `Bridge.launchableApps()` and `Bridge.profiles()` return flat `String[]` records, each a
//! fixed number of fields, in the order below. The fields and flags must stay in step with
//! `Bridge.java`, as the frame slots do with `HomeViews.java`. They are parsed here, apart from
//! JNI, so the parsing is tested without a device. A record that doesn't parse is left out,
//! never fatal.

use crate::model::{App, AppId, Profile, ProfileId, Rgb};

/// `launchableApps()`: package, activity, label, profile serial number, [`app_flag`]s.
pub(crate) const APP_FIELDS: usize = 5;
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
}

/// Every app's accent colour until #10 derives it from the app's icon (APPS-17).
const NEUTRAL_ACCENT: Rgb = Rgb::hex(0x808080);

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
    let [package, activity, label, serial, flags] = record else {
        return None;
    };
    let package = package.as_deref().filter(|text| !text.is_empty())?;
    let activity = activity.as_deref().filter(|text| !text.is_empty())?;
    let flags = number(flags)?;
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
        accent: NEUTRAL_ACCENT,
        available: flags & app_flag::AVAILABLE != 0,
        work: flags & app_flag::WORK != 0,
        can_uninstall: flags & app_flag::CAN_UNINSTALL != 0,
        // When an app was installed arrives with the new-app dot (APPS-07, #11).
        installed_at_ms: 0,
    })
}

fn profile(record: &[Option<String>]) -> Option<Profile> {
    let [serial, flags] = record else {
        return None;
    };
    let flags = number(flags)?;
    Some(Profile {
        id: ProfileId(number(serial)?),
        work: flags & profile_flag::WORK != 0,
        paused: flags & profile_flag::PAUSED != 0,
    })
}

fn number(field: &Option<String>) -> Option<u32> {
    field.as_deref()?.parse().ok()
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

    #[test]
    fn apps_are_read_field_by_field() {
        // APPS-01, APPS-09, APPS-20
        let records = fields(&[
            Some("com.example.mail"),
            Some("com.example.mail.Inbox"),
            Some("Mail"),
            Some("0"),
            Some("3"),
            Some("com.example.docs"),
            Some("com.example.docs.Main"),
            Some("Docs"),
            Some("10"),
            Some("4"),
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
                    accent: NEUTRAL_ACCENT,
                    available: true,
                    work: false,
                    can_uninstall: true,
                    installed_at_ms: 0,
                },
                App {
                    id: AppId {
                        profile: ProfileId(10),
                        package: "com.example.docs".into(),
                        activity: "com.example.docs.Main".into(),
                    },
                    label: "Docs".into(),
                    accent: NEUTRAL_ACCENT,
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
            let records = fields(&[
                Some("com.example.tool"),
                Some("a.Main"),
                label,
                Some("0"),
                Some("1"),
            ]);

            assert_eq!(apps(&records)[0].label, "com.example.tool");
        }
    }

    #[test]
    fn records_that_dont_parse_are_left_out() {
        let good = [
            Some("mail"),
            Some("mail.Main"),
            Some("Mail"),
            Some("0"),
            Some("1"),
        ];
        let mut values: Vec<Option<&str>> = Vec::new();
        for broken in [
            [None, Some("a.Main"), Some("A"), Some("0"), Some("1")],
            [Some("a"), Some(""), Some("A"), Some("0"), Some("1")],
            [Some("a"), Some("a.Main"), Some("A"), Some("-1"), Some("1")],
            [Some("a"), Some("a.Main"), Some("A"), Some("0"), Some("x")],
            [Some("a"), Some("a.Main"), Some("A"), None, Some("1")],
        ] {
            values.extend(broken);
        }
        values.extend(good);
        // An incomplete record at the end.
        values.extend([Some("b"), Some("b.Main")]);

        let apps = apps(&fields(&values));

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].label, "Mail");
    }

    #[test]
    fn profiles_say_whether_they_are_work_and_paused() {
        // APPS-09, APPS-20
        let records = fields(&[
            Some("0"),
            Some("0"),
            Some("10"),
            Some("3"),
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
                },
                Profile {
                    id: ProfileId(10),
                    work: true,
                    paused: true,
                },
            ]
        );
    }
}
