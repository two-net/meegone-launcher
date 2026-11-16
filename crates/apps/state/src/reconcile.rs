//! Keeping the layout in step with the installed apps (APPS-04 to APPS-08, APPS-18, APPS-21,
//! REL-02, FOLD-06; ADR 0012, ADR 0018).
//!
//! Pure logic, tested without a device (MAINT-01): the saved layout and what the system lists
//! go in, and the layout to save and show comes out.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};

use meegone_apps_data::{FolderId, Layout, LayoutEntry, Restore};
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
    /// The saved layout, restored from a backup that another copy of MeeGone made (APPS-21,
    /// ADR 0018). Its apps that aren't installed yet keep their places, out of sight, until
    /// `until_ms`. The apps it doesn't hold join the end alphabetically, and none is marked new:
    /// they were on this device before the backup arrived.
    Restored { until_ms: u64 },
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
/// - After a restore, a backed-up app that isn't installed yet keeps its place, out of sight,
///   while the restore waits for it (APPS-21). Installed, or under its new id, it shows there.
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
    let restoring = restoring(saved, start, &present, &renamed, profiles);
    let mut placed: HashSet<AppId> = HashSet::new();
    let mut still_restoring: BTreeSet<AppId> = BTreeSet::new();
    // The app this saved id stands for now, if it keeps a place and hasn't one already.
    let mut place = |id: &AppId| {
        let now = if present.contains_key(id) {
            id
        } else if let Some(new) = renamed.get(id) {
            new
        } else if waiting.contains(&id.profile) || restoring.contains(id) {
            id
        } else {
            return None;
        };
        if !placed.insert(now.clone()) {
            return None;
        }
        if restoring.contains(now) {
            still_restoring.insert(now.clone());
        }
        Some(now.clone())
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
        Start::Restored { .. } | Start::Rebuilt => {}
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

    let until_ms = match start {
        Start::Restored { until_ms } => Some(*until_ms),
        _ => saved.restore.as_ref().map(|restore| restore.until_ms),
    };
    let restore = until_ms
        .filter(|_| !still_restoring.is_empty())
        .map(|until_ms| Restore {
            until_ms,
            waiting: still_restoring,
        });

    Layout {
        entries,
        new,
        installed_ms: saved.installed_ms,
        restore,
    }
}

/// Whether `saved` came from a backup: another copy of MeeGone, installed at another time,
/// saved it (APPS-21, ADR 0018). A layout saved before version 3 has no install time, so only
/// this copy could have saved it.
pub fn is_restored(saved: &Layout, installed_ms: u64) -> bool {
    saved
        .installed_ms
        .is_some_and(|saved_by| saved_by != installed_ms)
}

