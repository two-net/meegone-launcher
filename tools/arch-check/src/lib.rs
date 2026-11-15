//! Checks the workspace's dependency graph against the architecture rules in ARCHITECTURE.md:
//!
//! - one-way layers, UI → state → data (ARCH-02);
//! - features depend only on core, and only the shell puts features together (ARCH-03);
//! - every third-party crate is listed with a reason in DEPENDENCIES.md (ARCH-08).
//!
//! The rules are pure functions over a [`Graph`]; [`load`] reads the graph with
//! `cargo metadata`. `tests/workspace.rs` runs them on this workspace, so CI fails on any
//! violation.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The areas a layered crate may belong to: the shared core and the six features (ARCH-03).
pub const AREAS: [&str; 7] = [
    "core",
    "apps",
    "events",
    "recent",
    "lockimage",
    "settings",
    "shell",
];
/// The package name of the app, the root package (ARCH-01).
pub const APP: &str = "meegone-launcher";

const CORE: &str = "core";
const SHELL: &str = "shell";
const PREFIX: &str = "meegone-";

/// A layer, lowest first (ARCH-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Data,
    State,
    Ui,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// `meegone-<area>-<layer>`, in `crates/<area>/<layer>`.
    Layered { area: String, layer: Layer },
    /// The root package: the one installable app.
    App,
    /// A development tool in `tools/`. Never part of the app.
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// `None` if the crate fits none of the roles.
    pub role: Option<Role>,
}

/// `from` depends on `to`, as a normal, dev or build dependency. The rules are the same for
/// every kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub members: Vec<Member>,
    /// Dependencies between workspace members.
    pub internal: Vec<Dependency>,
    /// Dependencies on crates from outside the workspace.
    pub external: Vec<Dependency>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation(pub String);

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Works out a member's role from its package name and where it lives.
pub fn role(name: &str, is_root: bool, in_tools: bool) -> Option<Role> {
    if is_root {
        return (name == APP).then_some(Role::App);
    }
    if in_tools {
        return Some(Role::Tool);
    }
    let (area, layer) = name.strip_prefix(PREFIX)?.rsplit_once('-')?;
    let layer = match layer {
        "data" => Layer::Data,
        "state" => Layer::State,
        "ui" => Layer::Ui,
        _ => return None,
    };
    AREAS.contains(&area).then(|| Role::Layered {
        area: area.to_owned(),
        layer,
    })
}

/// Every rule `graph` breaks. `listed` holds the third-party crates in DEPENDENCIES.md.
pub fn check(graph: &Graph, listed: &BTreeSet<String>) -> Vec<Violation> {
    let mut violations = BTreeSet::new();
    let role_of = |name: &str| {
        graph
            .members
            .iter()
            .find(|member| member.name == name)
            .and_then(|member| member.role.as_ref())
    };

    for member in graph.members.iter().filter(|member| member.role.is_none()) {
        violations.insert(Violation(format!(
            "`{}` fits no role: name app crates `meegone-<area>-<layer>` with an area from \
             {AREAS:?} and a layer of data, state or ui, or put tools in tools/",
            member.name
        )));
    }

    for dependency in &graph.internal {
        if let (Some(from), Some(to)) = (role_of(&dependency.from), role_of(&dependency.to)) {
            if let Some(rule) = broken_rule(from, to) {
                violations.insert(Violation(format!(
                    "`{}` → `{}`: {rule}",
                    dependency.from, dependency.to
                )));
            }
        }
    }

    for dependency in &graph.external {
        if !listed.contains(&dependency.to) {
            violations.insert(Violation(format!(
                "`{}` uses the third-party crate `{}`, which DEPENDENCIES.md doesn't list with \
                 a reason (ARCH-08)",
                dependency.from, dependency.to
            )));
        }
    }
    let used: BTreeSet<&str> = graph.external.iter().map(|d| d.to.as_str()).collect();
    for name in listed.iter().filter(|name| !used.contains(name.as_str())) {
        violations.insert(Violation(format!(
            "DEPENDENCIES.md lists `{name}`, which no crate uses any more"
        )));
    }

    violations.into_iter().collect()
}

