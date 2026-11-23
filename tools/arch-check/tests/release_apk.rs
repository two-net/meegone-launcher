//! Runs the manifest rules of ADR 0042 on the release APK as Gradle built it, and checks that it
//! holds none of the debug build's own classes (SEC-03, SEC-04, SEC-06). It needs the APK, so it
//! is ignored by default; CI's Android job runs it after the build:
//!
//! ```sh
//! (cd android && ./gradlew assembleRelease)
//! cargo test -p arch-check --test release_apk -- --ignored
//! ```
//!
//! It reads the APK with the SDK's own `aapt2` and `dexdump`, from the newest build tools in
//! `$ANDROID_HOME` (or `$ANDROID_SDK_ROOT`, or `sdk.dir` in `android/local.properties`).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use arch_check::a11y::java_files;
use arch_check::security::{debug_only_classes, release_apk_violations};

const APK: &str = "android/app/build/outputs/apk/release/app-release-unsigned.apk";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Android SDK, from the environment or from the Gradle build's `local.properties`.
fn sdk() -> PathBuf {
    let from_env = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .find_map(|name| env::var_os(name).filter(|value| !value.is_empty()));
    if let Some(sdk) = from_env {
        return PathBuf::from(sdk);
    }
    let properties = fs::read_to_string(root().join("android/local.properties"))
        .expect("set ANDROID_HOME, or build once so android/local.properties names the SDK");
    properties
        .lines()
        .find_map(|line| line.strip_prefix("sdk.dir="))
        .map(PathBuf::from)
        .expect("android/local.properties names no sdk.dir")
}

/// `tool` from the newest build tools installed.
fn build_tool(tool: &str) -> PathBuf {
    let build_tools = sdk().join("build-tools");
    let version = |path: &Path| -> Vec<u32> {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .split(['.', '-'])
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    let newest = fs::read_dir(&build_tools)
        .unwrap_or_else(|error| panic!("{}: {error}", build_tools.display()))
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.join(tool).is_file())
        .max_by_key(|path| version(path))
        .unwrap_or_else(|| panic!("no build tools in {} have {tool}", build_tools.display()));
    newest.join(tool)
}

/// What `tool` prints for `args`, which must succeed.
fn run(tool: &str, args: &[&str]) -> String {
    let path = build_tool(tool);
    let output = Command::new(&path)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert!(
        output.status.success(),
        "{tool} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("the tool prints UTF-8")
}

#[test]
#[ignore = "needs the release APK: run `./gradlew assembleRelease` in android/ first"]
fn the_release_apk_asks_only_for_srs_8_1s_access_and_holds_no_debug_feature() {
    // SEC-03, SEC-04, SEC-06; issue #41's acceptance criteria.
    let apk = root().join(APK);
    assert!(
        apk.is_file(),
        "{APK} wasn't found: run `./gradlew assembleRelease` in android/ first"
    );
    let apk = apk.to_string_lossy();
    let manifest = run(
        "aapt2",
        &["dump", "xmltree", "--file", "AndroidManifest.xml", &apk],
    );
    let dexdump = run("dexdump", &["-h", &apk]);
    let debug_only =
        debug_only_classes(&java_files(&root()).expect("the Java sources can be listed"));
    assert!(
        manifest.contains("E: application") && dexdump.contains("Class descriptor"),
        "the release APK's manifest or classes weren't read"
    );
    assert!(
        debug_only.iter().any(|class| class.ends_with(".PerfProbe")),
        "the debug build's own classes weren't found: {debug_only:?}"
    );

    let violations = release_apk_violations(&manifest, &dexdump, &debug_only);
    assert!(
        violations.is_empty(),
        "the release APK breaks the security rules (see ADR 0042):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}
