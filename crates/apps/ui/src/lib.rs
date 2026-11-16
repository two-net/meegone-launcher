//! Applications view (SRS 4.2). It renders [`AppsState`] and turns input into [`AppsIntent`]s.
//!
//! Android's views draw the grid (ADR 0006, ADR 0010). Each frame, [`AppsView::frame`] tells the
//! host the grid's columns, rows and scroll position, and a revision that changes whenever the
//! cells do. The host then reads the [`AppsView::cells`] it draws, and refers to a cell by its
//! place in them. Each cell says where it was in the cells the host last read, so the host moves
//! the same view, icon and all, to its new place (ANIM-21, ADR 0012).
//!
//! The open folder's panel is a second [`Surface`] drawn the same way, with cells of its own and
//! their own revision (ADR 0015), and search's results are a third (ADR 0016). The host owns the
//! search field's text while the user types, sends each change, and reads [`AppsView::query`]
//! back only when it has to show the field again, such as after a theme change. A cell's long-press menu works alike (ADR 0013): the frame says
//! at which cell, on which surface, it is open and gives it a revision, the host reads its
//! [`AppsView::menu_items`] when that changes, and refers to an item by its place in them. In edit
//! mode, the host names a cell the same way when its ✕ is tapped or a screen reader moves it
//! (ADR 0014).

use std::cell::RefCell;
use std::sync::Arc;

use meegone_apps_state::{AppMenu, AppsIntent, AppsState, GridItem, previous_places};
pub use meegone_apps_state::{AppMenuItem, Bounds, CellKey, FolderId, GridLayout, Surface};
use meegone_core_state::{App, AppId, Rect};
use meegone_core_ui::{Dispatch, Tokens, View};

/// What the host draws for the grid, the open folder's cells and search's results, in one frame.
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
    /// The open folder's cells (FOLD-04): the same, for its panel.
    pub folder: SurfaceCellsFrame,
    /// A cell's long-press menu, while it is open (APPS-11).
    pub menu: Option<AppMenuFrame>,
    /// Search's results (SRCH-02): the same, for the results grid.
    pub results: SurfaceCellsFrame,
    /// Search is open with something typed, so its results show instead of the grid.
    pub has_query: bool,
    /// Nothing matches what was typed (SRCH-05).
    pub nothing_found: bool,
    /// A query nothing matches offers a web search: a browser is installed (SRCH-05).
    pub web_search_offered: bool,
}

/// The cells of the open folder's panel (FOLD-04), or of search's results (SRCH-02), as the host
/// draws them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCellsFrame {
    /// Changes whenever its cells change, so the host reads them again.
    pub revision: u32,
    /// How far its grid is scrolled, in whole pixels.
    pub scroll_px: i32,
    /// Its rows, with no empty one after the last.
    pub rows: u16,
}

/// A cell's long-press menu as the host draws it (APPS-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppMenuFrame {
    /// Changes whenever its items do, so the host reads [`AppsView::menu_items`] again.
    pub revision: u32,
    /// Where the cell it is for is: the grid, or the open folder.
    pub surface: Surface,
    /// The cell it is for, which the host opens it at, or `None` when it has no cell on screen;
    /// the host then centres it.
    pub cell: Option<usize>,
}

/// One cell as the host draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell<'a> {
    /// The single-line label under the icon (APPS-16), or a folder's name, which is `""` until
    /// the user names it: then it shows as "Folder" (FOLD-03).
    pub label: &'a str,
    /// The app it launches and whose icon it shows, or `None` for a folder.
    pub app: Option<&'a AppId>,
    /// A folder's apps, which it previews (FOLD-02); none for an app.
    pub apps: &'a [App],
    /// False while the app can't open: it is drawn dimmed (APPS-09).
    pub available: bool,
    /// In a work profile: its name says so (APPS-20, A11Y-01).
    pub work: bool,
    /// Installed since the grid was set up and not opened yet: it carries a dot (APPS-07).
    pub new: bool,
    /// The user can uninstall it: in edit mode it carries a ✕ (APPS-12, APPS-13).
    pub uninstallable: bool,
    /// Its place in the cells the host last read, or `None` if it joined since. The host moves
    /// it from there, and the cells no one came from leave (ANIM-21, ANIM-13).
    pub from: Option<usize>,
}

