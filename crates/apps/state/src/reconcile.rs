//! Keeping the layout in step with the installed apps (APPS-04 to APPS-08, APPS-18, REL-02,
//! FOLD-06; ADR 0012).
//!
//! Pure logic, tested without a device (MAINT-01): the saved layout and what the system lists
//! go in, and the layout to save and show comes out.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};

use meegone_apps_data::{FolderId, Layout, LayoutEntry};
use meegone_core_data::{App, AppId, Profile, ProfileId};

use crate::GridItem;
use crate::folders::tidy;

/// How the layout being reconciled came about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// No layout was ever saved: the first start. The apps whose packages are listed come first,
    /// in that order, and the rest follow alphabetically (APPS-04, APPS-05).
    First { defaults: Vec<String> },
    /// The saved layout, read whole. Apps it doesn't hold yet join the end, oldest install first,
    /// and are marked new (APPS-06, APPS-07).
    Saved,
    /// What could be recovered from an unreadable layout, or nothing. The other apps join the
    /// end alphabetically, and none is marked new: they aren't new installs (REL-02).
    Rebuilt,
}

/// The layout `saved` becomes once it matches the `installed` apps of the listed `profiles`.
///
/// - The saved order is kept, and each app appears once (APPS-18).
/// - Apps that are gone are removed, also from folders. A folder left with one app becomes that
///   app, and an empty folder is deleted (APPS-08, FOLD-06). Folder ids stay unique.
/// - An app whose profile is locked, or lists no apps at all, keeps its place: the system can't
///   list it yet, as for a work profile with its own lock after a restart (APPS-18). It is in
///   the layout but not in the [`grid`]. Apps of profiles that no longer exist are removed.
/// - An update that renames an app's launcher activity keeps the app's place and mark: one app
///   gone, and one not placed yet, with the same profile and package (APPS-08).
/// - Apps not placed yet join the end, as `start` says.
pub fn reconcile(saved: &Layout, installed: &[App], profiles: &[Profile], start: &Start) -> Layout {
    tidy(&reconcile_draft(saved, installed, profiles, start))
}

/// [`reconcile`], except that a folder left with one app stays a folder: a drag in progress may
/// have left it so, and the drop tidies it (FOLD-05, ADR 0015). An empty folder still goes.
pub fn reconcile_draft(
    saved: &Layout,
    installed: &[App],
    profiles: &[Profile],
    start: &Start,
) -> Layout {
    let present: HashMap<&AppId, &App> = installed.iter().map(|app| (&app.id, app)).collect();
    let waiting = waiting_profiles(installed, profiles);
    let renamed = renamed(saved, &present, &waiting);
    let mut placed: HashSet<AppId> = HashSet::new();
    // The app this saved id stands for now, if it keeps a place and hasn't one already.
    let mut place = |id: &AppId| {
        let now = if present.contains_key(id) || waiting.contains(&id.profile) {
            id
        } else {
            renamed.get(id)?
        };
        placed.insert(now.clone()).then(|| now.clone())
    };
    let mut folder_ids = FolderIds::new(saved);

    let mut entries = Vec::new();
    for entry in &saved.entries {
        match entry {
            LayoutEntry::App(id) => entries.extend(place(id).map(LayoutEntry::App)),
            LayoutEntry::Folder { id, name, apps } => {
                let kept: Vec<AppId> = apps.iter().filter_map(&mut place).collect();
                if !kept.is_empty() {
                    entries.push(LayoutEntry::Folder {
                        id: folder_ids.unique(*id),
                        name: name.clone(),
                        apps: kept,
                    });
                }
            }
        }
    }

    // Inserting also leaves out an app the system listed twice.
    let mut joining: Vec<&App> = installed
        .iter()
        .filter(|app| placed.insert(app.id.clone()))
        .collect();
    joining.sort_by(|a, b| alphabetical(a, b));
    match start {
        Start::First { defaults } => joining = defaults_first(joining, defaults),
        Start::Saved => joining.sort_by_key(|app| app.installed_at_ms),
        Start::Rebuilt => {}
    }

    let mut new: BTreeSet<AppId> = saved
        .new
        .iter()
        .filter_map(|id| {
            let now = renamed.get(id).unwrap_or(id);
            placed.contains(now).then(|| now.clone())
        })
        .collect();
    if *start == Start::Saved {
        new.extend(joining.iter().map(|app| app.id.clone()));
    }
    entries.extend(
        joining
            .into_iter()
            .map(|app| LayoutEntry::App(app.id.clone())),
    );

    Layout { entries, new }
}

