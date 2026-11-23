//! The reliability rules of ADR 0043 that a scan can check, so the platform's own stability
//! reporting counts every crash, and no crash comes from code MeeGone didn't write (REL-07,
//! ARCH-09):
//!
//! - the shipped Java never replaces the handler that reports an uncaught exception, never
//!   kills a process or halts the VM, and ends its own process only where [`EXIT_ALLOWED`]
//!   says: a handler or a kill could hide a crash from the platform's reports;
//! - the Gradle build applies only the plugins in [`PLUGINS_ALLOWED`] and adds no library, so
//!   the APK holds no crash-reporting or other third-party SDK; the release APK, as built,
//!   holds only MeeGone's own classes (`release_apk_violations` in [`crate::security`]);
//! - the Rust app's `faults` feature, its bugs made to order, is turned on only in the debug
//!   build, and only for the Rust app (`check` in the crate root keeps it to dev-dependencies
//!   elsewhere).
//!
//! That panics unwind, which containing a failed feature needs, is checked by the compiler:
//! `meegone-core-state` doesn't build with `panic = "abort"`. `tests/reliability.rs` runs these
//! rules on this repository; `tools/fault-check.sh` checks on a device that a failed part
//! leaves the rest working.

use std::fs;
use std::path::Path;

use crate::Violation;
use crate::a11y::{java_code_only, line_of};
use crate::security::words;

/// Java APIs that could keep a crash from the platform's reports, or end the process as a
/// crash does, as `(name, what it is)`. Each is matched as a whole word, outside comments and
/// strings.
pub const UNREPORTED: [(&str, &str); 4] = [
    (
        "setDefaultUncaughtExceptionHandler",
        "replacing the handler that reports a crash",
    ),
    (
        "setUncaughtExceptionHandler",
        "replacing the handler that reports a crash",
    ),
    ("killProcess", "killing a process"),
    ("halt", "halting the VM"),
];

/// The shipped Java files that may end MeeGone's process with `exit`, as `(file, why)`. Every
/// one must, so the list stays true.
pub const EXIT_ALLOWED: [(&str, &str); 1] = [(
    "android/app/src/main/java/io/github/twonet/meegone/Restart.java",
    "Restart MeeGone, on a card saying a part stopped working: it ends with code 0, which the \
     platform records as the app exiting itself, never as a crash (ARCH-09, ADR 0043)",
)];

/// The Gradle plugins the build may apply, as `(id, why)`. Every one must be applied, so the
/// list stays true.
pub const PLUGINS_ALLOWED: [(&str, &str); 1] = [(
    "com.android.application",
    "builds the APK from the Java host and the Rust library (ADR 0005)",
)];

/// The Rust app's feature that makes bugs to order (ADR 0043).
pub const FAULTS: &str = "faults";

/// Where the shipped Java `sources`, as `(file, text)`, could keep a crash from the platform's
/// reports or end the process outside [`EXIT_ALLOWED`]; and which entries of `exit_allowed`
/// end nothing.
pub fn java_violations(
    sources: &[(String, String)],
    exit_allowed: &[(&str, &str)],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut exiting = Vec::new();
    for (file, text) in sources {
        let code = java_code_only(text);
        for (name, what) in UNREPORTED {
            for at in words(&code, name) {
                violations.push(Violation(format!(
                    "`{file}:{}` uses `{name}`, {what}: the platform's stability reports must \
                     see every crash, and nothing else as one (REL-07, ADR 0043)",
                    line_of(&code, at)
                )));
            }
        }
        for at in words(&code, "exit") {
            exiting.push(file.as_str());
            if !exit_allowed.iter().any(|(allowed, _)| allowed == file) {
                violations.push(Violation(format!(
                    "`{file}:{}` ends the process with `exit`; only Restart MeeGone does, so a \
                     part that stops is never a crash and a crash is never hidden (REL-07, \
                     ADR 0043)",
                    line_of(&code, at)
                )));
            }
        }
    }
    for (file, _) in exit_allowed {
        if !exiting.contains(file) {
            violations.push(Violation(format!(
                "EXIT_ALLOWED lets `{file}` end the process, but it doesn't any more; remove \
                 the entry (REL-07)"
            )));
        }
    }
    violations
}

