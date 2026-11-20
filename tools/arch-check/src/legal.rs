//! The legal and branding rules of ADR 0037:
//!
//! - the product is named "MeeGone Launcher", and its text and store listing don't use
//!   third-party names, except in the non-affiliation statement (LEGAL-01, LEGAL-05);
//! - every bundled asset is recorded in ASSETS.md with its source and licence (LEGAL-02);
//! - every bundled licence text is shown in About (LEGAL-06);
//! - the store listing carries the privacy notice and the non-affiliation statement word for
//!   word (PRIV-04, LEGAL-05).
//!
//! The rules are pure functions over text and file lists; [`bundled_assets`] and
//! [`text_resources`] and [`listing_files`] list the files. `tests/legal.rs` runs them on this repository.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Violation, code_rows, files_under, relative};

/// The product's name (LEGAL-01).
pub const PRODUCT_NAME: &str = "MeeGone Launcher";
/// The non-affiliation statement, word for word (LEGAL-05). It is the only place third-party
/// names may appear.
pub const LEGAL_05: &str = "MeeGone Launcher is an independent project and is not affiliated \
                            with or endorsed by Nokia or Microsoft.";
/// Names MeeGone may not use (LEGAL-01, SRS C6). They are matched as whole words in any case,
/// so "MeeGone" isn't "MeeGo". Bing isn't one: MeeGone must name the image source to show its
/// attribution (LEGAL-03), and it does so only to say where the image comes from (ADR 0037).
pub const THIRD_PARTY_MARKS: [&str; 5] = ["Nokia", "Microsoft", "MeeGo", "Harmattan", "N9"];

/// The Android app's source sets, relative to the repository root (ADR 0005).
pub const ANDROID_SOURCES: &str = "android/app/src";
/// The store listing, in the fastlane metadata layout (ADR 0037).
pub const LISTING: &str = "fastlane/metadata/android/en-US";
/// Google Play's limits, in characters. F-Droid's are no tighter.
pub const TITLE_MAX: usize = 30;
pub const SHORT_DESCRIPTION_MAX: usize = 80;
pub const FULL_DESCRIPTION_MAX: usize = 4000;

/// Resource directories that hold text configuration, such as strings, colours, themes and the
/// backup rules, rather than assets. They are named by their type before any qualifier, as in
/// `values-night`.
const CONFIGURATION: [&str; 2] = ["values", "xml"];

/// One row of ASSETS.md's table of bundled files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// The file, relative to the repository root.
    pub path: String,
    pub source: String,
    pub licence: String,
}

/// The store listing's fields.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub title: String,
    pub short_description: String,
    pub full_description: String,
}

/// Where `file` names a third-party mark outside the non-affiliation statement (LEGAL-01). XML
/// comments are ignored, because no user sees them.
pub fn mark_violations(file: &str, text: &str) -> Vec<Violation> {
    let text = without_xml_comments(text).replace(LEGAL_05, "");
    THIRD_PARTY_MARKS
        .iter()
        .filter(|mark| has_word(&text, mark))
        .map(|mark| {
            Violation(format!(
                "`{file}` names {mark} outside the non-affiliation statement; MeeGone may not use \
                 third-party names (LEGAL-01)"
            ))
        })
        .collect()
}

/// Whether `word` is in `text` as a whole word, in any case.
fn has_word(text: &str, word: &str) -> bool {
    // ASCII lowering keeps every byte offset, so the matches index the text as it is.
    let text = text.to_ascii_lowercase();
    let word = word.to_ascii_lowercase();
    text.match_indices(&word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

fn without_xml_comments(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        kept.push_str(&rest[..start]);
        rest = rest[start..]
            .find("-->")
            .map_or("", |end| &rest[start + end + 3..]);
    }
    kept.push_str(rest);
    kept
}

/// The text of the `<string>` resource called `name` in `xml`, as Android reads it: XML entities
/// and backslash escapes resolved, runs of white space made one space.
pub fn string_resource(xml: &str, name: &str) -> Option<String> {
    let key = format!("name=\"{name}\"");
    without_xml_comments(xml)
        .split("<string ")
        .skip(1)
        .find_map(|element| {
            let (attributes, rest) = element.split_once('>')?;
            if !attributes
                .split_whitespace()
                .any(|attribute| attribute == key)
            {
                return None;
            }
            Some(unescape(rest.split_once("</string>")?.0))
        })
}

fn unescape(value: &str) -> String {
    let value = value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => text.push('\n'),
            Some('t') => text.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                text.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some(other) => text.push(other),
            None => {}
        }
    }
    text
}

