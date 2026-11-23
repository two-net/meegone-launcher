//! The security and privacy rules of ADR 0042 that a scan of the Android host, and of the
//! release APK it builds, can check:
//!
//! - the app asks for exactly the permissions in [`PERMISSIONS_ALLOWED`], each serving a row of
//!   SRS 8.1 (SEC-03). The rest of SRS 8.1 needs no permission: the home-app role, the app
//!   catalogue, notification access (the system binds the listener), background scheduling and
//!   vibration (only `performHapticFeedback`);
//! - only the entries in [`EXPORTED_ALLOWED`] are reachable by other apps: the home entry and
//!   the settings entry, both on `HomeActivity`, and the notification listener, each answering
//!   only its own intent filters. Every component says whether it is exported (SEC-04);
//! - no component is an accessibility service or a device administrator (SEC-03), or shows over
//!   the lock screen or turns the screen on (SEC-05);
//! - the app names its network security config, which refuses cleartext and trusts only the
//!   system's certificates, with no exception for a domain or a debug build (SEC-01);
//! - the app isn't debuggable, test-only or profileable from the shell (SEC-06);
//! - the shipped Java uses none of the [`FORBIDDEN`] APIs: web views and links or HTML in text
//!   (SEC-02), showing over or dismissing the lock screen (SEC-05), device identifiers
//!   (PRIV-01), and stack traces in logcat (PRIV-03). It connects to the network only where
//!   [`NETWORK_ALLOWED`] says (PRIV-02), and no log passes an exception, whose message and stack
//!   trace can name apps (PRIV-03). No shipped XML turns text into links (SEC-02);
//! - the release APK, as built, passes the manifest rules and holds no class of the debug
//!   build's own, such as the audit or the probe (SEC-06).
//!
//! The rules are pure functions over text. [`Manifest`] reads a source manifest, and the release
//! APK's as `aapt2 dump xmltree` prints it, into the same shape. `tests/security.rs` runs the
//! rules on this repository, and `tests/release_apk.rs` on the release APK after a Gradle build.
//! Like the other scans, they read code, not meaning, so they catch accidents rather than
//! determined workarounds; `tools/net-audit.sh` checks on a device where the traffic goes.

use std::collections::BTreeSet;
use std::path::Path;

use crate::Violation;
use crate::a11y::{closing_paren, is_word_char, java_code_only, line_of, word_at, word_before};
use crate::l10n::{SHIPPED_SOURCE_SETS, xml_files};
use crate::legal::ANDROID_SOURCES;

/// The permissions the app asks for, as `(permission, the SRS 8.1 row it serves)`. The app must
/// ask for exactly these: one missing from the manifest is reported, so the list stays true.
pub const PERMISSIONS_ALLOWED: [(&str, &str); 5] = [
    (
        "android.permission.REQUEST_DELETE_PACKAGES",
        "uninstall requests: ✕ in edit mode, which the system confirms",
    ),
    (
        "android.permission.PACKAGE_USAGE_STATS",
        "usage access: Recent, once the user allows it in the system's settings",
    ),
    (
        "android.permission.INTERNET",
        "network: the daily image's download",
    ),
    (
        "android.permission.ACCESS_NETWORK_STATE",
        "network state: the daily image waits for a connection",
    ),
    (
        "android.permission.SET_WALLPAPER",
        "setting the wallpaper: the lock screen's daily image",
    ),
];

/// A component other apps may reach (SEC-04).
#[derive(Debug, Clone, Copy)]
pub struct Exported {
    /// The class's simple name.
    pub component: &'static str,
    /// The permission another app must hold to reach it, if any.
    pub permission: Option<&'static str>,
    /// Each intent filter it may have, as [`Filter::describe`] writes it.
    pub filters: &'static [&'static str],
    pub reason: &'static str,
}

/// Every component other apps may reach, and how. Exactly these are exported: an entry that
/// matches nothing is reported, so the list stays true.
pub const EXPORTED_ALLOWED: [Exported; 2] = [
    Exported {
        component: "HomeActivity",
        permission: None,
        filters: &[
            "MAIN, DEFAULT, HOME",
            "MAIN, LAUNCHER",
            "APPLICATION_PREFERENCES, DEFAULT",
        ],
        reason: "the home entry (HOME-01), the app's own entry so it runs without the role, and \
                 the settings entry the system shows beside it (SET-01)",
    },
    Exported {
        component: "NotificationListener",
        permission: Some("android.permission.BIND_NOTIFICATION_LISTENER_SERVICE"),
        filters: &["android.service.notification.NotificationListenerService"],
        reason: "Events' notifications: only the system, which alone holds the permission, may \
                 bind it (EVT-04)",
    },
];

/// Permissions that would make a component a role SEC-03 rules out, as `(permission, role)`.
const FORBIDDEN_ROLES: [(&str, &str); 2] = [
    (
        "android.permission.BIND_ACCESSIBILITY_SERVICE",
        "an accessibility service",
    ),
    (
        "android.permission.BIND_DEVICE_ADMIN",
        "a device administrator",
    ),
];

/// APIs the shipped Java never uses, as `(name, what it is, requirement)`. Each is matched as a
/// whole word, outside comments and strings.
pub const FORBIDDEN: [(&str, &str, &str); 22] = [
    (
        "WebView",
        "a web view, where downloaded pages would run",
        "SEC-02",
    ),
    ("fromHtml", "HTML read from text", "SEC-02"),
    ("Linkify", "links made from text", "SEC-02"),
    ("setAutoLinkMask", "links made from text", "SEC-02"),
    ("LinkMovementMethod", "links in text", "SEC-02"),
    (
        "setShowWhenLocked",
        "showing over the lock screen",
        "SEC-05",
    ),
    (
        "FLAG_SHOW_WHEN_LOCKED",
        "showing over the lock screen",
        "SEC-05",
    ),
    ("setTurnScreenOn", "turning the screen on", "SEC-05"),
    ("FLAG_TURN_SCREEN_ON", "turning the screen on", "SEC-05"),
    (
        "requestDismissKeyguard",
        "dismissing the lock screen",
        "SEC-05",
    ),
    (
        "FLAG_DISMISS_KEYGUARD",
        "dismissing the lock screen",
        "SEC-05",
    ),
    ("disableKeyguard", "turning the lock screen off", "SEC-05"),
    ("ANDROID_ID", "the device's identifier", "PRIV-01"),
    (
        "AdvertisingIdClient",
        "the advertising identifier",
        "PRIV-01",
    ),
    ("getSerial", "the device's serial number", "PRIV-01"),
    ("getImei", "the phone's IMEI", "PRIV-01"),
    ("getMeid", "the phone's MEID", "PRIV-01"),
    ("getDeviceId", "the phone's identifier", "PRIV-01"),
    ("getSubscriberId", "the SIM's identifier", "PRIV-01"),
    ("getLine1Number", "the phone number", "PRIV-01"),
    (
        "printStackTrace",
        "a stack trace in logcat, which can name apps",
        "PRIV-03",
    ),
    (
        "getStackTraceString",
        "a stack trace in logcat, which can name apps",
        "PRIV-03",
    ),
];

