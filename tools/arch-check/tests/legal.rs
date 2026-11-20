//! Runs the legal and branding rules of ADR 0037 on this repository. CI fails if any is broken.

use std::fs;
use std::path::{Path, PathBuf};

use arch_check::Violation;
use arch_check::legal::{
    LEGAL_05, LISTING, Listing, PRODUCT_NAME, asset_violations, bundled_assets, listed_assets,
    listing_files, listing_violations, mark_violations, string_resource, text_resources,
    unshown_licences,
};

/// MeeGone's own text, in its default language.
const STRINGS: &str = "android/app/src/main/res/values/strings.xml";
/// Where About lists the licence texts it shows (LEGAL-06).
const ABOUT_LICENCES: &str = "android/app/src/main/java/io/github/twonet/meegone/HomeViews.java";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn string(name: &str) -> String {
    string_resource(&read(STRINGS), name).unwrap_or_else(|| panic!("{STRINGS} has no {name}"))
}

fn assert_none(violations: &[Violation], broken: &str) {
    assert!(
        violations.is_empty(),
        "{broken} (see ADR 0037):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}

#[test]
fn every_bundled_asset_is_in_the_inventory() {
    // LEGAL-02, and #36's asset inventory
    let bundled = bundled_assets(&root()).expect("the Android resources can be listed");
    let listed = listed_assets(&read("ASSETS.md"));

    assert_none(
        &asset_violations(&bundled, &listed),
        "the asset inventory is out of date",
    );
}

#[test]
fn every_bundled_licence_text_is_shown_in_about() {
    // LEGAL-06
    let bundled = bundled_assets(&root()).expect("the Android resources can be listed");

    assert_none(
        &unshown_licences(&bundled, &read(ABOUT_LICENCES)),
        "a licence text isn't shown in About",
    );
}

#[test]
fn the_product_name_and_about_say_what_legal_requires() {
    assert_eq!(string("app_name"), PRODUCT_NAME, "LEGAL-01");
    assert_eq!(string("about_independent"), LEGAL_05, "LEGAL-05");
}

#[test]
fn the_store_listing_says_what_legal_requires() {
    // LEGAL-01, LEGAL-05, PRIV-04
    let field = |name: &str| read(&format!("{LISTING}/{name}.txt"));
    let listing = Listing {
        title: field("title"),
        short_description: field("short_description"),
        full_description: field("full_description"),
    };

    assert_none(
        &listing_violations(&listing, &string("about_privacy_notice")),
        "the store listing is incomplete",
    );
}

#[test]
fn user_facing_text_has_no_third_party_marks() {
    // LEGAL-01, and #36's store-listing review
    let resources = text_resources(&root()).expect("the Android resources can be listed");
    let listing = listing_files(&root()).expect("the store listing can be listed");
    assert!(resources.contains(STRINGS), "{STRINGS} wasn't found");
    assert!(!listing.is_empty(), "the store listing wasn't found");
    let files: Vec<&String> = resources.iter().chain(&listing).collect();

    let violations: Vec<Violation> = files
        .iter()
        .flat_map(|file| mark_violations(file, &read(file)))
        .collect();

    assert_none(&violations, "third-party names are used");
}
