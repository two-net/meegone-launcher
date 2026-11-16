use std::cell::RefCell;
use std::rc::Rc;

use meegone_core_state::{App, ProfileId, Rgb};

use super::*;

fn app(package: &str, label: &str, available: bool, work: bool) -> App {
    App {
        id: AppId {
            profile: ProfileId(u32::from(work) * 10),
            package: package.into(),
            activity: format!("{package}.Main"),
        },
        label: label.into(),
        sort_key: label.to_lowercase().into_bytes(),
        search_key: label.to_lowercase(),
        accent: Rgb::hex(0x808080),
        available,
        work,
        can_uninstall: true,
        installed_at_ms: 0,
    }
}

fn cell(app: App) -> GridItem {
    GridItem::App { app, new: false }
}

fn folder(id: u32, apps: Vec<App>) -> GridItem {
    GridItem::Folder {
        id: FolderId(id),
        name: "Games".into(),
        apps,
    }
}

fn view() -> (AppsView, Rc<RefCell<Vec<AppsIntent>>>) {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let view = AppsView::new(Dispatch::new({
        let sent = Rc::clone(&sent);
        move |intent| sent.borrow_mut().push(intent)
    }));
    (view, sent)
}

fn state(items: Vec<GridItem>) -> AppsState {
    AppsState {
        items: items.into(),
        ..AppsState::default()
    }
}

const ICON: Rect = Rect {
    left: 1,
    top: 2,
    right: 3,
    bottom: 4,
};

const GRID: Surface = Surface::Grid;

#[test]
fn the_revision_changes_only_with_the_cells() {
    let (mut view, _) = view();
    let mut apps = state(vec![cell(app("mail", "Mail", true, false))]);

    let first = view.frame(&apps).revision;
    assert_eq!(view.frame(&apps).revision, first);

    apps.scroll_px = 640.4;
    let frame = view.frame(&apps);
    assert_eq!((frame.revision, frame.scroll_px), (first, 640));

    apps.items = vec![cell(app("maps", "Maps", true, false))].into();
    assert_ne!(view.frame(&apps).revision, first);
}

#[test]
fn the_frame_carries_the_columns_and_rows() {
    // APPS-02, HOME-14
    let (mut view, _) = view();
    let items = (0..9)
        .map(|i| cell(app(&format!("app{i}"), "App", true, false)))
        .collect();
    let apps = AppsState {
        columns: 5,
        ..state(items)
    };

    let frame = view.frame(&apps);

    assert_eq!((frame.columns, frame.rows), (5, 3));
}

#[test]
fn cells_carry_what_the_host_draws() {
    // APPS-01, APPS-09, APPS-20, FOLD-02
    let (mut view, _) = view();
    let paused = app("mail", "Mail", false, true);
    let chess = app("chess", "Chess", true, false);
    view.frame(&state(vec![
        GridItem::App {
            app: paused.clone(),
            new: true,
        },
        folder(1, vec![chess.clone()]),
    ]));

    let cells: Vec<Cell<'_>> = view.cells(GRID).collect();

    assert_eq!(
        cells,
        [
            Cell {
                label: "Mail",
                app: Some(&paused.id),
                apps: &[],
                available: false,
                work: true,
                new: true,
                uninstallable: true,
                from: None,
            },
            Cell {
                label: "Games",
                app: None,
                apps: &[chess],
                available: true,
                work: false,
                new: false,
                uninstallable: false,
                from: None,
            },
        ]
    );
}

#[test]
fn a_tap_launches_an_app_from_its_icon_and_opens_a_folder() {
    // APPS-10, ANIM-08, FOLD-04
    let (mut view, sent) = view();
    let mail = app("mail", "Mail", true, false);
    let revision = view
        .frame(&state(vec![folder(1, Vec::new()), cell(mail.clone())]))
        .revision;

    assert!(view.cell_tapped(GRID, revision, 1, ICON));
    assert!(view.cell_tapped(GRID, revision, 0, ICON));
    // A cell that isn't there does nothing.
    assert!(!view.cell_tapped(GRID, revision, 2, ICON));

    let sent = sent.borrow();
    assert_eq!(sent.len(), 2);
    assert!(matches!(
        &sent[0],
        AppsIntent::Launch { app, from: Some(ICON) } if *app == mail.id
    ));
    assert!(matches!(sent[1], AppsIntent::OpenFolder(FolderId(1))));
}