/// The Java names that open a connection. Each is matched as a whole word, outside comments
/// and strings.
const NETWORK: [&str; 11] = [
    "openConnection",
    "openStream",
    "URLConnection",
    "HttpURLConnection",
    "HttpsURLConnection",
    "Socket",
    "SSLSocket",
    "SocketChannel",
    "SocketFactory",
    "DatagramSocket",
    "DownloadManager",
];

/// The Java files that may connect to the network, as `(file, reason)`. The file is relative to
/// the repository root. An entry that connects to nothing is reported, so the list stays true.
pub const NETWORK_ALLOWED: [(&str, &str); 1] = [(
    "android/app/src/main/java/io/github/twonet/meegone/Bridge.java",
    "`httpsGet`, the one request the Rust app makes: to the image feed, one hop at a time \
     (PRIV-02, ADR 0027)",
)];

/// The `Log` methods whose third argument would be an exception.
const LOG_METHODS: [&str; 6] = ["v", "d", "i", "w", "e", "wtf"];

/// The namespace `aapt2` prints in full for `android:` attributes.
const ANDROID_NAMESPACE: &str = "http://schemas.android.com/apk/res/android";

/// The elements that are components.
const COMPONENTS: [&str; 5] = [
    "activity",
    "activity-alias",
    "service",
    "receiver",
    "provider",
];

