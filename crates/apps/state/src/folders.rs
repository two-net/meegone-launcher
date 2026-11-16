//! Folder and arrangement edits (APPS-12, FOLD-01, FOLD-05, FOLD-06; ADR 0014, ADR 0015).
//!
//! Pure logic, tested without a device (MAINT-01): a layout and the apps that show go in, and
//! the edited layout comes out. The apps that show are the installed ones the user hasn't hidden
//! (APPS-19); entries that show no cell keep their slots. A drag edits a draft with these step by step; the drop tidies it
//! and saves it once. Each edit returns `None` when it can't apply, changing nothing.

use std::collections::HashSet;

use meegone_apps_data::{FolderId, Layout, LayoutEntry};
use meegone_core_data::{App, AppId};

use crate::reconcile::{follow_order, grid};
use crate::{CellKey, GridItem};

/// `layout` with `key`'s cell shown at place `to` among the cells the grid shows, or as far as
/// it goes, the cells between moving aside (APPS-12). Entries that show no cell keep their
/// slots.
pub fn arrange(layout: &Layout, shown: &[App], key: &CellKey, to: usize) -> Option<Layout> {
    let mut cells = grid(layout, shown);
    let from = cells.iter().position(|item| item.is(key))?;
    let cell = cells.remove(from);
    cells.insert(to.min(cells.len()), cell);
    Some(follow_order(layout, &cells))
}

/// `layout` with `app`, which has a cell of its own, dropped onto `onto` (FOLD-01, FOLD-05):
/// - onto another app's own cell, a new folder named `""` takes that app's slot and holds both,
///   that app first (FOLD-03);
/// - onto a folder, the app joins its end.
///
/// A folder never goes into anything, and nothing goes into an app inside a folder.
pub fn combine(layout: &Layout, app: &AppId, onto: &CellKey) -> Option<Layout> {
    let own = layout.entries.iter().position(|entry| is_app(entry, app))?;
    let mut entries = layout.entries.clone();
    match onto {
        CellKey::App(other) if other != app => {
            let at = entries.iter().position(|entry| is_app(entry, other))?;
            entries[at] = LayoutEntry::Folder {
                id: new_folder_id(layout),
                name: String::new(),
                apps: vec![other.clone(), app.clone()],
            };
        }
        CellKey::Folder(folder) => {
            let Some(LayoutEntry::Folder { apps, .. }) =
                entries.iter_mut().find(|entry| is_folder(entry, *folder))
            else {
                return None;
            };
            apps.push(app.clone());
        }
        CellKey::App(_) => return None,
    }
    entries.remove(own);
    Some(with_entries(layout, entries))
}

/// `layout` with `app` taken out of its folder and shown at place `to` in the grid (FOLD-05).
/// The folder may be left with one app, or none, until [`tidy`].
pub fn take_out(layout: &Layout, shown: &[App], app: &AppId, to: usize) -> Option<Layout> {
    let mut entries = layout.entries.clone();
    let at = entries.iter().position(
        |entry| matches!(entry, LayoutEntry::Folder { apps, .. } if apps.contains(app)),
    )?;
    if let LayoutEntry::Folder { apps, .. } = &mut entries[at] {
        apps.retain(|id| id != app);
    }
    entries.insert(at + 1, LayoutEntry::App(app.clone()));
    let out = with_entries(layout, entries);
    arrange(&out, shown, &CellKey::App(app.clone()), to)
}

/// `layout` with `app` out of its folder, just after it in the grid, as a screen reader's "Move
/// out of folder" puts it (A11Y-05).
pub fn move_out(layout: &Layout, shown: &[App], app: &AppId) -> Option<Layout> {
    let folder = folder_of(layout, app)?;
    let place = grid(layout, shown)
        .iter()
        .position(|item| item.is(&CellKey::Folder(folder)))?;
    take_out(layout, shown, app, place + 1)
}