/// The rows of ASSETS.md's tables, `| File | Kind | Source | Licence |`, whose first cell is a
/// path written as `` `path` ``.
pub fn listed_assets(markdown: &str) -> Vec<Asset> {
    code_rows(markdown)
        .into_iter()
        .map(|(path, cells)| {
            let cell = |i: usize| cells.get(i).cloned().unwrap_or_default();
            Asset {
                path,
                source: cell(1),
                licence: cell(2),
            }
        })
        .collect()
}

/// Every bundled file ASSETS.md doesn't record with its source and licence, and every file it
/// lists that isn't bundled any more (LEGAL-02, ADR 0037).
pub fn asset_violations(bundled: &BTreeSet<String>, listed: &[Asset]) -> Vec<Violation> {
    let mut violations = BTreeSet::new();
    let paths: BTreeSet<&str> = listed.iter().map(|asset| asset.path.as_str()).collect();
    for path in bundled.iter().filter(|path| !paths.contains(path.as_str())) {
        violations.insert(Violation(format!(
            "`{path}` is bundled, but ASSETS.md doesn't record its source and licence (LEGAL-02)"
        )));
    }
    for asset in listed {
        if !bundled.contains(&asset.path) {
            violations.insert(Violation(format!(
                "ASSETS.md lists `{}`, which isn't bundled any more",
                asset.path
            )));
        } else if asset.source.is_empty() || asset.licence.is_empty() {
            violations.insert(Violation(format!(
                "ASSETS.md lists `{}` without its source and licence (LEGAL-02)",
                asset.path
            )));
        }
    }
    violations.into_iter().collect()
}

/// Every licence text among the `bundled` files that `about` doesn't show as `R.raw.<name>`
/// (LEGAL-06). Licence texts are raw resources named `licence_*`.
pub fn unshown_licences(bundled: &BTreeSet<String>, about: &str) -> Vec<Violation> {
    bundled
        .iter()
        .filter_map(|path| {
            let (directory, file) = path.rsplit_once('/')?;
            let name = file.strip_prefix("licence_")?.split('.').next()?;
            directory
                .ends_with("/res/raw")
                .then(|| format!("R.raw.licence_{name}"))
                .filter(|reference| !about.contains(reference.as_str()))
                .map(|reference| {
                    Violation(format!(
                        "`{path}` is bundled but About doesn't show it: add `{reference}` to the \
                         licences in HomeViews.java (LEGAL-06)"
                    ))
                })
        })
        .collect()
}

/// Where the store listing breaks LEGAL-01, LEGAL-05 or PRIV-04, or a store's length limits.
/// `privacy_notice` is About's, which the listing repeats word for word.
pub fn listing_violations(listing: &Listing, privacy_notice: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let title = listing.title.trim();
    if title != PRODUCT_NAME {
        violations.push(Violation(format!(
            "the store listing's title is \"{title}\", not \"{PRODUCT_NAME}\" (LEGAL-01)"
        )));
    }
    for (field, text, max) in [
        ("title", &listing.title, TITLE_MAX),
        (
            "short description",
            &listing.short_description,
            SHORT_DESCRIPTION_MAX,
        ),
        (
            "full description",
            &listing.full_description,
            FULL_DESCRIPTION_MAX,
        ),
    ] {
        let length = text.trim().chars().count();
        if length == 0 || length > max {
            violations.push(Violation(format!(
                "the store listing's {field} has {length} characters; it needs 1 to {max}"
            )));
        }
    }
    let full = words(&listing.full_description);
    if !full.contains(LEGAL_05) {
        violations.push(Violation(
            "the store listing's full description doesn't carry the non-affiliation statement \
             word for word (LEGAL-05)"
                .to_owned(),
        ));
    }
    if privacy_notice.trim().is_empty() || !full.contains(&words(privacy_notice)) {
        violations.push(Violation(
            "the store listing's full description doesn't carry About's privacy notice word for \
             word (PRIV-04)"
                .to_owned(),
        ));
    }
    violations
}