/// The rule a dependency from `from` to `to` breaks, if any.
fn broken_rule(from: &Role, to: &Role) -> Option<&'static str> {
    match (from, to) {
        (Role::Tool, _) => None,
        (_, Role::App) => Some("nothing may depend on the app crate"),
        (_, Role::Tool) => Some("the app's crates may not depend on development tools"),
        (Role::App, Role::Layered { area, .. }) => (area != SHELL && area != CORE).then_some(
            "the app crate may depend only on shell and core crates; only the shell puts \
             features together (ARCH-03)",
        ),
        (
            Role::Layered {
                area: from_area,
                layer: from_layer,
            },
            Role::Layered {
                area: to_area,
                layer: to_layer,
            },
        ) => {
            if to_layer > from_layer {
                Some("depends on a higher layer; dependencies go UI → state → data (ARCH-02)")
            } else if *from_layer as u8 - *to_layer as u8 > 1 {
                Some("skips a layer; UI uses state, and state uses data (ARCH-02)")
            } else if from_area != to_area && to_area != CORE && from_area != SHELL {
                Some(
                    "one feature depends on another; features depend only on core, and only \
                     the shell puts features together (ARCH-03)",
                )
            } else {
                None
            }
        }
    }
}

/// The crate names in the first column of DEPENDENCIES.md's tables, written as `` `name` ``.
pub fn listed_crates(markdown: &str) -> BTreeSet<String> {
    markdown
        .lines()
        .filter_map(|line| line.trim().strip_prefix('|')?.split('|').next())
        .filter_map(|cell| cell.trim().strip_prefix('`')?.strip_suffix('`'))
        .map(str::to_owned)
        .collect()
}

