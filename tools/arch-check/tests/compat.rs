//! Runs the compatibility rules of ADR 0044 on SRS.md, the Gradle build and the Android host's
//! shipped sources. CI fails if any is broken; `tools/compat-check.sh` checks the device and
//! navigation-mode matrix on each device.

use std::path::{Path, PathBuf};

use arch_check::Violation;
use arch_check::compat::{
    API_HIDING_ALLOWED, api_hiding_violations, floor_violations, lint_violations,
};
use arch_check::l10n::{read, shipped_java_files};
use arch_check::reliability::gradle_scripts;

const APP_SCRIPT: &str = "android/app/build.gradle.kts";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assert_none(violations: &[Violation], broken: &str) {
    assert!(
        violations.is_empty(),
        "{broken} (see ADR 0044):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}

#[test]
fn the_oldest_android_is_the_same_in_the_srs_the_apk_and_the_rust_library() {
    // COMP-01
    let srs = read(&root(), "SRS.md");
    let gradle = read(&root(), APP_SCRIPT);
    assert_none(
        &floor_violations(&srs, (APP_SCRIPT, &gradle)),
        "the oldest Android MeeGone runs on is set differently in different places",
    );
}

#[test]
fn a_call_newer_than_android_10_fails_the_release_build() {
    // COMP-01
    let scripts = gradle_scripts(&root()).expect("the Gradle scripts can be read");
    assert!(
        scripts.iter().any(|(file, _)| file == APP_SCRIPT),
        "the Gradle scripts weren't found"
    );
    assert_none(
        &lint_violations(&scripts),
        "lint's NewApi check doesn't fail the release build",
    );
}

#[test]
fn the_shipped_java_hides_no_call_from_lint_without_a_reason() {
    // COMP-01
    let sources: Vec<(String, String)> = shipped_java_files(&root())
        .expect("the Java sources can be listed")
        .into_iter()
        .map(|file| {
            let text = read(&root(), &file);
            (file, text)
        })
        .collect();
    assert!(
        sources
            .iter()
            .any(|(file, _)| file.ends_with("/HomeViews.java")),
        "the shipped Java wasn't read"
    );
    assert_none(
        &api_hiding_violations(&sources, &API_HIDING_ALLOWED),
        "the shipped Java hides calls from lint's NewApi",
    );
}