/// `layout`, with its restore over once its wait has ended at `now_ms` (APPS-21). The apps it was
/// still waiting for are then gone, by the usual rule, at the next [`reconcile`].
pub fn end_restore(layout: Layout, now_ms: u64) -> Layout {
    match &layout.restore {
        Some(restore) if restore.until_ms <= now_ms => Layout {
            restore: None,
            ..layout
        },
        _ => layout,
    }
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
/// apps of the listed `profiles` into `reconciled` (APPS-19, ADR 0017). A hidden app keeps its
/// hiding exactly as [`reconcile`] keeps its place:
/// - while it is installed, or its profile can't list it yet (locked, or listing no apps);
/// - under its new id when an update renamed its launcher activity;
/// - while a restore waits for it (APPS-21).
///
/// Any other is forgotten, so an app that is uninstalled and installed again shows.
pub fn follow_hidden(
    hidden: &BTreeSet<AppId>,
    saved: &Layout,
    reconciled: &Layout,
    installed: &[App],
    profiles: &[Profile],
) -> BTreeSet<AppId> {
    let present: HashMap<&AppId, &App> = installed.iter().map(|app| (&app.id, app)).collect();
    let waiting = waiting_profiles(installed, profiles);
    let renamed = renamed(saved, &present, &waiting);
    let restoring = reconciled.restore.as_ref().map(|restore| &restore.waiting);
    hidden
        .iter()
        .filter_map(|id| {
            if present.contains_key(id)
                || waiting.contains(&id.profile)
                || restoring.is_some_and(|waiting| waiting.contains(id))
            {
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
    let mut arranged = layout.clone();
    for (&slot, entry) in slots.iter().zip(visible) {
        arranged.entries[slot] = entry.clone();
    }
    arranged
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

/// Every app `saved` places, in the grid or in a folder.
fn saved_ids(saved: &Layout) -> impl Iterator<Item = &AppId> {
    saved.entries.iter().flat_map(|entry| match entry {
        LayoutEntry::App(id) => std::slice::from_ref(id),
        LayoutEntry::Folder { apps, .. } => apps.as_slice(),
    })
}

/// The saved apps a restore waits for (APPS-21): those of a listed profile that aren't installed
/// and that no update renamed. At the restore that is every saved app so; afterwards, those it
/// was waiting for that still are.
fn restoring<'a>(
    saved: &'a Layout,
    start: &Start,
    present: &HashMap<&AppId, &App>,
    renamed: &HashMap<AppId, AppId>,
    profiles: &[Profile],
) -> HashSet<&'a AppId> {
    let listed: HashSet<ProfileId> = profiles.iter().map(|profile| profile.id).collect();
    let candidates: Box<dyn Iterator<Item = &AppId>> = match (start, &saved.restore) {
        (Start::Restored { .. }, _) => Box::new(saved_ids(saved)),
        (_, Some(restore)) => Box::new(restore.waiting.iter()),
        (_, None) => Box::new(std::iter::empty()),
    };
    candidates
        .filter(|id| {
            !present.contains_key(id) && !renamed.contains_key(*id) && listed.contains(&id.profile)
        })
        .collect()
}

/// Saved apps that are gone, each paired with the one app not placed yet that has the same
/// profile and package: an update renamed its launcher activity (APPS-08). An app a restore waits
/// for counts as gone here, so it takes the place of its package's app on this device.
fn renamed(
    saved: &Layout,
    present: &HashMap<&AppId, &App>,
    waiting: &HashSet<ProfileId>,
) -> HashMap<AppId, AppId> {
    let saved_ids: HashSet<&AppId> = saved_ids(saved).collect();
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

    /// [`follow_hidden`] after the usual reconcile of a saved layout.
    fn followed(
        hidden: &BTreeSet<AppId>,
        saved: &Layout,
        installed: &[App],
        profiles: &[Profile],
    ) -> BTreeSet<AppId> {
        let reconciled = reconcile(saved, installed, profiles, &Start::Saved);
        follow_hidden(hidden, saved, &reconciled, installed, profiles)
    }

    fn layout(entries: Vec<LayoutEntry>) -> Layout {
        Layout {
            entries,
            ..Layout::default()
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
            followed(&hidden, &saved, &personal, &[PERSONAL, locked]),
            hidden
        );
        // A profile that lists nothing at all is waiting too.
        assert_eq!(
            followed(&hidden, &saved, &personal, &[PERSONAL, WORK]),
            hidden
        );
        let mut all = personal.clone();
        all.push(docs);
        assert_eq!(followed(&hidden, &saved, &all, &[PERSONAL, WORK]), hidden);
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

        let still = followed(
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

        let still = followed(&[app_id("maps")].into(), &saved, &apps, &[PERSONAL]);

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

    /// How long the restores in these tests wait, from a start at 0.
    const UNTIL_MS: u64 = 1_000;

    fn restore(ids: &[&str]) -> Option<Restore> {
        Some(Restore {
            until_ms: UNTIL_MS,
            waiting: ids.iter().map(|id| app_id(id)).collect(),
        })
    }

    #[test]
    fn a_restore_keeps_the_places_of_apps_not_installed_yet() {
        // APPS-21: Chess and Maps aren't installed yet. Notes, which the backup doesn't hold,
        // joins the end, unmarked: it was on this device before the backup arrived.
        let saved = layout(vec![
            LayoutEntry::App(app_id("phone")),
            folder(1, &["chess", "go"]),
            LayoutEntry::App(app_id("maps")),
            LayoutEntry::App(app_id("mail")),
        ]);
        let apps = installed(&[
            ("phone", "Phone"),
            ("go", "Go"),
            ("mail", "Mail"),
            ("zoom", "Zoom"),
            ("notes", "Notes"),
        ]);

        let now = reconcile(
            &saved,
            &apps,
            &[PERSONAL],
            &Start::Restored { until_ms: UNTIL_MS },
        );

        assert_eq!(
            packages(&now),
            ["phone", "[1: chess go]", "maps", "mail", "notes", "zoom"]
        );
        assert!(now.new.is_empty());
        assert_eq!(now.restore, restore(&["chess", "maps"]));
        let cells: Vec<String> = grid(&now, &apps)
            .iter()
            .map(|item| item.label().to_owned())
            .collect();
        assert_eq!(cells, ["Phone", "Folder 1", "Mail", "Notes", "Zoom"]);
    }

    #[test]
    fn a_restore_doesnt_wait_for_another_devices_work_profile() {
        // APPS-21: profile ids are the system's serial numbers, which differ on a new device.
        let docs = work_app("docs", "Docs");
        let saved = layout(vec![
            LayoutEntry::App(docs.id.clone()),
            LayoutEntry::App(app_id("mail")),
        ]);

        let now = reconcile(
            &saved,
            &installed(&[("mail", "Mail")]),
            &[PERSONAL],
            &Start::Restored { until_ms: UNTIL_MS },
        );

        assert_eq!(packages(&now), ["mail"]);
        assert_eq!(now.restore, None);
    }

    #[test]
    fn a_restore_with_everything_installed_waits_for_nothing() {
        // APPS-21: as after reinstalling MeeGone on the same device.
        let saved = layout(entries(&["mail", "maps"]));

        let now = reconcile(
            &saved,
            &installed(&[("maps", "Maps"), ("mail", "Mail")]),
            &[PERSONAL],
            &Start::Restored { until_ms: UNTIL_MS },
        );

        assert_eq!(now, saved);
    }

    #[test]
    fn a_backed_up_app_takes_its_place_once_installed() {
        // APPS-21: in its folder, without a dot. An app the backup didn't hold is new.
        let mut saved = layout(vec![
            LayoutEntry::App(app_id("phone")),
            folder(1, &["chess", "go"]),
            LayoutEntry::App(app_id("maps")),
        ]);
        saved.restore = restore(&["chess", "maps"]);
        let mut apps = installed(&[("phone", "Phone"), ("go", "Go"), ("chess", "Chess")]);

        let chess = reconcile(&saved, &apps, &[PERSONAL], &Start::Saved);
        assert_eq!(packages(&chess), ["phone", "[1: chess go]", "maps"]);
        assert!(chess.new.is_empty());
        assert_eq!(chess.restore, restore(&["maps"]));

        apps.extend(installed(&[("maps", "Maps"), ("atlas", "Atlas")]));
        let all = reconcile(&chess, &apps, &[PERSONAL], &Start::Saved);
        assert_eq!(packages(&all), ["phone", "[1: chess go]", "maps", "atlas"]);
        assert_eq!(all.new, [app_id("atlas")].into());
        assert_eq!(all.restore, None);
    }

    #[test]
    fn a_backed_up_app_under_another_activity_takes_its_place() {
        // APPS-08, APPS-21: the version installed here names its launcher activity otherwise.
        let mut saved = layout(entries(&["maps", "mail"]));
        saved.restore = restore(&["maps"]);
        let mut apps = installed(&[("mail", "Mail"), ("maps", "Maps")]);
        apps[1].id.activity = "maps.Start".into();

        let now = reconcile(&saved, &apps, &[PERSONAL], &Start::Saved);

        assert_eq!(
            now.entries,
            [
                LayoutEntry::App(apps[1].id.clone()),
                LayoutEntry::App(app_id("mail"))
            ]
        );
        assert!(now.new.is_empty());
        assert_eq!(now.restore, None);
    }

    #[test]
    fn apps_still_missing_when_the_wait_ends_leave() {
        // APPS-21, FOLD-06: Go's folder turns back into Go.
        let mut saved = layout(vec![
            folder(1, &["chess", "go"]),
            LayoutEntry::App(app_id("maps")),
        ]);
        saved.restore = restore(&["chess", "maps"]);
        let apps = installed(&[("go", "Go")]);

        let waiting = end_restore(saved.clone(), UNTIL_MS - 1);
        assert_eq!(waiting, saved);
        assert_eq!(
            reconcile(&waiting, &apps, &[PERSONAL], &Start::Saved),
            saved
        );

        let ended = end_restore(saved, UNTIL_MS);
        assert_eq!(ended.restore, None);
        assert_eq!(
            packages(&reconcile(&ended, &apps, &[PERSONAL], &Start::Saved)),
            ["go"]
        );
    }

    #[test]
    fn only_a_layout_another_copy_saved_is_restored() {
        // APPS-21: one saved before version 3 has no install time, and only this copy saved it.
        let mut saved = layout(entries(&["mail"]));
        assert!(!is_restored(&saved, 5));
        saved.installed_ms = Some(5);
        assert!(!is_restored(&saved, 5));
        assert!(is_restored(&saved, 7));
    }

    #[test]
    fn a_hidden_app_a_restore_waits_for_stays_hidden() {
        // APPS-19, APPS-21: hidden on the old device, it stays hidden when it arrives here.
        let saved = layout(entries(&["maps", "mail"]));
        let hidden: BTreeSet<AppId> = [app_id("maps")].into();
        let apps = installed(&[("mail", "Mail")]);
        let start = Start::Restored { until_ms: UNTIL_MS };

        let restored = reconcile(&saved, &apps, &[PERSONAL], &start);
        assert_eq!(
            follow_hidden(&hidden, &saved, &restored, &apps, &[PERSONAL]),
            hidden
        );

        let ended = end_restore(restored, UNTIL_MS);
        let gone = reconcile(&ended, &apps, &[PERSONAL], &Start::Saved);
        assert!(follow_hidden(&hidden, &ended, &gone, &apps, &[PERSONAL]).is_empty());
    }

    #[test]
    fn restores_keep_every_backed_up_app_once_until_it_comes_or_the_wait_ends() {
        // APPS-21, MAINT-01: properties over many generated restores.
        let mut random = Random(17);
        let names: Vec<String> = (0..12).map(|i| format!("app{i:02}")).collect();
        let everything: Vec<App> = names.iter().map(|name| app(name, name)).collect();
        let ids = |layout: &Layout| -> Vec<AppId> { saved_ids(layout).cloned().collect() };
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
            let apps: Vec<App> = everything
                .iter()
                .filter(|_| random.below(3) != 0)
                .cloned()
                .collect();
            let is_installed = |id: &AppId| apps.iter().any(|app| app.id == *id);

            let restored = reconcile(
                &saved,
                &apps,
                &[PERSONAL],
                &Start::Restored { until_ms: UNTIL_MS },
            );

            let placed = ids(&restored);
            let unique: BTreeSet<&AppId> = placed.iter().collect();
            assert_eq!(unique.len(), placed.len(), "an app twice: {restored:?}");
            assert!(
                apps.iter().all(|app| unique.contains(&app.id)),
                "{restored:?}"
            );
            let missing: BTreeSet<AppId> = placed
                .iter()
                .filter(|id| !is_installed(id))
                .cloned()
                .collect();
            let waiting = restored.restore.clone().map(|restore| restore.waiting);
            assert_eq!(waiting.unwrap_or_default(), missing, "{restored:?}");
            // Reconciling again changes nothing.
            assert_eq!(
                reconcile(&restored, &apps, &[PERSONAL], &Start::Saved),
                restored
            );

            // Once every app is installed, each has its place, in the restored order.
            let arrived = reconcile(&restored, &everything, &[PERSONAL], &Start::Saved);
            assert_eq!(arrived.restore, None, "{arrived:?}");
            assert_eq!(ids(&arrived)[..placed.len()], placed[..], "{arrived:?}");

            // Once the wait ends, only installed apps are left.
            let ended = reconcile(
                &end_restore(restored, UNTIL_MS),
                &apps,
                &[PERSONAL],
                &Start::Saved,
            );
            assert_eq!(ended.restore, None);
            assert!(ids(&ended).iter().all(is_installed), "{ended:?}");
            assert_eq!(ids(&ended).len(), apps.len(), "{ended:?}");
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
