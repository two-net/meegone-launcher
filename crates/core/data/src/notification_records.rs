//! Active notifications as Android's `Bridge` reports them (EVT-04, ADR 0020).
//!
//! `Bridge.activeNotifications()` returns a flat `String[]` of records, each
//! [`NOTIFICATION_FIELDS`] fields long, in the order below. The fields and flags must stay in
//! step with `Bridge.java`. They are parsed here, apart from JNI, so the parsing is tested
//! without a device. A record that doesn't parse is left out, never fatal.
//!
//! The records hold notification content, so nothing here logs or keeps them (EVT-13).

use crate::model::{Notification, ProfileId};

/// `activeNotifications()`: key, package, the profile's serial number, the app's name as the
/// system gives it, title, text, when it was posted in decimal milliseconds since the Unix
/// epoch, and [`notification_flag`]s.
pub(crate) const NOTIFICATION_FIELDS: usize = 8;

/// The flags of one notification.
pub(crate) mod notification_flag {
    /// The user can remove it (EVT-07).
    pub const CLEARABLE: u32 = 1;
    /// Media playback, navigation or a running service (EVT-05).
    pub const ONGOING: u32 = 2;
    /// The summary of a bundle of the app's notifications (EVT-04).
    pub const SUMMARY: u32 = 4;
}

/// The notifications in `fields`, in order.
pub(crate) fn notifications(fields: &[Option<String>]) -> Vec<Notification> {
    fields
        .chunks_exact(NOTIFICATION_FIELDS)
        .filter_map(notification)
        .collect()
}

fn notification(record: &[Option<String>]) -> Option<Notification> {
    let [key, package, serial, app_label, title, text, posted, flags] = record else {
        return None;
    };
    let key = key.as_deref().filter(|text| !text.is_empty())?;
    let package = package.as_deref().filter(|text| !text.is_empty())?;
    let flags: u32 = number(flags)?;
    let trimmed = |field: &Option<String>| field.as_deref().unwrap_or_default().trim().to_owned();
    Some(Notification {
        key: key.to_owned(),
        profile: ProfileId(number(serial)?),
        package: package.to_owned(),
        app_label: trimmed(app_label),
        title: trimmed(title),
        text: trimmed(text),
        posted_at_ms: number(posted)?,
        clearable: flags & notification_flag::CLEARABLE != 0,
        ongoing: flags & notification_flag::ONGOING != 0,
        summary: flags & notification_flag::SUMMARY != 0,
    })
}

fn number<T: std::str::FromStr>(field: &Option<String>) -> Option<T> {
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

    /// A record for a clearable chat message, with `field` set to `value`.
    fn with(field: usize, value: Option<&'static str>) -> Vec<Option<&'static str>> {
        let mut record = vec![
            Some("0|chat|1|null|10001"),
            Some("com.example.chat"),
            Some("0"),
            Some("Chat"),
            Some("Anna"),
            Some("See you at eight"),
            Some("1794873600000"),
            Some("1"),
        ];
        record[field] = value;
        record
    }

    #[test]
    fn notifications_are_read_field_by_field() {
        // EVT-04, EVT-05, APPS-20
        let records = fields(&[
            Some("0|chat|1|null|10001"),
            Some("com.example.chat"),
            Some("0"),
            Some("Chat"),
            Some(" Anna "),
            Some("See you at eight"),
            Some("1794873600000"),
            Some("1"),
            Some("10|player|7|null|10002"),
            Some("com.example.player"),
            Some("10"),
            None,
            None,
            None,
            Some("5"),
            Some("6"),
        ]);

        let notifications = notifications(&records);

        assert_eq!(
            notifications,
            [
                Notification {
                    key: "0|chat|1|null|10001".into(),
                    profile: ProfileId(0),
                    package: "com.example.chat".into(),
                    app_label: "Chat".into(),
                    title: "Anna".into(),
                    text: "See you at eight".into(),
                    posted_at_ms: 1_794_873_600_000,
                    clearable: true,
                    ongoing: false,
                    summary: false,
                },
                Notification {
                    key: "10|player|7|null|10002".into(),
                    profile: ProfileId(10),
                    package: "com.example.player".into(),
                    app_label: String::new(),
                    title: String::new(),
                    text: String::new(),
                    posted_at_ms: 5,
                    clearable: false,
                    ongoing: true,
                    summary: true,
                },
            ]
        );
    }

    #[test]
    fn records_that_dont_parse_are_left_out() {
        let mut values: Vec<Option<&str>> = Vec::new();
        for (field, broken) in [
            (0, None),
            (0, Some("")),
            (1, None),
            (1, Some("")),
            (2, None),
            (2, Some("-1")),
            (6, None),
            (6, Some("-5")),
            (6, Some("soon")),
            (7, None),
            (7, Some("x")),
        ] {
            values.extend(with(field, broken));
        }
        values.extend(with(4, Some("Ben")));
        // An incomplete record at the end.
        values.extend([Some("k"), Some("com.example.mail")]);

        let notifications = notifications(&fields(&values));

        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].title, "Ben");
    }
}
