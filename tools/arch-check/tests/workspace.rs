//! Runs the architecture rules on this workspace. CI fails if any rule is broken.

use std::path::Path;

use arch_check::{Role, check, listed_crates, load, os_access_violations};

#[test]
fn the_workspace_follows_the_architecture_rules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let graph = load(&root.join("Cargo.toml")).expect("cargo metadata runs");
    let listed = listed_crates(
        &std::fs::read_to_string(root.join("DEPENDENCIES.md")).expect("DEPENDENCIES.md exists"),
    );

    assert!(
        graph.members.iter().any(|m| m.role == Some(Role::App)),
        "the app crate wasn't found"
    );
    let violations = check(&graph, &listed);
    assert!(
        violations.is_empty(),
        "architecture rules broken (see ARCHITECTURE.md):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}

#[test]
fn only_platform_adapters_reach_the_operating_system() {
    // ARCH-04
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let violations = os_access_violations(&root).expect("the sources can be read");

    assert!(
        violations.is_empty(),
        "the operating system is reached outside a platform adapter (see ARCHITECTURE.md):\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}\n"))
            .collect::<String>()
    );
}