/// One surface's cells, as the host was last told about them, with a revision.
struct CellSet {
    /// The cells the host was last told about: the snapshot it draws (ARCH-06).
    cells: Arc<[GridItem]>,
    /// The cells the host last read. Two revisions can come between its reads, so each cell
    /// says where it was in these, not in the revision before.
    read: RefCell<Arc<[GridItem]>>,
    /// Where each of `cells` was in `read`.
    from: Vec<Option<usize>>,
    revision: u32,
}

impl CellSet {
    fn new() -> Self {
        Self {
            cells: Arc::new([]),
            read: RefCell::new(Arc::new([])),
            from: Vec::new(),
            revision: 0,
        }
    }

    /// Takes on `items` if they changed, and where each was in the cells the host last read.
    fn take(&mut self, items: &Arc<[GridItem]>) {
        if !Arc::ptr_eq(&self.cells, items) {
            self.from = previous_places(&self.read.borrow(), items);
            self.cells = Arc::clone(items);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn cells(&self) -> impl ExactSizeIterator<Item = Cell<'_>> {
        *self.read.borrow_mut() = Arc::clone(&self.cells);
        self.cells
            .iter()
            .zip(&self.from)
            .map(|(item, from)| match item {
                GridItem::App { app, new } => Cell {
                    label: &app.label,
                    app: Some(&app.id),
                    apps: &[],
                    available: app.available,
                    work: app.work,
                    new: *new,
                    uninstallable: app.can_uninstall,
                    from: *from,
                },
                GridItem::Folder { name, apps, .. } => Cell {
                    label: name,
                    app: None,
                    apps,
                    available: true,
                    work: false,
                    new: false,
                    uninstallable: false,
                    from: *from,
                },
            })
    }

    /// The cell at `index` of the cells at `revision`, or `None` when the cells changed since
    /// the host drew them: then the host can't know what it meant.
    fn at(&self, revision: u32, index: usize) -> Option<&GridItem> {
        (revision == self.revision)
            .then(|| self.cells.get(index))
            .flatten()
    }

    fn place_of(&self, key: &CellKey) -> Option<usize> {
        self.cells.iter().position(|item| item.is(key))
    }
}

pub struct AppsView {
    dispatch: Dispatch<AppsIntent>,
    grid: CellSet,
    folder: CellSet,
    results: CellSet,
    /// The query as of the last frame, while search is open.
    query: Option<String>,
    /// The menu the host was last told about.
    menu: Option<Arc<AppMenu>>,
    menu_revision: u32,
}

impl AppsView {
    pub fn new(dispatch: Dispatch<AppsIntent>) -> Self {
        Self {
            dispatch,
            grid: CellSet::new(),
            folder: CellSet::new(),
            results: CellSet::new(),
            query: None,
            menu: None,
            menu_revision: 0,
        }
    }

    fn set(&self, surface: Surface) -> &CellSet {
        match surface {
            Surface::Grid => &self.grid,
            Surface::Folder => &self.folder,
            Surface::Search => &self.results,
        }
    }

    /// What the host draws for the grid, the open folder and search's results now. Takes on the
    /// state's cells if they changed, and where each was in the cells the host last read, and
    /// the menu and the query if they changed.
    pub fn frame(&mut self, state: &AppsState) -> AppsFrame {
        self.grid.take(&state.items);
        self.folder.take(&state.folder_items);
        self.results.take(&state.results);
        if self.query != state.search {
            self.query.clone_from(&state.search);
        }
        let same_menu = match (&self.menu, &state.menu) {
            (Some(shown), Some(menu)) => Arc::ptr_eq(shown, menu),
            (shown, menu) => shown.is_none() && menu.is_none(),
        };
        if !same_menu {
            self.menu = state.menu.clone();
            self.menu_revision = self.menu_revision.wrapping_add(1);
        }
        let menu = self.menu.as_ref().map(|menu| {
            // An app in the open folder opens its menu there.
            let (surface, cell) = match self.folder.place_of(&menu.item) {
                Some(place) => (Surface::Folder, Some(place)),
                None => (Surface::Grid, self.grid.place_of(&menu.item)),
            };
            AppMenuFrame {
                revision: self.menu_revision,
                surface,
                cell,
            }
        });
        AppsFrame {
            revision: self.grid.revision,
            scroll_px: whole_px(state.scroll_px),
            columns: u16::try_from(state.columns).unwrap_or(u16::MAX),
            rows: u16::try_from(state.rows()).unwrap_or(u16::MAX),
            folder: SurfaceCellsFrame {
                revision: self.folder.revision,
                scroll_px: whole_px(state.folder_scroll_px),
                rows: u16::try_from(state.folder_rows()).unwrap_or(u16::MAX),
            },
            menu,
            results: SurfaceCellsFrame {
                revision: self.results.revision,
                scroll_px: whole_px(state.results_scroll_px),
                rows: u16::try_from(state.results_rows()).unwrap_or(u16::MAX),
            },
            has_query: state.has_query(),
            nothing_found: state.has_query() && state.results.is_empty(),
            web_search_offered: state.web_search_offered,
        }
    }

