//! Applications view (SRS 4.2). It renders [`AppsState`] and turns input into [`AppsIntent`]s.
//!
//! Android's views draw the grid (ADR 0006, ADR 0010). Each frame, [`AppsView::frame`] tells the
//! host the grid's columns, rows and scroll position, and a revision that changes whenever the
//! cells do. The host then reads the [`AppsView::cells`] it draws, and refers to a cell by its
//! place in them. Each cell says where it was in the cells before, so the host moves the same
//! view, icon and all, to its new place (ANIM-21, ADR 0012).
//!
//! An app's long-press menu works the same way (ADR 0013): the frame says at which cell it is
//! open and gives it a revision, the host reads its [`AppsView::menu_items`] when that changes,
//! and refers to an item by its place in them.

use std::sync::Arc;

pub use meegone_apps_state::AppMenuItem;
use meegone_apps_state::{
    AppMenu, AppsIntent, AppsState, FolderId, GridItem, GridLayout, previous_places,
};
use meegone_core_state::{AppId, Rect};
use meegone_core_ui::{Dispatch, Tokens, View};

/// What the host draws for the grid in one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppsFrame {
    /// Changes whenever the cells change, so the host reads [`AppsView::cells`] again.
    pub revision: u32,
    /// How far the grid is scrolled from its top, in whole pixels. The host follows it when it
    /// didn't report it itself, as after Home (HOME-07) or a new layout.
    pub scroll_px: i32,
    /// Columns (APPS-02).
    pub columns: u16,
    /// Rows, the last one always empty (HOME-14).
    pub rows: u16,
    /// An app's long-press menu, while it is open (APPS-11).
    pub menu: Option<AppMenuFrame>,
}

/// An app's long-press menu as the host draws it (APPS-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppMenuFrame {
    /// Changes whenever its items do, so the host reads [`AppsView::menu_items`] again.
    pub revision: u32,
    /// The cell of the app it is for, which the host opens it at, or `None` when the app has
    /// no cell of its own, such as inside a folder; the host then centres it.
    pub cell: Option<usize>,
}

/// One cell as the host draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell<'a> {
    /// The single-line label under the icon (APPS-16).
    pub label: &'a str,
    /// The app it launches and whose icon it shows, or `None` for a folder, which #14 draws.
    pub app: Option<&'a AppId>,
    /// False while the app can't open: it is drawn dimmed (APPS-09).
    pub available: bool,
    /// In a work profile: its name says so (APPS-20, A11Y-01).
    pub work: bool,
    /// Installed since the grid was set up and not opened yet: it carries a dot (APPS-07).
    pub new: bool,
    /// Its place in the cells of the revision before, or `None` if it just joined the grid. The
    /// host moves it from there, and the cells no one came from leave (ANIM-21).
    pub from: Option<usize>,
}

pub struct AppsView {
    dispatch: Dispatch<AppsIntent>,
    /// The cells the host was last told about: the snapshot it draws (ARCH-06).
    cells: Arc<[GridItem]>,
    /// Where each of `cells` was in the cells before them.
    from: Vec<Option<usize>>,
    revision: u32,
    /// The app menu the host was last told about.
    menu: Option<Arc<AppMenu>>,
    menu_revision: u32,
}

impl AppsView {
    pub fn new(dispatch: Dispatch<AppsIntent>) -> Self {
        Self {
            dispatch,
            cells: Arc::new([]),
            from: Vec::new(),
            revision: 0,
            menu: None,
            menu_revision: 0,
        }
    }

    /// What the host draws for the grid now. Takes on the state's cells if they changed, and
    /// where each was in the cells the host drew before, and the app menu if it changed.
    pub fn frame(&mut self, state: &AppsState) -> AppsFrame {
        if !Arc::ptr_eq(&self.cells, &state.items) {
            self.from = previous_places(&self.cells, &state.items);
            self.cells = Arc::clone(&state.items);
            self.revision = self.revision.wrapping_add(1);
        }
        let same_menu = match (&self.menu, &state.menu) {
            (Some(shown), Some(menu)) => Arc::ptr_eq(shown, menu),
            (shown, menu) => shown.is_none() && menu.is_none(),
        };
        if !same_menu {
            self.menu = state.menu.clone();
            self.menu_revision = self.menu_revision.wrapping_add(1);
        }
        AppsFrame {
            revision: self.revision,
            scroll_px: state.scroll_px.round().clamp(0.0, f64::from(i32::MAX)) as i32,
            columns: u16::try_from(state.columns).unwrap_or(u16::MAX),
            rows: u16::try_from(state.rows()).unwrap_or(u16::MAX),
            menu: self.menu.as_ref().map(|menu| AppMenuFrame {
                revision: self.menu_revision,
                cell: self.place_of(&menu.app),
            }),
        }
    }