/// Reads the dependency graph of the workspace whose root manifest is `manifest`, using
/// `cargo metadata --no-deps` (local only, no network).
pub fn load(manifest: &Path) -> Result<Graph, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .map_err(|error| format!("couldn't run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("couldn't read cargo metadata: {error}"))?;
    graph_from_metadata(&metadata)
}

fn graph_from_metadata(metadata: &Value) -> Result<Graph, String> {
    let text = |value: &Value, key: &str| {
        value[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("cargo metadata has no `{key}`"))
    };
    let root = PathBuf::from(text(metadata, "workspace_root")?);
    let tools = root.join("tools");
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no `packages`")?;

    let mut members = Vec::new();
    let mut dependencies = Vec::new();
    for package in packages {
        let name = text(package, "name")?;
        let manifest = PathBuf::from(text(package, "manifest_path")?);
        let dir = manifest.parent().unwrap_or(&manifest);
        members.push(Member {
            role: role(&name, dir == root.as_path(), dir.starts_with(&tools)),
            name: name.clone(),
        });
        for dependency in package["dependencies"].as_array().into_iter().flatten() {
            dependencies.push(Dependency {
                from: name.clone(),
                to: text(dependency, "name")?,
            });
        }
    }

    let names: BTreeSet<&str> = members.iter().map(|m| m.name.as_str()).collect();
    let (internal, external) = dependencies
        .into_iter()
        .partition(|dependency| names.contains(dependency.to.as_str()));
    Ok(Graph {
        members,
        internal,
        external,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A graph of the named crates. `meegone-launcher` is the app and `tool-*` are tools.
    fn graph(edges: &[(&str, &str)], external: &[(&str, &str)]) -> Graph {
        let mut names: Vec<&str> = edges.iter().flat_map(|(from, to)| [*from, *to]).collect();
        names.extend(external.iter().map(|(from, _)| *from));
        names.sort_unstable();
        names.dedup();
        let dependency = |(from, to): &(&str, &str)| Dependency {
            from: (*from).to_owned(),
            to: (*to).to_owned(),
        };
        Graph {
            members: names
                .iter()
                .map(|name| Member {
                    name: (*name).to_owned(),
                    role: role(name, *name == APP, name.starts_with("tool-")),
                })
                .collect(),
            internal: edges.iter().map(dependency).collect(),
            external: external.iter().map(dependency).collect(),
        }
    }

    fn violations(edges: &[(&str, &str)]) -> Vec<Violation> {
        check(&graph(edges, &[]), &BTreeSet::new())
    }

    fn assert_rejected(edge: (&str, &str), rule: &str) {
        let found = violations(&[edge]);
        assert!(
            found.len() == 1 && found[0].0.contains(rule),
            "{edge:?} should break `{rule}`, found {found:?}"
        );
    }

    #[test]
    fn allowed_dependencies_pass() {
        let allowed = [
            ("meegone-apps-ui", "meegone-apps-state"),
            ("meegone-apps-state", "meegone-apps-data"),
            ("meegone-apps-ui", "meegone-core-ui"),
            ("meegone-apps-ui", "meegone-core-state"),
            ("meegone-apps-state", "meegone-core-state"),
            ("meegone-apps-state", "meegone-core-data"),
            ("meegone-core-ui", "meegone-core-state"),
            ("meegone-shell-ui", "meegone-events-ui"),
            ("meegone-shell-ui", "meegone-lockimage-ui"),
            ("meegone-shell-state", "meegone-apps-state"),
            ("meegone-shell-state", "meegone-apps-data"),
            ("meegone-launcher", "meegone-shell-ui"),
            ("meegone-launcher", "meegone-core-data"),
            ("tool-check", "meegone-launcher"),
        ];

        let found = violations(&allowed);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_feature_depending_on_another_feature_is_rejected() {
        assert_rejected(
            ("meegone-events-state", "meegone-apps-state"),
            "one feature depends on another",
        );
        assert_rejected(
            ("meegone-events-ui", "meegone-lockimage-ui"),
            "one feature depends on another",
        );
    }

    #[test]
    fn data_depending_on_ui_is_rejected() {
        assert_rejected(
            ("meegone-lockimage-data", "meegone-core-ui"),
            "depends on a higher layer",
        );
    }

    #[test]
    fn state_depending_on_ui_is_rejected() {
        assert_rejected(
            ("meegone-apps-state", "meegone-apps-ui"),
            "depends on a higher layer",
        );
    }

    #[test]
    fn ui_skipping_state_to_reach_data_is_rejected() {
        assert_rejected(("meegone-apps-ui", "meegone-apps-data"), "skips a layer");
        assert_rejected(("meegone-shell-ui", "meegone-core-data"), "skips a layer");
    }

    #[test]
    fn core_depending_on_a_feature_is_rejected() {
        assert_rejected(
            ("meegone-core-state", "meegone-apps-data"),
            "one feature depends on another",
        );
    }

    #[test]
    fn only_the_shell_may_be_used_by_the_app_crate() {
        assert_rejected(
            ("meegone-launcher", "meegone-apps-state"),
            "may depend only on shell and core",
        );
    }

    #[test]
    fn nothing_may_depend_on_the_app_or_on_tools() {
        assert_rejected(
            ("meegone-shell-ui", "meegone-launcher"),
            "nothing may depend on the app",
        );
        assert_rejected(("meegone-core-data", "tool-check"), "development tools");
    }

    #[test]
    fn crates_that_fit_no_role_are_rejected() {
        for name in ["meegone-weather-state", "meegone-apps-view", "helpers"] {
            let found = violations(&[(name, "meegone-core-data")]);
            assert!(
                found
                    .iter()
                    .any(|v| v.0.contains(&format!("`{name}` fits no role"))),
                "{name}: {found:?}"
            );
        }
    }

    #[test]
    fn third_party_crates_must_be_listed() {
        let graph = graph(&[], &[("meegone-core-data", "leftpad")]);

        let unlisted = check(&graph, &BTreeSet::new());
        let listed = check(&graph, &BTreeSet::from(["leftpad".to_owned()]));

        assert_eq!(unlisted.len(), 1);
        assert!(unlisted[0].0.contains("doesn't list"));
        assert!(listed.is_empty(), "{listed:?}");
    }

    #[test]
    fn listed_crates_nothing_uses_are_reported() {
        let found = check(&Graph::default(), &BTreeSet::from(["oldcrate".to_owned()]));

        assert_eq!(found.len(), 1);
        assert!(found[0].0.contains("no crate uses"));
    }

    #[test]
    fn listed_crates_are_read_from_the_first_table_column() {
        let markdown = "\
# Third-party dependencies

| Crate | Used by | Reason |
|---|---|---|
| `serde_json` | `tools/arch-check` | Reads `cargo metadata` |
| not code | x | y |
";

        assert_eq!(
            listed_crates(markdown),
            BTreeSet::from(["serde_json".to_owned()])
        );
    }
}
