//! The compatibility rules of ADR 0044 that a scan can check, so MeeGone keeps running on
//! Android 10, its oldest version (COMP-01):
//!
//! - the oldest version is the same wherever it is set: the SRS's API level, the APK's
//!   `minSdk` and the platform cargo-ndk builds the Rust library for;
//! - the Gradle build keeps lint's `NewApi` check fatal, so a call newer than that version
//!   without its version check fails the release build, and nothing turns the check off;
//! - the shipped Java hides no call from that check, with `@TargetApi`, `@RequiresApi` or
//!   `@SuppressLint("NewApi")`, except where [`API_HIDING_ALLOWED`] says why its callers check
//!   the version.
//!
//! `tests/compat.rs` runs these rules on this repository; `tools/compat-check.sh` checks the
//! device and navigation-mode matrix on each device (COMP-02 to COMP-04).

use crate::Violation;
use crate::a11y::{closing_paren, is_word_char, java_code_only, line_of};
use crate::reliability::kotlin_code_only;
use crate::security::words;

/// The shipped Java methods that may hide their calls from `NewApi`, as `("file#method", why)`.
/// Every one must still be so annotated, so the list stays true.
pub const API_HIDING_ALLOWED: [(&str, &str); 2] = [
    (
        "android/app/src/main/java/io/github/twonet/meegone/HomeViews.java#clearOfCorners",
        "rounded corners are Android 12's: `safeArea`, its one caller, calls it only behind \
         `SDK_INT >= S` (ADR 0009)",
    ),
    (
        "android/app/src/main/java/io/github/twonet/meegone/HomeViews.java#cornerInset",
        "called only from `clearOfCorners`, itself only from Android 12 (ADR 0009)",
    ),
];

/// The annotations that hide a method's calls from lint's `NewApi`.
const API_HIDING: [&str; 3] = ["TargetApi", "RequiresApi", "SuppressLint"];

/// The Gradle settings that would let a release build past lint's checks, as `(text, what)`.
const LINT_OFF: [(&str, &str); 3] = [
    (
        "checkReleaseBuilds = false",
        "turns off lint's checks of the release build",
    ),
    (
        "abortOnError = false",
        "lets the build go on past lint's errors",
    ),
    ("baseline", "lets every problem in its file through"),
];

/// Where the SRS, as `srs`, and the Gradle build script `gradle`, as `(file, text)`, disagree
/// on the oldest Android: every "API level N" in the SRS, the script's `minSdk`, and the
/// platform it passes cargo-ndk with `-P`.
pub fn floor_violations(srs: &str, gradle: (&str, &str)) -> Vec<Violation> {
    let mut violations = Vec::new();
    let levels: Vec<u32> = srs
        .match_indices("API level ")
        .filter_map(|(at, found)| number(&srs[at + found.len()..]))
        .collect();
    let Some(&floor) = levels.first() else {
        return vec![Violation(
            "SRS.md names no API level, so the oldest Android MeeGone runs on can't be checked \
             (COMP-01, ADR 0044)"
                .to_owned(),
        )];
    };
    if levels.iter().any(|&level| level != floor) {
        violations.push(Violation(format!(
            "SRS.md names more than one oldest API level, {levels:?}; COMP-01 and section 2.5 \
             must agree (ADR 0044)"
        )));
    }

    let (file, text) = gradle;
    let code = kotlin_code_only(text);
    let min_sdk: Vec<(usize, Option<u32>)> = words(&code, "minSdk")
        .map(|at| {
            let rest = code[at + "minSdk".len()..].trim_start();
            (at, rest.strip_prefix('=').and_then(number))
        })
        .collect();
    if min_sdk.is_empty() {
        violations.push(Violation(format!(
            "`{file}` sets no `minSdk`; it must be {floor}, SRS.md's oldest API level (COMP-01, \
             ADR 0044)"
        )));
    }
    for (at, level) in min_sdk {
        if level != Some(floor) {
            violations.push(Violation(format!(
                "`{file}:{}` sets `minSdk` to {}, not {floor}, SRS.md's oldest API level \
                 (COMP-01, ADR 0044)",
                line_of(&code, at),
                shown(level)
            )));
        }
    }

    let platforms: Vec<(usize, Option<u32>)> = code
        .match_indices("\"-P\"")
        .map(|(at, found)| {
            let rest = code[at + found.len()..].trim_start();
            let level = rest
                .strip_prefix(',')
                .map(str::trim_start)
                .and_then(|rest| rest.strip_prefix('"'))
                .and_then(number);
            (at, level)
        })
        .collect();
    if platforms.is_empty() {
        violations.push(Violation(format!(
            "`{file}` passes cargo-ndk no `-P` platform; the Rust library must be built for API \
             level {floor}, as the APK is (COMP-01, ADR 0044)"
        )));
    }
    for (at, level) in platforms {
        if level != Some(floor) {
            violations.push(Violation(format!(
                "`{file}:{}` builds the Rust library for API level {}, not {floor}, SRS.md's \
                 oldest (COMP-01, ADR 0044)",
                line_of(&code, at),
                shown(level)
            )));
        }
    }
    violations
}