/// The cells to show for `layout`: its apps that show, and its folders that hold any. `shown`
/// is the installed apps the user hasn't hidden (APPS-19). Entries kept for a locked profile
/// stay out of sight until it unlocks, and hidden apps until they are unhidden; both keep their
/// slots.
pub fn grid(layout: &Layout, shown: &[App]) -> Vec<GridItem> {
    let by_id: HashMap<&AppId, &App> = shown.iter().map(|app| (&app.id, app)).collect();
    let app = |id: &AppId| by_id.get(id).map(|app| (*app).clone());
    layout
        .entries
        .iter()
        .filter_map(|entry| match entry {
            LayoutEntry::App(id) => app(id).map(|app| GridItem::App {
                app,
                new: layout.new.contains(id),
            }),
            LayoutEntry::Folder { id, name, apps } => {
                let apps: Vec<App> = apps.iter().filter_map(app).collect();
                (!apps.is_empty()).then(|| GridItem::Folder {
                    id: *id,
                    name: name.clone(),
                    apps,
                })
            }
        })
        .collect()
}

/// The `hidden` apps that still stand for an app once `saved` is reconciled with the `installed`
/// apps of the listed `profiles` (APPS-19, ADR 0017). A hidden app keeps its hiding exactly as
/// [`reconcile`] keeps its place:
/// - while it is installed, or its profile can't list it yet (locked, or listing no apps);
/// - under its new id when an update renamed its launcher activity.
///
/// Any other is forgotten, so an app that is uninstalled and installed again shows.
pub fn follow_hidden(
    hidden: &BTreeSet<AppId>,
    saved: &Layout,
    installed: &[App],
    profiles: &[Profile],
) -> BTreeSet<AppId> {
    let present: HashMap<&AppId, &App> = installed.iter().map(|app| (&app.id, app)).collect();
    let waiting = waiting_profiles(installed, profiles);
    let renamed = renamed(saved, &present, &waiting);
    hidden
        .iter()
        .filter_map(|id| {
            if present.contains_key(id) || waiting.contains(&id.profile) {
                Some(id.clone())
            } else {
                renamed.get(id).cloned()
            }
        })
        .collect()
}

/// Where each cell of `after` was in `before`, matched by the app or folder it shows, or `None`
/// for a cell that just joined the grid. The host moves each cell from its old place (ANIM-21).
///
/// A cell that changed what it is keeps its place, so the host reuses its view (ADR 0015): a
/// folder that just formed was the cell of its first app, and an app a folder turned back into
/// was that folder's cell (FOLD-06).
pub fn previous_places(before: &[GridItem], after: &[GridItem]) -> Vec<Option<usize>> {
    let mut places: HashMap<Key<'_>, usize> = HashMap::new();
    for (index, item) in before.iter().enumerate() {
        places.entry(Key::of(item)).or_insert(index);
    }
    let still: HashSet<Key<'_>> = after.iter().map(Key::of).collect();
    // The apps of each folder that has gone, and where that folder was.
    let mut gone_folders: HashMap<&AppId, usize> = HashMap::new();
    for (index, item) in before.iter().enumerate() {
        if let GridItem::Folder { apps, .. } = item {
            if !still.contains(&Key::of(item)) {
                for app in apps {
                    gone_folders.entry(&app.id).or_insert(index);
                }
            }
        }
    }
    after
        .iter()
        .map(|item| {
            places.get(&Key::of(item)).copied().or_else(|| match item {
                GridItem::Folder { apps, .. } => apps
                    .first()
                    .and_then(|first| places.get(&Key::App(&first.id)).copied()),
                GridItem::App { app, .. } => gone_folders.get(&app.id).copied(),
            })
        })
        .collect()
}

/// Whether any cell joined, left or changed place between `before` and `after`, so the grid
/// moves (ANIM-21, ANIM-13). A change of label, icon or availability alone doesn't.
pub fn cells_moved(before: &[GridItem], after: &[GridItem]) -> bool {
    before.len() != after.len()
        || previous_places(before, after)
            .iter()
            .enumerate()
            .any(|(place, from)| *from != Some(place))
}

