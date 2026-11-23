//! Runs the localisation rules of ADR 0040 on this repository. CI fails if any is broken.

use std::path::{Path, PathBuf};

use arch_check::Violation;
use arch_check::l10n::{
    PROSE_ALLOWED, TEXT_ALLOWED, app_rust_files, java_violations, problem_violations, read,
    resource_violations, rust_violations, shipped_java_files, xml_files, xml_violations,
};
use arch_check::legal::text_resources;

/// MeeGone's own text, in its default language.
const STRINGS: &str = "android/app/src/main/res/values/strings.xml";
/// Every problem the Rust app can report, by its code.
const PROBLEM_RS: &str = "crates/core/data/src/problem.rs";
/// Where the host words each problem.
const PROBLEMS_JAVA: &str = "android/app/src/main/java/io/github/twonet/meegone/Problems.java";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Each of `files`, with its text.
fn sources(files: impl IntoIterator<Item = String>) -> Vec<(String, String)> {
    files
        .into_iter()
        .map(|file| {
            let text = read(&root(), &file);
            (file, text)
        })
        .collect()
}

fn assert_none(violations: &[Violation], broken: &str) {
    assert!(
        violations.is_empty(),
        "{broken} (see ADR 0040):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}

#[test]
fn the_host_writes_no_text_for_the_user() {
    // L10N-01, L10N-03
    let files = shipped_java_files(&root()).expect("the Java sources can be listed");
    assert!(
        files.iter().any(|file| file.ends_with("/HomeViews.java")),
        "the Java sources weren't found"
    );

    assert_none(
        &java_violations(&sources(files), &TEXT_ALLOWED),
        "the Java host writes text that can't be translated",
    );
}

#[test]
fn no_layout_or_manifest_attribute_holds_text() {
    // L10N-01
    let files = xml_files(&root()).expect("the XML files can be listed");
    assert!(
        files
            .iter()
            .any(|file| file.ends_with("main/AndroidManifest.xml")),
        "the manifest wasn't found"
    );
    let violations: Vec<Violation> = sources(files)
        .iter()
        .flat_map(|(file, xml)| xml_violations(file, xml))
        .collect();

    assert_none(&violations, "an XML attribute holds text, not a resource");
}

#[test]
fn the_rust_app_writes_no_words() {
    // L10N-01
    let files = app_rust_files(&root()).expect("the Rust sources can be listed");
    assert!(
        files.iter().any(|file| file == PROBLEM_RS),
        "the Rust sources weren't found"
    );

    assert_none(
        &rust_violations(&sources(files), &PROSE_ALLOWED),
        "the Rust app writes words for the user",
    );
}

#[test]
fn every_problem_is_worded_in_the_host() {
    // L10N-01
    assert_none(
        &problem_violations(
            &read(&root(), PROBLEM_RS),
            &read(&root(), PROBLEMS_JAVA),
            &read(&root(), STRINGS),
        ),
        "a Problem's code, its case in Problems.java and its string resource disagree",
    );
}

#[test]
fn every_string_resource_can_be_translated() {
    // L10N-01
    let files = text_resources(&root()).expect("the text resources can be listed");
    assert!(files.contains(STRINGS), "{STRINGS} wasn't found");
    let violations: Vec<Violation> = sources(files)
        .iter()
        .flat_map(|(file, xml)| resource_violations(file, xml))
        .collect();

    assert_none(&violations, "a string resource can't be translated well");
}