/// What a manifest says about the access the app asks for and what other apps can reach.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// Each `<uses-permission>` and `<uses-permission-sdk-23>`, as `(line, permission)`.
    pub permissions: Vec<(usize, String)>,
    pub application: Application,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Application {
    pub line: usize,
    pub network_security_config: bool,
    pub uses_cleartext_traffic: bool,
    pub debuggable: bool,
    pub test_only: bool,
    /// `<profileable android:shell="true">`, which lets adb profile the app.
    pub profileable_by_shell: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Component {
    pub line: usize,
    /// `activity`, `activity-alias`, `service`, `receiver` or `provider`.
    pub kind: String,
    /// The class's simple name, such as `HomeActivity`.
    pub name: String,
    /// `android:exported`, where it is written.
    pub exported: Option<bool>,
    /// `android:permission`: what another app must hold to reach it.
    pub permission: Option<String>,
    pub filters: Vec<Filter>,
    /// `android:showWhenLocked` or `android:turnScreenOn`.
    pub over_lock_screen: bool,
}

impl Component {
    /// Whether other apps can reach it: as written, or Android's default, which exports a
    /// component with an intent filter.
    pub fn reachable(&self) -> bool {
        self.exported.unwrap_or(!self.filters.is_empty())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub actions: Vec<String>,
    pub categories: Vec<String>,
    /// Each `<data>`, as `data` and its attributes.
    pub data: Vec<String>,
}

impl Filter {
    /// The filter as [`EXPORTED_ALLOWED`] writes it: its actions, then its categories, each in
    /// alphabetical order and without the `android.intent.action.` or
    /// `android.intent.category.` prefix, then its data, joined by commas.
    pub fn describe(&self) -> String {
        let short = |name: &String| {
            name.strip_prefix("android.intent.action.")
                .or_else(|| name.strip_prefix("android.intent.category."))
                .unwrap_or(name)
                .to_owned()
        };
        let sorted = |names: &[String]| {
            let mut names: Vec<String> = names.iter().map(short).collect();
            names.sort();
            names
        };
        sorted(&self.actions)
            .into_iter()
            .chain(sorted(&self.categories))
            .chain(self.data.iter().cloned())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Adds the filter's `part`, an `action`, `category` or `data` element.
    fn add(&mut self, part: &str, element: &Element, name: Option<String>) {
        match part {
            "action" => self.actions.extend(name),
            "category" => self.categories.extend(name),
            _ => {
                let attributes = element
                    .attributes
                    .iter()
                    .map(|(attribute, value)| {
                        format!("{}={value}", attribute.trim_start_matches("android:"))
                    })
                    .collect::<Vec<_>>();
                self.data.push(format!("data {}", attributes.join(" ")));
            }
        }
    }
}

/// An element as either reader finds it.
#[derive(Debug)]
struct Element {
    name: String,
    line: usize,
    /// How deep it is. Only the order matters: a child is deeper than its parent.
    depth: usize,
    /// Its attributes, `android:` ones named with that prefix.
    attributes: Vec<(String, String)>,
}

impl Element {
    fn get(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(attribute, _)| attribute == name)
            .map(|(_, value)| value.as_str())
    }

    fn is_true(&self, name: &str) -> bool {
        self.get(name) == Some("true")
    }
}

impl Manifest {
    /// Reads a manifest as written in a source set.
    pub fn from_source(xml: &str) -> Self {
        Self::from_elements(&source_elements(xml))
    }

    /// Reads a built manifest as `aapt2 dump xmltree --file AndroidManifest.xml` prints it.
    pub fn from_aapt2(tree: &str) -> Self {
        Self::from_elements(&aapt2_elements(tree))
    }

    fn from_elements(elements: &[Element]) -> Self {
        let mut manifest = Self::default();
        // The elements the current one is inside, outermost first.
        let mut open: Vec<&Element> = Vec::new();
        for element in elements {
            while open.last().is_some_and(|last| last.depth >= element.depth) {
                open.pop();
            }
            let parent = open.last().map(|parent| parent.name.as_str());
            let grandparent = open.len().checked_sub(2).map(|at| open[at].name.as_str());
            let in_filter = parent == Some("intent-filter")
                && grandparent.is_some_and(|name| COMPONENTS.contains(&name));
            let name = element.get("android:name").map(str::to_owned);
            match element.name.as_str() {
                "uses-permission" | "uses-permission-sdk-23" => {
                    manifest
                        .permissions
                        .extend(name.map(|name| (element.line, name)));
                }
                "application" => {
                    manifest.application = Application {
                        line: element.line,
                        network_security_config: element
                            .get("android:networkSecurityConfig")
                            .is_some(),
                        uses_cleartext_traffic: element.is_true("android:usesCleartextTraffic"),
                        debuggable: element.is_true("android:debuggable"),
                        test_only: element.is_true("android:testOnly"),
                        profileable_by_shell: false,
                    };
                }
                "profileable" if parent == Some("application") => {
                    manifest.application.profileable_by_shell = element.is_true("android:shell");
                }
                kind if COMPONENTS.contains(&kind) && parent == Some("application") => {
                    manifest.components.push(Component {
                        line: element.line,
                        kind: kind.to_owned(),
                        name: simple_name(name.as_deref().unwrap_or_default()).to_owned(),
                        exported: element.get("android:exported").map(|value| value == "true"),
                        permission: element.get("android:permission").map(str::to_owned),
                        filters: Vec::new(),
                        over_lock_screen: element.is_true("android:showWhenLocked")
                            || element.is_true("android:turnScreenOn"),
                    });
                }
                "intent-filter" if parent.is_some_and(|name| COMPONENTS.contains(&name)) => {
                    if let Some(component) = manifest.components.last_mut() {
                        component.filters.push(Filter::default());
                    }
                }
                part @ ("action" | "category" | "data") if in_filter => {
                    let filter = manifest
                        .components
                        .last_mut()
                        .and_then(|component| component.filters.last_mut());
                    if let Some(filter) = filter {
                        filter.add(part, element, name);
                    }
                }
                _ => {}
            }
            open.push(element);
        }
        manifest
    }
}

/// `io.github.twonet.meegone.HomeActivity` or `.HomeActivity` as `HomeActivity`.
fn simple_name(class: &str) -> &str {
    class.rsplit('.').next().unwrap_or(class)
}

/// The elements of XML `text`, in order, skipping comments, declarations and processing
/// instructions.
fn source_elements(text: &str) -> Vec<Element> {
    let xml = blank_xml_comments(text);
    let mut elements = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    while let Some(start) = xml[from..].find('<').map(|at| from + at) {
        let Some(end) = tag_end(&xml[start..]).map(|at| start + at) else {
            break;
        };
        let tag = &xml[start + 1..end];
        from = end + 1;
        if tag.starts_with(['?', '!']) {
            continue;
        }
        if tag.starts_with('/') {
            depth = depth.saturating_sub(1);
            continue;
        }
        let closed = tag.ends_with('/');
        let tag = tag.trim_end_matches('/');
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        elements.push(Element {
            name: tag[..name_end].to_owned(),
            line: line_of(&xml, start),
            depth,
            attributes: xml_attributes(&tag[name_end..]),
        });
        if !closed {
            depth += 1;
        }
    }
    elements
}

/// XML `text` with each comment blanked out, keeping its line breaks, so lines stay the same.
fn blank_xml_comments(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        kept.push_str(&rest[..start]);
        let end = rest[start..]
            .find("-->")
            .map_or(rest.len(), |at| start + at + 3);
        kept.extend(
            rest[start..end]
                .chars()
                .map(|c| if c == '\n' { '\n' } else { ' ' }),
        );
        rest = &rest[end..];
    }
    kept.push_str(rest);
    kept
}

/// The byte index of the `>` that ends the tag starting `text`, outside quoted values.
fn tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (at, c) in text.char_indices() {
        match quote {
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return Some(at),
            Some(open) if c == open => quote = None,
            _ => {}
        }
    }
    None
}

/// The `name="value"` attributes in `text`, the inside of a tag after its name.
fn xml_attributes(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(equals) = rest.find('=') {
        let name = rest[..equals].trim();
        let after = rest[equals + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(length) = after[1..].find(quote) else {
            break;
        };
        found.push((name.to_owned(), after[1..1 + length].to_owned()));
        rest = &after[1 + length + 1..];
    }
    found
}

/// The elements of an `aapt2 dump xmltree` printout: `E: name (line=N)` lines, each followed by
/// its `A:` attribute lines, nested by indentation.
fn aapt2_elements(tree: &str) -> Vec<Element> {
    let mut elements: Vec<Element> = Vec::new();
    for line in tree.lines() {
        let text = line.trim_start();
        let depth = line.len() - text.len();
        if let Some(rest) = text.strip_prefix("E: ") {
            let (name, place) = rest.split_once(' ').unwrap_or((rest, ""));
            elements.push(Element {
                name: name.to_owned(),
                line: place
                    .strip_prefix("(line=")
                    .and_then(|number| number.strip_suffix(')'))
                    .and_then(|number| number.parse().ok())
                    .unwrap_or(0),
                depth,
                attributes: Vec::new(),
            });
        } else if let Some(rest) = text.strip_prefix("A: ") {
            if let (Some(element), Some(attribute)) = (elements.last_mut(), aapt2_attribute(rest)) {
                element.attributes.push(attribute);
            }
        }
    }
    elements
}

/// `http://schemas.android.com/apk/res/android:name(0x01010003)="value" (Raw: "value")` as
/// `("android:name", "value")`. A boolean reads as `true` or `false`, and a resource as its id.
fn aapt2_attribute(text: &str) -> Option<(String, String)> {
    let (name, value) = text.split_once('=')?;
    let name = name.split('(').next()?;
    let name = match name.rsplit_once(':') {
        Some((namespace, local)) if namespace == ANDROID_NAMESPACE => format!("android:{local}"),
        _ => name.to_owned(),
    };
    let value = match value.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        None => value.split_whitespace().next()?,
    };
    Some((name, value.to_owned()))
}