#[test]
fn a_tap_on_cells_that_have_since_changed_does_nothing() {
    // The grid changed under the finger: the tap can't know what it meant.
    let (mut view, sent) = view();
    let old = view
        .frame(&state(vec![cell(app("mail", "Mail", true, false))]))
        .revision;
    view.frame(&state(vec![cell(app("maps", "Maps", true, false))]));

    assert!(!view.cell_tapped(GRID, old, 0, ICON));
    assert!(sent.borrow().is_empty());
}

#[test]
fn each_cell_says_where_it_was_in_the_cells_the_host_drew_before() {
    // ANIM-21: Maps was removed, Atlas installed; Phone moves up a place.
    let (mut view, _) = view();
    let mail = app("mail", "Mail", true, false);
    let maps = app("maps", "Maps", true, false);
    let phone = app("phone", "Phone", true, false);
    let atlas = app("atlas", "Atlas", true, false);
    view.frame(&state(vec![
        cell(mail.clone()),
        cell(maps),
        cell(phone.clone()),
    ]));
    let from = |view: &AppsView| view.cells(GRID).map(|cell| cell.from).collect::<Vec<_>>();
    assert_eq!(from(&view), [None, None, None]);

    let mut after = state(vec![cell(mail.clone()), cell(phone.clone()), cell(atlas)]);
    view.frame(&after);
    assert_eq!(from(&view), [Some(0), Some(2), None]);

    // A new snapshot of the same cells moves none of them.
    after.items = vec![cell(mail), cell(phone)].into();
    view.frame(&after);
    assert_eq!(from(&view), [Some(0), Some(1)]);
    view.frame(&after);
    assert_eq!(from(&view), [Some(0), Some(1)]);
}

#[test]
fn each_cell_says_where_it_was_in_the_cells_the_host_last_read() {
    // ANIM-13: two moves before the host reads again still move the right views.
    let (mut view, _) = view();
    let [mail, maps, phone] = ["mail", "maps", "phone"].map(|p| cell(app(p, p, true, false)));
    view.frame(&state(vec![mail.clone(), maps.clone(), phone.clone()]));
    let _ = view.cells(GRID).count();

    view.frame(&state(vec![maps.clone(), mail.clone(), phone.clone()]));
    view.frame(&state(vec![phone, maps, mail]));

    let from: Vec<_> = view.cells(GRID).map(|cell| cell.from).collect();
    assert_eq!(from, [Some(2), Some(1), Some(0)]);
}

#[test]
fn cells_are_named_by_surface_revision_and_place() {
    // APPS-11, FOLD-04: a long-press on cells that have since changed means nothing.
    let (mut view, _) = view();
    let mail = app("mail", "Mail", true, false);
    let maps = app("maps", "Maps", true, false);
    let mut apps = state(vec![folder(1, vec![maps.clone()]), cell(mail.clone())]);
    apps.open_folder = Some(FolderId(1));
    apps.folder_items = vec![cell(maps.clone())].into();
    let frame = view.frame(&apps);
    let (grid, open) = (frame.revision, frame.folder.revision);

    assert_eq!(view.app_at(GRID, grid, 1), Some(&mail.id));
    assert_eq!(view.app_at(GRID, grid, 0), None);
    assert_eq!(
        view.key_at(GRID, grid, 0),
        Some(CellKey::Folder(FolderId(1)))
    );
    assert_eq!(view.app_at(GRID, grid, 2), None);
    assert_eq!(view.app_at(GRID, grid.wrapping_add(1), 1), None);
    assert_eq!(view.app_at(Surface::Folder, open, 0), Some(&maps.id));

    let mail_key = CellKey::App(mail.id.clone());
    assert_eq!(view.place_of(GRID, &mail_key), Some(1));
    // An app in a folder has no cell of its own in the grid.
    let maps_key = CellKey::App(maps.id.clone());
    assert_eq!(view.place_of(GRID, &maps_key), None);
    assert_eq!(view.place_of(Surface::Folder, &maps_key), Some(0));
}