/// `layout` in the order of the cells `shown` (APPS-12, APPS-18). Entries that show no cell,
/// such as the apps of a locked profile, keep their slots; the slots of the entries that do are
/// filled again in the order they are shown.
pub fn follow_order(layout: &Layout, shown: &[GridItem]) -> Layout {
    let order: HashMap<Key<'_>, usize> = shown
        .iter()
        .enumerate()
        .map(|(place, item)| (Key::of(item), place))
        .collect();
    let slots: Vec<usize> = layout
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| order.contains_key(&Key::of_entry(entry)))
        .map(|(slot, _)| slot)
        .collect();
    let mut visible: Vec<&LayoutEntry> = slots.iter().map(|&slot| &layout.entries[slot]).collect();
    visible.sort_by_key(|entry| order[&Key::of_entry(entry)]);
    let mut entries = layout.entries.clone();
    for (&slot, entry) in slots.iter().zip(visible) {
        entries[slot] = entry.clone();
    }
    Layout {
        entries,
        new: layout.new.clone(),
    }
}

/// What identifies a cell across changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key<'a> {
    App(&'a AppId),
    Folder(FolderId),
}

impl<'a> Key<'a> {
    fn of(item: &'a GridItem) -> Self {
        match item {
            GridItem::App { app, .. } => Key::App(&app.id),
            GridItem::Folder { id, .. } => Key::Folder(*id),
        }
    }

    /// The key of the cell `entry` shows, if it shows one.
    fn of_entry(entry: &'a LayoutEntry) -> Self {
        match entry {
            LayoutEntry::App(id) => Key::App(id),
            LayoutEntry::Folder { id, .. } => Key::Folder(*id),
        }
    }
}

/// The locale's alphabetical order by label, from the platform's collation keys (L10N-04).
/// Equal labels put the personal app first; the id makes the order total.
pub(crate) fn alphabetical(a: &App, b: &App) -> Ordering {
    (&a.sort_key, &a.label, a.work, &a.id).cmp(&(&b.sort_key, &b.label, b.work, &b.id))
}

/// The listed profiles whose apps the system can't list yet: locked, or listing none at all.
fn waiting_profiles(installed: &[App], profiles: &[Profile]) -> HashSet<ProfileId> {
    let listing: HashSet<ProfileId> = installed.iter().map(|app| app.id.profile).collect();
    profiles
        .iter()
        .filter(|profile| profile.locked || !listing.contains(&profile.id))
        .map(|profile| profile.id)
        .collect()
}

/// Saved apps that are gone, each paired with the one app not placed yet that has the same
/// profile and package: an update renamed its launcher activity (APPS-08).
fn renamed(
    saved: &Layout,
    present: &HashMap<&AppId, &App>,
    waiting: &HashSet<ProfileId>,
) -> HashMap<AppId, AppId> {
    let saved_ids: HashSet<&AppId> = saved
        .entries
        .iter()
        .flat_map(|entry| match entry {
            LayoutEntry::App(id) => std::slice::from_ref(id),
            LayoutEntry::Folder { apps, .. } => apps.as_slice(),
        })
        .collect();
    type Package<'a> = (ProfileId, &'a str);
    let mut gone: HashMap<Package<'_>, Vec<&AppId>> = HashMap::new();
    for id in &saved_ids {
        if !present.contains_key(id) && !waiting.contains(&id.profile) {
            gone.entry((id.profile, &id.package)).or_default().push(id);
        }
    }
    let mut unplaced: HashMap<Package<'_>, Vec<&AppId>> = HashMap::new();
    for id in present.keys() {
        if !saved_ids.contains(id) {
            unplaced
                .entry((id.profile, &id.package))
                .or_default()
                .push(id);
        }
    }
    gone.into_iter()
        .filter_map(
            |(package, gone)| match (gone.as_slice(), unplaced.get(&package)?.as_slice()) {
                ([old], [new]) => Some(((*old).clone(), (*new).clone())),
                _ => None,
            },
        )
        .collect()
}

/// The default apps first, in the order listed, then the rest as they are (APPS-05). Each is the
/// personal app of its package, the first alphabetically if it has several.
fn defaults_first<'a>(joining: Vec<&'a App>, defaults: &[String]) -> Vec<&'a App> {
    let mut first: Vec<&App> = Vec::new();
    for package in defaults {
        let default = joining
            .iter()
            .find(|app| !app.work && app.id.package == *package)
            .filter(|app| !first.iter().any(|chosen| chosen.id == app.id));
        first.extend(default);
    }
    let rest: Vec<&App> = joining
        .into_iter()
        .filter(|app| !first.iter().any(|chosen| chosen.id == app.id))
        .collect();
    first.extend(rest);
    first
}