/// Where `manifest`, read from `file`, asks for access SRS 8.1 doesn't list (SEC-03), lets
/// other apps reach more than [`EXPORTED_ALLOWED`] (SEC-04), takes a role SEC-03 rules out,
/// shows over the lock screen (SEC-05), allows cleartext (SEC-01), or is a debug build
/// (SEC-06); and which entries of the allowed lists it no longer needs. With `written`, as in a
/// source manifest, every component must also say whether it is exported.
pub fn manifest_violations(file: &str, manifest: &Manifest, written: bool) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut report = |message: String| violations.push(Violation(message));

    for (line, permission) in &manifest.permissions {
        if !PERMISSIONS_ALLOWED
            .iter()
            .any(|(name, _)| name == permission)
        {
            report(format!(
                "`{file}:{line}` asks for `{permission}`, which isn't access SRS 8.1 lists; \
                 remove it, or add it to PERMISSIONS_ALLOWED with the row of SRS 8.1 it serves \
                 (SEC-03)"
            ));
        }
    }
    for (permission, _) in PERMISSIONS_ALLOWED {
        if !manifest
            .permissions
            .iter()
            .any(|(_, name)| name == permission)
        {
            report(format!(
                "PERMISSIONS_ALLOWED lists `{permission}`, but `{file}` doesn't ask for it any \
                 more; remove the entry (SEC-03)"
            ));
        }
    }

    for component in &manifest.components {
        let (line, kind, name) = (component.line, &component.kind, &component.name);
        if written && component.exported.is_none() {
            report(format!(
                "`{file}:{line}` doesn't say whether the {kind} `{name}` is exported; write \
                 `android:exported`, false unless other apps must reach it (SEC-04)"
            ));
        }
        if let Some(permission) = &component.permission {
            if let Some((_, role)) = FORBIDDEN_ROLES.iter().find(|(name, _)| name == permission) {
                report(format!(
                    "`{file}:{line}` makes `{name}` {role}, which MeeGone never is (SEC-03)"
                ));
            }
        }
        if component.over_lock_screen {
            report(format!(
                "`{file}:{line}` lets `{name}` show over the lock screen or turn the screen on; \
                 MeeGone never covers or weakens the lock screen (SEC-05)"
            ));
        }
        if !component.reachable() {
            continue;
        }
        let Some(allowed) = EXPORTED_ALLOWED
            .iter()
            .find(|allowed| allowed.component == name)
        else {
            report(format!(
                "`{file}:{line}` lets other apps reach the {kind} `{name}`; only the home \
                 entry, the settings entry and the notification listener may be, as \
                 EXPORTED_ALLOWED lists (SEC-04)"
            ));
            continue;
        };
        if component.permission.as_deref() != allowed.permission {
            report(format!(
                "`{file}:{line}` guards `{name}` with {}, but EXPORTED_ALLOWED says {} (SEC-04)",
                guard(component.permission.as_deref()),
                guard(allowed.permission),
            ));
        }
        let filters: Vec<String> = component.filters.iter().map(Filter::describe).collect();
        for filter in &filters {
            if !allowed.filters.contains(&filter.as_str()) {
                report(format!(
                    "`{file}:{line}` lets other apps reach `{name}` with \"{filter}\", which \
                     EXPORTED_ALLOWED doesn't list (SEC-04)"
                ));
            }
        }
        for filter in allowed.filters {
            if !filters.iter().any(|found| found == filter) {
                report(format!(
                    "EXPORTED_ALLOWED lets other apps reach `{name}` with \"{filter}\", but \
                     `{file}` doesn't any more; remove it (SEC-04)"
                ));
            }
        }
    }
    for allowed in EXPORTED_ALLOWED {
        let found = manifest
            .components
            .iter()
            .any(|component| component.name == allowed.component && component.reachable());
        if !found {
            report(format!(
                "EXPORTED_ALLOWED lists `{}`, but `{file}` doesn't export it any more; remove \
                 the entry (SEC-04)",
                allowed.component
            ));
        }
    }

    let application = &manifest.application;
    let line = application.line;
    if !application.network_security_config {
        report(format!(
            "`{file}:{line}`'s application names no `android:networkSecurityConfig`, so nothing \
             refuses cleartext or user-added certificates (SEC-01)"
        ));
    }
    if application.uses_cleartext_traffic {
        report(format!(
            "`{file}:{line}` allows cleartext traffic; every connection is HTTPS (SEC-01)"
        ));
    }
    for (set, what) in [
        (application.debuggable, "debuggable"),
        (application.test_only, "test-only"),
        (
            application.profileable_by_shell,
            "profileable from the shell",
        ),
    ] {
        if set {
            report(format!(
                "`{file}:{line}` makes the app {what}, a debug feature no release has (SEC-06)"
            ));
        }
    }
    violations
}

fn guard(permission: Option<&str>) -> String {
    permission.map_or_else(|| "no permission".to_owned(), |name| format!("`{name}`"))
}

/// Where the network security config `xml`, read from `file`, would allow cleartext or trust
/// certificates other than the system's (SEC-01).
pub fn network_config_violations(file: &str, xml: &str) -> Vec<Violation> {
    let elements = source_elements(xml);
    let mut violations = Vec::new();
    let base = elements
        .iter()
        .find(|element| element.name == "base-config");
    if base.and_then(|base| base.get("cleartextTrafficPermitted")) != Some("false") {
        violations.push(Violation(format!(
            "`{file}` doesn't refuse cleartext: its `base-config` must say \
             `cleartextTrafficPermitted=\"false\"` (SEC-01)"
        )));
    }
    let sources: Vec<&Element> = elements
        .iter()
        .filter(|element| element.name == "certificates")
        .collect();
    if sources.is_empty() {
        violations.push(Violation(format!(
            "`{file}` names no trust anchors, so a default applies; trust exactly the system's \
             certificates (SEC-01)"
        )));
    }
    for certificates in sources {
        if certificates.get("src") != Some("system") {
            violations.push(Violation(format!(
                "`{file}:{}` trusts certificates other than the system's; only the system's \
                 are trusted (SEC-01)",
                certificates.line
            )));
        }
    }
    for element in &elements {
        if matches!(element.name.as_str(), "debug-overrides" | "domain-config") {
            violations.push(Violation(format!(
                "`{file}:{}` has a `{}`, an exception to the rules every connection follows \
                 (SEC-01)",
                element.line, element.name
            )));
        }
    }
    violations
}