    /// The cells of the last [`AppsView::frame`], in order.
    pub fn cells(&self) -> impl ExactSizeIterator<Item = Cell<'_>> {
        self.cells
            .iter()
            .zip(&self.from)
            .map(|(item, from)| match item {
                GridItem::App { app, new } => Cell {
                    label: &app.label,
                    app: Some(&app.id),
                    available: app.available,
                    work: app.work,
                    new: *new,
                    from: *from,
                },
                GridItem::Folder { name, .. } => Cell {
                    label: name,
                    app: None,
                    available: true,
                    work: false,
                    new: false,
                    from: *from,
                },
            })
    }

    /// The app of cell `index` of the cells at `revision`, or `None` for a folder, or when the
    /// cells changed since the host drew them: then the host can't know what it meant.
    pub fn app_at(&self, revision: u32, index: usize) -> Option<&AppId> {
        if revision != self.revision {
            return None;
        }
        match self.cells.get(index)? {
            GridItem::App { app, .. } => Some(&app.id),
            GridItem::Folder { .. } => None,
        }
    }

    /// The place of `app`'s own cell in the cells of the last [`AppsView::frame`], if it has
    /// one: an app in a folder has none.
    pub fn place_of(&self, app: &AppId) -> Option<usize> {
        self.cells
            .iter()
            .position(|item| matches!(item, GridItem::App { app: shown, .. } if shown.id == *app))
    }

    /// The items of the app menu of the last [`AppsView::frame`], in order, or none while it is
    /// closed (APPS-11).
    pub fn menu_items(&self) -> &[AppMenuItem] {
        self.menu.as_ref().map_or(&[], |menu| &menu.items)
    }

    /// Item `index` of the app menu at `revision` was chosen (APPS-11). Returns whether it does
    /// anything: not when the menu changed or closed since the host drew it.
    pub fn menu_item_chosen(&self, revision: u32, index: usize) -> bool {
        if revision != self.menu_revision {
            return false;
        }
        match self.menu_items().get(index) {
            Some(item) => {
                self.dispatch.send(AppsIntent::ChooseMenuItem(item.clone()));
                true
            }
            None => false,
        }
    }

    /// Cell `index` of the cells at `revision` was tapped. Its icon is at `from` on the screen,
    /// which the app's window grows from (APPS-10, ANIM-08). Returns whether it launches
    /// anything: not when the cells changed since the host drew them, nor for a folder until #14.
    pub fn cell_tapped(&self, revision: u32, index: usize, from: Rect) -> bool {
        if revision != self.revision {
            return false;
        }
        match self.cells.get(index) {
            Some(GridItem::App { app, .. }) => {
                self.icon_tapped(app.id.clone(), Some(from));
                true
            }
            Some(GridItem::Folder { .. }) | None => false,
        }
    }

    /// APPS-10: launches `app`, growing its window from `from` (ANIM-08).
    pub fn icon_tapped(&self, app: AppId, from: Option<Rect>) {
        self.dispatch.send(AppsIntent::Launch { app, from });
    }

    /// FOLD-04
    pub fn folder_tapped(&self, folder: FolderId) {
        self.dispatch.send(AppsIntent::OpenFolder(folder));
    }

    /// The grid scrolled to `px` from its top (HOME-07, REL-05).
    pub fn grid_scrolled(&self, px: f64) {
        self.dispatch.send(AppsIntent::Scrolled(px));
    }

    /// The grid was laid out `width_px` wide inside its margins, with rows `row_height_px`
    /// apart, at `density` pixels per dp (APPS-02, APPS-16).
    pub fn grid_laid_out(&self, width_px: f64, row_height_px: f64, density: f64) {
        self.dispatch.send(AppsIntent::Layout(GridLayout {
            width_px,
            row_height_px,
            density,
        }));
    }
}

impl View for AppsView {
    type State = AppsState;

