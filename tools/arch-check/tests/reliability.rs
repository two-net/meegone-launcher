//! Runs the reliability rules of ADR 0043 on the Android host's sources and its Gradle build. CI
//! fails if any is broken. `release_apk.rs` checks the release APK as built holds only
//! MeeGone's own code.

use std::path::{Path, PathBuf};

use arch_check::Violation;
use arch_check::l10n::{read, shipped_java_files};
use arch_check::reliability::{
    EXIT_ALLOWED, PLUGINS_ALLOWED, gradle_scripts, gradle_violations, java_violations,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assert_none(violations: &[Violation], broken: &str) {
    assert!(
        violations.is_empty(),
        "{broken} (see ADR 0043):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}

#[test]
fn the_shipped_java_hides_no_crash_and_ends_the_process_only_to_restart() {
    // REL-07, ARCH-09
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
            .any(|(file, _)| file.ends_with("/Restart.java")),
        "the shipped Java wasn't read"
    );

    assert_none(
        &java_violations(&sources, &EXIT_ALLOWED),
        "the shipped Java could keep a crash from the platform's reports",
    );
}

#[test]
fn the_gradle_build_adds_no_library_or_plugin_and_no_faults_outside_debug() {
    // REL-07, SEC-06
    let scripts = gradle_scripts(&root()).expect("the Gradle scripts can be read");
    assert!(
        scripts
            .iter()
            .any(|(file, _)| file == "android/app/build.gradle.kts"),
        "the Gradle scripts weren't found: {:?}",
        scripts.iter().map(|(file, _)| file).collect::<Vec<_>>()
    );

    assert_none(
        &gradle_violations(&scripts, &PLUGINS_ALLOWED),
        "the Gradle build adds a library or a plugin, or bugs made to order to a release",
    );
}