/// Where the shipped Java `sources`, as `(file, text)`, use a [`FORBIDDEN`] API, connect to the
/// network outside `network_allowed` (PRIV-02), or log an exception (PRIV-03); and which entries
/// of `network_allowed` connect to nothing.
pub fn java_violations(
    sources: &[(String, String)],
    network_allowed: &[(&str, &str)],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut connecting = BTreeSet::new();
    for (file, text) in sources {
        let code = java_code_only(text);
        for (name, what, requirement) in FORBIDDEN {
            for at in words(&code, name) {
                violations.push(Violation(format!(
                    "`{file}:{}` uses `{name}`, for {what}; MeeGone rules that out \
                     ({requirement})",
                    line_of(&code, at)
                )));
            }
        }
        for name in NETWORK {
            for at in words(&code, name) {
                connecting.insert(file.as_str());
                if !network_allowed.iter().any(|(allowed, _)| allowed == file) {
                    violations.push(Violation(format!(
                        "`{file}:{}` connects to the network with `{name}`; only `Bridge`'s \
                         `httpsGet` does, so the image feed stays the one destination \
                         (PRIV-02)",
                        line_of(&code, at)
                    )));
                }
            }
        }
        for at in exception_logs(&code) {
            violations.push(Violation(format!(
                "`{file}:{}` logs an exception, whose message and stack trace can name apps; \
                 log only its kind: `e.getClass().getSimpleName()` (PRIV-03)",
                line_of(&code, at)
            )));
        }
    }
    for (file, _) in network_allowed {
        if !connecting.contains(file) {
            violations.push(Violation(format!(
                "NETWORK_ALLOWED lets `{file}` connect to the network, but it doesn't any more; \
                 remove the entry (PRIV-02)"
            )));
        }
    }
    violations
}

/// Where `name` stands as a whole word in `code`.
fn words<'a>(code: &'a str, name: &'a str) -> impl Iterator<Item = usize> + 'a {
    code.match_indices(name).filter_map(move |(at, _)| {
        let after = code[at + name.len()..].chars().next();
        (!word_before(code, at) && !after.is_some_and(is_word_char)).then_some(at)
    })
}

/// Where Java `code` calls `Log.v`, `.d`, `.i`, `.w`, `.e` or `.wtf` with a third argument, an
/// exception.
fn exception_logs(code: &str) -> Vec<usize> {
    words(code, "Log")
        .filter(|&at| {
            let Some(after) = code[at + "Log".len()..].strip_prefix('.') else {
                return false;
            };
            let method = word_at(after);
            let Some(arguments) = after[method.len()..].trim_start().strip_prefix('(') else {
                return false;
            };
            LOG_METHODS.contains(&method)
                && top_level_commas(&arguments[..closing_paren(arguments)]) >= 2
        })
        .collect()
}

/// The commas in `arguments` outside brackets.
fn top_level_commas(arguments: &str) -> usize {
    let mut depth = 0usize;
    let mut commas = 0;
    for c in arguments.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    commas
}

/// Where the shipped XML `xml`, read from `file`, turns text into links (SEC-02).
pub fn xml_violations(file: &str, xml: &str) -> Vec<Violation> {
    source_elements(xml)
        .iter()
        .filter(|element| element.get("android:autoLink").is_some())
        .map(|element| {
            Violation(format!(
                "`{file}:{}` turns text into links with `android:autoLink`; links open only \
                 from MeeGone's own buttons, in the browser (SEC-02)",
                element.line
            ))
        })
        .collect()
}

/// Every manifest and other XML file of the source sets that ship, relative to `root`.
pub fn shipped_xml_files(root: &Path) -> Result<BTreeSet<String>, String> {
    Ok(xml_files(root)?
        .into_iter()
        .filter(|file| in_sets(file, &SHIPPED_SOURCE_SETS))
        .collect())
}

fn in_sets(file: &str, sets: &[&str]) -> bool {
    sets.iter()
        .any(|set| file.starts_with(&format!("{ANDROID_SOURCES}/{set}/")))
}

/// The classes only the debug build's own source sets define, by their full names: the Java
/// `files` outside the [`SHIPPED_SOURCE_SETS`] whose class no shipped set defines too.
pub fn debug_only_classes(files: &BTreeSet<String>) -> BTreeSet<String> {
    let class = |file: &String| {
        let (_, path) = file.split_once("/java/")?;
        Some(path.strip_suffix(".java")?.replace('/', "."))
    };
    let shipped: BTreeSet<String> = files
        .iter()
        .filter(|file| in_sets(file, &SHIPPED_SOURCE_SETS))
        .filter_map(class)
        .collect();
    files
        .iter()
        .filter(|file| !in_sets(file, &SHIPPED_SOURCE_SETS))
        .filter_map(class)
        .filter(|name| !shipped.contains(name))
        .collect()
}

/// The top-level classes `dexdump` lists, by their full names: `A11yAudit$1` counts as
/// `A11yAudit`.
pub fn dex_classes(dexdump: &str) -> BTreeSet<String> {
    dexdump
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("Class descriptor"))
        .filter_map(|rest| {
            rest.split_once("'L")?
                .1
                .split_once(';')
                .map(|(name, _)| name)
        })
        .map(|name| name.split('$').next().unwrap_or(name).replace('/', "."))
        .collect()
}