/// `text` with every run of white space made one space, so a wrapped paragraph still matches.
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every file the APK bundles as an asset, relative to `root`: whatever is in the source sets'
/// `res` directories, apart from text configuration such as `values*` and `xml`, and in their
/// `assets` directories. Hidden files are skipped, as the Android build skips them.
pub fn bundled_assets(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = Vec::new();
    for source_set in directories(&root.join(ANDROID_SOURCES))? {
        files_under(&source_set.join("assets"), &mut files)?;
        for resources in directories(&source_set.join("res"))? {
            let name = resources.file_name().unwrap_or_default().to_string_lossy();
            let kind = name.split('-').next().unwrap_or_default();
            if !CONFIGURATION.contains(&kind) {
                files_under(&resources, &mut files)?;
            }
        }
    }
    Ok(files
        .iter()
        .filter(|file| {
            !file
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .map(|file| relative(root, file))
        .collect())
}

/// Every XML file of text resources in the source sets, `res/values*/*.xml`, relative to
/// `root`: the text MeeGone shows (L10N-01).
pub fn text_resources(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = Vec::new();
    for source_set in directories(&root.join(ANDROID_SOURCES))? {
        for resources in directories(&source_set.join("res"))? {
            let name = resources.file_name().unwrap_or_default().to_string_lossy();
            if name.split('-').next() == Some("values") {
                files_under(&resources, &mut files)?;
            }
        }
    }
    Ok(files
        .iter()
        .filter(|file| file.extension().is_some_and(|extension| extension == "xml"))
        .map(|file| relative(root, file))
        .collect())
}

/// Every text file of the store listing, in all its folders, relative to `root`.
pub fn listing_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = Vec::new();
    files_under(&root.join(LISTING), &mut files)?;
    Ok(files
        .iter()
        .filter(|file| file.extension().is_some_and(|extension| extension == "txt"))
        .map(|file| relative(root, file))
        .collect())
}

/// The directories directly in `dir`, or none if it doesn't exist.
pub(crate) fn directories(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in
        fs::read_dir(dir).map_err(|error| format!("couldn't list {}: {error}", dir.display()))?
    {
        let path = entry
            .map_err(|error| format!("couldn't list {}: {error}", dir.display()))?
            .path();
        if path.is_dir() {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(text: &str) -> Vec<String> {
        mark_violations("f", text)
            .into_iter()
            .map(|Violation(message)| message)
            .collect()
    }

    #[test]
    fn third_party_marks_are_found_as_whole_words_in_any_case() {
        let found = marks("A NOKIA-style n9 look, MeeGo-like, from microsoft's Harmattan.");

        assert_eq!(found.len(), THIRD_PARTY_MARKS.len(), "{found:?}");
    }

    #[test]
    fn meegone_and_words_that_contain_a_mark_pass() {
        assert!(marks("MeeGone Launcher, meegone, N90, AN9, Nokian tyres").is_empty());
    }

    #[test]
    fn the_non_affiliation_statement_and_xml_comments_may_name_marks() {
        let text = format!("<!-- Like the N9's MeeGo. --><string name=\"a\">{LEGAL_05}</string>");

        assert!(marks(&text).is_empty());
        assert_eq!(marks(&format!("{LEGAL_05} Nokia")).len(), 1);
    }

    #[test]
    fn string_resources_are_read_as_android_reads_them() {
        let xml = r#"<resources>
    <!-- <string name="a">in a comment</string> -->
    <string name="ab">Not this one</string>
    <string name="a" translatable="false">It\'s   one
        &amp; \"two\"\nthree…</string>
</resources>"#;

        assert_eq!(
            string_resource(xml, "a").as_deref(),
            Some("It's one & \"two\"\nthree…")
        );
        assert_eq!(string_resource(xml, "b"), None);
    }

    fn asset(path: &str, source: &str, licence: &str) -> Asset {
        Asset {
            path: path.to_owned(),
            source: source.to_owned(),
            licence: licence.to_owned(),
        }
    }

    #[test]
    fn listed_assets_are_read_from_tables_whose_first_cell_is_a_path() {
        let markdown = "\
| File | Kind | Source | Licence |
|---|---|---|---|
| `res/raw/a.txt` | Licence text | apache.org | Apache-2.0 |
| `res/raw/b.txt` | Icon |  |  |
| not a path | x | y | z |
";

        assert_eq!(
            listed_assets(markdown),
            vec![
                asset("res/raw/a.txt", "apache.org", "Apache-2.0"),
                asset("res/raw/b.txt", "", ""),
            ]
        );
    }

    #[test]
    fn every_bundled_asset_needs_a_row_with_its_source_and_licence() {
        let bundled = BTreeSet::from(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let listed = [
            asset("a", "Original", "Project"),
            asset("b", "", "MIT"),
            asset("gone", "Original", "Project"),
        ];

        let violations = asset_violations(&bundled, &listed);

        assert_eq!(violations.len(), 3, "{violations:?}");
        let text = format!("{violations:?}");
        assert!(text.contains("`b` without its source"));
        assert!(text.contains("`c` is bundled"));
        assert!(text.contains("`gone`, which isn't bundled"));
    }

    #[test]
    fn every_bundled_licence_text_must_be_shown_in_about() {
        let bundled = BTreeSet::from([
            "app/src/main/res/raw/licence_mit.txt".to_owned(),
            "app/src/main/res/raw/licence_zlib.txt".to_owned(),
            "app/src/main/res/raw/chime.ogg".to_owned(),
        ]);

        let violations = unshown_licences(&bundled, "{ R.raw.licence_mit, }");

        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].0.contains("R.raw.licence_zlib"));
    }

    fn listing(full: &str) -> Listing {
        Listing {
            title: "MeeGone Launcher\n".to_owned(),
            short_description: "Three views, one swipe apart.".to_owned(),
            full_description: full.to_owned(),
        }
    }

    const NOTICE: &str = "No accounts, no tracking.";

    #[test]
    fn a_listing_with_the_name_the_notice_and_the_statement_passes() {
        let full = format!("Intro.\n\nNo accounts,\nno tracking.\n\n{LEGAL_05}\n");

        assert_eq!(listing_violations(&listing(&full), NOTICE), vec![]);
    }

    #[test]
    fn a_listing_without_the_notice_or_the_statement_is_rejected() {
        let violations = listing_violations(&listing("Intro."), NOTICE);

        let text = format!("{violations:?}");
        assert_eq!(violations.len(), 2, "{text}");
        assert!(text.contains("LEGAL-05") && text.contains("PRIV-04"));
    }

    #[test]
    fn a_listing_with_another_title_or_too_long_a_field_is_rejected() {
        let full = format!("{NOTICE} {LEGAL_05}");
        let mut listing = listing(&full);
        listing.title = "MeeGo Launcher".to_owned();
        listing.short_description = "x".repeat(SHORT_DESCRIPTION_MAX + 1);

        let violations = listing_violations(&listing, NOTICE);

        let text = format!("{violations:?}");
        assert_eq!(violations.len(), 2, "{text}");
        assert!(text.contains("LEGAL-01") && text.contains("short description has 81"));
    }
}