    fn render(&mut self, _state: &AppsState, _tokens: &Tokens) {
        // The host draws the grid from `frame` and `cells` (ADR 0010).
    }
}

#[cfg(test)]
mod tests {
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
        // APPS-01, APPS-09, APPS-20
        let (mut view, _) = view();
        let paused = app("mail", "Mail", false, true);
        view.frame(&state(vec![
            GridItem::App {
                app: paused.clone(),
                new: true,
            },
            GridItem::Folder {
                id: FolderId(1),
                name: "Games".into(),
                apps: Vec::new(),
            },
        ]));

        let cells: Vec<Cell<'_>> = view.cells().collect();

        assert_eq!(
            cells,
            [
                Cell {
                    label: "Mail",
                    app: Some(&paused.id),
                    available: false,
                    work: true,
                    new: true,
                    from: None,
                },
                Cell {
                    label: "Games",
                    app: None,
                    available: true,
                    work: false,
                    new: false,
                    from: None,
                },
            ]
        );
    }

    #[test]
    fn a_tap_launches_the_app_the_host_drew_from_its_icon() {
        // APPS-10, ANIM-08
        let (mut view, sent) = view();
        let mail = app("mail", "Mail", true, false);
        let revision = view
            .frame(&state(vec![
                GridItem::Folder {
                    id: FolderId(1),
                    name: "Games".into(),
                    apps: Vec::new(),
                },
                cell(mail.clone()),
            ]))
            .revision;

        assert!(view.cell_tapped(revision, 1, ICON));
        // A folder opens with #14; a cell that isn't there does nothing.
        assert!(!view.cell_tapped(revision, 0, ICON));
        assert!(!view.cell_tapped(revision, 2, ICON));

        let sent = sent.borrow();
        assert_eq!(sent.len(), 1);
        assert!(matches!(
            &sent[0],
            AppsIntent::Launch { app, from: Some(ICON) } if *app == mail.id
        ));
    }

    #[test]
    fn a_tap_on_cells_that_have_since_changed_does_nothing() {
        // The grid changed under the finger: the tap can't know what it meant.
        let (mut view, sent) = view();
        let old = view
            .frame(&state(vec![cell(app("mail", "Mail", true, false))]))
            .revision;
        view.frame(&state(vec![cell(app("maps", "Maps", true, false))]));

        assert!(!view.cell_tapped(old, 0, ICON));
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
        let from = |view: &AppsView| view.cells().map(|cell| cell.from).collect::<Vec<_>>();
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
    fn cells_are_named_by_revision_and_place() {
        // APPS-11: a long-press on cells that have since changed means nothing.
        let (mut view, _) = view();
        let mail = app("mail", "Mail", true, false);
        let maps = app("maps", "Maps", true, false);
        let revision = view
            .frame(&state(vec![
                GridItem::Folder {
                    id: FolderId(1),
                    name: "Games".into(),
                    apps: vec![maps.clone()],
                },
                cell(mail.clone()),
            ]))
            .revision;

        assert_eq!(view.app_at(revision, 1), Some(&mail.id));
        assert_eq!(view.app_at(revision, 0), None);
        assert_eq!(view.app_at(revision, 2), None);
        assert_eq!(view.app_at(revision.wrapping_add(1), 1), None);

        assert_eq!(view.place_of(&mail.id), Some(1));
        // An app in a folder has no cell of its own.
        assert_eq!(view.place_of(&maps.id), None);
    }

    #[test]
    fn the_app_menu_is_at_its_apps_cell_with_a_new_revision_for_each_menu() {
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
            app: mail.id.clone(),
            items: vec![AppMenuItem::AppInfo, AppMenuItem::Uninstall],
        }));
        let menu = view.frame(&apps).menu.expect("the menu is open");
        assert_eq!(menu.cell, Some(1));
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

        // Another menu, even with the same items, is new: a choice from the old one does
        // nothing.
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
    fn the_grid_reports_its_layout_and_scroll_position() {
        let (view, sent) = view();

        view.grid_laid_out(1_000.0, 200.0, 2.5);
        view.grid_scrolled(320.0);

        let sent = sent.borrow();
        assert!(matches!(
            sent[0],
            AppsIntent::Layout(GridLayout {
                width_px: 1_000.0,
                row_height_px: 200.0,
                density: 2.5,
            })
        ));
        assert!(matches!(sent[1], AppsIntent::Scrolled(320.0)));
    }
}