/// Where the Gradle build scripts, as `(file, text)`, apply a plugin outside
/// `plugins_allowed`, add a library, or turn on the Rust app's `faults` feature for a build
/// other than debug; and which entries of `plugins_allowed` apply nothing.
pub fn gradle_violations(
    scripts: &[(String, String)],
    plugins_allowed: &[(&str, &str)],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut applied = Vec::new();
    for (file, text) in scripts {
        let code = kotlin_code_only(text);
        for (index, line) in code.lines().enumerate() {
            let at = index + 1;
            for (call, from) in line.match_indices("id(\"") {
                let rest = &line[call + from.len()..];
                let Some(id) = rest.split('"').next() else {
                    continue;
                };
                applied.push(id.to_owned());
                if !plugins_allowed.iter().any(|(allowed, _)| *allowed == id) {
                    violations.push(Violation(format!(
                        "`{file}:{at}` applies the plugin `{id}`; only {} may, so no SDK, \
                         such as a crash reporter, comes into the APK (REL-07, ADR 0043)",
                        names(plugins_allowed)
                    )));
                }
            }
            for word in ["dependencies", "alias", "implementation", "classpath"] {
                if words(line, word).next().is_some() {
                    violations.push(Violation(format!(
                        "`{file}:{at}` uses `{word}`: the Android app adds no library, so no \
                         SDK, such as a crash reporter, comes into the APK (DEPENDENCIES.md, \
                         REL-07, ADR 0043)"
                    )));
                }
            }
        }
        // A string naming the feature counts only on the line that limits it to debug.
        for (index, line) in text.lines().enumerate() {
            let quoted =
                line.contains(&format!("/{FAULTS}\"")) || line.contains(&format!("\"{FAULTS}\""));
            if quoted && !line.contains("buildType == \"debug\"") {
                violations.push(Violation(format!(
                    "`{file}:{}` turns on the Rust app's `{FAULTS}` feature outside the debug \
                     build; a release has no bugs made to order (SEC-06, ADR 0043)",
                    index + 1
                )));
            }
        }
    }
    for (id, _) in plugins_allowed {
        if !applied.iter().any(|applied| applied == id) {
            violations.push(Violation(format!(
                "PLUGINS_ALLOWED lets the build apply `{id}`, but it doesn't any more; remove \
                 the entry (REL-07)"
            )));
        }
    }
    violations
}

/// The plugins of `allowed`, for a message.
fn names(allowed: &[(&str, &str)]) -> String {
    allowed
        .iter()
        .map(|(id, _)| format!("`{id}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Kotlin `text` with its comments blanked, keeping every line where it was.
pub(crate) fn kotlin_code_only(text: &str) -> String {
    let mut code = String::with_capacity(text.len());
    let mut in_block = false;
    for line in text.lines() {
        let mut kept = String::new();
        let mut rest = line;
        loop {
            if in_block {
                match rest.find("*/") {
                    Some(end) => {
                        rest = &rest[end + 2..];
                        in_block = false;
                    }
                    None => break,
                }
            } else {
                let line_comment = rest.find("//");
                let block = rest.find("/*");
                match (line_comment, block) {
                    (Some(at), Some(start)) if at < start => {
                        kept.push_str(&rest[..at]);
                        break;
                    }
                    (Some(at), None) => {
                        kept.push_str(&rest[..at]);
                        break;
                    }
                    (_, Some(start)) => {
                        kept.push_str(&rest[..start]);
                        rest = &rest[start + 2..];
                        in_block = true;
                    }
                    (None, None) => {
                        kept.push_str(rest);
                        break;
                    }
                }
            }
        }
        code.push_str(&kept);
        code.push('\n');
    }
    code
}

/// The Gradle build scripts under `android/`, as `(file, text)`, outside what Gradle builds.
pub fn gradle_scripts(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut scripts = Vec::new();
    collect_scripts(root, &root.join("android"), &mut scripts)?;
    scripts.sort();
    Ok(scripts)
}

fn collect_scripts(
    root: &Path,
    dir: &Path,
    scripts: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|error| format!("{}: {error}", dir.display()))?
            .path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            if !matches!(name.as_ref(), "build" | ".gradle" | "gradle") {
                collect_scripts(root, &path, scripts)?;
            }
        } else if name.ends_with(".gradle.kts") {
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let file = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            scripts.push((file, text));
        }
    }
    Ok(())
}

/// Where the release APK's `bytes` hold the debug build's fault hook, which ships in no
/// release (SEC-06, ADR 0043). Its native library is stored uncompressed, so its exported
/// names read as they are; that `HomeActivity`'s start is found shows they do.
pub fn release_library_violations(bytes: &[u8]) -> Vec<Violation> {
    let holds = |name: &str| {
        bytes
            .windows(name.len())
            .any(|window| window == name.as_bytes())
    };
    let mut violations = Vec::new();
    if !holds("Java_io_github_twonet_meegone_HomeActivity_nativeStart") {
        violations.push(Violation(
            "the release APK's native library wasn't found uncompressed, so its fault hook \
             can't be looked for (ADR 0043)"
                .to_owned(),
        ));
    } else if holds("Java_io_github_twonet_meegone_DebugHooks_nativeArmFault") {
        violations.push(Violation(
            "the release APK's native library has the fault hook, `nativeArmFault`: the Rust \
             app's `faults` feature is for the debug build alone (SEC-06, ADR 0043)"
                .to_owned(),
        ));
    }
    violations
}