/// Where the Gradle build scripts, as `(file, text)`, leave lint's `NewApi` check short of
/// fatal, turn it off, or let a release build past lint's errors.
pub fn lint_violations(scripts: &[(String, String)]) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut fatal = false;
    for (file, text) in scripts {
        let code = kotlin_code_only(text);
        for (index, line) in code.lines().enumerate() {
            let at = index + 1;
            if line.contains("\"NewApi\"") {
                if words(line, "fatal").next().is_some() {
                    fatal = true;
                } else {
                    violations.push(Violation(format!(
                        "`{file}:{at}` makes lint's `NewApi` less than fatal; a call newer than \
                         Android 10 without its version check must fail the build (COMP-01, \
                         ADR 0044)"
                    )));
                }
            }
            for (setting, what) in LINT_OFF {
                let found = if setting.contains(' ') {
                    line.contains(setting)
                } else {
                    words(line, setting).next().is_some()
                };
                if found {
                    violations.push(Violation(format!(
                        "`{file}:{at}` sets `{setting}`, which {what}; `NewApi` must fail the \
                         release build (COMP-01, ADR 0044)"
                    )));
                }
            }
        }
    }
    if !fatal {
        violations.push(Violation(
            "No Gradle script makes lint's `NewApi` fatal (`lint { fatal += \"NewApi\" }`); \
             lint's vital checks leave it out, so a call newer than Android 10 would ship \
             (COMP-01, ADR 0044)"
                .to_owned(),
        ));
    }
    violations
}

/// Where the shipped Java `sources`, as `(file, text)`, hide a method's calls from `NewApi`
/// outside `allowed`; and which entries of `allowed` hide nothing.
pub fn api_hiding_violations(
    sources: &[(String, String)],
    allowed: &[(&str, &str)],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut hiding = Vec::new();
    for (file, text) in sources {
        let code = java_code_only(text);
        let lines: Vec<&str> = text.lines().collect();
        for name in API_HIDING {
            for at in words(&code, name) {
                if !code[..at].ends_with('@') {
                    continue;
                }
                let line = line_of(&code, at);
                if name == "SuppressLint"
                    && !lines
                        .get(line - 1)
                        .is_some_and(|text| text.contains("NewApi"))
                {
                    continue;
                }
                let method = method_after(&code, at + name.len());
                let key = format!("{file}#{method}");
                if allowed.iter().any(|(entry, _)| *entry == key) {
                    hiding.push(key);
                } else {
                    violations.push(Violation(format!(
                        "`{file}:{line}` hides `{method}`'s calls from lint's `NewApi` with \
                         `@{name}`; check the version where it is called instead, or add it to \
                         API_HIDING_ALLOWED with why every caller does (COMP-01, ADR 0044)"
                    )));
                }
            }
        }
    }
    for (entry, _) in allowed {
        if !hiding.iter().any(|key| key == entry) {
            violations.push(Violation(format!(
                "API_HIDING_ALLOWED lets `{entry}` hide its calls from `NewApi`, but it doesn't \
                 any more; remove the entry (COMP-01)"
            )));
        }
    }
    violations
}

/// The number `text` starts with, if it does.
fn number(text: &str) -> Option<u32> {
    let text = text.trim_start();
    let digits = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text[..digits].parse().ok()
}

fn shown(level: Option<u32>) -> String {
    level.map_or_else(|| "something else".to_owned(), |level| level.to_string())
}