/// Where the release APK breaks the manifest rules, read from `aapt2`'s `manifest_tree`, or
/// holds a class only the debug build should, read from `dexdump` (SEC-06).
pub fn release_apk_violations(
    manifest_tree: &str,
    dexdump: &str,
    debug_only: &BTreeSet<String>,
) -> Vec<Violation> {
    let manifest = Manifest::from_aapt2(manifest_tree);
    let mut violations =
        manifest_violations("the release APK's AndroidManifest.xml", &manifest, false);
    for class in dex_classes(dexdump).intersection(debug_only) {
        violations.push(Violation(format!(
            "the release APK holds `{class}`, which only the debug build may: a release has no \
             debug features or test endpoints (SEC-06)"
        )));
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of the app's own manifest, which passes.
    const MANIFEST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- A comment with <activity android:name=".Hidden" android:exported="true" /> in it. -->
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.REQUEST_DELETE_PACKAGES" />
    <uses-permission android:name="android.permission.PACKAGE_USAGE_STATS" />
    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
    <uses-permission android:name="android.permission.SET_WALLPAPER" />
    <queries>
        <intent>
            <action android:name="android.intent.action.VIEW" />
            <data android:scheme="https" />
        </intent>
    </queries>
    <application
        android:label="@string/app_name"
        android:networkSecurityConfig="@xml/network_security_config">
        <activity android:name=".HomeActivity" android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.HOME" />
                <category android:name="android.intent.category.DEFAULT" />
            </intent-filter>
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
            <intent-filter>
                <action android:name="android.intent.action.APPLICATION_PREFERENCES" />
                <category android:name="android.intent.category.DEFAULT" />
            </intent-filter>
        </activity>
        <service
            android:name=".NotificationListener"
            android:exported="true"
            android:permission="android.permission.BIND_NOTIFICATION_LISTENER_SERVICE">
            <intent-filter>
                <action android:name="android.service.notification.NotificationListenerService" />
            </intent-filter>
        </service>
        <service
            android:name=".ImageJobService"
            android:exported="false"
            android:permission="android.permission.BIND_JOB_SERVICE" />
    </application>
</manifest>
"#;

    /// The same manifest as `aapt2 dump xmltree` prints a release build's.
    const TREE: &str = r#"N: android=http://schemas.android.com/apk/res/android (line=6)
  E: manifest (line=6)
    A: package="io.github.twonet.meegone" (Raw: "io.github.twonet.meegone")
      E: uses-permission (line=20)
        A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.permission.REQUEST_DELETE_PACKAGES" (Raw: "android.permission.REQUEST_DELETE_PACKAGES")
      E: uses-permission (line=27)
        A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.permission.PACKAGE_USAGE_STATS" (Raw: "android.permission.PACKAGE_USAGE_STATS")
      E: uses-permission (line=34)
        A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.permission.INTERNET" (Raw: "android.permission.INTERNET")
      E: uses-permission (line=35)
        A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.permission.ACCESS_NETWORK_STATE" (Raw: "android.permission.ACCESS_NETWORK_STATE")
      E: uses-permission (line=42)
        A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.permission.SET_WALLPAPER" (Raw: "android.permission.SET_WALLPAPER")
      E: queries (line=49)
          E: intent (line=55)
              E: action (line=56)
                A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.intent.action.VIEW" (Raw: "android.intent.action.VIEW")
              E: data (line=58)
                A: http://schemas.android.com/apk/res/android:scheme(0x01010027)="https" (Raw: "https")
      E: application (line=74)
        A: http://schemas.android.com/apk/res/android:label(0x01010001)=@0x7f06002d
        A: http://schemas.android.com/apk/res/android:networkSecurityConfig(0x01010527)=@0x7f080002
          E: activity (line=96)
            A: http://schemas.android.com/apk/res/android:name(0x01010003)="io.github.twonet.meegone.HomeActivity" (Raw: "io.github.twonet.meegone.HomeActivity")
            A: http://schemas.android.com/apk/res/android:exported(0x01010010)=true
              E: intent-filter (line=107)
                  E: action (line=108)
                    A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.intent.action.MAIN" (Raw: "android.intent.action.MAIN")
                  E: category (line=110)
                    A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.intent.category.HOME" (Raw: "android.intent.category.HOME")
                  E: category (line=111)
                    A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.intent.category.DEFAULT" (Raw: "android.intent.category.DEFAULT")
              E: intent-filter (line=115)
                  E: action (line=116)
                    A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.intent.action.MAIN" (Raw: "android.intent.action.MAIN")
                  E: category (line=118)
                    A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.intent.category.LAUNCHER" (Raw: "android.intent.category.LAUNCHER")
              E: intent-filter (line=125)
                  E: action (line=126)
                    A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.intent.action.APPLICATION_PREFERENCES" (Raw: "android.intent.action.APPLICATION_PREFERENCES")
                  E: category (line=128)
                    A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.intent.category.DEFAULT" (Raw: "android.intent.category.DEFAULT")
          E: service (line=138)
            A: http://schemas.android.com/apk/res/android:name(0x01010003)="io.github.twonet.meegone.NotificationListener" (Raw: "io.github.twonet.meegone.NotificationListener")
            A: http://schemas.android.com/apk/res/android:permission(0x01010006)="android.permission.BIND_NOTIFICATION_LISTENER_SERVICE" (Raw: "android.permission.BIND_NOTIFICATION_LISTENER_SERVICE")
            A: http://schemas.android.com/apk/res/android:exported(0x01010010)=true
              E: intent-filter (line=143)
                  E: action (line=144)
                    A: http://schemas.android.com/apk/res/android:name(0x01010003)="android.service.notification.NotificationListenerService" (Raw: "android.service.notification.NotificationListenerService")
          E: service (line=150)
            A: http://schemas.android.com/apk/res/android:name(0x01010003)="io.github.twonet.meegone.ImageJobService" (Raw: "io.github.twonet.meegone.ImageJobService")
            A: http://schemas.android.com/apk/res/android:permission(0x01010006)="android.permission.BIND_JOB_SERVICE" (Raw: "android.permission.BIND_JOB_SERVICE")
            A: http://schemas.android.com/apk/res/android:exported(0x01010010)=false
"#;

    fn source_violations(xml: &str) -> Vec<String> {
        manifest_violations("AndroidManifest.xml", &Manifest::from_source(xml), true)
            .into_iter()
            .map(|violation| violation.0)
            .collect()
    }

    fn assert_one(found: &[String], part: &str) {
        assert!(
            found.len() == 1 && found[0].contains(part),
            "expected one violation with {part:?}, found {found:#?}"
        );
    }

    #[test]
    fn the_apps_own_manifest_passes() {
        assert_eq!(source_violations(MANIFEST), Vec::<String>::new());
    }

    #[test]
    fn both_readers_read_the_same_access() {
        let source = Manifest::from_source(MANIFEST);
        let built = Manifest::from_aapt2(TREE);

        let names = |manifest: &Manifest| -> Vec<String> {
            manifest
                .permissions
                .iter()
                .map(|(_, name)| name.clone())
                .collect()
        };
        assert_eq!(names(&source), names(&built));
        assert_eq!(source.components.len(), 3);
        for (source, built) in source.components.iter().zip(&built.components) {
            assert_eq!(
                (
                    &source.kind,
                    &source.name,
                    source.exported,
                    &source.permission
                ),
                (&built.kind, &built.name, built.exported, &built.permission)
            );
            assert_eq!(source.filters, built.filters);
        }
        assert!(source.application.network_security_config);
        assert!(built.application.network_security_config);
        assert_eq!(
            manifest_violations("apk", &built, false),
            Vec::<Violation>::new()
        );
    }

    #[test]
    fn a_permission_srs_8_1_doesnt_list_is_refused() {
        // SEC-03
        let camera = MANIFEST.replace(
            "<queries>",
            r#"<uses-permission android:name="android.permission.CAMERA" />
    <queries>"#,
        );
        assert_one(
            &source_violations(&camera),
            "asks for `android.permission.CAMERA`",
        );

        let sdk_23 = MANIFEST.replace(
            "<queries>",
            r#"<uses-permission-sdk-23 android:name="android.permission.READ_CONTACTS" />
    <queries>"#,
        );
        assert_one(&source_violations(&sdk_23), "READ_CONTACTS");
    }

    #[test]
    fn an_allowed_permission_no_longer_asked_for_is_reported() {
        let without = MANIFEST.replace(
            r#"<uses-permission android:name="android.permission.SET_WALLPAPER" />"#,
            "",
        );
        assert_one(
            &source_violations(&without),
            "PERMISSIONS_ALLOWED lists `android.permission.SET_WALLPAPER`",
        );
    }

    #[test]
    fn a_component_other_apps_shouldnt_reach_is_refused() {
        // SEC-04
        let exported = MANIFEST.replace(
            r#"android:name=".ImageJobService"
            android:exported="false""#,
            r#"android:name=".ImageJobService"
            android:exported="true""#,
        );
        assert_one(
            &source_violations(&exported),
            "reach the service `ImageJobService`",
        );

        // Android exports a component with an intent filter unless it says otherwise.
        let receiver = MANIFEST.replace(
            "    </application>",
            r#"        <receiver android:name=".Boot">
            <intent-filter>
                <action android:name="android.intent.action.BOOT_COMPLETED" />
            </intent-filter>
        </receiver>
    </application>"#,
        );
        let found = source_violations(&receiver);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].contains("doesn't say whether the receiver `Boot` is exported"));
        assert!(found[1].contains("reach the receiver `Boot`"));
    }

    #[test]
    fn a_new_way_into_an_exported_component_is_refused() {
        // SEC-04: a filter added to the home activity makes a new entry.
        let view = MANIFEST.replace(
            "        </activity>",
            r#"            <intent-filter>
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.BROWSABLE" />
                <data android:scheme="https" android:host="www.bing.com" />
            </intent-filter>
        </activity>"#,
        );
        assert_one(
            &source_violations(&view),
            "with \"VIEW, BROWSABLE, data scheme=https host=www.bing.com\"",
        );

        let unguarded = MANIFEST.replace(
            r#"android:permission="android.permission.BIND_NOTIFICATION_LISTENER_SERVICE""#,
            "",
        );
        assert_one(
            &source_violations(&unguarded),
            "guards `NotificationListener` with no permission",
        );
    }

    #[test]
    fn an_allowed_entry_no_longer_there_is_reported() {
        let without = MANIFEST.replace(
            r#"            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
"#,
            "",
        );
        assert_one(
            &source_violations(&without),
            "EXPORTED_ALLOWED lets other apps reach `HomeActivity` with \"MAIN, LAUNCHER\"",
        );
    }

    #[test]
    fn a_component_must_say_whether_it_is_exported() {
        // SEC-04
        let unsaid = MANIFEST.replace(
            r#"            android:exported="false"
"#,
            "",
        );
        assert_one(
            &source_violations(&unsaid),
            "doesn't say whether the service `ImageJobService` is exported",
        );
    }

    #[test]
    fn roles_and_the_lock_screen_are_refused() {
        // SEC-03
        let accessibility = MANIFEST.replace(
            "android.permission.BIND_JOB_SERVICE",
            "android.permission.BIND_ACCESSIBILITY_SERVICE",
        );
        assert_one(
            &source_violations(&accessibility),
            "an accessibility service",
        );
        // SEC-05
        let over = MANIFEST.replace(
            r#"<activity android:name=".HomeActivity""#,
            r#"<activity android:showWhenLocked="true" android:name=".HomeActivity""#,
        );
        assert_one(&source_violations(&over), "show over the lock screen");
    }

    #[test]
    fn cleartext_and_debug_builds_are_refused() {
        // SEC-01
        let unconfigured = MANIFEST.replace(
            r#"android:networkSecurityConfig="@xml/network_security_config""#,
            "",
        );
        assert_one(
            &source_violations(&unconfigured),
            "no `android:networkSecurityConfig`",
        );
        let cleartext = MANIFEST.replace(
            r#"android:label="@string/app_name""#,
            r#"android:usesCleartextTraffic="true""#,
        );
        assert_one(&source_violations(&cleartext), "allows cleartext");
        // SEC-06
        let debuggable = TREE.replace(
            "        A: http://schemas.android.com/apk/res/android:label(0x01010001)=@0x7f06002d",
            "        A: http://schemas.android.com/apk/res/android:debuggable(0x0101000f)=true",
        );
        let found = manifest_violations("apk", &Manifest::from_aapt2(&debuggable), false);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].0.contains("debuggable"));
        let profileable = MANIFEST.replace(
            r#"        <service
            android:name=".ImageJobService""#,
            r#"        <profileable android:shell="true" />
        <service
            android:name=".ImageJobService""#,
        );
        assert_one(
            &source_violations(&profileable),
            "profileable from the shell",
        );
    }

    const CONFIG: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- <debug-overrides> would be an exception. -->