#[test]
fn the_open_folders_cells_have_their_own_revision() {
    // FOLD-04
    let (mut view, _) = view();
    let chess = app("chess", "Chess", true, false);
    let mut apps = state(vec![folder(1, vec![chess.clone()])]);
    let closed = view.frame(&apps);
    assert_eq!(closed.folder.rows, 0);

    apps.open_folder = Some(FolderId(1));
    apps.folder_items = vec![cell(chess)].into();
    apps.folder_scroll_px = 12.6;
    let open = view.frame(&apps);

    assert_eq!(open.revision, closed.revision);
    assert_ne!(open.folder.revision, closed.folder.revision);
    assert_eq!((open.folder.rows, open.folder.scroll_px), (1, 13));
    assert_eq!(view.cells(Surface::Folder).count(), 1);
}

#[test]
fn the_menu_is_at_its_cell_with_a_new_revision_for_each_menu() {
    // APPS-11
    let (mut view, sent) = view();
    let mail = app("mail", "Mail", true, false);
    let mut apps = state(vec![
        cell(app("maps", "Maps", true, false)),
        cell(mail.clone()),
    ]);
    assert_eq!(view.frame(&apps).menu, None);
    assert!(view.menu_items().is_empty());

    apps.menu = Some(Arc::new(AppMenu {
        item: CellKey::App(mail.id.clone()),
        items: vec![AppMenuItem::AppInfo, AppMenuItem::Uninstall],
    }));
    let menu = view.frame(&apps).menu.expect("the menu is open");
    assert_eq!((menu.surface, menu.cell), (GRID, Some(1)));
    assert_eq!(
        view.menu_items(),
        [AppMenuItem::AppInfo, AppMenuItem::Uninstall]
    );
    // The same menu keeps its revision.
    assert_eq!(view.frame(&apps).menu, Some(menu));

    assert!(view.menu_item_chosen(menu.revision, 1));
    assert!(!view.menu_item_chosen(menu.revision, 2));
    assert!(matches!(
        sent.borrow()[..],
        [AppsIntent::ChooseMenuItem(AppMenuItem::Uninstall)]
    ));

    // Another menu, even with the same items, is new: a choice from the old one does nothing.
    apps.menu = Some(Arc::new(AppMenu::clone(apps.menu.as_ref().unwrap())));
    let again = view.frame(&apps).menu.unwrap();
    assert_ne!(again.revision, menu.revision);
    assert!(!view.menu_item_chosen(menu.revision, 0));

    apps.menu = None;
    assert_eq!(view.frame(&apps).menu, None);
    assert!(view.menu_items().is_empty());
    assert!(!view.menu_item_chosen(again.revision, 0));
    assert_eq!(sent.borrow().len(), 1);
}

#[test]
fn the_menu_of_an_app_in_the_open_folder_opens_there() {
    let (mut view, _) = view();
    let chess = app("chess", "Chess", true, false);
    let mut apps = state(vec![folder(1, vec![chess.clone()])]);
    apps.open_folder = Some(FolderId(1));
    apps.folder_items = vec![cell(chess.clone())].into();
    apps.menu = Some(Arc::new(AppMenu {
        item: CellKey::App(chess.id),
        items: vec![AppMenuItem::AppInfo],
    }));

    let menu = view.frame(&apps).menu.unwrap();

    assert_eq!((menu.surface, menu.cell), (Surface::Folder, Some(0)));
}