    /// The query as of the last [`AppsView::frame`], while search is open (SRCH-01).
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// The cells of `surface` as of the last [`AppsView::frame`], in order. The host reads them
    /// to draw them, so the next cells say where they were in these.
    pub fn cells(&self, surface: Surface) -> impl ExactSizeIterator<Item = Cell<'_>> {
        self.set(surface).cells()
    }

    /// What cell `index` of `surface`'s cells at `revision` shows, or `None` when the cells
    /// changed since the host drew them.
    pub fn key_at(&self, surface: Surface, revision: u32, index: usize) -> Option<CellKey> {
        self.set(surface).at(revision, index).map(GridItem::key)
    }

    /// The app of cell `index` of `surface`'s cells at `revision`, or `None` for a folder, or
    /// when the cells changed since the host drew them.
    pub fn app_at(&self, surface: Surface, revision: u32, index: usize) -> Option<&AppId> {
        match self.set(surface).at(revision, index)? {
            GridItem::App { app, .. } => Some(&app.id),
            GridItem::Folder { .. } => None,
        }
    }

    /// The place of `key`'s cell on `surface`, as of the last [`AppsView::frame`].
    pub fn place_of(&self, surface: Surface, key: &CellKey) -> Option<usize> {
        self.set(surface).place_of(key)
    }

    /// The items of the menu of the last [`AppsView::frame`], in order, or none while it is
    /// closed (APPS-11).
    pub fn menu_items(&self) -> &[AppMenuItem] {
        self.menu.as_ref().map_or(&[], |menu| &menu.items)
    }

    /// Item `index` of the menu at `revision` was chosen (APPS-11). Returns whether it does
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

    /// Cell `index` of `surface`'s cells at `revision` was tapped, with its icon at `from` on the
    /// screen. An app launches, its window growing from there (APPS-10, ANIM-08); a folder opens
    /// (FOLD-04). Returns whether it did anything: not when the cells changed since the host
    /// drew them.
    pub fn cell_tapped(&self, surface: Surface, revision: u32, index: usize, from: Rect) -> bool {
        match self.set(surface).at(revision, index) {
            Some(GridItem::App { app, .. }) => {
                self.icon_tapped(app.id.clone(), Some(from));
                true
            }
            Some(GridItem::Folder { id, .. }) => {
                self.dispatch.send(AppsIntent::OpenFolder(*id));
                true
            }
            None => false,
        }
    }

    /// The ✕ of cell `index` of `surface`'s cells at `revision` was tapped, in edit mode: the
    /// system asks the user to confirm the app's uninstall (APPS-13). Returns whether it asked
    /// anything of Applications: not for a folder, nor when the cells changed since.
    pub fn badge_tapped(&self, surface: Surface, revision: u32, index: usize) -> bool {
        match self.app_at(surface, revision, index) {
            Some(app) => {
                self.dispatch
                    .send(AppsIntent::RequestUninstall(app.clone()));
                true
            }
            None => false,
        }
    }

    /// A screen reader moved cell `index` of `surface`'s cells at `revision` to place `to`, in
    /// edit mode: in the grid, or within the open folder (A11Y-05). Returns whether it asked
    /// anything of Applications.
    pub fn moved(&self, surface: Surface, revision: u32, index: usize, to: usize) -> bool {
        let intent = match (surface, self.key_at(surface, revision, index)) {
            (Surface::Grid, Some(item)) => AppsIntent::Move { item, to },
            (Surface::Folder, Some(CellKey::App(app))) => AppsIntent::MoveInFolder { app, to },
            _ => return false,
        };
        self.dispatch.send(intent);
        true
    }

