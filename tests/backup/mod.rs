//! What the system's backup keeps of MeeGone, read from the Android host's own rules, so the
//! tests back up and restore exactly the files a device would (APPS-21, ADR 0018).

use std::collections::{BTreeMap, BTreeSet};

use meegone_core_data::StoredFile;
use meegone_core_data::fakes::FakeStorage;

/// The rules for Android 10 and 11.
pub const BACKUP_RULES: &str = include_str!("../../android/app/src/main/res/xml/backup_rules.xml");
/// The rules from Android 12, for backups to the cloud and for transfers between devices.
pub const DATA_EXTRACTION_RULES: &str =
    include_str!("../../android/app/src/main/res/xml/data_extraction_rules.xml");

/// The files the `section` element of `rules` includes. Every one must be in the app's files
/// folder, where `FileStorage` keeps MeeGone's files, and be one of them.
pub fn included(rules: &str, section: &str) -> BTreeSet<StoredFile> {
    let rules = without_comments(rules);
    let start = rules
        .find(&format!("<{section}"))
        .unwrap_or_else(|| panic!("no <{section}>"));
    let end = rules[start..]
        .find(&format!("</{section}>"))
        .unwrap_or_else(|| panic!("<{section}> isn't closed"));
    let body = &rules[start..start + end];
    assert!(!body.contains("<exclude"), "<{section}> excludes: {body}");
    body.split("<include")
        .skip(1)
        .map(|tag| {
            let tag = &tag[..tag.find('>').expect("a whole tag")];
            assert_eq!(attribute(tag, "domain"), "file", "{tag}");
            let path = attribute(tag, "path");
            *StoredFile::ALL
                .iter()
                .find(|file| file.path() == path)
                .unwrap_or_else(|| panic!("MeeGone stores no file {path:?}"))
        })
        .collect()
}

/// The files every rule backs up, after checking that each kind of backup keeps the same ones.
pub fn backed_up_files() -> BTreeSet<StoredFile> {
    let cloud = included(DATA_EXTRACTION_RULES, "cloud-backup");
    assert_eq!(included(DATA_EXTRACTION_RULES, "device-transfer"), cloud);
    assert_eq!(included(BACKUP_RULES, "full-backup-content"), cloud);
    cloud
}

/// A backup of `storage`: the files the rules keep, as they are.
pub fn back_up(storage: &FakeStorage) -> BTreeMap<StoredFile, Vec<u8>> {
    let kept = backed_up_files();
    let files = storage.files.lock().unwrap();
    files
        .iter()
        .filter(|(file, _)| kept.contains(file))
        .map(|(file, bytes)| (*file, bytes.clone()))
        .collect()
}

/// Restores `backup` into `storage`, as the system does when it installs MeeGone, before
/// MeeGone first runs.
pub fn restore(storage: &FakeStorage, backup: &BTreeMap<StoredFile, Vec<u8>>) {
    storage.files.lock().unwrap().extend(backup.clone());
}

fn without_comments(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let end = rest[start..].find("-->").expect("a closed comment");
        rest = &rest[start + end + 3..];
    }
    out.push_str(rest);
    out
}

/// The value of `name="…"` in `tag`.
fn attribute<'a>(tag: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let start = tag
        .find(&key)
        .unwrap_or_else(|| panic!("no {name} in {tag}"))
        + key.len();
    let end = tag[start..].find('"').expect("a closed attribute");
    &tag[start..start + end]
}