/// `layout` with `app`, in a folder, at place `to` among the folder's apps that show, or as far
/// as it goes (FOLD-05). Apps that don't show keep their slots.
pub fn arrange_in_folder(layout: &Layout, shown: &[App], app: &AppId, to: usize) -> Option<Layout> {
    let showing: HashSet<&AppId> = shown.iter().map(|app| &app.id).collect();
    let mut entries = layout.entries.clone();
    let Some(LayoutEntry::Folder { apps, .. }) = entries
        .iter_mut()
        .find(|entry| matches!(entry, LayoutEntry::Folder { apps, .. } if apps.contains(app)))
    else {
        return None;
    };
    let slots: Vec<usize> = (0..apps.len())
        .filter(|&slot| showing.contains(&apps[slot]))
        .collect();
    let mut visible: Vec<AppId> = slots.iter().map(|&slot| apps[slot].clone()).collect();
    let from = visible.iter().position(|id| id == app)?;
    let moved = visible.remove(from);
    visible.insert(to.min(visible.len()), moved);
    for (&slot, id) in slots.iter().zip(visible) {
        apps[slot] = id;
    }
    Some(with_entries(layout, entries))
}

/// FOLD-06: a folder left with one app turns back into that app, and an empty folder goes.
pub fn tidy(layout: &Layout) -> Layout {
    let entries = layout
        .entries
        .iter()
        .filter_map(|entry| match entry {
            LayoutEntry::Folder { apps, .. } if apps.len() <= 1 => {
                apps.first().cloned().map(LayoutEntry::App)
            }
            other => Some(other.clone()),
        })
        .collect();
    with_entries(layout, entries)
}

/// `layout` with these `entries`, and everything else about it as it was: its marks, and a
/// restore's waiting apps, which keep their slots (APPS-21).
fn with_entries(layout: &Layout, entries: Vec<LayoutEntry>) -> Layout {
    Layout {
        entries,
        ..layout.clone()
    }
}

/// The folder `app` is in, if it is in one.
pub fn folder_of(layout: &Layout, app: &AppId) -> Option<FolderId> {
    layout.entries.iter().find_map(|entry| match entry {
        LayoutEntry::Folder { id, apps, .. } if apps.contains(app) => Some(*id),
        _ => None,
    })
}

/// How many folders `after` has that `before` hadn't: the folders a change made (ANIM-24).
pub fn folders_made(before: &Layout, after: &Layout) -> usize {
    let had = folder_ids(before);
    folder_ids(after).difference(&had).count()
}

/// An id no folder of `layout` has.
fn new_folder_id(layout: &Layout) -> FolderId {
    let used = folder_ids(layout);
    let mut id = used
        .iter()
        .map(|id| id.0)
        .max()
        .map_or(0, |highest| highest.wrapping_add(1));
    while used.contains(&FolderId(id)) {
        id = id.wrapping_add(1);
    }
    FolderId(id)
}

fn folder_ids(layout: &Layout) -> HashSet<FolderId> {
    layout
        .entries
        .iter()
        .filter_map(|entry| match entry {
            LayoutEntry::Folder { id, .. } => Some(*id),
            LayoutEntry::App(_) => None,
        })
        .collect()
}

fn is_app(entry: &LayoutEntry, app: &AppId) -> bool {
    matches!(entry, LayoutEntry::App(id) if id == app)
}

fn is_folder(entry: &LayoutEntry, folder: FolderId) -> bool {
    matches!(entry, LayoutEntry::Folder { id, .. } if *id == folder)
}

impl GridItem {
    /// Whether this cell shows `key`.
    pub fn is(&self, key: &CellKey) -> bool {
        match (self, key) {
            (GridItem::App { app, .. }, CellKey::App(id)) => app.id == *id,
            (GridItem::Folder { id, .. }, CellKey::Folder(folder)) => id == folder,
            _ => false,
        }
    }

    /// What this cell shows.
    pub fn key(&self) -> CellKey {
        match self {
            GridItem::App { app, .. } => CellKey::App(app.id.clone()),
            GridItem::Folder { id, .. } => CellKey::Folder(*id),
        }
    }
}