/// The classes in the release APK, as `dexdump` lists them, that aren't MeeGone's own: a
/// library's or an SDK's, such as a crash reporter's (REL-07, ADR 0043).
pub fn foreign_classes(classes: &std::collections::BTreeSet<String>) -> Vec<Violation> {
    classes
        .iter()
        .filter(|class| !class.starts_with("io.github.twonet.meegone."))
        .map(|class| {
            Violation(format!(
                "the release APK holds `{class}`, which isn't MeeGone's own: the app adds no \
                 library or SDK, such as a crash reporter (DEPENDENCIES.md, REL-07, ADR 0043)"
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn java(file: &str, text: &str) -> Vec<(String, String)> {
        vec![(file.to_owned(), text.to_owned())]
    }

    const RESTART: &str = "android/app/src/main/java/io/github/twonet/meegone/Restart.java";

    #[test]
    fn an_uncaught_exception_handler_or_a_kill_is_reported() {
        let sources = java(
            "Home.java",
            "class Home {\n\
             void a() { Thread.setDefaultUncaughtExceptionHandler(null); }\n\
             void b() { android.os.Process.killProcess(1); }\n\
             void c() { Runtime.getRuntime().halt(1); }\n\
             // Thread.setDefaultUncaughtExceptionHandler in a comment\n\
             String d = \"killProcess in a string\";\n\
             }\n",
        );

        let found = java_violations(&sources, &[]);

        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].0.contains("Home.java:2"));
        assert!(found[1].0.contains("killProcess"));
        assert!(found[2].0.contains("halt"));
    }

    #[test]
    fn exit_outside_the_restart_is_reported() {
        let mut sources = java(
            RESTART,
            "class Restart { void now() { System.exit(0); } }\n",
        );
        sources.extend(java(
            "Other.java",
            "class Other { void x() { System.exit(1); } }\n",
        ));

        let found = java_violations(&sources, &EXIT_ALLOWED);

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.contains("Other.java:1"));
    }

    #[test]
    fn an_unused_exit_allowance_is_reported() {
        let found = java_violations(&java(RESTART, "class Restart {}\n"), &EXIT_ALLOWED);

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.contains("doesn't any more"));
    }

    #[test]
    fn a_gradle_dependency_or_another_plugin_is_reported() {
        let ok = "plugins {\n    id(\"com.android.application\") // the APK\n}\n\
                  /* dependencies { implementation(\"x\") } */\n";
        assert!(gradle_violations(&java("build.gradle.kts", ok), &PLUGINS_ALLOWED).is_empty());

        let bad = "plugins {\n    id(\"com.android.application\")\n    \
                   id(\"com.google.gms.google-services\")\n    alias(libs.plugins.x)\n}\n\
                   dependencies {\n    implementation(\"com.example:crash:1.0\")\n}\n";
        let found = gradle_violations(&java("build.gradle.kts", bad), &PLUGINS_ALLOWED);

        assert_eq!(found.len(), 4, "{found:?}");
        assert!(found[0].0.contains("com.google.gms.google-services"));
        assert!(found[1].0.contains("`alias`"));
        assert!(found[2].0.contains("`dependencies`"));
        assert!(found[3].0.contains("`implementation`"));
    }

    #[test]
    fn faults_outside_the_debug_build_and_an_unused_plugin_allowance_are_reported() {
        let debug_only = "id(\"com.android.application\")\n\
                          features = if (variant.buildType == \"debug\") listOf(\"meegone-launcher/faults\") else listOf()\n";
        assert!(gradle_violations(&java("a.gradle.kts", debug_only), &PLUGINS_ALLOWED).is_empty());

        let always = "features = listOf(\"meegone-launcher/faults\")\n";
        let found = gradle_violations(&java("a.gradle.kts", always), &PLUGINS_ALLOWED);

        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].0.contains("outside the debug build"));
        assert!(found[1].0.contains("PLUGINS_ALLOWED"));
    }

    #[test]
    fn the_fault_hook_in_the_release_library_is_reported() {
        let start = b"..Java_io_github_twonet_meegone_HomeActivity_nativeStart..";
        let hook = b"Java_io_github_twonet_meegone_DebugHooks_nativeArmFault";

        assert!(release_library_violations(start).is_empty());
        assert_eq!(
            release_library_violations(&[start.as_slice(), hook].concat()).len(),
            1
        );
        // Compressed, nothing can be read: that is reported too.
        assert_eq!(release_library_violations(b"deflated").len(), 1);
    }

    #[test]
    fn a_class_outside_meegones_package_is_reported() {
        let classes = [
            "io.github.twonet.meegone.HomeActivity",
            "io.github.twonet.meegone.R",
            "com.example.crash.Reporter",
        ]
        .map(str::to_owned)
        .into();

        let found = foreign_classes(&classes);

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0.contains("com.example.crash.Reporter"));
    }
}
