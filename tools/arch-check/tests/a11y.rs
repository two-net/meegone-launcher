//! Runs the accessibility rules of ADR 0038 on the Java host. CI fails if any is broken.

use std::fs;
use std::path::{Path, PathBuf};

use arch_check::Violation;
use arch_check::a11y::{COLOUR_ALLOWED, colour_violations, java_files, text_size_violations};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every Java file of the host, with its text.
fn sources() -> Vec<(String, String)> {
    let files = java_files(&root()).expect("the Java sources can be listed");
    assert!(
        files.iter().any(|file| file.ends_with("/HomeViews.java")),
        "the Java sources weren't found"
    );
    files
        .into_iter()
        .map(|file| {
            let text = fs::read_to_string(root().join(&file))
                .unwrap_or_else(|error| panic!("{file}: {error}"));
            (file, text)
        })
        .collect()
}

fn assert_none(violations: &[Violation], broken: &str) {
    assert!(
        violations.is_empty(),
        "{broken} (see ADR 0038):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}

#[test]
fn every_colour_the_host_draws_comes_from_the_theme() {
    // A11Y-03, SRS 5.3
    assert_none(
        &colour_violations(&sources(), &COLOUR_ALLOWED),
        "the host draws a colour that isn't one of the theme's tokens",
    );
}

#[test]
fn every_text_size_follows_the_system_font_size() {
    // A11Y-04
    let violations: Vec<Violation> = sources()
        .iter()
        .flat_map(|(file, text)| text_size_violations(file, text))
        .collect();

    assert_none(
        &violations,
        "a text size doesn't scale with the system font size",
    );
}