/// Gives each folder an id no other folder has: recovery can bring back two with the same id.
pub(crate) struct FolderIds {
    used: HashSet<FolderId>,
    next: u32,
}

impl FolderIds {
    pub(crate) fn new(saved: &Layout) -> Self {
        let highest = saved
            .entries
            .iter()
            .filter_map(|entry| match entry {
                LayoutEntry::Folder { id, .. } => Some(id.0),
                LayoutEntry::App(_) => None,
            })
            .max();
        Self {
            used: HashSet::new(),
            next: highest.map_or(0, |id| id.saturating_add(1)),
        }
    }

    pub(crate) fn unique(&mut self, id: FolderId) -> FolderId {
        if self.used.insert(id) {
            return id;
        }
        while !self.used.insert(FolderId(self.next)) {
            self.next = self.next.wrapping_add(1);
        }
        FolderId(self.next)
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::{WORK_PROFILE, app, app_id, work_app};

    use super::*;

    const PERSONAL: Profile = Profile {
        id: ProfileId(0),
        work: false,
        paused: false,
        locked: false,
    };

    const WORK: Profile = Profile {
        id: WORK_PROFILE,
        work: true,
        paused: false,
        locked: false,
    };

    fn installed(apps: &[(&str, &str)]) -> Vec<App> {
        apps.iter()
            .map(|(package, label)| app(package, label))
            .collect()
    }

    fn entries(ids: &[&str]) -> Vec<LayoutEntry> {
        ids.iter().map(|id| LayoutEntry::App(app_id(id))).collect()
    }

    fn layout(entries: Vec<LayoutEntry>) -> Layout {
        Layout {
            entries,
            new: BTreeSet::new(),
        }
    }

    fn folder(id: u32, ids: &[&str]) -> LayoutEntry {
        LayoutEntry::Folder {
            id: FolderId(id),
            name: format!("Folder {id}"),
            apps: ids.iter().map(|id| app_id(id)).collect(),
        }
    }

    fn packages(layout: &Layout) -> Vec<String> {
        layout
            .entries
            .iter()
            .map(|entry| match entry {
                LayoutEntry::App(id) => id.package.clone(),
                LayoutEntry::Folder { id, apps, .. } => format!(
                    "[{}: {}]",
                    id.0,
                    apps.iter()
                        .map(|app| app.package.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
            })
            .collect()
    }

    fn marked(layout: &Layout) -> Vec<&str> {
        layout.new.iter().map(|id| id.package.as_str()).collect()
    }

    #[test]
    fn the_first_grid_is_alphabetical_by_the_locales_order() {
        // APPS-04, L10N-04: the platform's keys decide, not the labels' bytes.
        let mut apps = installed(&[("zoom", "Zoom"), ("eclair", "Éclair"), ("docs", "docs")]);
        apps[1].sort_key = b"eclair".to_vec();

        let first = reconcile(
            &Layout::default(),
            &apps,
            &[PERSONAL],
            &Start::First {
                defaults: Vec::new(),
            },
        );

        assert_eq!(packages(&first), ["docs", "eclair", "zoom"]);
        assert!(first.new.is_empty());
    }

    #[test]
    fn the_default_apps_come_first_on_the_first_start() {
        // APPS-05: phone, messaging, browser, camera, where the system identifies them. A work
        // app of the same package stays in alphabetical order.
        let mut apps = installed(&[
            ("atlas", "Atlas"),
            ("browser", "Browser"),
            ("camera", "Camera"),
            ("dialer", "Phone"),
            ("sms", "Messages"),
        ]);
        apps.push(work_app("browser", "Browser"));
        let defaults = ["dialer", "sms", "browser", "missing", "camera", "dialer"]
            .map(String::from)
            .to_vec();

        let first = reconcile(
            &Layout::default(),
            &apps,
            &[PERSONAL, WORK],
            &Start::First { defaults },
        );

        assert_eq!(
            packages(&first),
            ["dialer", "sms", "browser", "camera", "atlas", "browser"]
        );
        assert_eq!(
            first.entries[5],
            LayoutEntry::App(work_app("browser", "").id)
        );
    }

    #[test]
    fn the_saved_order_is_kept_and_apps_that_are_gone_are_removed() {
        // APPS-08, APPS-18
        let saved = layout(entries(&["phone", "gone", "mail", "phone"]));

        let now = reconcile(
            &saved,
            &installed(&[("mail", "Mail"), ("phone", "Phone")]),
            &[PERSONAL],
            &Start::Saved,
        );

        assert_eq!(packages(&now), ["phone", "mail"]);
    }

    #[test]
    fn new_apps_join_the_end_oldest_install_first_and_are_marked() {
        // APPS-06, APPS-07: the newest app is last.
        let mut apps = installed(&[("mail", "Mail"), ("zoom", "Zoom"), ("atlas", "Atlas")]);
        apps[1].installed_at_ms = 100;
        apps[2].installed_at_ms = 200;
        let saved = layout(entries(&["mail"]));

        let now = reconcile(&saved, &apps, &[PERSONAL], &Start::Saved);

        assert_eq!(packages(&now), ["mail", "zoom", "atlas"]);
        assert_eq!(marked(&now), ["atlas", "zoom"]);
    }

    #[test]
    fn apps_installed_at_the_same_time_join_alphabetically() {
        // APPS-06, L10N-04
        let saved = layout(entries(&["mail"]));

        let now = reconcile(
            &saved,
            &installed(&[("zoom", "Zoom"), ("mail", "Mail"), ("atlas", "atlas")]),
            &[PERSONAL],
            &Start::Saved,
        );

        assert_eq!(packages(&now), ["mail", "atlas", "zoom"]);
    }

    #[test]
    fn a_mark_stays_until_opened_and_goes_with_the_app() {
        // APPS-07
        let mut saved = layout(entries(&["mail", "atlas", "zoom"]));
        saved.new = [app_id("atlas"), app_id("zoom")].into();

        let now = reconcile(
            &saved,
            &installed(&[("mail", "Mail"), ("atlas", "Atlas")]),
            &[PERSONAL],
            &Start::Saved,
        );

        assert_eq!(packages(&now), ["mail", "atlas"]);
        assert_eq!(marked(&now), ["atlas"]);
    }

    #[test]
    fn a_rebuilt_layout_keeps_what_was_recovered_and_marks_nothing() {
        // REL-02: the rest joins alphabetically; those apps aren't new installs.
        let mut recovered = layout(entries(&["zoom"]));
        recovered.new = [app_id("zoom")].into();
        let mut apps = installed(&[("mail", "Mail"), ("zoom", "Zoom"), ("atlas", "Atlas")]);
        // Install times don't matter here: as new installs, Mail would come before Atlas.
        apps[0].installed_at_ms = 100;
        apps[2].installed_at_ms = 200;

        let now = reconcile(&recovered, &apps, &[PERSONAL], &Start::Rebuilt);

        assert_eq!(packages(&now), ["zoom", "atlas", "mail"]);
        assert_eq!(marked(&now), ["zoom"]);
    }

    #[test]
    fn folders_lose_apps_that_are_gone_and_collapse() {
        // APPS-08, FOLD-06
        let saved = layout(vec![
            folder(1, &["maps", "gone"]),
            folder(2, &["chess", "go"]),
            folder(3, &["gone"]),
            folder(4, &["mail", "maps", "docs", "go"]),
        ]);

        let now = reconcile(
            &saved,
            &installed(&[
                ("maps", "Maps"),
                ("chess", "Chess"),
                ("go", "Go"),
                ("mail", "Mail"),
                ("docs", "Docs"),
            ]),
            &[PERSONAL],
            &Start::Saved,
        );

        assert_eq!(packages(&now), ["maps", "[2: chess go]", "[4: mail docs]"]);
    }

    #[test]
    fn folders_that_share_an_id_get_one_each() {
        // REL-02: recovery can bring back two folders with the same id.
        let saved = layout(vec![
            folder(3, &["a", "b"]),
            folder(3, &["c", "d"]),
            folder(4, &["e", "f"]),
        ]);
        let apps = installed(&[
            ("a", "A"),
            ("b", "B"),
            ("c", "C"),
            ("d", "D"),
            ("e", "E"),
            ("f", "F"),
        ]);

        let now = reconcile(&saved, &apps, &[PERSONAL], &Start::Rebuilt);

        assert_eq!(packages(&now), ["[3: a b]", "[5: c d]", "[4: e f]"]);
        let far = layout(vec![
            folder(u32::MAX, &["a", "b"]),
            folder(u32::MAX, &["c", "d"]),
        ]);
        assert_eq!(
            packages(&reconcile(&far, &apps, &[PERSONAL], &Start::Rebuilt))[..2],
            [format!("[{}: a b]", u32::MAX), "[0: c d]".to_owned()]
        );
    }

    #[test]
    fn a_locked_profile_keeps_its_apps_places_until_it_unlocks() {
        // APPS-18: after a restart, a work profile with its own lock lists no apps.
        let docs = work_app("docs", "Docs");
        let mut saved = layout(vec![
            LayoutEntry::App(docs.id.clone()),
            LayoutEntry::App(app_id("mail")),
        ]);
        saved.new = [docs.id.clone()].into();
        let locked = Profile {
            locked: true,
            ..WORK
        };
        let personal = installed(&[("mail", "Mail")]);

        let while_locked = reconcile(&saved, &personal, &[PERSONAL, locked], &Start::Saved);
        assert_eq!(while_locked, saved);
        assert_eq!(grid(&while_locked, &personal).len(), 1);

        // A profile that lists nothing at all is waiting too.
        let silent = reconcile(&saved, &personal, &[PERSONAL, WORK], &Start::Saved);
        assert_eq!(silent, saved);

        let mut all = personal.clone();
        all.push(docs);
        let unlocked = reconcile(&while_locked, &all, &[PERSONAL, WORK], &Start::Saved);
        assert_eq!(unlocked, saved);
        assert_eq!(grid(&unlocked, &all).len(), 2);
    }

    #[test]
    fn apps_of_a_profile_that_no_longer_exists_are_removed() {
        // APPS-18: the work profile was deleted.
        let saved = layout(vec![
            LayoutEntry::App(work_app("docs", "Docs").id),
            LayoutEntry::App(app_id("mail")),
        ]);

        let now = reconcile(
            &saved,
            &installed(&[("mail", "Mail")]),
            &[PERSONAL],
            &Start::Saved,
        );

        assert_eq!(packages(&now), ["mail"]);
    }

    #[test]
    fn an_update_that_renames_the_activity_keeps_the_place_and_mark() {
        // APPS-08
        let mut saved = layout(vec![
            LayoutEntry::App(app_id("mail")),
            folder(1, &["maps", "chess"]),
            LayoutEntry::App(app_id("go")),
        ]);
        saved.new = [app_id("mail")].into();
        let mut apps = installed(&[("go", "Go"), ("maps", "Maps"), ("mail", "Mail")]);
        apps[1].id.activity = "maps.Start".into();
        apps[2].id.activity = "mail.Inbox".into();
        let mut chess = app("chess", "Chess");
        chess.id.activity = "chess.Board".into();
        apps.push(chess);

        let now = reconcile(&saved, &apps, &[PERSONAL], &Start::Saved);

        assert_eq!(packages(&now), ["mail", "[1: maps chess]", "go"]);
        assert_eq!(now.entries[0], LayoutEntry::App(apps[2].id.clone()));
        assert_eq!(now.new, [apps[2].id.clone()].into());
    }

    #[test]
    fn a_hidden_app_stays_hidden_while_it_keeps_its_place() {
        // APPS-19: installed, or of a profile that can't list it yet.
        let docs = work_app("docs", "Docs");
        let saved = layout(vec![
            LayoutEntry::App(docs.id.clone()),
            folder(1, &["maps", "chess"]),
        ]);
        let hidden: BTreeSet<AppId> = [docs.id.clone(), app_id("maps")].into();
        let personal = installed(&[("maps", "Maps"), ("chess", "Chess")]);
        let locked = Profile {
            locked: true,
            ..WORK
        };

        assert_eq!(
            follow_hidden(&hidden, &saved, &personal, &[PERSONAL, locked]),
            hidden
        );
        // A profile that lists nothing at all is waiting too.
        assert_eq!(
            follow_hidden(&hidden, &saved, &personal, &[PERSONAL, WORK]),
            hidden
        );
        let mut all = personal.clone();
        all.push(docs);
        assert_eq!(
            follow_hidden(&hidden, &saved, &all, &[PERSONAL, WORK]),
            hidden
        );
    }

    #[test]
    fn a_hidden_app_that_is_gone_is_forgotten() {
        // APPS-19: so installing it again shows it. Its profile going counts too.
        let docs = work_app("docs", "Docs");
        let saved = layout(vec![
            LayoutEntry::App(docs.id.clone()),
            LayoutEntry::App(app_id("maps")),
            LayoutEntry::App(app_id("mail")),
        ]);
        let hidden: BTreeSet<AppId> = [docs.id.clone(), app_id("maps"), app_id("mail")].into();

        let still = follow_hidden(
            &hidden,
            &saved,
            &installed(&[("mail", "Mail")]),
            &[PERSONAL],
        );

        assert_eq!(still, [app_id("mail")].into());
    }

    #[test]
    fn a_hidden_app_whose_activity_is_renamed_stays_hidden() {
        // APPS-08, APPS-19
        let saved = layout(entries(&["maps", "mail"]));
        let mut apps = installed(&[("maps", "Maps"), ("mail", "Mail")]);
        apps[0].id.activity = "maps.Start".into();

        let still = follow_hidden(&[app_id("maps")].into(), &saved, &apps, &[PERSONAL]);

        assert_eq!(still, [apps[0].id.clone()].into());
    }

    #[test]
    fn a_package_with_two_new_activities_isnt_taken_for_a_rename() {
        // APPS-08: which one took the place can't be told, so both join the end.
        let saved = layout(entries(&["suite", "mail"]));
        let mut apps = installed(&[("mail", "Mail"), ("suite", "Docs"), ("suite", "Sheets")]);
        apps[1].id.activity = "suite.Docs".into();
        apps[2].id.activity = "suite.Sheets".into();

        let now = reconcile(&saved, &apps, &[PERSONAL], &Start::Saved);

        assert_eq!(packages(&now), ["mail", "suite", "suite"]);
    }

    #[test]
    fn the_same_app_listed_twice_gets_one_cell() {
        let apps = installed(&[("mail", "Mail"), ("mail", "Mail")]);

        let first = reconcile(
            &Layout::default(),
            &apps,
            &[PERSONAL],
            &Start::First {
                defaults: Vec::new(),
            },
        );

        assert_eq!(packages(&first), ["mail"]);
    }

    /// A small generator, so the properties below run on many layouts without a dependency.
    struct Random(u64);

    impl Random {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            ((self.0 >> 33) % n as u64) as usize
        }
    }

    #[test]
    fn every_installed_app_appears_once_and_the_kept_order_holds() {
        // APPS-18, MAINT-01: properties over many generated layouts.
        let mut random = Random(11);
        let names: Vec<String> = (0..12).map(|i| format!("app{i:02}")).collect();
        for _ in 0..2_000 {
            let pick = |random: &mut Random| app_id(&names[random.below(names.len())]);
            let mut entries = Vec::new();
            for _ in 0..random.below(10) {
                entries.push(if random.below(4) == 0 {
                    LayoutEntry::Folder {
                        id: FolderId(random.below(3) as u32),
                        name: String::new(),
                        apps: (0..random.below(4)).map(|_| pick(&mut random)).collect(),
                    }
                } else {
                    LayoutEntry::App(pick(&mut random))
                });
            }
            let saved = layout(entries);
            let apps: Vec<App> = names
                .iter()
                .filter(|_| random.below(3) != 0)
                .map(|name| app(name, name))
                .collect();

            let now = reconcile(&saved, &apps, &[PERSONAL], &Start::Saved);

            let mut shown: Vec<&AppId> = Vec::new();
            for entry in &now.entries {
                match entry {
                    LayoutEntry::App(id) => shown.push(id),
                    LayoutEntry::Folder { apps, .. } => {
                        assert!(apps.len() > 1, "{now:?}");
                        shown.extend(apps);
                    }
                }
            }
            let mut sorted = shown.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), shown.len(), "an app twice: {now:?}");
            assert!(
                shown.iter().all(|id| apps.iter().any(|app| app.id == **id)),
                "an app that isn't installed: {now:?}"
            );
            assert_eq!(shown.len(), apps.len(), "{saved:?} became {now:?}");
            // Apps kept from the saved layout stay in the order they first appeared there.
            let first_seen: Vec<&AppId> = saved
                .entries
                .iter()
                .flat_map(|entry| match entry {
                    LayoutEntry::App(id) => std::slice::from_ref(id),
                    LayoutEntry::Folder { apps, .. } => apps.as_slice(),
                })
                .fold(Vec::new(), |mut seen, id| {
                    if !seen.contains(&id) && shown.contains(&id) {
                        seen.push(id);
                    }
                    seen
                });
            assert_eq!(shown[..first_seen.len()], first_seen[..], "{saved:?}");
            let ids: HashSet<FolderId> = now
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    LayoutEntry::Folder { id, .. } => Some(*id),
                    LayoutEntry::App(_) => None,
                })
                .collect();
            let folders = now
                .entries
                .iter()
                .filter(|entry| matches!(entry, LayoutEntry::Folder { .. }))
                .count();
            assert_eq!(ids.len(), folders, "folder ids repeat: {now:?}");
            // Reconciling again changes nothing.
            assert_eq!(reconcile(&now, &apps, &[PERSONAL], &Start::Saved), now);
        }
    }

    fn cell(package: &str) -> GridItem {
        GridItem::App {
            app: app(package, package),
            new: false,
        }
    }

    fn cells(packages: &[&str]) -> Vec<GridItem> {
        packages.iter().map(|package| cell(package)).collect()
    }

    #[test]
    fn previous_places_follow_each_app_and_folder() {
        // ANIM-21
        let games = GridItem::Folder {
            id: FolderId(1),
            name: "Games".into(),
            apps: vec![app("chess", "Chess"), app("go", "Go")],
        };
        let mut before = cells(&["mail", "maps", "phone"]);
        before.insert(1, games.clone());
        let mut after = cells(&["mail", "phone", "atlas"]);
        after.insert(0, games);

        assert_eq!(
            previous_places(&before, &after),
            [Some(1), Some(0), Some(3), None]
        );
    }

    #[test]
    fn only_cells_coming_going_or_changing_place_move_the_grid() {
        // ANIM-21, ANIM-13: an update's new label or accent changes no place.
        let before = cells(&["mail", "maps"]);
        let mut relabelled = before.clone();
        if let GridItem::App { app, new } = &mut relabelled[0] {
            app.label = "Email".into();
            *new = true;
        }

        assert!(!cells_moved(&before, &relabelled));
        assert!(cells_moved(&before, &cells(&["mail"])));
        assert!(cells_moved(&before, &cells(&["mail", "maps", "atlas"])));
        assert!(cells_moved(&before, &cells(&["mail", "atlas"])));
        assert!(cells_moved(&before, &cells(&["maps", "mail"])));
        assert!(!cells_moved(&[], &[]));
    }

    #[test]
    fn following_the_shown_order_keeps_hidden_entries_in_their_slots() {
        // APPS-12, APPS-18, ADR 0012: the work apps of a locked profile show no cell, and keep
        // their places among the others.
        let mut saved = layout(vec![
            LayoutEntry::App(app_id("camera")),
            LayoutEntry::App(work_app("mail", "Mail").id),
            folder(1, &["chess", "go"]),
            LayoutEntry::App(app_id("maps")),
            LayoutEntry::App(work_app("docs", "Docs").id),
            LayoutEntry::App(app_id("phone")),
        ]);
        saved.new.insert(app_id("maps"));
        let mut grid = cells(&["camera", "maps", "phone"]);
        grid.insert(
            1,
            GridItem::Folder {
                id: FolderId(1),
                name: "Folder 1".into(),
                apps: vec![app("chess", "Chess"), app("go", "Go")],
            },
        );
        let phone = grid.remove(3);
        grid.insert(0, phone);

        let kept = follow_order(&saved, &grid);

        assert_eq!(
            packages(&kept),
            ["phone", "mail", "camera", "[1: chess go]", "docs", "maps"]
        );
        assert_eq!(kept.entries[1], saved.entries[1]);
        assert_eq!(kept.entries[4], saved.entries[4]);
        // The marks stay.
        assert_eq!(kept.new, saved.new);
        // The order shown already is the layout's.
        assert_eq!(follow_order(&kept, &grid), kept);
    }

    #[test]
    fn a_folder_that_forms_or_collapses_keeps_its_cells_place() {
        // ADR 0015: Maps was dropped onto Mail; later the folder turned back into Mail.
        let folder = |apps: &[&str]| GridItem::Folder {
            id: FolderId(1),
            name: String::new(),
            apps: apps.iter().map(|package| app(package, package)).collect(),
        };
        let before = cells(&["camera", "mail", "maps"]);
        let formed = vec![cell("camera"), folder(&["mail", "maps"])];
        assert_eq!(previous_places(&before, &formed), [Some(0), Some(1)]);

        let collapsed = cells(&["camera", "mail", "maps"]);
        let before = vec![cell("camera"), folder(&["mail"]), cell("maps")];
        assert_eq!(
            previous_places(&before, &collapsed),
            [Some(0), Some(1), Some(2)]
        );
        assert!(!cells_moved(&before, &collapsed));
    }
}