/// The name of the method that Java `code`, from `after` an annotation's name on, annotates:
/// past its arguments and any other annotations, the word before the next `(`.
fn method_after(code: &str, after: usize) -> String {
    let mut rest = &code[after..];
    loop {
        rest = rest.trim_start();
        if let Some(arguments) = rest.strip_prefix('(') {
            rest = &arguments[(closing_paren(arguments) + 1).min(arguments.len())..];
        } else if let Some(annotation) = rest.strip_prefix('@') {
            let end = annotation
                .find(|c: char| !is_word_char(c) && c != '.')
                .unwrap_or(annotation.len());
            rest = &annotation[end..];
        } else {
            break;
        }
    }
    let Some(open) = rest.find('(') else {
        return String::new();
    };
    let before = rest[..open].trim_end();
    let start = before
        .rfind(|c: char| !is_word_char(c))
        .map_or(0, |at| at + 1);
    before[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRS: &str = "- Android 10 (API level 29) or later.\n\
                       | COMP-01 | MeeGone shall run on Android 10 (API level 29) and later. |\n";
    const GRADLE: &str = "android {\n    defaultConfig {\n        minSdk = 29 // Android 10\n    \
                          }\n    lint {\n        fatal += \"NewApi\"\n    }\n}\n\
                          commandLine(listOf(\"cargo\", \"ndk\", \"-P\", \"29\"))\n";

    fn script(text: &str) -> Vec<(String, String)> {
        vec![("app/build.gradle.kts".to_owned(), text.to_owned())]
    }

    fn java(text: &str) -> Vec<(String, String)> {
        vec![("A.java".to_owned(), text.to_owned())]
    }

    #[test]
    fn the_floor_agrees_everywhere() {
        assert!(floor_violations(SRS, ("build.gradle.kts", GRADLE)).is_empty());
    }

    #[test]
    fn a_floor_set_differently_anywhere_is_found() {
        let newer_apk = GRADLE.replace("minSdk = 29", "minSdk = 30");
        let found = floor_violations(SRS, ("build.gradle.kts", &newer_apk));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.contains("`minSdk` to 30"), "{found:?}");

        let newer_library = GRADLE.replace("\"-P\", \"29\"", "\"-P\", \"31\"");
        let found = floor_violations(SRS, ("build.gradle.kts", &newer_library));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.contains("API level 31"), "{found:?}");

        let srs = SRS.replacen("API level 29", "API level 30", 1);
        assert_eq!(
            floor_violations(&srs, ("build.gradle.kts", GRADLE)).len(),
            3,
            "SRS.md disagrees with itself, and with both of the build's"
        );
    }

    #[test]
    fn a_floor_set_nowhere_is_found() {
        assert_eq!(
            floor_violations("No level here.", ("build.gradle.kts", GRADLE)).len(),
            1
        );
        let nothing = "android {\n}\n";
        assert_eq!(
            floor_violations(SRS, ("build.gradle.kts", nothing)).len(),
            2,
            "no minSdk and no cargo-ndk platform"
        );
    }

    #[test]
    fn new_api_stays_fatal() {
        assert!(lint_violations(&script(GRADLE)).is_empty());

        let missing = GRADLE.replace("fatal += \"NewApi\"", "");
        assert_eq!(lint_violations(&script(&missing)).len(), 1);

        let commented = GRADLE.replace("fatal += \"NewApi\"", "// fatal += \"NewApi\"");
        assert_eq!(lint_violations(&script(&commented)).len(), 1);

        let disabled = GRADLE.replace("fatal += \"NewApi\"", "disable += \"NewApi\"");
        assert_eq!(
            lint_violations(&script(&disabled)).len(),
            2,
            "turned off, and so not fatal"
        );

        for off in [
            "checkReleaseBuilds = false",
            "abortOnError = false",
            "baseline = file(\"lint-baseline.xml\")",
        ] {
            let text = GRADLE.replace(
                "fatal += \"NewApi\"",
                &format!("fatal += \"NewApi\"\n{off}"),
            );
            assert_eq!(lint_violations(&script(&text)).len(), 1, "{off}");
        }
    }

    #[test]
    fn hiding_calls_from_new_api_needs_a_reason() {
        let source = "class A {\n    @TargetApi(Build.VERSION_CODES.S)\n    \
                      @SuppressWarnings(\"deprecation\")\n    private static int corner(int a) \
                      {\n        return a;\n    }\n}\n";
        let found = api_hiding_violations(&java(source), &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].0.contains("`A.java:2` hides `corner`'s"),
            "{found:?}"
        );
        assert!(api_hiding_violations(&java(source), &[("A.java#corner", "why")]).is_empty());

        let suppressed = "class A {\n    @SuppressLint(\"NewApi\")\n    void late() {}\n}\n";
        assert_eq!(api_hiding_violations(&java(suppressed), &[]).len(), 1);
        let required = "class A {\n    @RequiresApi(31) void late() {}\n}\n";
        assert_eq!(api_hiding_violations(&java(required), &[]).len(), 1);
    }

    #[test]
    fn other_annotations_and_mentions_hide_nothing() {
        let source = "class A {\n    @SuppressLint(\"WrongConstant\")\n    void a() {}\n    // \
                      @TargetApi(31) in a comment\n    String b = \"@RequiresApi\";\n}\n";
        assert!(api_hiding_violations(&java(source), &[]).is_empty());
    }

    #[test]
    fn an_entry_that_hides_nothing_is_found() {
        let found = api_hiding_violations(&java("class A {}\n"), &[("A.java#gone", "why")]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.contains("remove the entry"), "{found:?}");
    }
}
