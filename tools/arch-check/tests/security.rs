//! Runs the security and privacy rules of ADR 0042 on the Android host's sources. CI fails if
//! any is broken. `release_apk.rs` runs the manifest rules again on the release APK as built.

use std::path::{Path, PathBuf};

use arch_check::Violation;
use arch_check::l10n::{read, shipped_java_files};
use arch_check::security::{
    Manifest, NETWORK_ALLOWED, java_violations, manifest_violations, network_config_violations,
    shipped_xml_files, xml_violations,
};

const MANIFEST: &str = "android/app/src/main/AndroidManifest.xml";
const NETWORK_CONFIG: &str = "android/app/src/main/res/xml/network_security_config.xml";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assert_none(violations: &[Violation], broken: &str) {
    assert!(
        violations.is_empty(),
        "{broken} (see ADR 0042):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}

#[test]
fn the_manifest_asks_only_for_srs_8_1s_access_and_exports_only_its_entries() {
    // SEC-01, SEC-03, SEC-04, SEC-05, SEC-06
    let manifests: Vec<String> = shipped_xml_files(&root())
        .expect("the XML files can be listed")
        .into_iter()
        .filter(|file| file.ends_with("/AndroidManifest.xml"))
        .collect();
    // Only main's: one in the release source set would merge into the release build, and
    // `release_apk.rs` checks the result.
    assert_eq!(manifests, [MANIFEST], "the shipped manifests");

    let manifest = Manifest::from_source(&read(&root(), MANIFEST));
    assert!(
        manifest.components.len() >= 3,
        "the manifest's components weren't read"
    );
    assert_none(
        &manifest_violations(MANIFEST, &manifest, true),
        "the manifest asks for more access, or lets other apps reach more, than it may",
    );
}

#[test]
fn the_network_config_refuses_cleartext_and_trusts_only_the_system() {
    // SEC-01
    assert_none(
        &network_config_violations(NETWORK_CONFIG, &read(&root(), NETWORK_CONFIG)),
        "the network security config allows more than HTTPS with the system's certificates",
    );
}

#[test]
fn the_shipped_java_stays_off_forbidden_apis_and_connects_only_from_the_bridge() {
    // SEC-02, SEC-05, PRIV-01, PRIV-02, PRIV-03
    let files = shipped_java_files(&root()).expect("the Java sources can be listed");
    assert!(
        files.iter().any(|file| file.ends_with("/Bridge.java")),
        "the Java sources weren't found"
    );
    let sources: Vec<(String, String)> = files
        .into_iter()
        .map(|file| {
            let text = read(&root(), &file);
            (file, text)
        })
        .collect();

    assert_none(
        &java_violations(&sources, &NETWORK_ALLOWED),
        "the shipped Java uses an API MeeGone rules out, connects outside the bridge, or logs \
         an exception",
    );
}

#[test]
fn no_shipped_xml_turns_text_into_links() {
    // SEC-02
    let violations: Vec<Violation> = shipped_xml_files(&root())
        .expect("the XML files can be listed")
        .iter()
        .flat_map(|file| xml_violations(file, &read(&root(), file)))
        .collect();

    assert_none(&violations, "shipped XML turns text into links");
}
