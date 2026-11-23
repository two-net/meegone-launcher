//! Checks the workspace against the architecture rules in ARCHITECTURE.md:
//!
//! - one-way layers, UI → state → data (ARCH-02);
//! - features depend only on core, and only the shell puts features together (ARCH-03);
//! - only platform adapters reach the operating system (ARCH-04);
//! - every third-party crate is listed with a reason in DEPENDENCIES.md (ARCH-08);
//! - test fakes are enabled only in dev-dependencies (SEC-06);
//! - in [`legal`], the legal and branding rules of ADR 0037: the product's name, no
//!   third-party names, every bundled asset in ASSETS.md, every licence in About, and the store
//!   listing (LEGAL-01 to LEGAL-06, PRIV-04);
//! - in [`a11y`], the accessibility rules of ADR 0038: every colour the Java host draws comes
//!   from the theme's tokens, and every text size follows the system font size (A11Y-03,
//!   A11Y-04);
//! - in [`l10n`], the localisation rules of ADR 0040: neither the Java host nor the Rust app
//!   writes text for the user outside string resources, no number reaches the screen in ASCII
//!   digits whatever the language, every `Problem` is worded, and every string resource can be
//!   translated (L10N-01, L10N-03);
//! - and, in [`security`], the security and privacy rules of ADR 0042: the manifest asks only
//!   for SRS 8.1's access and lets other apps reach only the home entry, the settings entry and
//!   the notification listener, the network config allows only HTTPS with the system's
//!   certificates, and the shipped Java stays off the APIs MeeGone rules out, connects only from
//!   the bridge and logs no exception (SEC-01 to SEC-06, PRIV-01 to PRIV-03).
//!
//! The rules are pure functions over a [`Graph`] and over source text; [`load`] reads the graph
//! with `cargo metadata`, and [`os_access_violations`] reads the app's sources.
//! `tests/workspace.rs`, `tests/legal.rs`, `tests/a11y.rs`, `tests/l10n.rs` and
//! `tests/security.rs` run them on this repository, so CI fails on any violation.
//! `tests/release_apk.rs` runs the manifest rules on the release APK once Gradle has built it.

pub mod a11y;
pub mod l10n;
pub mod legal;
pub mod security;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
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

/// The feature that turns on a crate's test fakes (SEC-06).
pub const FAKES: &str = "fakes";

/// The modules of `std` that reach the operating system. Only platform adapters may use them
/// (ARCH-04).
pub const OS_MODULES: [&str; 5] = ["env", "fs", "net", "os", "process"];
/// The types that read the clock. Only `SystemConfigAdapter` may (ARCH-04).
pub const CLOCKS: [&str; 2] = ["Instant", "SystemTime"];
/// The keyword that foreign code, such as Android's JNI, needs. Only platform adapters may use
/// it (ARCH-04, ADR 0005).
pub const UNSAFE: &str = "unsafe";
/// Source files, relative to the workspace root, that may reach the operating system directly
/// (ADR 0004, ADR 0005):
///
/// - `FileStorage`, which keeps the three persistent stores' files (REL-03);
/// - the JNI binding and the Android `SystemConfigAdapter`, which reads the clock;
/// - the app's Android entry points, which JNI calls.
pub const OS_ACCESS_ALLOWED: [&str; 4] = [
    "crates/core/data/src/storage/files.rs",
    "crates/core/data/src/android/jni.rs",
    "crates/core/data/src/android/system.rs",
    "src/android.rs",
];

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