#[cfg(test)]
mod tests {

    use meegone_core_data::fakes::{app, app_id, work_app};

    use super::*;

    fn installed(packages: &[&str]) -> Vec<App> {
        packages
            .iter()
            .map(|package| app(package, package))
            .collect()
    }

    fn layout(entries: Vec<LayoutEntry>) -> Layout {
        Layout {
            entries,
            ..Layout::default()
        }
    }

    fn apps(packages: &[&str]) -> Vec<LayoutEntry> {
        packages
            .iter()
            .map(|package| LayoutEntry::App(app_id(package)))
            .collect()
    }

    fn folder(id: u32, name: &str, packages: &[&str]) -> LayoutEntry {
        LayoutEntry::Folder {
            id: FolderId(id),
            name: name.into(),
            apps: packages.iter().map(|package| app_id(package)).collect(),
        }
    }

    /// Each entry as its package, or a folder as `[id name: packages]`.
    fn shape(layout: &Layout) -> Vec<String> {
        layout
            .entries
            .iter()
            .map(|entry| match entry {
                LayoutEntry::App(id) => id.package.clone(),
                LayoutEntry::Folder { id, name, apps } => format!(
                    "[{} {name}: {}]",
                    id.0,
                    apps.iter()
                        .map(|app| app.package.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
            })
            .collect()
    }

    fn key(package: &str) -> CellKey {
        CellKey::App(app_id(package))
    }

    #[test]
    fn arranging_moves_a_cell_and_the_others_aside() {
        // APPS-12: an app or a folder, to a place or past the end.
        let installed = installed(&["camera", "chess", "go", "mail", "maps"]);
        let saved = layout(
            [
                apps(&["camera"]),
                vec![folder(1, "Games", &["chess", "go"])],
                apps(&["mail", "maps"]),
            ]
            .concat(),
        );

        let moved = arrange(&saved, &installed, &key("maps"), 0).unwrap();
        assert_eq!(
            shape(&moved),
            ["maps", "camera", "[1 Games: chess go]", "mail"]
        );

        let moved = arrange(&saved, &installed, &CellKey::Folder(FolderId(1)), 99).unwrap();
        assert_eq!(
            shape(&moved),
            ["camera", "mail", "maps", "[1 Games: chess go]"]
        );

        // An app in a folder, or one that isn't there, has no cell of its own.
        assert_eq!(arrange(&saved, &installed, &key("chess"), 0), None);
        assert_eq!(arrange(&saved, &installed, &key("atlas"), 0), None);
    }

    #[test]
    fn arranging_keeps_entries_that_show_no_cell_in_their_slots() {
        // ADR 0012: the work apps of a locked profile.
        let mail = work_app("mail", "Mail").id;
        let saved = layout(vec![
            LayoutEntry::App(app_id("camera")),
            LayoutEntry::App(mail.clone()),
            LayoutEntry::App(app_id("maps")),
        ]);
        let installed = installed(&["camera", "maps"]);

        let moved = arrange(&saved, &installed, &key("maps"), 0).unwrap();

        assert_eq!(moved.entries[1], LayoutEntry::App(mail));
        assert_eq!(shape(&moved)[0], "maps");
    }

    #[test]
    fn dropping_an_app_on_another_makes_a_folder_in_its_place() {
        // FOLD-01, FOLD-03: unnamed, so it shows as "Folder"; the target first.
        let mut saved = layout(
            [
                apps(&["camera", "mail"]),
                vec![folder(4, "Games", &["chess", "go"])],
                apps(&["maps"]),
            ]
            .concat(),
        );
        saved.new.insert(app_id("maps"));

        let made = combine(&saved, &app_id("maps"), &key("mail")).unwrap();

        assert_eq!(
            shape(&made),
            ["camera", "[5 : mail maps]", "[4 Games: chess go]"]
        );
        // Marks stay with their apps.
        assert_eq!(made.new, saved.new);
        assert_eq!(folders_made(&saved, &made), 1);
    }

    #[test]
    fn dropping_an_app_on_a_folder_adds_it_at_the_end() {
        // FOLD-05
        let saved = layout([vec![folder(1, "Games", &["chess", "go"])], apps(&["maps"])].concat());

        let added = combine(&saved, &app_id("maps"), &CellKey::Folder(FolderId(1))).unwrap();

        assert_eq!(shape(&added), ["[1 Games: chess go maps]"]);
        assert_eq!(folders_made(&saved, &added), 0);
    }

    #[test]
    fn folders_never_go_into_anything() {
        // FOLD-05: nor does an app go onto itself, an app in a folder, or a folder that isn't
        // there.
        let saved = layout(
            [
                vec![folder(1, "Games", &["chess", "go"])],
                apps(&["mail", "maps"]),
            ]
            .concat(),
        );

        assert_eq!(combine(&saved, &app_id("mail"), &key("mail")), None);
        assert_eq!(combine(&saved, &app_id("chess"), &key("mail")), None);
        assert_eq!(combine(&saved, &app_id("mail"), &key("chess")), None);
        assert_eq!(
            combine(&saved, &app_id("mail"), &CellKey::Folder(FolderId(9))),
            None
        );
    }

    #[test]
    fn taking_an_app_out_puts_it_where_the_finger_is_and_tidying_collapses_the_folder() {
        // FOLD-05, FOLD-06, AC-08
        let installed = installed(&["camera", "mail", "maps"]);
        let saved = layout([apps(&["camera"]), vec![folder(1, "", &["mail", "maps"])]].concat());

        let out = take_out(&saved, &installed, &app_id("maps"), 0).unwrap();

        // Mid-drag, the folder holds one app.
        assert_eq!(shape(&out), ["maps", "camera", "[1 : mail]"]);
        assert_eq!(shape(&tidy(&out)), ["maps", "camera", "mail"]);
        assert_eq!(take_out(&saved, &installed, &app_id("camera"), 0), None);
    }

    #[test]
    fn moving_out_puts_the_app_just_after_its_folder() {
        // A11Y-05
        let installed = installed(&["camera", "chess", "go", "mail"]);
        let saved = layout(
            [
                vec![folder(1, "Games", &["chess", "go"])],
                apps(&["camera", "mail"]),
            ]
            .concat(),
        );

        let out = move_out(&saved, &installed, &app_id("chess")).unwrap();

        assert_eq!(shape(&out), ["[1 Games: go]", "chess", "camera", "mail"]);
    }

    #[test]
    fn apps_move_within_their_folder() {
        // FOLD-05: an app that doesn't show keeps its slot.
        let installed = installed(&["chess", "go", "poker"]);
        let saved = layout(vec![folder(
            1,
            "Games",
            &["chess", "hidden", "go", "poker"],
        )]);

        let moved = arrange_in_folder(&saved, &installed, &app_id("poker"), 0).unwrap();

        assert_eq!(shape(&moved), ["[1 Games: poker hidden chess go]"]);
        assert_eq!(
            arrange_in_folder(&saved, &installed, &app_id("mail"), 0),
            None
        );
    }

    #[test]
    fn tidying_leaves_folders_of_two_alone() {
        let saved = layout(vec![
            folder(1, "A", &["chess", "go"]),
            folder(2, "B", &["mail"]),
            folder(3, "C", &[]),
        ]);

        assert_eq!(shape(&tidy(&saved)), ["[1 A: chess go]", "mail"]);
    }

    #[test]
    fn a_new_folder_takes_an_id_no_folder_has() {
        let saved = layout(vec![
            folder(u32::MAX, "A", &["a", "b"]),
            folder(0, "B", &["c", "d"]),
        ]);
        assert_eq!(new_folder_id(&saved), FolderId(1));
        assert_eq!(new_folder_id(&layout(Vec::new())), FolderId(0));
    }
}