    /// A screen reader put the app of grid cell `index` at `revision` into a folder with the
    /// cell after it: "Make folder with…", or "Move into…" when that is a folder (A11Y-05).
    /// Returns whether it asked anything of Applications.
    pub fn grouped(&self, revision: u32, index: usize) -> bool {
        let app = self.app_at(Surface::Grid, revision, index).cloned();
        let with = self.key_at(Surface::Grid, revision, index + 1);
        let (Some(app), Some(with)) = (app, with) else {
            return false;
        };
        self.dispatch.send(AppsIntent::Group { app, with });
        true
    }

    /// A screen reader took the app of the open folder's cell `index` at `revision` out of the
    /// folder: "Move out of folder" (A11Y-05). Returns whether it asked anything of
    /// Applications.
    pub fn moved_out(&self, revision: u32, index: usize) -> bool {
        match self.app_at(Surface::Folder, revision, index) {
            Some(app) => {
                self.dispatch.send(AppsIntent::MoveOutOfFolder(app.clone()));
                true
            }
            None => false,
        }
    }

    /// Done: edit mode ends (APPS-12).
    pub fn done(&self) {
        self.dispatch.send(AppsIntent::ExitEditMode);
    }

    /// A tap outside the open folder closes it (FOLD-04).
    pub fn close_folder(&self) {
        self.dispatch.send(AppsIntent::CloseFolder);
    }

    /// The user tapped the open folder's name, to rename it (FOLD-03).
    pub fn rename_requested(&self, folder: FolderId) {
        self.dispatch.send(AppsIntent::StartRename(folder));
    }

    /// The user named `folder` `name` (FOLD-03).
    pub fn folder_renamed(&self, folder: FolderId, name: String) {
        self.dispatch
            .send(AppsIntent::RenameFolder { folder, name });
    }

    /// The user left the folder's name as it was.
    pub fn rename_cancelled(&self) {
        self.dispatch.send(AppsIntent::CancelRename);
    }

    /// APPS-10: launches `app`, growing its window from `from` (ANIM-08).
    pub fn icon_tapped(&self, app: AppId, from: Option<Rect>) {
        self.dispatch.send(AppsIntent::Launch { app, from });
    }

    /// `surface` scrolled to `px` from its top (HOME-07, REL-05).
    pub fn scrolled(&self, surface: Surface, px: f64) {
        self.dispatch.send(match surface {
            Surface::Grid => AppsIntent::Scrolled(px),
            Surface::Folder => AppsIntent::FolderScrolled(px),
            Surface::Search => AppsIntent::ResultsScrolled(px),
        });
    }

    /// The grid was laid out (APPS-02, APPS-16), in physical pixels.
    pub fn grid_laid_out(&self, layout: GridLayout) {
        self.dispatch.send(AppsIntent::Layout(layout));
    }

    /// The open folder's grid was laid out, in its panel at `bounds` on screen (FOLD-04).
    pub fn folder_laid_out(&self, layout: GridLayout, bounds: Bounds) {
        self.dispatch
            .send(AppsIntent::FolderLayout { layout, bounds });
    }

    /// The results grid was laid out, below the search field's `band` on screen (ANIM-16).
    pub fn search_laid_out(&self, layout: GridLayout, band: Bounds) {
        self.dispatch
            .send(AppsIntent::SearchLayout { layout, band });
    }

    /// A screen reader's "Search apps": search opens without a pull (A11Y-05).
    pub fn open_search(&self) {
        self.dispatch.send(AppsIntent::OpenSearch);
    }

    /// The user typed: the field now says `query` (SRCH-02).
    pub fn search_changed(&self, query: String) {
        self.dispatch.send(AppsIntent::Search(query));
    }

    /// The keyboard's action key: the first result launches from its icon at `from`, or with
    /// no results the web is searched (SRCH-03, SRCH-05).
    pub fn search_submitted(&self, from: Option<Rect>) {
        self.dispatch.send(AppsIntent::SubmitSearch { from });
    }

    /// "Search the web for '…'" (SRCH-05).
    pub fn search_web(&self) {
        self.dispatch.send(AppsIntent::SearchWeb);
    }
}

/// A scroll position in whole pixels.
fn whole_px(px: f64) -> i32 {
    px.round().clamp(0.0, f64::from(i32::MAX)) as i32
}

impl View for AppsView {
    type State = AppsState;

    fn render(&mut self, _state: &AppsState, _tokens: &Tokens) {
        // The host draws the grid from `frame` and `cells` (ADR 0010).
    }
}

#[cfg(test)]
mod tests;