/// `from` depends on `to`, as a normal, dev or build dependency. The layer and feature rules
/// are the same for every kind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dependency {
    pub from: String,
    pub to: String,
    /// A dev-dependency, used only by tests, examples and benchmarks.
    pub dev: bool,
    /// The features of `to` that this dependency turns on.
    pub features: Vec<String>,
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

    for dependency in &graph.internal {
        if !dependency.dev && dependency.features.iter().any(|f| f == FAKES) {
            violations.insert(Violation(format!(
                "`{}` turns on the `{FAKES}` feature of `{}` outside [dev-dependencies]; fakes \
                 are for tests only (SEC-06)",
                dependency.from, dependency.to
            )));
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

/// Every place the app's sources reach the operating system other than through a platform
/// adapter (ARCH-04): every `.rs` file in `crates/` and in the root package's `src/`, except
/// those in [`OS_ACCESS_ALLOWED`].
///
/// This is a source scan, so it guards against accidents rather than determined workarounds.
/// Cargo already keeps out third-party crates that aren't listed in DEPENDENCIES.md.
pub fn os_access_violations(root: &Path) -> Result<Vec<Violation>, String> {
    let mut files = Vec::new();
    for dir in ["crates", "src"] {
        rust_files(&root.join(dir), &mut files)?;
    }
    files.sort();

    let mut violations = Vec::new();
    for file in files {
        let relative = relative(root, &file);
        if OS_ACCESS_ALLOWED.contains(&relative.as_str()) {
            continue;
        }
        let source = fs::read_to_string(&file)
            .map_err(|error| format!("couldn't read {}: {error}", file.display()))?;
        for path in os_access(&source) {
            violations.push(Violation(format!(
                "`{relative}` uses `{path}`; only platform adapters may reach the operating \
                 system (ARCH-04)"
            )));
        }
    }
    Ok(violations)
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut all = Vec::new();
    files_under(dir, &mut all)?;
    files.extend(
        all.into_iter()
            .filter(|path| path.extension().is_some_and(|extension| extension == "rs")),
    );
    Ok(())
}

/// Adds every file in `dir` and its subdirectories to `files`. A missing `dir` adds nothing.
fn files_under(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries =
        fs::read_dir(dir).map_err(|error| format!("couldn't list {}: {error}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|error| format!("couldn't list {}: {error}", dir.display()))?
            .path();
        if path.is_dir() {
            files_under(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// `file`'s path from `root`, with `/` between its parts on every platform.
fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The paths in `source` that reach the operating system: any path into one of
/// [`OS_MODULES`], including inside `use std::{…}` groups, any use of the [`CLOCKS`], and the
/// [`UNSAFE`] keyword. Comments and string literals are ignored.
pub fn os_access(source: &str) -> BTreeSet<String> {
    let code = code_only(source);
    let tokens = tokens(&code);
    let mut found = BTreeSet::new();
    for (i, token) in tokens.iter().enumerate() {
        if *token == UNSAFE {
            found.insert(UNSAFE.to_owned());
        } else if *token == "std" && tokens.get(i + 1) == Some(&"::") {
            let mut paths = Vec::new();
            use_tree(&tokens, i + 2, "std", &mut paths);
            found.extend(paths.into_iter().filter(|path| reaches_os(path)));
        } else if CLOCKS.contains(token) && tokens.get(i + 1) == Some(&"::") {
            found.insert(format!("{token}::{}", tokens.get(i + 2).unwrap_or(&"")));
        }
    }
    found
}

fn reaches_os(path: &str) -> bool {
    let mut segments = path.split("::").skip(1);
    segments
        .next()
        .is_some_and(|module| OS_MODULES.contains(&module))
        || segments.any(|segment| CLOCKS.contains(&segment))
}

/// Expands the path or `use` tree starting at `tokens[i]` under `prefix` into full paths, and
/// returns the index after it.
fn use_tree(tokens: &[&str], i: usize, prefix: &str, paths: &mut Vec<String>) -> usize {
    match tokens.get(i) {
        Some(&"{") => {
            let mut i = i + 1;
            loop {
                match tokens.get(i) {
                    None => return i,
                    Some(&"}") => return i + 1,
                    Some(&",") => i += 1,
                    Some(_) => i = use_tree(tokens, i, prefix, paths).max(i + 1),
                }
            }
        }
        Some(segment) if is_word(segment) || *segment == "*" => {
            let path = format!("{prefix}::{segment}");
            if tokens.get(i + 1) == Some(&"::") {
                use_tree(tokens, i + 2, &path, paths)
            } else {
                paths.push(path);
                i + 1
            }
        }
        _ => {
            paths.push(prefix.to_owned());
            i
        }
    }
}

fn is_word(token: &str) -> bool {
    token.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Words, `::`, and single punctuation characters.
fn tokens(code: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut rest = code.trim_start();
    while let Some(c) = rest.chars().next() {
        let len = if c.is_alphanumeric() || c == '_' {
            rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len())
        } else if rest.starts_with("::") {
            2
        } else {
            c.len_utf8()
        };
        tokens.push(&rest[..len]);
        rest = rest[len..].trim_start();
    }
    tokens
}

/// `source` without comments, and with every string and character literal emptied.
fn code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let at = |i: usize, text: &str| {
        text.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    let word = |i: usize| {
        chars
            .get(i)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    };
    let mut code = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        if at(i, "//") {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if at(i, "/*") {
            // Block comments nest.
            let mut depth = 0;
            while i < chars.len() {
                if at(i, "/*") {
                    depth += 1;
                    i += 2;
                } else if at(i, "*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if chars[i] == 'r'
            && (i == 0 || !word(i - 1) || (chars[i - 1] == 'b' && (i < 2 || !word(i - 2))))
            && (at(i + 1, "\"") || at(i + 1, "#"))
            && raw_string_end(&chars, i + 1).is_some()
        {
            i = raw_string_end(&chars, i + 1).unwrap_or(chars.len());
            code.push_str("\"\"");
        } else if chars[i] == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            code.push_str("\"\"");
        } else if at(i, "'\\") {
            i += 3;
            while i < chars.len() && chars[i] != '\'' {
                i += 1;
            }
            i += 1;
            code.push_str("' '");
        } else if chars[i] == '\'' && chars.get(i + 2) == Some(&'\'') {
            i += 3;
            code.push_str("' '");
        } else {
            code.push(chars[i]);
            i += 1;
        }
    }
    code
}

/// Where the raw string whose `#`s or opening quote start at `start` ends, if it is one.
fn raw_string_end(chars: &[char], start: usize) -> Option<usize> {
    let hashes = chars[start..].iter().take_while(|c| **c == '#').count();
    let open = start + hashes;
    if chars.get(open) != Some(&'"') {
        return None;
    }
    let mut i = open + 1;
    while i < chars.len() {
        if chars[i] == '"'
            && chars[i + 1..]
                .iter()
                .take(hashes)
                .filter(|c| **c == '#')
                .count()
                == hashes
        {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    Some(chars.len())
}

/// The crate names in the first column of DEPENDENCIES.md's tables, written as `` `name` ``.
pub fn listed_crates(markdown: &str) -> BTreeSet<String> {
    code_rows(markdown)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// The rows of `markdown`'s tables whose first cell is code, written as `` `code` ``: that
/// code, then the row's other cells, trimmed.
fn code_rows(markdown: &str) -> Vec<(String, Vec<String>)> {
    markdown
        .lines()
        .filter_map(|line| {
            let row = line.trim().strip_prefix('|')?;
            let mut cells = row
                .strip_suffix('|')
                .unwrap_or(row)
                .split('|')
                .map(str::trim);
            let code = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
            Some((code.to_owned(), cells.map(str::to_owned).collect()))
        })
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
                dev: dependency["kind"].as_str() == Some("dev"),
                features: dependency["features"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|feature| feature.as_str().map(str::to_owned))
                    .collect(),
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
            ..Dependency::default()
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
    fn fakes_may_be_turned_on_only_in_dev_dependencies() {
        // SEC-06
        let with_fakes = |dev| {
            let mut graph = graph(&[("meegone-apps-state", "meegone-core-data")], &[]);
            graph.internal[0].dev = dev;
            graph.internal[0].features = vec![FAKES.to_owned()];
            check(&graph, &BTreeSet::new())
        };

        let normal = with_fakes(false);
        assert_eq!(normal.len(), 1, "{normal:?}");
        assert!(normal[0].0.contains("fakes are for tests only"));
        assert!(with_fakes(true).is_empty());
    }

    fn os(source: &str) -> Vec<String> {
        os_access(source).into_iter().collect()
    }

    #[test]
    fn os_modules_are_found_in_paths_and_use_groups() {
        assert_eq!(os("use std::fs;"), ["std::fs"]);
        assert_eq!(
            os("let out = std::process::Command::new(\"ls\");"),
            ["std::process::Command::new"]
        );
        assert_eq!(
            os("use std::{collections::HashMap, env::{self, var}, net::TcpStream as Tcp};"),
            ["std::env::self", "std::env::var", "std::net::TcpStream"]
        );
        assert_eq!(os("use ::std::os::unix;"), ["std::os::unix"]);
    }

    #[test]
    fn reading_the_clock_is_found() {
        assert_eq!(
            os("use std::time::{Duration, Instant};"),
            ["std::time::Instant"]
        );
        assert_eq!(
            os("use std::time;\nlet now = time::SystemTime::now();"),
            ["SystemTime::now"]
        );
    }

    #[test]
    fn unsafe_code_is_found() {
        // Foreign code, such as JNI, needs `unsafe` (ADR 0005).
        assert_eq!(os("let n = unsafe { strlen(text) };"), ["unsafe"]);
        assert_eq!(
            os("#[unsafe(no_mangle)]\npub extern \"system\" fn Java_f() {}"),
            ["unsafe"]
        );
        assert_eq!(os("unsafe impl Send for Handle {}"), ["unsafe"]);
    }

    #[test]
    fn safe_std_paths_comments_and_strings_are_ignored() {
        let source = r##"
            //! Only adapters may use std::fs or unsafe code.
            use std::sync::{Arc, Mutex};
            use std::time::Duration;
            /* std::net /* nested std::env */ std::process */
            const URL: &str = "https://example.com/std::fs";
            const RAW: &str = r#"std::env "quoted" std::os"#;
            const QUOTE: char = '"';
            const ESCAPED: char = '\'';
            fn f<'a>(x: &'a str) -> &'a str { x } // std::process
            const WORD: &str = "unsafe";
            fn not_unsafe_at_all() {}
        "##;

        assert_eq!(os(source), Vec::<String>::new());
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
