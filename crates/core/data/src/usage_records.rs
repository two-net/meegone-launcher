//! Recent app use as Android's `Bridge` reports it (REC-03, ADR 0023).
//!
//! `Bridge.recentUses(since)` returns a flat `String[]` of records, each [`USE_FIELDS`] fields
//! long, in the order below. The fields must stay in step with `Bridge.java`. They are parsed
//! here, apart from JNI, so the parsing is tested without a device. A record that doesn't parse
//! is left out, never fatal.
//!
//! Usage history stays in memory: nothing here logs or keeps it (PRIV-03).

use crate::model::{ProfileId, RecentUse};

/// `recentUses(since)`: package, the class of the activity last resumed, the profile's serial
/// number, and when the app was last used, in decimal milliseconds since the Unix epoch.
pub(crate) const USE_FIELDS: usize = 4;

/// The uses in `fields`, in order.
pub(crate) fn uses(fields: &[Option<String>]) -> Vec<RecentUse> {
    fields
        .chunks_exact(USE_FIELDS)
        .filter_map(recent_use)
        .collect()
}

fn recent_use(record: &[Option<String>]) -> Option<RecentUse> {
    let [package, activity, serial, last_used] = record else {
        return None;
    };
    let package = package.as_deref().filter(|text| !text.is_empty())?;
    Some(RecentUse {
        profile: ProfileId(number(serial)?),
        package: package.to_owned(),
        activity: activity.clone().unwrap_or_default(),
        last_used_ms: number(last_used)?,
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

    /// A record for Mail, with `field` set to `value`.
    fn with(field: usize, value: Option<&'static str>) -> Vec<Option<&'static str>> {
        let mut record = vec![
            Some("com.example.mail"),
            Some("com.example.mail.Inbox"),
            Some("0"),
            Some("1794873600000"),
        ];
        record[field] = value;
        record
    }

    #[test]
    fn uses_are_read_field_by_field() {
        // REC-03
        let records = fields(&[
            Some("com.example.mail"),
            Some("com.example.mail.Inbox"),
            Some("0"),
            Some("1794873600000"),
            Some("com.example.maps"),
            None,
            Some("10"),
            Some("5"),
        ]);

        assert_eq!(
            uses(&records),
            [
                RecentUse {
                    profile: ProfileId(0),
                    package: "com.example.mail".into(),
                    activity: "com.example.mail.Inbox".into(),
                    last_used_ms: 1_794_873_600_000,
                },
                RecentUse {
                    profile: ProfileId(10),
                    package: "com.example.maps".into(),
                    activity: String::new(),
                    last_used_ms: 5,
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
            (2, None),
            (2, Some("-1")),
            (2, Some("work")),
            (3, None),
            (3, Some("-5")),
            (3, Some("soon")),
        ] {
            values.extend(with(field, broken));
        }
        values.extend(with(1, Some("com.example.mail.Compose")));
        // An incomplete record at the end.
        values.extend([Some("com.example.maps"), Some("com.example.maps.Main")]);

        let uses = uses(&fields(&values));

        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].activity, "com.example.mail.Compose");
    }
}