<network-security-config>
    <base-config cleartextTrafficPermitted="false">
        <trust-anchors>
            <certificates src="system" />
        </trust-anchors>
    </base-config>
</network-security-config>
"#;

    #[test]
    fn the_network_config_refuses_cleartext_and_trusts_only_the_system() {
        // SEC-01
        assert_eq!(
            network_config_violations("config.xml", CONFIG),
            Vec::<Violation>::new()
        );
        for (broken, part) in [
            (
                CONFIG.replace(r#"cleartextTrafficPermitted="false""#, ""),
                "doesn't refuse cleartext",
            ),
            (
                CONFIG.replace(
                    r#"<certificates src="system" />"#,
                    r#"<certificates src="system" /><certificates src="user" />"#,
                ),
                "other than the system's",
            ),
            (
                CONFIG.replace(
                    "</network-security-config>",
                    "<debug-overrides><trust-anchors><certificates src=\"user\" />\
                     </trust-anchors></debug-overrides></network-security-config>",
                ),
                "`debug-overrides`",
            ),
            (
                CONFIG.replace(
                    "</network-security-config>",
                    r#"<domain-config cleartextTrafficPermitted="true"><domain>example.com</domain></domain-config></network-security-config>"#,
                ),
                "`domain-config`",
            ),
        ] {
            let found = network_config_violations("config.xml", &broken);
            assert!(
                found.iter().any(|violation| violation.0.contains(part)),
                "{part}: {found:#?}"
            );
        }
    }

    const BRIDGE: &str = "android/app/src/main/java/io/github/twonet/meegone/Bridge.java";

    /// The violations in Java `text`, written in `file`, where only [`BRIDGE`] may connect.
    fn java(file: &str, text: &str) -> Vec<String> {
        java_violations(&[(file.to_owned(), text.to_owned())], &[(BRIDGE, "a test")])
            .into_iter()
            .map(|violation| violation.0)
            .filter(|violation| !violation.starts_with("NETWORK_ALLOWED") || file == BRIDGE)
            .collect()
    }

    #[test]
    fn forbidden_apis_are_refused_outside_comments_and_strings() {
        // SEC-02, SEC-05, PRIV-01, PRIV-03
        for (code, name) in [
            ("WebView view = new WebView(context);", "WebView"),
            ("text.setText(Html.fromHtml(title));", "fromHtml"),
            ("Linkify.addLinks(text, Linkify.WEB_URLS);", "Linkify"),
            ("activity.setShowWhenLocked(true);", "setShowWhenLocked"),
            (
                "String id = Settings.Secure.getString(resolver, Settings.Secure.ANDROID_ID);",
                "ANDROID_ID",
            ),
            ("e.printStackTrace();", "printStackTrace"),
        ] {
            let found = java("A.java", code);
            assert!(
                !found.is_empty() && found.iter().all(|found| found.contains(name)),
                "{code}: {found:#?}"
            );
        }
        assert_eq!(
            java(
                "A.java",
                "// A WebView would run pages.\nString s = \"fromHtml\"; MyWebViewish x;"
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn only_the_bridge_connects_to_the_network() {
        // PRIV-02
        let connect = "HttpsURLConnection c = (HttpsURLConnection) url.openConnection();";
        let found = java("A.java", connect);
        assert_eq!(found.len(), 3, "{found:#?}");
        assert!(found[0].contains("`A.java:1` connects to the network"));
        assert_eq!(java(BRIDGE, connect), Vec::<String>::new());
        assert_eq!(
            java(BRIDGE, "Uri link = Uri.parse(url);"),
            [
                "NETWORK_ALLOWED lets `android/app/src/main/java/io/github/twonet/meegone/Bridge.java` \
              connect to the network, but it doesn't any more; remove the entry (PRIV-02)"
            ]
        );
    }

    #[test]
    fn a_log_never_passes_an_exception() {
        // PRIV-03
        let found = java("A.java", "\n  Log.w(TAG, \"Couldn't list the apps\", e);");
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].contains("`A.java:2` logs an exception"));
        for fine in [
            "Log.w(TAG, \"Couldn't list the apps: \" + e.getClass().getSimpleName());",
            "Log.i(TAG, \"Closed; accepted: \" + accepted(a, b) + \", shown: \" + shown);",
            "Log.isLoggable(TAG, Log.DEBUG);",
            "Log.println(Log.WARN, TAG, \"x\");",
        ] {
            assert_eq!(java("A.java", fine), Vec::<String>::new(), "{fine}");
        }
    }

    #[test]
    fn auto_links_in_xml_are_refused() {
        // SEC-02
        let found = xml_violations(
            "res/layout/a.xml",
            "<!-- android:autoLink=\"web\" -->\n<TextView android:autoLink=\"web\" />",
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].0.contains("`res/layout/a.xml:2`"));
    }

    #[test]
    fn classes_only_the_debug_build_defines_are_listed() {
        let files: BTreeSet<String> = [
            "android/app/src/main/java/io/github/twonet/meegone/Bridge.java",
            "android/app/src/release/java/io/github/twonet/meegone/DebugHooks.java",
            "android/app/src/debug/java/io/github/twonet/meegone/DebugHooks.java",
            "android/app/src/debug/java/io/github/twonet/meegone/PerfProbe.java",
        ]
        .map(str::to_owned)
        .into();

        assert_eq!(
            debug_only_classes(&files),
            BTreeSet::from(["io.github.twonet.meegone.PerfProbe".to_owned()])
        );
    }

    #[test]
    fn a_debug_class_in_the_release_apk_is_refused() {
        // SEC-06
        let dexdump = "Processing 'app.apk'...\n\
            Class #0 header:\n\
            \x20 Class descriptor  : 'Lio/github/twonet/meegone/Bridge;'\n\
            \x20 Class descriptor  : 'Lio/github/twonet/meegone/PerfProbe$1;'\n";
        let debug_only = BTreeSet::from(["io.github.twonet.meegone.PerfProbe".to_owned()]);

        assert_eq!(
            dex_classes(dexdump),
            BTreeSet::from([
                "io.github.twonet.meegone.Bridge".to_owned(),
                "io.github.twonet.meegone.PerfProbe".to_owned(),
            ])
        );
        let found = release_apk_violations(TREE, dexdump, &debug_only);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found[0]
                .0
                .contains("holds `io.github.twonet.meegone.PerfProbe`")
        );
    }
}