#[test]
fn the_grid_and_the_folder_report_their_layout_and_scroll_position() {
    let (view, sent) = view();
    let layout = GridLayout {
        width_px: 1_000.0,
        row_height_px: 200.0,
        density: 2.5,
        top_px: 60.0,
        bottom_px: 40.0,
        height_px: 2_000.0,
        left_px: 40.0,
        view_top_px: 0.0,
        icon_px: 140.0,
        icon_top_px: 20.0,
    };
    let bounds = Bounds {
        left: 1.0,
        top: 2.0,
        right: 3.0,
        bottom: 4.0,
    };
    view.grid_laid_out(layout);
    view.scrolled(GRID, 320.0);
    view.folder_laid_out(layout, bounds);
    view.scrolled(Surface::Folder, 10.0);

    let sent = sent.borrow();
    assert!(matches!(sent[0], AppsIntent::Layout(sent) if sent == layout));
    assert!(matches!(sent[1], AppsIntent::Scrolled(320.0)));
    assert!(matches!(
        sent[2],
        AppsIntent::FolderLayout { layout: l, bounds: b } if l == layout && b == bounds
    ));
    assert!(matches!(sent[3], AppsIntent::FolderScrolled(10.0)));
}

#[test]
fn edit_mode_names_cells_by_revision_and_place() {
    // APPS-12, APPS-13, A11Y-05
    let (mut view, sent) = view();
    let mail = app("mail", "Mail", true, false);
    let maps = app("maps", "Maps", true, false);
    let revision = view
        .frame(&state(vec![
            folder(1, Vec::new()),
            cell(mail.clone()),
            cell(maps.clone()),
        ]))
        .revision;

    assert!(view.badge_tapped(GRID, revision, 1));
    assert!(view.moved(GRID, revision, 1, 0));
    assert!(view.moved(GRID, revision, 0, 2));
    assert!(view.grouped(revision, 1));
    view.done();
    // A folder's ✕, grouping a folder or the last cell, a place with no cell, or cells that
    // changed since, do nothing.
    assert!(!view.badge_tapped(GRID, revision, 0));
    assert!(!view.grouped(revision, 0));
    assert!(!view.grouped(revision, 2));
    assert!(!view.moved(GRID, revision, 3, 0));
    assert!(!view.badge_tapped(GRID, revision.wrapping_add(1), 1));

    let sent = sent.borrow();
    assert_eq!(sent.len(), 5);
    assert!(matches!(&sent[0], AppsIntent::RequestUninstall(app) if *app == mail.id));
    assert!(matches!(
        &sent[1],
        AppsIntent::Move { item: CellKey::App(app), to: 0 } if *app == mail.id
    ));
    assert!(matches!(
        &sent[2],
        AppsIntent::Move {
            item: CellKey::Folder(FolderId(1)),
            to: 2
        }
    ));
    assert!(matches!(
        &sent[3],
        AppsIntent::Group { app, with: CellKey::App(with) } if *app == mail.id && *with == maps.id
    ));
    assert!(matches!(sent[4], AppsIntent::ExitEditMode));
}

#[test]
fn in_the_open_folder_apps_move_within_it_and_out_of_it() {
    // A11Y-05
    let (mut view, sent) = view();
    let chess = app("chess", "Chess", true, false);
    let mut apps = state(vec![folder(1, vec![chess.clone()])]);
    apps.open_folder = Some(FolderId(1));
    apps.folder_items = vec![cell(chess.clone())].into();
    let revision = view.frame(&apps).folder.revision;

    assert!(view.moved(Surface::Folder, revision, 0, 3));
    assert!(view.moved_out(revision, 0));
    assert!(!view.moved_out(revision, 1));
    view.close_folder();
    view.rename_requested(FolderId(1));
    view.folder_renamed(FolderId(1), "Games".into());
    view.rename_cancelled();

    let sent = sent.borrow();
    assert!(matches!(
        &sent[0],
        AppsIntent::MoveInFolder { app, to: 3 } if *app == chess.id
    ));
    assert!(matches!(&sent[1], AppsIntent::MoveOutOfFolder(app) if *app == chess.id));
    assert!(matches!(sent[2], AppsIntent::CloseFolder));
    assert!(matches!(sent[3], AppsIntent::StartRename(FolderId(1))));
    assert!(matches!(
        &sent[4],
        AppsIntent::RenameFolder { folder: FolderId(1), name } if name == "Games"
    ));
    assert!(matches!(sent[5], AppsIntent::CancelRename));
}

#[test]
fn searchs_results_have_their_own_revision_and_launch_when_tapped() {
    // SRCH-02, SRCH-03, APPS-10
    let (mut view, sent) = view();
    let mail = app("mail", "Mail", true, false);
    let maps = app("maps", "Maps", true, false);
    let mut apps = state(vec![cell(mail.clone()), cell(maps.clone())]);
    let closed = view.frame(&apps);
    assert_eq!(view.query(), None);
    assert!(!closed.has_query && !closed.nothing_found);

    apps.search = Some("ma".into());
    apps.results = vec![cell(maps.clone()), cell(mail.clone())].into();
    apps.results_scroll_px = 40.4;
    let open = view.frame(&apps);

    assert_eq!(open.revision, closed.revision);
    assert_ne!(open.results.revision, closed.results.revision);
    assert_eq!((open.results.rows, open.results.scroll_px), (1, 40));
    assert!(open.has_query && !open.nothing_found);
    assert_eq!(view.query(), Some("ma"));
    let labels: Vec<&str> = view.cells(Surface::Search).map(|cell| cell.label).collect();
    assert_eq!(labels, ["Maps", "Mail"]);

    assert!(view.cell_tapped(Surface::Search, open.results.revision, 0, ICON));
    // A stale revision launches nothing, and results can't be moved.
    assert!(!view.cell_tapped(Surface::Search, closed.results.revision, 0, ICON));
    assert!(!view.moved(Surface::Search, open.results.revision, 0, 1));
    let sent = sent.borrow();
    assert_eq!(sent.len(), 1);
    assert!(matches!(
        &sent[0],
        AppsIntent::Launch { app, from: Some(ICON) } if *app == maps.id
    ));
}

#[test]
fn a_query_nothing_matches_says_so_and_whether_the_web_is_offered() {
    // SRCH-05
    let (mut view, _) = view();
    let mut apps = state(vec![cell(app("mail", "Mail", true, false))]);
    apps.search = Some(String::new());
    apps.web_search_offered = true;
    let empty = view.frame(&apps);
    assert!(!empty.has_query && !empty.nothing_found && empty.web_search_offered);

    apps.search = Some("zebra".into());
    let none = view.frame(&apps);

    assert!(none.has_query && none.nothing_found);
    assert_eq!(none.results.rows, 0);
}

#[test]
fn search_input_becomes_intents() {
    // SRCH-01 to SRCH-05, A11Y-05
    let (view, sent) = view();
    let layout = GridLayout {
        width_px: 1000.0,
        row_height_px: 300.0,
        density: 2.75,
        top_px: 300.0,
        bottom_px: 50.0,
        height_px: 2000.0,
        left_px: 0.0,
        view_top_px: 0.0,
        icon_px: 140.0,
        icon_top_px: 20.0,
    };
    let band = Bounds {
        left: 0.0,
        top: 100.0,
        right: 1000.0,
        bottom: 300.0,
    };

    view.open_search();
    view.search_changed("Ma".into());
    view.search_submitted(Some(ICON));
    view.search_web();
    view.search_laid_out(layout, band);
    view.scrolled(Surface::Search, 25.0);

    let sent = sent.borrow();
    assert!(matches!(sent[0], AppsIntent::OpenSearch));
    assert!(matches!(&sent[1], AppsIntent::Search(query) if query == "Ma"));
    assert!(matches!(
        sent[2],
        AppsIntent::SubmitSearch { from: Some(ICON) }
    ));
    assert!(matches!(sent[3], AppsIntent::SearchWeb));
    assert!(matches!(
        sent[4],
        AppsIntent::SearchLayout { layout: l, band: b } if l == layout && b == band
    ));
    assert!(matches!(sent[5], AppsIntent::ResultsScrolled(25.0)));
}
