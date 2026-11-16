//! Applications state (SRS 4.2): the grid, its layout and scroll position, an app's or a folder's
//! long-press menu, edit mode with the drag's draft, the open folder, search, and hidden apps.
//! [`reconcile`] keeps the layout in step with the installed apps (ADR 0012), [`folders`] holds
//! the pure edits a drag and the folder rules make (ADR 0014, ADR 0015), and [`search`] ranks the
//! apps that match a query (ADR 0016). A hidden app keeps its place in the layout but shows no
//! cell, so it is in neither the grid nor search until it is unhidden (APPS-19, ADR 0017). After
//! a restore, so does a backed-up app until it is installed (APPS-21, ADR 0018).

pub mod folders;
mod reconcile;
pub mod search;

use std::collections::BTreeSet;
use std::sync::Arc;

use meegone_apps_data::{Layout, LayoutStore, SavedLayout};
use meegone_core_data::{AppsAdapter, DataError, Profile, SettingsStore, SystemConfigAdapter};
use meegone_core_state::{App, AppId, Rect, Shortcut, StateHolder};

pub use meegone_apps_data::FolderId;
use reconcile::{
    Start, alphabetical, end_restore, follow_hidden, grid, is_restored, reconcile, reconcile_draft,
};
pub use reconcile::{cells_moved, previous_places};

/// Columns on a phone (APPS-02).
pub const PHONE_COLUMNS: usize = 4;
/// On a wider grid, columns are added so no cell is wider than this, in dp (APPS-02)…
pub const MAX_CELL_DP: f64 = 128.0;
/// …and none narrower than this, which holds for any grid at least four of them wide.
pub const MIN_CELL_DP: f64 = 96.0;
/// An open folder shows its apps in this many columns (FOLD-04).
pub const FOLDER_COLUMNS: usize = 4;
/// A folder's name is at most this many characters (FOLD-03).
pub const MAX_FOLDER_NAME: usize = 64;
/// How long a restored layout waits for its backed-up apps, in ms: an app installed in that time
/// takes its old place (APPS-21, ADR 0018). The system's restore usually installs them within
/// hours; a week also covers apps that wait for Wi-Fi, and those the user installs again by hand.
pub const RESTORE_WAIT_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// How many columns a grid `width_dp` wide has, inside its margins (APPS-02).
///
/// Four, unless four cells would be wider than [`MAX_CELL_DP`]: then as few columns as keep
/// every cell at most that wide. A phone's grid is under 512 dp, so it always has four. A
/// screen 600 dp wide or more leaves a grid of at least 384 dp inside its safe area and
/// margins, where every cell is between [`MIN_CELL_DP`] and [`MAX_CELL_DP`]. A folder's panel is
/// four of the grid's cells wide, so it has four too.
pub fn columns_for(width_dp: f64) -> usize {
    if !width_dp.is_finite() || width_dp <= 0.0 {
        return PHONE_COLUMNS;
    }
    PHONE_COLUMNS.max((width_dp / MAX_CELL_DP).ceil() as usize)
}

/// An app's long-press menu lists at most this many of its shortcuts, as the system's own
/// launcher does (APPS-11, ADR 0013).
pub const MAX_SHORTCUTS: usize = 4;

/// A dragged app's centre within this fraction of an icon's size from another cell's icon
/// centre is over that icon: holding it there makes a folder (FOLD-01). Elsewhere in the cell,
/// it takes the cell's place (APPS-12).
pub const MERGE_RADIUS: f64 = 0.4;

/// A folder's icon shows its first apps as a 2×2 preview (FOLD-02).
pub const PREVIEW_APPS: usize = 4;
/// The preview's slots start this fraction of the icon in from its edges…
pub const PREVIEW_INSET: f64 = 0.14;
/// …with this fraction of it between them. The host draws them the same way.
pub const PREVIEW_GAP: f64 = 0.06;

/// Where slot `slot` of a folder's preview is: its centre, as fractions of the icon from its top
/// left, and its size, as a fraction of the icon. Slots go in reading order, mirrored right to
/// left (FOLD-02).
pub fn preview_slot(slot: usize, rtl: bool) -> (f64, f64, f64) {
    let size = (1.0 - 2.0 * PREVIEW_INSET - PREVIEW_GAP) / 2.0;
    let column = slot % 2;
    let column = if rtl { 1 - column } else { column };
    let row = (slot / 2).min(1);
    let at = |index: usize| PREVIEW_INSET + index as f64 * (size + PREVIEW_GAP) + size / 2.0;
    (at(column), at(row), size)
}

/// What a cell shows, which identifies it across changes: an app's own cell, or a folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CellKey {
    App(AppId),
    Folder(FolderId),
}

/// Where cells are drawn: the grid, the open folder's panel (FOLD-04), or search's results
/// (SRCH-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Surface {
    #[default]
    Grid,
    Folder,
    Search,
}

/// One item of a cell's long-press menu (APPS-11), in the order it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMenuItem {
    /// Opens the system's App info screen.
    AppInfo,
    /// Opens the system's uninstall confirmation (APPS-13). Only for apps the user can
    /// uninstall.
    Uninstall,
    /// Leaves the app out of the grid, search and Recent until it is unhidden in Settings
    /// (APPS-19). It keeps its place. An app's menu only.
    Hide,
    /// Enters edit mode. Not offered while edit mode is on.
    EditApplications,
    /// Starts one of the app's shortcuts. Offered only while MeeGone is the default home app.
    Shortcut(Shortcut),
    /// Asks the user for a folder's new name (FOLD-03). A folder's menu only.
    RenameFolder,
}

/// A cell's long-press menu: an app's (APPS-11) or a folder's (ADR 0015). It covers the home
/// views while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMenu {
    /// The app or folder it is for. The host opens it at that cell.
    pub item: CellKey,
    /// What it offers, in order. An app's: App info, Uninstall, Hide, Edit applications, then
    /// the shortcuts. A folder's: Rename folder, then Edit applications.
    pub items: Vec<AppMenuItem>,
}

impl AppMenu {
    /// Whether the menu offers `item`.
    pub fn offers(&self, item: &AppMenuItem) -> bool {
        self.items.contains(item)
    }
}

/// One cell of the grid, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridItem {
    App {
        app: App,
        /// Installed since the grid was set up and not opened from MeeGone yet: it carries a
        /// dot (APPS-07).
        new: bool,
    },
    Folder {
        id: FolderId,
        /// Its name, or `""` until the user names it: then it shows as "Folder", in the user's
        /// language (FOLD-03).
        name: String,
        apps: Vec<App>,
    },
}

impl GridItem {
    /// The single-line label under the cell: the app's label or the folder's name (APPS-16,
    /// FOLD-02).
    pub fn label(&self) -> &str {
        match self {
            GridItem::App { app, .. } => &app.label,
            GridItem::Folder { name, .. } => name,
        }
    }
}

/// A rectangle on screen, in px from the home views' top left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Bounds {
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
    }

    fn is_valid(&self) -> bool {
        [self.left, self.top, self.right, self.bottom]
            .iter()
            .all(|value| value.is_finite())
            && self.right >= self.left
            && self.bottom >= self.top
    }
}

/// How the host laid a grid out, in physical pixels: the Applications grid, or an open folder's
/// (ADR 0015).
///
/// The cells start `top_px` below the grid's top and scroll under the system bars (ADR 0010).
/// Cell places are worked out exactly as the host lays them out, in whole pixels, so the shell
/// can tell which place is under a dragged icon, and where on screen each icon is (ADR 0014).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    /// The grid's width inside its margins and the safe area.
    pub width_px: f64,
    /// From the top of one row to the top of the next: a cell and the gap below it. Cells grow
    /// taller with the system font size (APPS-16).
    pub row_height_px: f64,
    /// Physical pixels per dp.
    pub density: f64,
    /// Room above the first row and below the last, inside the grid: the safe area.
    pub top_px: f64,
    pub bottom_px: f64,
    /// The grid's height on screen, that room included, or 0 before it has one.
    pub height_px: f64,
    /// Where the cells start, from the home views' left, and where the grid's top is, from the
    /// home views' top.
    pub left_px: f64,
    pub view_top_px: f64,
    /// How big each cell's icon is, and how far below the cell's top it starts, or 0 if the host
    /// hasn't said.
    pub icon_px: f64,
    pub icon_top_px: f64,
}

impl GridLayout {
    fn is_valid(&self) -> bool {
        let sizes = [self.width_px, self.row_height_px, self.density];
        let room = [
            self.top_px,
            self.bottom_px,
            self.height_px,
            self.icon_px,
            self.icon_top_px,
        ];
        sizes.iter().all(|value| value.is_finite() && *value > 0.0)
            && room.iter().all(|value| value.is_finite() && *value >= 0.0)
            && self.left_px.is_finite()
            && self.view_top_px.is_finite()
    }

    /// How many columns it lays out (APPS-02).
    pub fn columns(&self) -> usize {
        columns_for(self.width_px / self.density)
    }

    /// The column `place` is drawn in, from the left: mirrored right to left (L10N-02).
    fn column(&self, place: usize, rtl: bool) -> usize {
        let columns = self.columns();
        let column = place % columns;
        if rtl { columns - 1 - column } else { column }
    }

    /// Where the `column`th column from the left starts, in px from the cells' left. The
    /// leftover pixels are spread, as `AppsGrid` does, so the cells fill the width exactly.
    fn column_left(&self, column: usize) -> f64 {
        let width = self.width_px.round() as i64;
        (width * column as i64 / self.columns() as i64) as f64
    }

    /// Where cell `place` starts, in px from the cells' left.
    pub fn cell_left(&self, place: usize, rtl: bool) -> f64 {
        self.column_left(self.column(place, rtl))
    }

    /// How wide cell `place` is, in px.
    pub fn cell_width(&self, place: usize, rtl: bool) -> f64 {
        let column = self.column(place, rtl);
        self.column_left(column + 1) - self.column_left(column)
    }

    /// Where cell `place` starts, in px from the cells' top.
    pub fn cell_top(&self, place: usize) -> f64 {
        (place / self.columns()) as f64 * self.row_height_px.round()
    }

    /// The centre of cell `place`'s icon, in px from the cells' top left, with the whole cell
    /// drawn at `scale` around its centre, as edit mode draws it (ANIM-12). The cell's centre
    /// if the host hasn't said where its icon is.
    pub fn icon_centre(&self, place: usize, rtl: bool, scale: f64) -> (f64, f64) {
        let row = self.row_height_px.round();
        let x = self.cell_left(place, rtl) + self.cell_width(place, rtl) / 2.0;
        let middle = self.cell_top(place) + row / 2.0;
        if self.icon_px <= 0.0 {
            return (x, middle);
        }
        let icon = self.cell_top(place) + self.icon_top_px + self.icon_px / 2.0;
        (x, middle + (icon - middle) * scale)
    }

    /// Where the cells' top left is on screen, with the grid scrolled `scroll_px`.
    pub fn origin(&self, scroll_px: f64) -> (f64, f64) {
        (self.left_px, self.view_top_px + self.top_px - scroll_px)
    }

    /// Whether `point`, in px from the cells' top left, is over cell `place`'s icon, drawn at
    /// `scale`: within [`MERGE_RADIUS`] of an icon of its centre (FOLD-01).
    pub fn over_icon(&self, place: usize, rtl: bool, scale: f64, point: (f64, f64)) -> bool {
        if self.icon_px <= 0.0 {
            return false;
        }
        let (x, y) = self.icon_centre(place, rtl, scale);
        let reach = MERGE_RADIUS * self.icon_px * scale;
        (point.0 - x).hypot(point.1 - y) <= reach
    }

    /// The place of the cell at (`x`, `y`) px from the cells' top left, among `count` cells:
    /// the nearest one, so a point past the last cell, or outside the grid, is still a place.
    /// `None` when there are no cells.
    pub fn place_at(&self, x: f64, y: f64, count: usize, rtl: bool) -> Option<usize> {
        let last = count.checked_sub(1)?;
        let columns = self.columns();
        let on_screen = (0..columns)
            .rev()
            .find(|&column| x >= self.column_left(column))
            .unwrap_or(0);
        let column = if rtl {
            columns - 1 - on_screen
        } else {
            on_screen
        };
        let row = (y / self.row_height_px.round()).floor().max(0.0) as usize;
        Some((row * columns + column).min(last))
    }

    /// How far a grid of `rows` rows scrolls at most, in px.
    pub fn max_scroll(&self, rows: usize) -> f64 {
        let content = rows as f64 * self.row_height_px.round() + self.top_px + self.bottom_px;
        (content - self.height_px).max(0.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppsState {
    /// The cells, in order. Shared, so a snapshot is cheap to take on every scroll, and the UI
    /// can tell when they changed.
    pub items: Arc<[GridItem]>,
    /// How many columns the grid lays out (APPS-02): four until the host reports its width.
    pub columns: usize,
    /// How far the grid is scrolled from its top, in pixels, as the host reports it. Home sets
    /// it back to 0 (HOME-07), and it is kept when the system ends MeeGone's process (REL-05).
    /// The host scrolls its grid to this position whenever it differs from its own (ADR 0007).
    pub scroll_px: f64,
    /// How the host last laid the grid out, if it has yet.
    pub layout: Option<GridLayout>,
    /// A cell's long-press menu, while it is open (APPS-11). Shared, so the UI can tell when it
    /// changed.
    pub menu: Option<Arc<AppMenu>>,
    /// Edit mode (APPS-12): ✕ badges on the apps the user can uninstall, icons that can be
    /// dragged, and a Done button. A tap on an icon launches nothing.
    pub edit_mode: bool,
    /// The folder open as a panel over the grid (FOLD-04).
    pub open_folder: Option<FolderId>,
    /// The open folder's apps, in order, as its panel shows them; none while it is closed.
    pub folder_items: Arc<[GridItem]>,
    /// How the host laid the open folder's grid out, and where its panel is on screen.
    pub folder_layout: Option<GridLayout>,
    pub folder_bounds: Option<Bounds>,
    /// How far the open folder's grid is scrolled, in pixels.
    pub folder_scroll_px: f64,
    /// The folder whose new name the host is asking the user for (FOLD-03).
    pub renaming: Option<FolderId>,
    /// How many folders have been made since MeeGone started, so the shell can tick for each
    /// (ANIM-24).
    pub folders_made: u32,
    /// The query while search is open, as the user typed it, or `None` while it is closed
    /// (SRCH-01). It is never saved.
    pub search: Option<String>,
    /// The apps that match the query, best first, as the results grid shows them (SRCH-02).
    /// None while the query is empty: the grid shows below the field instead.
    pub results: Arc<[GridItem]>,
    /// How the host laid the results grid out, and the band the search field takes above it,
    /// on screen. The field shows as far as the band is pulled down (ANIM-16).
    pub results_layout: Option<GridLayout>,
    pub search_band: Option<Bounds>,
    /// How far the results grid is scrolled, in pixels. Each new query starts at its top.
    pub results_scroll_px: f64,
    /// Whether a query nothing matches offers a web search: a browser is installed (SRCH-05).
    pub web_search_offered: bool,
    /// The installed apps the user hid, in the locale's alphabetical order, for Settings →
    /// Applications → Hidden apps (APPS-19). Shared, so the UI and the shell can tell when it
    /// changed.
    pub hidden_apps: Arc<[App]>,
    /// The last problem, in plain words. The grid keeps working (REL-01).
    pub problem: Option<String>,
}

impl Default for AppsState {
    fn default() -> Self {
        Self {
            items: Arc::new([]),
            columns: PHONE_COLUMNS,
            scroll_px: 0.0,
            layout: None,
            menu: None,
            edit_mode: false,
            open_folder: None,
            folder_items: Arc::new([]),
            folder_layout: None,
            folder_bounds: None,
            folder_scroll_px: 0.0,
            renaming: None,
            folders_made: 0,
            search: None,
            results: Arc::new([]),
            results_layout: None,
            search_band: None,
            results_scroll_px: 0.0,
            web_search_offered: false,
            hidden_apps: Arc::new([]),
            problem: None,
        }
    }
}

impl AppsState {
    /// How many rows the grid lays out: every item's, plus one empty row below the last, so a
    /// long-press on empty space can always open the home menu (HOME-14, HOME-10).
    pub fn rows(&self) -> usize {
        self.items.len().div_ceil(self.columns.max(1)) + 1
    }

    /// How many rows the open folder's grid lays out, in [`FOLDER_COLUMNS`] (FOLD-04).
    pub fn folder_rows(&self) -> usize {
        self.folder_items.len().div_ceil(FOLDER_COLUMNS)
    }

    /// How many rows the results grid lays out: as many as its apps need, in the grid's columns
    /// (SRCH-02).
    pub fn results_rows(&self) -> usize {
        let columns = self
            .results_layout
            .map_or(self.columns, |layout| layout.columns());
        self.results.len().div_ceil(columns.max(1))
    }

    /// Whether search is open with something typed, so its results show instead of the grid.
    pub fn has_query(&self) -> bool {
        self.search
            .as_deref()
            .is_some_and(|query| !query.trim().is_empty())
    }

    /// How tall the search field's band is, in px, once the host has laid it out (ANIM-16).
    pub fn search_band_px(&self) -> Option<f64> {
        self.search_band
            .map(|band| band.bottom - band.top)
            .filter(|height| *height > 0.0)
    }

    /// The cells `surface` shows.
    pub fn cells(&self, surface: Surface) -> &Arc<[GridItem]> {
        match surface {
            Surface::Grid => &self.items,
            Surface::Folder => &self.folder_items,
            Surface::Search => &self.results,
        }
    }

    /// How the host laid `surface` out, if it has.
    pub fn layout_of(&self, surface: Surface) -> Option<GridLayout> {
        match surface {
            Surface::Grid => self.layout,
            Surface::Folder => self.folder_layout,
            Surface::Search => self.results_layout,
        }
    }

    /// How far `surface` is scrolled, in pixels.
    pub fn scroll_of(&self, surface: Surface) -> f64 {
        match surface {
            Surface::Grid => self.scroll_px,
            Surface::Folder => self.folder_scroll_px,
            Surface::Search => self.results_scroll_px,
        }
    }

    /// How many rows `surface` lays out.
    pub fn rows_of(&self, surface: Surface) -> usize {
        match surface {
            Surface::Grid => self.rows(),
            Surface::Folder => self.folder_rows(),
            Surface::Search => self.results_rows(),
        }
    }

    /// The place of `key`'s cell on `surface`, if it has one there. An app in a folder has a
    /// place only in the open folder's panel.
    pub fn place_of(&self, surface: Surface, key: &CellKey) -> Option<usize> {
        self.cells(surface).iter().position(|item| item.is(key))
    }

    /// The app `id`, in a cell of its own or in a folder.
    pub fn app(&self, id: &AppId) -> Option<&App> {
        self.items.iter().find_map(|item| match item {
            GridItem::App { app, .. } => (app.id == *id).then_some(app),
            GridItem::Folder { apps, .. } => apps.iter().find(|app| app.id == *id),
        })
    }
}

#[derive(Debug)]
pub enum AppsIntent {
    /// Rebuilds the grid from the installed apps and the saved layout.
    Refresh,
    /// Launches an app, growing its window from `from`, the tapped icon (APPS-10, ANIM-08). An
    /// unavailable app is launched too, so the system explains why it can't open (APPS-09). In
    /// edit mode, nothing launches.
    Launch { app: AppId, from: Option<Rect> },
    /// The host laid the grid out, at start or after a size, density or font change.
    Layout(GridLayout),
    /// The host laid the open folder's grid out, in its panel at `bounds` (FOLD-04).
    FolderLayout { layout: GridLayout, bounds: Bounds },
    /// Opens the long-press menu of an app or a folder (APPS-11, ADR 0015).
    OpenMenu(CellKey),
    /// Closes the menu, if it is open.
    CloseMenu,
    /// An item of the menu was chosen. The menu closes, then does what the item says. An item it
    /// doesn't offer is ignored.
    ChooseMenuItem(AppMenuItem),
    /// Enters edit mode (APPS-12). An open menu closes.
    EnterEditMode,
    /// Done: leaves edit mode (APPS-12). A drag's draft not dropped yet is undone.
    ExitEditMode,
    /// In edit mode, shows `item`'s cell at place `to` in the grid, or the last place, while the
    /// others move aside, as a dragged icon goes (APPS-12). Nothing is saved until
    /// [`AppsIntent::Drop`].
    Arrange { item: CellKey, to: usize },
    /// In edit mode, shows `app` at place `to` among its folder's apps (FOLD-05). Not saved
    /// until [`AppsIntent::Drop`].
    ArrangeInFolder { app: AppId, to: usize },
    /// In edit mode, takes `app` out of its folder and shows it at place `to` in the grid, as
    /// when a dragged icon leaves the folder's panel (FOLD-05). The folder closes. Not saved
    /// until [`AppsIntent::Drop`].
    TakeOut { app: AppId, to: usize },
    /// In edit mode, puts `app` into the folder `onto`, or makes a folder of it and the app
    /// `onto` (FOLD-01, FOLD-05). Not saved until [`AppsIntent::Drop`].
    Combine { app: AppId, onto: CellKey },
    /// Keeps what the drag changed, tidies folders left with one app or none (FOLD-06), and
    /// saves the layout (APPS-12, APPS-18).
    Drop,
    /// Shows the saved layout again, as when a drag ends any way but a drop.
    CancelArrange,
    /// In edit mode, moves `item` to place `to` in the grid and saves it: an arrangement and a
    /// drop at once, as a screen reader's "Move to…" actions do (A11Y-05).
    Move { item: CellKey, to: usize },
    /// In edit mode, moves `app` to place `to` in its folder and saves it (A11Y-05).
    MoveInFolder { app: AppId, to: usize },
    /// In edit mode, puts `app` into the folder `with`, or makes a folder of it and the app
    /// `with`, and saves it: a screen reader's "Make folder with…" and "Move into…" (A11Y-05).
    Group { app: AppId, with: CellKey },
    /// In edit mode, takes `app` out of its folder, just after it in the grid, and saves it: a
    /// screen reader's "Move out of folder" (A11Y-05).
    MoveOutOfFolder(AppId),
    /// The ✕ on `app` was tapped, in edit mode, in the grid or the open folder: the system asks
    /// the user to confirm its uninstall (APPS-13). Nothing happens for an app the user can't
    /// uninstall.
    RequestUninstall(AppId),
    /// Opens a folder as a panel over the grid (FOLD-04), in edit mode too.
    OpenFolder(FolderId),
    /// Closes the open folder, as a tap outside it does (FOLD-04).
    CloseFolder,
    /// Asks the user for `folder`'s new name, as tapping its name in the open panel does
    /// (FOLD-03).
    StartRename(FolderId),
    /// The user named `folder` `name`. It is trimmed and cut to [`MAX_FOLDER_NAME`]
    /// characters; an empty name shows as "Folder" (FOLD-03).
    RenameFolder { folder: FolderId, name: String },
    /// The user didn't rename the folder after all.
    CancelRename,
    /// The host scrolled the grid to this many pixels from its top, or MeeGone restores where
    /// it was (REL-05).
    Scrolled(f64),
    /// The open folder's grid was scrolled to this many pixels from its top.
    FolderScrolled(f64),
    /// Home on Applications with nothing open (HOME-07).
    ScrollToTop,
    /// Opens search with nothing typed, as pulling the field down far enough or a screen
    /// reader's "Search apps" does (SRCH-01, A11Y-05). Not in edit mode or with a folder open.
    /// An open menu closes.
    OpenSearch,
    /// The user typed: the results follow (SRCH-02). Ignored while search is closed.
    Search(String),
    /// The keyboard's action key: launches the first result, its window growing from `from`,
    /// its icon (SRCH-03). With no results, it searches the web if that is offered (SRCH-05).
    SubmitSearch { from: Option<Rect> },
    /// "Search the web for '…'", offered when nothing matches (SRCH-05).
    SearchWeb,
    /// Closes search: the grid shows again, where it was (SRCH-04).
    CloseSearch,
    /// The host laid the results grid out, below the search field's `band` (ANIM-16).
    SearchLayout { layout: GridLayout, band: Bounds },
    /// The results grid was scrolled to this many pixels from its top.
    ResultsScrolled(f64),
    /// Settings → Applications → Hidden apps: the app shows again, in its old place, in its
    /// folder too (APPS-19). Ignored unless it is hidden. A failed save changes nothing.
    Unhide(AppId),
}

pub struct AppsHolder {
    apps: Arc<dyn AppsAdapter>,
    /// Whether a browser is installed, and web search (SRCH-05).
    system: Arc<dyn SystemConfigAdapter>,
    store: Arc<dyn LayoutStore>,
    /// Where the hidden apps are kept (APPS-19). Applications is that field's only writer.
    settings: Arc<dyn SettingsStore>,
    /// The layout, kept once the store has been read. Applications is its only writer, so the
    /// file is read at start, or until a read succeeds, and written after each change.
    layout: Option<Layout>,
    /// The kept layout's last save failed, so the next refresh saves it again.
    unsaved: bool,
    /// Every app the system listed at the last refresh, hidden ones too: [`reconcile`] keeps
    /// their places.
    installed: Vec<App>,
    /// The apps the user hid, as last read or saved (APPS-19).
    hidden: BTreeSet<AppId>,
    /// The installed apps that aren't hidden: the only ones the cells, the drag's edits and
    /// search see.
    shown: Vec<App>,
    /// In edit mode, the layout as the drag in progress has changed it, shown but not saved
    /// until it is dropped (APPS-12, FOLD-05). A refresh keeps it.
    draft: Option<Layout>,
    /// The query, folded by the platform as the labels are (SRCH-02, L10N-04).
    query_key: String,
    state: AppsState,
}

impl AppsHolder {
    pub fn new(
        apps: Arc<dyn AppsAdapter>,
        system: Arc<dyn SystemConfigAdapter>,
        layout: Arc<dyn LayoutStore>,
        settings: Arc<dyn SettingsStore>,
    ) -> Self {
        Self {
            apps,
            system,
            store: layout,
            settings,
            layout: None,
            unsaved: false,
            installed: Vec::new(),
            hidden: BTreeSet::new(),
            shown: Vec::new(),
            draft: None,
            query_key: String::new(),
            state: AppsState::default(),
        }
    }

    /// Reconciles the layout with the installed apps, saves it if it changed, and shows it
    /// (APPS-06, APPS-08, APPS-18). A drag's draft is reconciled too, but keeps a folder it left
    /// with one app until the drop. If the system can't list the apps, the grid stays as it is.
    ///
    /// Hidden apps are reconciled as installed, so they keep their places, but show no cell. A
    /// hidden app that is gone is forgotten, and saved so, so installing it again shows it
    /// (APPS-19).
    ///
    /// A layout another copy of MeeGone saved came from a backup: its apps that aren't installed
    /// yet keep their places for [`RESTORE_WAIT_MS`], out of sight (APPS-21). Every layout is
    /// saved with this copy's install time, so a restore is recognised once.
    fn refresh(&mut self) {
        let (installed, profiles) = match self.catalogue() {
            Ok(catalogue) => catalogue,
            Err(error) => {
                self.state.problem = Some(error.to_string());
                return;
            }
        };
        self.state.problem = None;
        let hidden = self.settings.get().hidden_apps;
        let installed_ms = self.system.installed_ms();
        let now_ms = self.system.now_ms();
        // The layout `saved` becomes, saved as this copy's, whether it changed, and the hidden
        // apps still standing. A restore whose wait is over ends first (APPS-21).
        let reconciled = |saved: &Layout, start: &Start| {
            let current = end_restore(saved.clone(), now_ms);
            let mut layout = reconcile(&current, &installed, &profiles, start);
            layout.installed_ms = Some(installed_ms);
            let changed = layout != *saved;
            let still = follow_hidden(&hidden, saved, &layout, &installed, &profiles);
            (layout, changed, still)
        };
        let (layout, changed, still) = match self.layout.take() {
            Some(kept) => reconciled(&kept, &Start::Saved),
            None => match self.store.load() {
                Ok(SavedLayout::Missing) => {
                    let defaults = self.apps.default_apps();
                    let (layout, _, still) =
                        reconciled(&Layout::default(), &Start::First { defaults });
                    (layout, true, still)
                }
                // Saved at once with this copy's install time, which differs from the backup's,
                // so it is restored only once (APPS-21).
                Ok(SavedLayout::Read(saved)) if is_restored(&saved, installed_ms) => {
                    let until_ms = now_ms.saturating_add(RESTORE_WAIT_MS);
                    reconciled(&saved, &Start::Restored { until_ms })
                }
                Ok(SavedLayout::Read(saved)) => reconciled(&saved, &Start::Saved),
                // Saved at once, so the damaged file is replaced before anything else can read
                // it (REL-02).
                Ok(SavedLayout::Recovered(recovered)) => {
                    let (layout, _, still) = reconciled(&recovered, &Start::Rebuilt);
                    (layout, true, still)
                }
                Err(error) => {
                    // Shown but never saved: the file may be fine, so the next refresh reads it
                    // again instead of replacing it (REL-02). Nothing can be edited meanwhile,
                    // and no hidden app is forgotten.
                    let rebuilt = reconciled(&Layout::default(), &Start::Rebuilt).0;
                    self.take_on(installed, hidden);
                    self.draft = None;
                    self.show_layout(&rebuilt);
                    self.state.problem = Some(error.to_string());
                    return;
                }
            },
        };
        if changed || self.unsaved {
            self.save(&layout);
        }
        let hidden = if still == hidden {
            hidden
        } else {
            // A failed save keeps them; the next refresh tries again.
            match self
                .settings
                .update(&mut |settings| settings.hidden_apps = still.clone())
            {
                Ok(settings) => settings.hidden_apps,
                Err(error) => {
                    self.state.problem = Some(error.to_string());
                    hidden
                }
            }
        };
        self.draft = self.draft.take().map(|draft| {
            reconcile_draft(
                &end_restore(draft, now_ms),
                &installed,
                &profiles,
                &Start::Saved,
            )
        });
        self.take_on(installed, hidden);
        self.layout = Some(layout);
        self.show();
        // The menu of an app or folder that was just removed closes.
        let gone = |menu: &Arc<AppMenu>| !self.has(&menu.item);
        if self.state.menu.as_ref().is_some_and(gone) {
            self.state.menu = None;
        }
    }

    /// Takes on the apps the system listed and the ones the user hid: the cells show only the
    /// others, and Settings lists the hidden ones that are installed (APPS-19). The list is
    /// replaced only when it changes.
    fn take_on(&mut self, installed: Vec<App>, hidden: BTreeSet<AppId>) {
        self.shown = installed
            .iter()
            .filter(|app| !hidden.contains(&app.id))
            .cloned()
            .collect();
        let mut listed: Vec<App> = installed
            .iter()
            .filter(|app| hidden.contains(&app.id))
            .cloned()
            .collect();
        listed.sort_by(alphabetical);
        // An app the system listed twice is listed once.
        listed.dedup_by(|a, b| a.id == b.id);
        if *self.state.hidden_apps != *listed {
            self.state.hidden_apps = listed.into();
        }
        self.installed = installed;
        self.hidden = hidden;
    }

    /// APPS-19: changes which apps are hidden and saves it in the settings store; the cells,
    /// the open folder, search and the list follow. The layout never changes, so a hidden app
    /// keeps its place and comes back to it. A failed save changes nothing.
    fn change_hidden(&mut self, change: impl Fn(&mut BTreeSet<AppId>)) -> Result<(), DataError> {
        let settings = self
            .settings
            .update(&mut |settings| change(&mut settings.hidden_apps))?;
        let installed = std::mem::take(&mut self.installed);
        self.take_on(installed, settings.hidden_apps);
        self.show();
        Ok(())
    }

    /// Settings → Applications → Hidden apps: `app` shows again, in its old place (APPS-19).
    fn unhide(&mut self, app: &AppId) {
        if !self.hidden.contains(app) {
            return;
        }
        self.state.problem = self
            .change_hidden(|hidden| {
                hidden.remove(app);
            })
            .err()
            .map(|error| error.to_string());
    }

    fn catalogue(&self) -> Result<(Vec<App>, Vec<Profile>), DataError> {
        Ok((self.apps.launchable_apps()?, self.apps.profiles()?))
    }

    fn save(&mut self, layout: &Layout) {
        let saved = self.store.save(layout);
        self.unsaved = saved.is_err();
        if let Err(error) = saved {
            self.state.problem = Some(error.to_string());
        }
    }

    /// Shows the drag's draft, or else the kept layout. This is the only place the cells are
    /// made, so they never go stale. They are replaced only when they change, so the host
    /// rebinds nothing it doesn't need to.
    fn show(&mut self) {
        let items = match self.draft.as_ref().or(self.layout.as_ref()) {
            Some(layout) => grid(layout, &self.shown),
            None => return,
        };
        self.show_cells(items);
    }

    fn show_layout(&mut self, layout: &Layout) {
        let items = grid(layout, &self.shown);
        self.show_cells(items);
    }

    /// Takes on `items`, the open folder's apps among them, and the results among them. A
    /// folder that has gone closes.
    fn show_cells(&mut self, items: Vec<GridItem>) {
        if *self.state.items != *items {
            self.state.items = items.into();
        }
        let open = self.state.open_folder.and_then(|folder| {
            self.state.items.iter().find_map(|item| match item {
                GridItem::Folder { id, apps, .. } if *id == folder => Some(apps.clone()),
                _ => None,
            })
        });
        let folder_items: Vec<GridItem> = match open {
            Some(apps) => apps
                .into_iter()
                .map(|app| GridItem::App { app, new: false })
                .collect(),
            None => {
                self.state.open_folder = None;
                Vec::new()
            }
        };
        if *self.state.folder_items != *folder_items {
            self.state.folder_items = folder_items.into();
        }
        if self
            .state
            .renaming
            .is_some_and(|folder| !self.has(&CellKey::Folder(folder)))
        {
            self.state.renaming = None;
        }
        self.show_results();
    }

    /// Ranks the grid's apps for the query (SRCH-02). Installs and removals reach the results
    /// with the cells. They are replaced only when they change.
    fn show_results(&mut self) {
        let results = match self.state.search {
            Some(_) => search::rank(&self.state.items, &self.query_key),
            None => Vec::new(),
        };
        if *self.state.results != *results {
            self.state.results = results.into();
        }
    }

    /// SRCH-01: opens search with nothing typed, unless edit mode or a folder is open. It reads
    /// whether a browser is installed each time, for the web search offer (SRCH-05).
    fn open_search(&mut self) {
        if self.state.edit_mode || self.state.open_folder.is_some() || self.state.search.is_some() {
            return;
        }
        self.state.menu = None;
        self.state.search = Some(String::new());
        self.state.results_scroll_px = 0.0;
        self.state.web_search_offered = self.system.has_browser();
        self.query_key.clear();
        self.show_results();
    }

    /// SRCH-02: the platform folds the query as it folded the labels, and the results follow,
    /// from their top. Ignored while search is closed.
    fn search(&mut self, query: String) {
        if self
            .state
            .search
            .as_ref()
            .is_none_or(|typed| *typed == query)
        {
            return;
        }
        self.query_key = self.apps.search_key(&query);
        self.state.search = Some(query);
        self.state.results_scroll_px = 0.0;
        self.show_results();
    }

    /// SRCH-04: the grid shows again, where it was; search never moves it. Returns whether
    /// search was open.
    fn close_search(&mut self) -> bool {
        let was_open = self.state.search.take().is_some();
        if was_open {
            self.query_key.clear();
            self.state.results_scroll_px = 0.0;
            self.show_results();
        }
        was_open
    }

    /// SRCH-03: the first result launches. With none, the web is searched if that is offered
    /// (SRCH-05).
    fn submit_search(&mut self, from: Option<Rect>) {
        if self.state.search.is_none() {
            return;
        }
        match self.state.results.first() {
            Some(GridItem::App { app, .. }) => {
                let id = app.id.clone();
                self.launch(&id, from);
            }
            _ => self.search_web(),
        }
    }

    /// SRCH-05: only when the query matches nothing and a browser is installed. The device's
    /// default search handler takes the query; search closes, as after a launch.
    fn search_web(&mut self) {
        let Some(query) = self.state.search.as_deref().map(str::trim) else {
            return;
        };
        if query.is_empty() || !self.state.results.is_empty() || !self.state.web_search_offered {
            return;
        }
        match self.system.web_search(query) {
            Ok(()) => {
                self.state.problem = None;
                self.close_search();
            }
            Err(error) => self.state.problem = Some(error.to_string()),
        }
    }

    /// Whether the grid shows `key`, in a cell of its own or, for an app, in a folder.
    fn has(&self, key: &CellKey) -> bool {
        match key {
            CellKey::App(id) => self.state.app(id).is_some(),
            CellKey::Folder(_) => self.state.place_of(Surface::Grid, key).is_some(),
        }
    }

    fn launch(&mut self, app: &AppId, from: Option<Rect>) {
        // In edit mode a tap arranges; it never leaves MeeGone (APPS-12).
        if self.state.edit_mode {
            return;
        }
        self.state.problem = self
            .apps
            .launch(app, from)
            .err()
            .map(|error| error.to_string());
        if self.state.problem.is_none() {
            self.opened(app);
            // Coming back shows the grid, not an old query (SRCH-04).
            self.close_search();
        }
    }

    /// An app opened from MeeGone loses its dot, for good (APPS-07).
    fn opened(&mut self, app: &AppId) {
        let Some(mut layout) = self.layout.take() else {
            return;
        };
        if layout.new.remove(app) {
            self.save(&layout);
            if let Some(draft) = &mut self.draft {
                draft.new.remove(app);
            }
        }
        self.layout = Some(layout);
        self.show();
    }

    /// Takes on a new layout. When it changes the columns or the row height, as folding,
    /// rotating or a new density or font size can, the grid keeps the same first visible item
    /// at its top (COMP-03, ADR 0009).
    fn lay_out(&mut self, grid: GridLayout) {
        if !grid.is_valid() {
            return;
        }
        let columns = grid.columns();
        if let Some(before) = self.state.layout {
            if before.row_height_px != grid.row_height_px || self.state.columns != columns {
                let row = (self.state.scroll_px / before.row_height_px).floor() as usize;
                let first = row * self.state.columns;
                self.state.scroll_px = (first / columns) as f64 * grid.row_height_px;
            }
        }
        self.state.layout = Some(grid);
        self.state.columns = columns;
    }

    /// FOLD-04: opens a folder the grid shows, in or out of edit mode. Another folder that was
    /// open closes, and the panel starts at its top.
    fn open_folder(&mut self, folder: FolderId) {
        if self
            .state
            .place_of(Surface::Grid, &CellKey::Folder(folder))
            .is_none()
        {
            return;
        }
        if self.state.open_folder != Some(folder) {
            self.state.folder_scroll_px = 0.0;
        }
        // A folder tapped in the grid below an empty search field opens instead.
        self.close_search();
        self.state.open_folder = Some(folder);
        self.show();
    }

    fn close_folder(&mut self) -> bool {
        let was_open = self.state.open_folder.take().is_some();
        if was_open {
            self.show();
        }
        was_open
    }

    /// APPS-11: App info, Uninstall when the user can uninstall the app (APPS-13), Hide
    /// (APPS-19), Edit applications unless edit mode is on, then the app's first shortcuts. The
    /// system offers shortcuts only while MeeGone is the default home app; otherwise, or if it
    /// can't say, the menu has none. A folder's menu offers Rename folder, then Edit
    /// applications.
    fn open_menu(&mut self, key: CellKey) {
        let mut items = Vec::new();
        match &key {
            CellKey::App(id) => {
                let Some(app) = self.state.app(id) else {
                    return;
                };
                items.push(AppMenuItem::AppInfo);
                if app.can_uninstall {
                    items.push(AppMenuItem::Uninstall);
                }
                items.push(AppMenuItem::Hide);
            }
            CellKey::Folder(_) if self.has(&key) => items.push(AppMenuItem::RenameFolder),
            CellKey::Folder(_) => return,
        }
        if !self.state.edit_mode {
            items.push(AppMenuItem::EditApplications);
        }
        if let CellKey::App(id) = &key {
            let shortcuts = self.apps.shortcuts(id).unwrap_or_default();
            items.extend(
                shortcuts
                    .into_iter()
                    .take(MAX_SHORTCUTS)
                    .map(AppMenuItem::Shortcut),
            );
        }
        self.close_search();
        self.state.menu = Some(Arc::new(AppMenu { item: key, items }));
    }

    /// Closes the menu, then does what `item` says. Leaving MeeGone for a system screen or a
    /// shortcut can fail; the failure is the problem, shown briefly.
    fn choose(&mut self, item: AppMenuItem) {
        let Some(menu) = self.state.menu.take_if(|menu| menu.offers(&item)) else {
            return;
        };
        let done = match (item, &menu.item) {
            (AppMenuItem::AppInfo, CellKey::App(app)) => self.apps.open_app_info(app),
            // MeeGone never uninstalls anything itself: the system asks the user (APPS-13).
            (AppMenuItem::Uninstall, CellKey::App(app)) => self.apps.request_uninstall(app),
            (AppMenuItem::Hide, CellKey::App(app)) => self.change_hidden(|hidden| {
                hidden.insert(app.clone());
            }),
            (AppMenuItem::Shortcut(shortcut), _) => self.apps.launch_shortcut(&shortcut),
            (AppMenuItem::EditApplications, _) => {
                self.state.edit_mode = true;
                Ok(())
            }
            (AppMenuItem::RenameFolder, CellKey::Folder(folder)) => {
                self.state.renaming = Some(*folder);
                Ok(())
            }
            _ => Ok(()),
        };
        self.state.problem = done.err().map(|error| error.to_string());
    }

    /// Leaves edit mode, undoing a drag's draft that wasn't dropped. Returns whether it was on.
    fn exit_edit_mode(&mut self) -> bool {
        self.cancel_draft();
        std::mem::take(&mut self.state.edit_mode)
    }

    /// Applies one step of a drag to its draft (APPS-12, FOLD-05). Only in edit mode, and only
    /// while the layout could be read, since nothing could be saved otherwise (REL-02). The step
    /// gets the apps that show, so hidden apps keep their slots (APPS-19). Returns whether it
    /// applied.
    fn edit(&mut self, step: impl FnOnce(&Layout, &[App]) -> Option<Layout>) -> bool {
        if !self.state.edit_mode {
            return false;
        }
        let Some(base) = self.draft.as_ref().or(self.layout.as_ref()) else {
            return false;
        };
        let Some(next) = step(base, &self.shown) else {
            return false;
        };
        self.draft = Some(next);
        self.show();
        true
    }

    /// APPS-12, APPS-18: the draft becomes the layout, its folders of one or none tidied
    /// (FOLD-06), and saved once. A failed save is the problem, and the next refresh saves again
    /// (ADR 0012).
    fn commit(&mut self) {
        let Some(draft) = self.draft.take() else {
            return;
        };
        let Some(layout) = self.layout.take() else {
            return;
        };
        let kept = folders::tidy(&draft);
        let made = folders::folders_made(&layout, &kept);
        self.state.folders_made = self.state.folders_made.wrapping_add(made as u32);
        if kept != layout {
            self.save(&kept);
        }
        self.layout = Some(kept);
        self.show();
    }

    fn cancel_draft(&mut self) {
        if self.draft.take().is_some() {
            self.show();
        }
    }

    /// FOLD-03: the trimmed name, at most [`MAX_FOLDER_NAME`] characters, kept in the layout
    /// and in any draft, and saved. A folder that has gone is left alone.
    fn rename(&mut self, folder: FolderId, name: &str) {
        self.state.renaming = None;
        let name: String = name.trim().chars().take(MAX_FOLDER_NAME).collect();
        let name = name.trim_end().to_owned();
        let Some(mut layout) = self.layout.take() else {
            return;
        };
        if set_name(&mut layout, folder, &name) {
            self.save(&layout);
        }
        if let Some(draft) = &mut self.draft {
            set_name(draft, folder, &name);
        }
        self.layout = Some(layout);
        self.show();
    }

    /// APPS-13: the ✕ asks the system, which asks the user. MeeGone never uninstalls anything
    /// itself, and apps the user can't uninstall have no ✕. It is on an app's own cell, or on
    /// an app in the open folder.
    fn request_uninstall(&mut self, id: &AppId) {
        let key = CellKey::App(id.clone());
        let shown = self.state.place_of(Surface::Grid, &key).is_some()
            || self.state.place_of(Surface::Folder, &key).is_some();
        let can = self.state.edit_mode
            && shown
            && self.state.app(id).is_some_and(|app| app.can_uninstall);
        if can {
            self.state.problem = self
                .apps
                .request_uninstall(id)
                .err()
                .map(|error| error.to_string());
        }
    }
}

/// Names `folder` `name` in `layout`. Returns whether it had that folder with another name.
fn set_name(layout: &mut Layout, folder: FolderId, name: &str) -> bool {
    layout.entries.iter_mut().any(|entry| match entry {
        meegone_apps_data::LayoutEntry::Folder { id, name: old, .. }
            if *id == folder && old != name =>
        {
            *old = name.to_owned();
            true
        }
        _ => false,
    })
}

impl StateHolder for AppsHolder {
    type State = AppsState;
    type Intent = AppsIntent;

    fn state(&self) -> &AppsState {
        &self.state
    }

    fn handle(&mut self, intent: AppsIntent) {
        match intent {
            AppsIntent::Refresh => self.refresh(),
            AppsIntent::Launch { app, from } => self.launch(&app, from),
            AppsIntent::Layout(grid) => self.lay_out(grid),
            AppsIntent::FolderLayout { layout, bounds } => {
                if layout.is_valid() && bounds.is_valid() {
                    self.state.folder_layout = Some(layout);
                    self.state.folder_bounds = Some(bounds);
                }
            }
            AppsIntent::OpenMenu(key) => self.open_menu(key),
            AppsIntent::CloseMenu => self.state.menu = None,
            AppsIntent::ChooseMenuItem(item) => self.choose(item),
            AppsIntent::EnterEditMode => {
                self.state.menu = None;
                self.close_search();
                self.state.edit_mode = true;
            }
            AppsIntent::ExitEditMode => {
                self.state.menu = None;
                self.exit_edit_mode();
            }
            AppsIntent::Arrange { item, to } => {
                self.edit(|layout, shown| folders::arrange(layout, shown, &item, to));
            }
            AppsIntent::ArrangeInFolder { app, to } => {
                self.edit(|layout, shown| folders::arrange_in_folder(layout, shown, &app, to));
            }
            AppsIntent::TakeOut { app, to } => {
                if self.edit(|layout, shown| folders::take_out(layout, shown, &app, to)) {
                    self.close_folder();
                }
            }
            AppsIntent::Combine { app, onto } => {
                self.edit(|layout, _| folders::combine(layout, &app, &onto));
            }
            AppsIntent::Drop => self.commit(),
            AppsIntent::CancelArrange => self.cancel_draft(),
            AppsIntent::Move { item, to } => {
                if self.edit(|layout, shown| folders::arrange(layout, shown, &item, to)) {
                    self.commit();
                }
            }
            AppsIntent::MoveInFolder { app, to } => {
                if self.edit(|layout, shown| folders::arrange_in_folder(layout, shown, &app, to)) {
                    self.commit();
                }
            }
            AppsIntent::Group { app, with } => {
                if self.edit(|layout, _| folders::combine(layout, &app, &with)) {
                    self.commit();
                }
            }
            AppsIntent::MoveOutOfFolder(app) => {
                if self.edit(|layout, shown| folders::move_out(layout, shown, &app)) {
                    self.commit();
                }
            }
            AppsIntent::RequestUninstall(app) => self.request_uninstall(&app),
            AppsIntent::OpenFolder(folder) => self.open_folder(folder),
            AppsIntent::CloseFolder => {
                self.close_folder();
            }
            AppsIntent::StartRename(folder) => {
                if self.has(&CellKey::Folder(folder)) && self.layout.is_some() {
                    self.state.renaming = Some(folder);
                }
            }
            AppsIntent::RenameFolder { folder, name } => self.rename(folder, &name),
            AppsIntent::CancelRename => self.state.renaming = None,
            AppsIntent::Scrolled(px) => {
                if px.is_finite() {
                    self.state.scroll_px = px.max(0.0);
                }
            }
            AppsIntent::FolderScrolled(px) => {
                if px.is_finite() {
                    self.state.folder_scroll_px = px.max(0.0);
                }
            }
            AppsIntent::ScrollToTop => self.state.scroll_px = 0.0,
            AppsIntent::OpenSearch => self.open_search(),
            AppsIntent::Search(query) => self.search(query),
            AppsIntent::SubmitSearch { from } => self.submit_search(from),
            AppsIntent::SearchWeb => self.search_web(),
            AppsIntent::CloseSearch => {
                self.close_search();
            }
            AppsIntent::SearchLayout { layout, band } => {
                if layout.is_valid() && band.is_valid() {
                    self.state.results_layout = Some(layout);
                    self.state.search_band = Some(band);
                }
            }
            AppsIntent::ResultsScrolled(px) => {
                if px.is_finite() {
                    self.state.results_scroll_px = px.max(0.0);
                }
            }
            AppsIntent::Unhide(app) => self.unhide(&app),
        }
    }

    /// HOME-08: the menu, then the open folder, then search (SRCH-04), then edit mode.
    fn on_back(&mut self) -> bool {
        if self.state.menu.take().is_some() || self.close_folder() || self.close_search() {
            return true;
        }
        self.exit_edit_mode()
    }
}

#[cfg(test)]
mod tests {
    use meegone_apps_data::{LayoutEntry, PersistentLayoutStore, Restore};
    use meegone_core_data::fakes::{FakeStorage, Fakes, app, app_id, work_app};
    use meegone_core_data::storage::format::seal;
    use meegone_core_data::{PersistentSettingsStore, ProfileId, StoredFile};

    use super::*;

    /// A holder over `layout`, with its settings in `fakes.storage`, so a test can fail settings
    /// saves without failing the layout's.
    fn holder(fakes: &Fakes, layout: &Arc<PersistentLayoutStore>) -> AppsHolder {
        let mut holder = AppsHolder::new(
            fakes.apps.clone(),
            fakes.system.clone(),
            layout.clone(),
            Arc::new(PersistentSettingsStore::new(fakes.storage.clone())),
        );
        holder.handle(AppsIntent::Refresh);
        holder
    }

    fn layout_store() -> Arc<PersistentLayoutStore> {
        Arc::new(PersistentLayoutStore::new(Arc::new(FakeStorage::default())))
    }

    fn labels(state: &AppsState) -> Vec<String> {
        state
            .items
            .iter()
            .map(|item| match item {
                GridItem::App { app, new: false } => app.label.clone(),
                GridItem::App { app, new: true } => format!("{}•", app.label),
                GridItem::Folder { name, .. } => format!("[{name}]"),
            })
            .collect()
    }

    fn install(fakes: &Fakes, apps: &[(&str, &str)]) {
        *fakes.apps.apps.lock().unwrap() = apps
            .iter()
            .map(|(package, label)| app(package, label))
            .collect();
    }

    #[test]
    fn the_first_grid_is_alphabetical_and_saved() {
        // APPS-04
        let fakes = Fakes::default();
        install(
            &fakes,
            &[("mail", "Mail"), ("camera", "camera"), ("phone", "Phone")],
        );
        let layout = layout_store();

        let holder = holder(&fakes, &layout);

        assert_eq!(labels(holder.state()), ["camera", "Mail", "Phone"]);
        let Ok(SavedLayout::Read(saved)) = layout.load() else {
            panic!("not saved");
        };
        assert_eq!(saved.entries.len(), 3);
    }

    #[test]
    fn the_saved_order_is_kept_and_new_apps_go_at_the_end() {
        // APPS-06, APPS-18
        let fakes = Fakes::default();
        let layout = layout_store();
        layout
            .save(&Layout {
                entries: vec![
                    LayoutEntry::App(app_id("phone")),
                    LayoutEntry::App(app_id("gone")),
                    LayoutEntry::App(app_id("mail")),
                ],
                ..Layout::default()
            })
            .unwrap();
        install(
            &fakes,
            &[("mail", "Mail"), ("phone", "Phone"), ("atlas", "Atlas")],
        );

        let holder = holder(&fakes, &layout);

        // Atlas is new: it carries a dot (APPS-07).
        assert_eq!(labels(holder.state()), ["Phone", "Mail", "Atlas•"]);
    }

    #[test]
    fn a_folder_left_with_one_app_becomes_that_app() {
        // FOLD-06
        let fakes = Fakes::default();
        let layout = layout_store();
        layout
            .save(&Layout {
                entries: vec![
                    LayoutEntry::Folder {
                        id: FolderId(1),
                        name: "Folder".into(),
                        apps: vec![app_id("maps"), app_id("gone")],
                    },
                    LayoutEntry::Folder {
                        id: FolderId(2),
                        name: "Games".into(),
                        apps: vec![app_id("chess"), app_id("go")],
                    },
                ],
                ..Layout::default()
            })
            .unwrap();
        install(
            &fakes,
            &[("maps", "Maps"), ("chess", "Chess"), ("go", "Go")],
        );

        let holder = holder(&fakes, &layout);

        assert_eq!(labels(holder.state()), ["Maps", "[Games]"]);
    }

    /// The icon's rect, which a launch grows from (ANIM-08).
    const ICON: Rect = Rect {
        left: 40,
        top: 300,
        right: 208,
        bottom: 468,
    };

    #[test]
    fn launching_goes_through_the_apps_adapter_from_the_icon() {
        // APPS-10, ANIM-08; the side effect runs in the data layer (ARCH-05).
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout_store());

        holder.handle(AppsIntent::Launch {
            app: app_id("mail"),
            from: Some(ICON),
        });

        assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
        assert_eq!(*fakes.apps.launched_from.lock().unwrap(), [Some(ICON)]);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn an_unavailable_app_is_launched_so_the_system_explains() {
        // APPS-09: paused, suspended or on missing storage, it is dimmed, and a tap still
        // reaches the system, which says why it can't open.
        let fakes = Fakes::default();
        let mut paused = work_app("mail", "Mail");
        paused.available = false;
        *fakes.apps.apps.lock().unwrap() = vec![paused.clone()];
        let mut holder = holder(&fakes, &layout_store());
        assert_eq!(
            *holder.state().items,
            [GridItem::App {
                app: paused.clone(),
                new: false
            }]
        );

        holder.handle(AppsIntent::Launch {
            app: paused.id.clone(),
            from: None,
        });

        assert_eq!(*fakes.apps.launched.lock().unwrap(), [paused.id]);
    }

    #[test]
    fn a_launch_the_system_refuses_becomes_a_short_message() {
        // APPS-09: when the system gives no explanation of its own.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout_store());
        *fakes.apps.error.lock().unwrap() =
            Some(DataError::Unsupported("App isn't available".into()));

        holder.handle(AppsIntent::Launch {
            app: app_id("mail"),
            from: Some(ICON),
        });
        assert_eq!(
            holder.state().problem.as_deref(),
            Some("App isn't available")
        );

        *fakes.apps.error.lock().unwrap() = None;
        holder.handle(AppsIntent::Launch {
            app: app_id("mail"),
            from: Some(ICON),
        });
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn personal_and_work_apps_share_one_grid() {
        // APPS-01, APPS-20
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![
            app("mail", "Mail"),
            work_app("mail", "Mail"),
            work_app("docs", "Docs"),
        ];

        let holder = holder(&fakes, &layout_store());

        let work: Vec<(&str, bool)> = holder
            .state()
            .items
            .iter()
            .map(|item| match item {
                GridItem::App { app, .. } => (app.label.as_str(), app.work),
                GridItem::Folder { .. } => unreachable!(),
            })
            .collect();
        assert_eq!(work, [("Docs", true), ("Mail", false), ("Mail", true)]);
    }

    #[test]
    fn a_refresh_that_changes_nothing_keeps_the_same_cells() {
        // The UI pulls the cells again only when they change.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        let mut holder = holder(&fakes, &layout_store());
        let before = Arc::clone(&holder.state().items);

        holder.handle(AppsIntent::Refresh);
        assert!(Arc::ptr_eq(&before, &holder.state().items));

        install(&fakes, &[("mail", "Mail")]);
        holder.handle(AppsIntent::Refresh);
        assert!(!Arc::ptr_eq(&before, &holder.state().items));
        assert_eq!(labels(holder.state()), ["Mail"]);
    }

    #[test]
    fn phones_have_four_columns() {
        // APPS-02: phones are under 600 dp wide; their grid is at most 568 dp, usually under
        // 450 dp.
        for width_dp in (200..=511).map(f64::from) {
            assert_eq!(columns_for(width_dp), 4, "{width_dp} dp");
        }
        for nonsense in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(columns_for(nonsense), 4);
        }
    }

    #[test]
    fn on_large_screens_every_cell_is_96_to_128_dp_wide() {
        // APPS-02: a window 600 dp wide or more leaves a grid of at least 384 dp inside its
        // safe area and 16 dp margins.
        for tenths in 3_840..=25_600 {
            let width_dp = f64::from(tenths) / 10.0;
            let cell = width_dp / columns_for(width_dp) as f64;
            assert!(
                (MIN_CELL_DP..=MAX_CELL_DP).contains(&cell),
                "{width_dp} dp: {cell} dp cells"
            );
        }
        assert_eq!(columns_for(568.0), 5);
        assert_eq!(columns_for(1_248.0), 10);
    }

    /// A grid `width_dp` wide at `density`, with rows `row_dp` apart.
    fn grid_layout(width_dp: f64, row_dp: f64, density: f64) -> GridLayout {
        GridLayout {
            width_px: width_dp * density,
            row_height_px: row_dp * density,
            density,
            top_px: 0.0,
            bottom_px: 0.0,
            height_px: 0.0,
            left_px: 0.0,
            view_top_px: 0.0,
            icon_px: 0.0,
            icon_top_px: 0.0,
        }
    }

    #[test]
    fn the_layout_sets_the_columns_and_rows() {
        // APPS-02, HOME-14
        let fakes = Fakes::default();
        let apps: Vec<(String, String)> = (0..12)
            .map(|i| (format!("app{i:02}"), format!("App {i:02}")))
            .collect();
        let apps: Vec<(&str, &str)> = apps.iter().map(|(p, l)| (p.as_str(), l.as_str())).collect();
        install(&fakes, &apps);
        let mut holder = holder(&fakes, &layout_store());
        assert_eq!((holder.state().columns, holder.state().rows()), (4, 4));

        holder.handle(AppsIntent::Layout(grid_layout(568.0, 104.0, 2.0)));
        assert_eq!((holder.state().columns, holder.state().rows()), (5, 4));

        holder.handle(AppsIntent::Layout(grid_layout(1_000.0, 104.0, 2.0)));
        assert_eq!((holder.state().columns, holder.state().rows()), (8, 3));

        // A layout the host couldn't measure changes nothing.
        holder.handle(AppsIntent::Layout(grid_layout(f64::NAN, 104.0, 2.0)));
        holder.handle(AppsIntent::Layout(grid_layout(400.0, 0.0, 2.0)));
        assert_eq!(holder.state().columns, 8);
    }

    #[test]
    fn a_new_layout_keeps_the_first_visible_item_at_the_top() {
        // COMP-03, ADR 0009: unfolding adds columns; a new density or font size changes the
        // row height. Either way the same app stays first.
        let fakes = Fakes::default();
        let mut holder = holder(&fakes, &layout_store());
        holder.handle(AppsIntent::Layout(grid_layout(400.0, 100.0, 2.0)));
        // Row 10 of 4 columns, part-way down: item 40 is the first one visible.
        holder.handle(AppsIntent::Scrolled(10.5 * 200.0));

        holder.handle(AppsIntent::Layout(grid_layout(800.0, 100.0, 2.0)));
        // 7 columns: item 40 is in row 5.
        assert_eq!(holder.state().columns, 7);
        assert_eq!(holder.state().scroll_px, 5.0 * 200.0);

        holder.handle(AppsIntent::Layout(grid_layout(800.0, 100.0, 3.0)));
        // Same columns, a new density: still row 5, now 300 px apart.
        assert_eq!(holder.state().scroll_px, 5.0 * 300.0);

        // A layout that changes neither leaves the position alone.
        holder.handle(AppsIntent::Scrolled(1_234.0));
        holder.handle(AppsIntent::Layout(grid_layout(800.0, 100.0, 3.0)));
        assert_eq!(holder.state().scroll_px, 1_234.0);
    }

    #[test]
    fn the_first_layout_keeps_a_restored_position() {
        // REL-05: the position comes back before the grid is laid out.
        let fakes = Fakes::default();
        let mut holder = holder(&fakes, &layout_store());
        holder.handle(AppsIntent::Scrolled(720.0));

        holder.handle(AppsIntent::Layout(grid_layout(800.0, 100.0, 2.0)));

        assert_eq!(holder.state().scroll_px, 720.0);
    }

    #[test]
    fn a_failing_adapter_becomes_a_problem_not_a_crash() {
        let fakes = Fakes::default();
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed("App list unavailable".into()));

        let holder = holder(&fakes, &layout_store());

        assert_eq!(
            holder.state().problem.as_deref(),
            Some("App list unavailable")
        );
    }

    #[test]
    fn back_closes_the_folder_then_edit_mode() {
        // HOME-08
        let fakes = Fakes::default();
        let layout = layout_store();
        layout
            .save(&Layout {
                entries: vec![LayoutEntry::Folder {
                    id: FolderId(1),
                    name: "Folder".into(),
                    apps: vec![app_id("maps"), app_id("mail")],
                }],
                ..Layout::default()
            })
            .unwrap();
        install(&fakes, &[("maps", "Maps"), ("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout);
        holder.handle(AppsIntent::EnterEditMode);
        holder.handle(AppsIntent::OpenFolder(FolderId(1)));

        assert!(holder.on_back());
        assert_eq!(holder.state().open_folder, None);
        assert!(holder.state().edit_mode);
        assert!(holder.on_back());
        assert!(!holder.state().edit_mode);
        assert!(!holder.on_back());
    }

    fn shortcut(package: &str, id: &str) -> Shortcut {
        Shortcut {
            app: app_id(package),
            id: id.into(),
            label: id.to_uppercase(),
        }
    }

    fn menu(holder: &AppsHolder) -> Option<Vec<AppMenuItem>> {
        holder.state().menu.as_ref().map(|menu| menu.items.clone())
    }

    #[test]
    fn an_apps_menu_offers_app_info_uninstall_hide_and_edit_applications() {
        // APPS-11, APPS-13, APPS-19: a system app can't be uninstalled, so its menu doesn't
        // offer it. It can still be hidden.
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![
            app("mail", "Mail"),
            App {
                can_uninstall: false,
                ..app("settings", "Settings")
            },
        ];
        let mut holder = holder(&fakes, &layout_store());
        assert_eq!(holder.state().menu, None);

        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));
        assert_eq!(
            holder.state().menu.as_deref(),
            Some(&AppMenu {
                item: CellKey::App(app_id("mail")),
                items: vec![
                    AppMenuItem::AppInfo,
                    AppMenuItem::Uninstall,
                    AppMenuItem::Hide,
                    AppMenuItem::EditApplications,
                ],
            })
        );

        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("settings"))));
        assert_eq!(
            menu(&holder),
            Some(vec![
                AppMenuItem::AppInfo,
                AppMenuItem::Hide,
                AppMenuItem::EditApplications
            ])
        );

        // An app that isn't in the grid has no menu.
        holder.handle(AppsIntent::CloseMenu);
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("gone"))));
        assert_eq!(holder.state().menu, None);
    }

    #[test]
    fn shortcuts_appear_only_while_meegone_is_the_default_home_app() {
        // APPS-11: at most four, after the fixed items.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let ids = ["inbox", "compose", "anna", "ben", "cleo"];
        fakes.apps.shortcuts.lock().unwrap().insert(
            app_id("mail"),
            ids.iter().map(|id| shortcut("mail", id)).collect(),
        );
        let mut holder = holder(&fakes, &layout_store());

        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));
        assert_eq!(menu(&holder).unwrap().len(), 4);

        *fakes.apps.shortcuts_allowed.lock().unwrap() = true;
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));
        let items = menu(&holder).unwrap();
        assert_eq!(
            items[..4],
            [
                AppMenuItem::AppInfo,
                AppMenuItem::Uninstall,
                AppMenuItem::Hide,
                AppMenuItem::EditApplications
            ]
        );
        assert_eq!(
            items[4..],
            ids[..MAX_SHORTCUTS]
                .iter()
                .map(|id| AppMenuItem::Shortcut(shortcut("mail", id)))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn each_menu_item_does_what_it_says() {
        // APPS-11, APPS-13: App info, the system's uninstall confirmation, edit mode and a
        // shortcut, each through the data layer. Choosing closes the menu.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        *fakes.apps.shortcuts_allowed.lock().unwrap() = true;
        fakes
            .apps
            .shortcuts
            .lock()
            .unwrap()
            .insert(app_id("mail"), vec![shortcut("mail", "compose")]);
        let mut holder = holder(&fakes, &layout_store());
        let choose = |holder: &mut AppsHolder, item: AppMenuItem| {
            holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));
            holder.handle(AppsIntent::ChooseMenuItem(item));
            assert_eq!(holder.state().menu, None);
        };

        choose(&mut holder, AppMenuItem::AppInfo);
        assert_eq!(
            *fakes.apps.app_info_opened.lock().unwrap(),
            [app_id("mail")]
        );

        choose(&mut holder, AppMenuItem::Uninstall);
        assert_eq!(
            *fakes.apps.uninstall_requested.lock().unwrap(),
            [app_id("mail")]
        );
        // MeeGone uninstalls nothing itself: Mail stays until the system reports it gone.
        assert_eq!(labels(holder.state()), ["Mail"]);

        choose(
            &mut holder,
            AppMenuItem::Shortcut(shortcut("mail", "compose")),
        );
        assert_eq!(
            *fakes.apps.shortcuts_launched.lock().unwrap(),
            [shortcut("mail", "compose")]
        );

        assert!(!holder.state().edit_mode);
        choose(&mut holder, AppMenuItem::EditApplications);
        assert!(holder.state().edit_mode);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn an_item_the_menu_doesnt_offer_or_with_no_menu_open_does_nothing() {
        // APPS-13: a system app's menu never uninstalls it.
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![App {
            can_uninstall: false,
            ..app("settings", "Settings")
        }];
        let mut holder = holder(&fakes, &layout_store());

        holder.handle(AppsIntent::ChooseMenuItem(AppMenuItem::AppInfo));
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("settings"))));
        holder.handle(AppsIntent::ChooseMenuItem(AppMenuItem::Uninstall));
        holder.handle(AppsIntent::ChooseMenuItem(AppMenuItem::Shortcut(shortcut(
            "settings", "wifi",
        ))));

        assert!(holder.state().menu.is_some());
        assert!(fakes.apps.app_info_opened.lock().unwrap().is_empty());
        assert!(fakes.apps.uninstall_requested.lock().unwrap().is_empty());
        assert!(fakes.apps.shortcuts_launched.lock().unwrap().is_empty());
    }

    #[test]
    fn a_menu_item_the_system_refuses_becomes_a_short_message() {
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout_store());
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));
        *fakes.apps.error.lock().unwrap() =
            Some(DataError::Failed("App info can't open right now".into()));

        holder.handle(AppsIntent::ChooseMenuItem(AppMenuItem::AppInfo));

        assert_eq!(holder.state().menu, None);
        assert_eq!(
            holder.state().problem.as_deref(),
            Some("App info can't open right now")
        );
    }

    #[test]
    fn back_closes_the_menu_then_the_folder_then_edit_mode() {
        // HOME-08: the menu is on top.
        let fakes = Fakes::default();
        let layout = layout_store();
        layout
            .save(&Layout {
                entries: vec![
                    LayoutEntry::Folder {
                        id: FolderId(1),
                        name: "Folder".into(),
                        apps: vec![app_id("maps"), app_id("mail")],
                    },
                    LayoutEntry::App(app_id("phone")),
                ],
                ..Layout::default()
            })
            .unwrap();
        install(
            &fakes,
            &[("maps", "Maps"), ("mail", "Mail"), ("phone", "Phone")],
        );
        let mut holder = holder(&fakes, &layout);
        holder.handle(AppsIntent::EnterEditMode);
        holder.handle(AppsIntent::OpenFolder(FolderId(1)));
        // An app inside a folder has a menu too.
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("maps"))));
        assert!(holder.state().menu.is_some());

        assert!(holder.on_back());
        assert_eq!(holder.state().menu, None);
        assert_eq!(holder.state().open_folder, Some(FolderId(1)));
        assert!(holder.on_back());
        assert!(holder.on_back());
        assert!(!holder.state().edit_mode);
        assert!(!holder.on_back());
    }

    #[test]
    fn the_menu_of_an_app_that_was_removed_closes() {
        // APPS-08: another app's removal leaves it open.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        let mut holder = holder(&fakes, &layout_store());
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));

        install(&fakes, &[("mail", "Mail")]);
        holder.handle(AppsIntent::Refresh);
        assert!(holder.state().menu.is_some());

        install(&fakes, &[]);
        holder.handle(AppsIntent::Refresh);
        assert_eq!(holder.state().menu, None);
    }

    #[test]
    fn in_edit_mode_the_menu_has_no_edit_applications() {
        // APPS-11, APPS-12: entering edit mode closes the menu.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout_store());
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));

        holder.handle(AppsIntent::EnterEditMode);
        assert_eq!(holder.state().menu, None);

        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));
        assert_eq!(
            menu(&holder),
            Some(vec![
                AppMenuItem::AppInfo,
                AppMenuItem::Uninstall,
                AppMenuItem::Hide
            ])
        );
    }

    fn hide(holder: &mut AppsHolder, app: AppId) {
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app)));
        holder.handle(AppsIntent::ChooseMenuItem(AppMenuItem::Hide));
    }

    fn labels_of(items: &[GridItem]) -> Vec<&str> {
        items.iter().map(GridItem::label).collect()
    }

    fn hidden_labels(holder: &AppsHolder) -> Vec<&str> {
        holder
            .state()
            .hidden_apps
            .iter()
            .map(|app| app.label.as_str())
            .collect()
    }

    /// The hidden apps as saved, read the way a restarted MeeGone would.
    fn saved_hidden(fakes: &Fakes) -> BTreeSet<AppId> {
        PersistentSettingsStore::new(fakes.storage.clone())
            .get()
            .hidden_apps
    }

    #[test]
    fn hiding_leaves_an_app_out_of_the_grid_and_search_and_saves_it() {
        // APPS-19: through its menu, which closes.
        let fakes = Fakes::default();
        install(
            &fakes,
            &[("phone", "Phone"), ("maps", "Maps"), ("mail", "Mail")],
        );
        let mut holder = holder(&fakes, &layout_store());

        hide(&mut holder, app_id("maps"));

        assert_eq!(holder.state().menu, None);
        assert_eq!(holder.state().problem, None);
        assert_eq!(labels(holder.state()), ["Mail", "Phone"]);
        assert_eq!(hidden_labels(&holder), ["Maps"]);
        assert_eq!(saved_hidden(&fakes), [app_id("maps")].into());
        holder.handle(AppsIntent::OpenSearch);
        holder.handle(AppsIntent::Search("ma".into()));
        assert_eq!(
            results(holder.state()),
            ["Mail"],
            "search leaves it out too"
        );
        // A hidden app has no menu of its own.
        holder.handle(AppsIntent::CloseSearch);
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("maps"))));
        assert_eq!(holder.state().menu, None);
    }

    #[test]
    fn an_unhidden_app_comes_back_in_its_old_place_without_a_layout_write() {
        // APPS-19: the layout never changed, so nothing needs saving.
        let fakes = Fakes::default();
        install(
            &fakes,
            &[("phone", "Phone"), ("maps", "Maps"), ("mail", "Mail")],
        );
        let storage = Arc::new(FakeStorage::default());
        let layout = Arc::new(PersistentLayoutStore::new(storage.clone()));
        layout
            .save(&Layout {
                entries: ["phone", "maps", "mail"]
                    .iter()
                    .map(|id| LayoutEntry::App(app_id(id)))
                    .collect(),
                ..Layout::default()
            })
            .unwrap();
        let mut holder = holder(&fakes, &layout);
        let writes = storage.writes.lock().unwrap().len();

        hide(&mut holder, app_id("maps"));
        assert_eq!(labels(holder.state()), ["Phone", "Mail"]);
        // Hiding again, or unhiding an app that isn't hidden, changes nothing.
        holder.handle(AppsIntent::Unhide(app_id("phone")));
        holder.handle(AppsIntent::Unhide(app_id("maps")));

        assert_eq!(labels(holder.state()), ["Phone", "Maps", "Mail"]);
        assert!(hidden_labels(&holder).is_empty());
        assert!(saved_hidden(&fakes).is_empty());
        assert_eq!(storage.writes.lock().unwrap().len(), writes);
    }

    #[test]
    fn a_hidden_app_keeps_its_slot_in_its_folder_through_a_drag() {
        // APPS-19, FOLD-05: arranging the folder's other apps leaves the hidden one's slot alone.
        let fakes = Fakes::default();
        let layout = layout_store();
        layout
            .save(&Layout {
                entries: vec![
                    LayoutEntry::Folder {
                        id: FolderId(1),
                        name: "Games".into(),
                        apps: vec![app_id("chess"), app_id("go"), app_id("poker")],
                    },
                    LayoutEntry::App(app_id("mail")),
                ],
                ..Layout::default()
            })
            .unwrap();
        install(
            &fakes,
            &[
                ("chess", "Chess"),
                ("go", "Go"),
                ("poker", "Poker"),
                ("mail", "Mail"),
            ],
        );
        let mut holder = holder(&fakes, &layout);
        holder.handle(AppsIntent::OpenFolder(FolderId(1)));
        hide(&mut holder, app_id("go"));
        assert_eq!(labels_of(&holder.state().folder_items), ["Chess", "Poker"]);

        holder.handle(AppsIntent::EnterEditMode);
        holder.handle(AppsIntent::MoveInFolder {
            app: app_id("poker"),
            to: 0,
        });
        assert_eq!(labels_of(&holder.state().folder_items), ["Poker", "Chess"]);
        holder.handle(AppsIntent::Unhide(app_id("go")));

        assert_eq!(
            labels_of(&holder.state().folder_items),
            ["Poker", "Go", "Chess"]
        );
        let Ok(SavedLayout::Read(saved)) = layout.load() else {
            panic!("not saved");
        };
        assert_eq!(
            saved.entries[0],
            LayoutEntry::Folder {
                id: FolderId(1),
                name: "Games".into(),
                apps: vec![app_id("poker"), app_id("go"), app_id("chess")],
            }
        );
    }

    #[test]
    fn hiding_the_open_folders_last_shown_app_closes_it() {
        // APPS-19, FOLD-04: a folder that shows no app has no cell, as if a refresh removed it.
        let fakes = Fakes::default();
        let layout = layout_store();
        layout
            .save(&Layout {
                entries: vec![
                    LayoutEntry::Folder {
                        id: FolderId(1),
                        name: "Games".into(),
                        apps: vec![app_id("chess"), app_id("go")],
                    },
                    LayoutEntry::App(app_id("mail")),
                ],
                ..Layout::default()
            })
            .unwrap();
        install(
            &fakes,
            &[("chess", "Chess"), ("go", "Go"), ("mail", "Mail")],
        );
        let mut holder = holder(&fakes, &layout);
        holder.handle(AppsIntent::OpenFolder(FolderId(1)));
        holder.handle(AppsIntent::StartRename(FolderId(1)));

        hide(&mut holder, app_id("chess"));
        assert_eq!(holder.state().open_folder, Some(FolderId(1)));
        assert_eq!(labels(holder.state()), ["[Games]", "Mail"]);
        hide(&mut holder, app_id("go"));

        assert_eq!(holder.state().open_folder, None);
        assert_eq!(holder.state().renaming, None);
        assert_eq!(labels(holder.state()), ["Mail"]);
        holder.handle(AppsIntent::Unhide(app_id("go")));
        assert_eq!(labels(holder.state()), ["[Games]", "Mail"]);
    }

    #[test]
    fn a_folders_menu_never_offers_hide() {
        // APPS-19: Hide is an app's.
        let fakes = Fakes::default();
        let layout = layout_store();
        layout
            .save(&Layout {
                entries: vec![LayoutEntry::Folder {
                    id: FolderId(1),
                    name: "Games".into(),
                    apps: vec![app_id("chess"), app_id("go")],
                }],
                ..Layout::default()
            })
            .unwrap();
        install(&fakes, &[("chess", "Chess"), ("go", "Go")]);
        let mut holder = holder(&fakes, &layout);

        holder.handle(AppsIntent::OpenMenu(CellKey::Folder(FolderId(1))));
        holder.handle(AppsIntent::ChooseMenuItem(AppMenuItem::Hide));

        assert!(holder.state().menu.is_some());
        assert!(saved_hidden(&fakes).is_empty());
        assert_eq!(labels(holder.state()), ["[Games]"]);
    }

    #[test]
    fn a_hide_or_unhide_that_cant_be_saved_changes_nothing_and_is_the_problem() {
        // APPS-19, REL-03
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        let mut holder = holder(&fakes, &layout_store());
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed("disk full".into()));

        hide(&mut holder, app_id("maps"));
        assert_eq!(holder.state().menu, None);
        assert_eq!(holder.state().problem.as_deref(), Some("disk full"));
        assert_eq!(labels(holder.state()), ["Mail", "Maps"]);
        assert!(hidden_labels(&holder).is_empty());

        *fakes.storage.error.lock().unwrap() = None;
        hide(&mut holder, app_id("maps"));
        assert_eq!(holder.state().problem, None);
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed("disk full".into()));
        holder.handle(AppsIntent::Unhide(app_id("maps")));
        assert_eq!(holder.state().problem.as_deref(), Some("disk full"));
        assert_eq!(labels(holder.state()), ["Mail"]);
        assert_eq!(hidden_labels(&holder), ["Maps"]);
    }

    #[test]
    fn hidden_apps_are_listed_alphabetically_and_kept_while_nothing_changes() {
        // APPS-19, L10N-04: the collation keys decide. A refresh that changes nothing keeps the
        // list, so the UI rereads nothing.
        let fakes = Fakes::default();
        let mut apps = vec![
            app("zoom", "Zoom"),
            app("eclair", "Éclair"),
            app("docs", "docs"),
            app("mail", "Mail"),
        ];
        apps[1].sort_key = b"eclair".to_vec();
        *fakes.apps.apps.lock().unwrap() = apps;
        let mut holder = holder(&fakes, &layout_store());
        for package in ["zoom", "eclair", "docs"] {
            hide(&mut holder, app_id(package));
        }
        assert_eq!(hidden_labels(&holder), ["docs", "Éclair", "Zoom"]);
        let listed = holder.state().hidden_apps.clone();

        holder.handle(AppsIntent::Refresh);

        assert!(Arc::ptr_eq(&listed, &holder.state().hidden_apps));
        assert_eq!(labels(holder.state()), ["Mail"]);
    }

    #[test]
    fn hidden_apps_stay_hidden_after_a_restart() {
        // APPS-19: they are in the settings store.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        let layout = layout_store();
        hide(&mut holder(&fakes, &layout), app_id("maps"));

        let restarted = holder(&fakes, &layout);

        assert_eq!(labels(restarted.state()), ["Mail"]);
        assert_eq!(hidden_labels(&restarted), ["Maps"]);
    }

    #[test]
    fn an_uninstalled_hidden_app_is_forgotten_so_installing_it_again_shows_it() {
        // APPS-19, APPS-06: it joins the end, with its dot.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        let mut holder = holder(&fakes, &layout_store());
        hide(&mut holder, app_id("maps"));

        install(&fakes, &[("mail", "Mail")]);
        holder.handle(AppsIntent::Refresh);
        assert!(saved_hidden(&fakes).is_empty());
        assert!(hidden_labels(&holder).is_empty());

        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        holder.handle(AppsIntent::Refresh);
        assert_eq!(labels(holder.state()), ["Mail", "Maps•"]);
    }

    #[test]
    fn a_locked_profiles_hidden_app_stays_hidden_once_it_unlocks() {
        // APPS-19, APPS-18: while the profile is locked, the app isn't listed but isn't
        // forgotten either.
        let fakes = Fakes::default();
        let docs = work_app("docs", "Docs");
        fakes.apps.profiles.lock().unwrap().push(Profile {
            id: docs.id.profile,
            work: true,
            paused: false,
            locked: false,
        });
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), docs.clone()];
        let mut holder = holder(&fakes, &layout_store());
        hide(&mut holder, docs.id.clone());
        assert_eq!(saved_hidden(&fakes), [docs.id.clone()].into());

        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
        fakes.apps.profiles.lock().unwrap()[1].locked = true;
        holder.handle(AppsIntent::Refresh);
        assert!(hidden_labels(&holder).is_empty());
        assert_eq!(saved_hidden(&fakes), [docs.id.clone()].into());

        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), docs];
        fakes.apps.profiles.lock().unwrap()[1].locked = false;
        holder.handle(AppsIntent::Refresh);
        assert_eq!(labels(holder.state()), ["Mail"]);
        assert_eq!(hidden_labels(&holder), ["Docs"]);
    }

    #[test]
    fn the_grid_keeps_the_scroll_position_the_host_reports() {
        // HOME-07, REL-05
        let fakes = Fakes::default();
        let mut holder = holder(&fakes, &layout_store());

        holder.handle(AppsIntent::Scrolled(640.0));
        assert_eq!(holder.state().scroll_px, 640.0);
        holder.handle(AppsIntent::Scrolled(f64::NAN));
        assert_eq!(holder.state().scroll_px, 640.0);
        holder.handle(AppsIntent::Scrolled(-12.0));
        assert_eq!(holder.state().scroll_px, 0.0);

        holder.handle(AppsIntent::Scrolled(300.0));
        holder.handle(AppsIntent::ScrollToTop);
        assert_eq!(holder.state().scroll_px, 0.0);
    }

    #[test]
    fn the_grid_always_ends_with_an_empty_row() {
        // HOME-14: below the last row, a whole row is left empty for the home menu.
        let fakes = Fakes::default();
        let rows = |count: usize| {
            let apps: Vec<(String, String)> = (0..count)
                .map(|i| (format!("app{i:02}"), format!("App {i:02}")))
                .collect();
            let apps: Vec<(&str, &str)> = apps
                .iter()
                .map(|(package, label)| (package.as_str(), label.as_str()))
                .collect();
            install(&fakes, &apps);
            holder(&fakes, &layout_store()).state().rows()
        };

        assert_eq!(rows(0), 1);
        assert_eq!(rows(4), 2);
        assert_eq!(rows(5), 3);
        assert_eq!(rows(40), 11);
        let no_columns = AppsState {
            columns: 0,
            ..AppsState::default()
        };
        assert_eq!(no_columns.rows(), 1);
    }

    #[test]
    fn closing_a_folder_keeps_the_scroll_position() {
        // HOME-07: the folder closes and Applications stays where it was.
        let fakes = Fakes::default();
        let layout = layout_store();
        layout
            .save(&Layout {
                entries: vec![LayoutEntry::Folder {
                    id: FolderId(1),
                    name: "Folder".into(),
                    apps: vec![app_id("maps"), app_id("mail")],
                }],
                ..Layout::default()
            })
            .unwrap();
        install(&fakes, &[("maps", "Maps"), ("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout);
        holder.handle(AppsIntent::Scrolled(480.0));
        holder.handle(AppsIntent::OpenFolder(FolderId(1)));

        assert!(holder.on_back());
        assert_eq!(holder.state().scroll_px, 480.0);
    }

    fn storage_and_store() -> (Arc<FakeStorage>, Arc<PersistentLayoutStore>) {
        let storage = Arc::new(FakeStorage::default());
        (
            storage.clone(),
            Arc::new(PersistentLayoutStore::new(storage)),
        )
    }

    fn saved(layout: &PersistentLayoutStore) -> Layout {
        match layout.load() {
            Ok(SavedLayout::Read(saved)) => saved,
            other => panic!("not saved: {other:?}"),
        }
    }

    #[test]
    fn the_first_start_puts_the_default_apps_first_and_marks_nothing() {
        // APPS-04, APPS-05
        let fakes = Fakes::default();
        install(
            &fakes,
            &[
                ("atlas", "Atlas"),
                ("camera", "Camera"),
                ("dialer", "Phone"),
                ("sms", "Messages"),
            ],
        );
        *fakes.apps.default_apps.lock().unwrap() = vec!["dialer".into(), "camera".into()];
        let layout = layout_store();

        let holder = holder(&fakes, &layout);

        assert_eq!(
            labels(holder.state()),
            ["Phone", "Camera", "Atlas", "Messages"]
        );
        assert!(saved(&layout).new.is_empty());
    }

    #[test]
    fn the_defaults_apply_only_to_the_first_start() {
        // APPS-05: a saved grid keeps its order when the default apps change.
        let fakes = Fakes::default();
        install(&fakes, &[("atlas", "Atlas"), ("dialer", "Phone")]);
        let layout = layout_store();
        holder(&fakes, &layout);

        *fakes.apps.default_apps.lock().unwrap() = vec!["dialer".into()];
        let holder = holder(&fakes, &layout);

        assert_eq!(labels(holder.state()), ["Atlas", "Phone"]);
    }

    #[test]
    fn a_damaged_layout_is_rebuilt_around_what_it_kept_and_replaced_at_once() {
        // REL-02: Zoom's cell survived; the rest is alphabetical, with no dots.
        let fakes = Fakes::default();
        install(
            &fakes,
            &[("mail", "Mail"), ("zoom", "Zoom"), ("atlas", "Atlas")],
        );
        let (storage, layout) = storage_and_store();
        let mut damaged = seal(
            "layout",
            meegone_apps_data::LAYOUT_VERSION,
            "app 0 zoom zoom.Main\n",
        );
        damaged.truncate(damaged.len() - 3);
        storage
            .files
            .lock()
            .unwrap()
            .insert(StoredFile::Layout, damaged);

        let holder = holder(&fakes, &layout);

        assert_eq!(labels(holder.state()), ["Zoom", "Atlas", "Mail"]);
        assert_eq!(holder.state().problem, None);
        assert_eq!(saved(&layout).entries.len(), 3);
    }

    #[test]
    fn a_layout_that_cant_be_read_is_shown_rebuilt_but_never_overwritten() {
        // REL-02: a passing read error must not cost the user their arrangement.
        let fakes = Fakes::default();
        install(&fakes, &[("atlas", "Atlas"), ("mail", "Mail")]);
        let (storage, layout) = storage_and_store();
        layout
            .save(&Layout {
                entries: vec![
                    LayoutEntry::App(app_id("mail")),
                    LayoutEntry::App(app_id("atlas")),
                ],
                // Saved by this copy, so reading it changes nothing (APPS-21).
                installed_ms: Some(0),
                ..Layout::default()
            })
            .unwrap();
        storage.writes.lock().unwrap().clear();
        *storage.error.lock().unwrap() = Some(DataError::Failed("Disk busy".into()));

        let mut holder = holder(&fakes, &layout);
        assert_eq!(labels(holder.state()), ["Atlas", "Mail"]);
        assert_eq!(holder.state().problem.as_deref(), Some("Disk busy"));

        *storage.error.lock().unwrap() = None;
        holder.handle(AppsIntent::Refresh);
        assert_eq!(labels(holder.state()), ["Mail", "Atlas"]);
        assert_eq!(holder.state().problem, None);
        assert!(storage.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn the_layout_is_read_once_and_saved_only_when_it_changes() {
        // PERF-04: every accent the host derives refreshes the grid.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let (storage, layout) = storage_and_store();
        let mut holder = holder(&fakes, &layout);
        assert_eq!(*storage.writes.lock().unwrap(), [StoredFile::Layout]);

        for _ in 0..5 {
            holder.handle(AppsIntent::Refresh);
        }
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        holder.handle(AppsIntent::Refresh);

        assert_eq!(*storage.reads.lock().unwrap(), [StoredFile::Layout]);
        assert_eq!(storage.writes.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_failed_save_is_tried_again_at_the_next_refresh() {
        // REL-03: the failure shows as the problem until a save succeeds.
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let (storage, layout) = storage_and_store();
        let mut holder = holder(&fakes, &layout);
        *storage.error.lock().unwrap() = Some(DataError::Failed("Disk full".into()));
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);

        holder.handle(AppsIntent::Refresh);
        assert_eq!(holder.state().problem.as_deref(), Some("Disk full"));
        assert_eq!(labels(holder.state()), ["Mail", "Maps•"]);

        *storage.error.lock().unwrap() = None;
        holder.handle(AppsIntent::Refresh);
        assert_eq!(holder.state().problem, None);
        assert_eq!(saved(&layout).entries.len(), 2);
    }

    #[test]
    fn opening_a_new_app_takes_its_dot_away_for_good() {
        // APPS-07
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let layout = layout_store();
        let mut holder = holder(&fakes, &layout);
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        holder.handle(AppsIntent::Refresh);
        assert_eq!(labels(holder.state()), ["Mail", "Maps•"]);

        // A launch the system refuses didn't open it.
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed("No".into()));
        holder.handle(AppsIntent::Launch {
            app: app_id("maps"),
            from: None,
        });
        assert_eq!(labels(holder.state()), ["Mail", "Maps•"]);

        *fakes.apps.error.lock().unwrap() = None;
        holder.handle(AppsIntent::Launch {
            app: app_id("maps"),
            from: None,
        });
        assert_eq!(labels(holder.state()), ["Mail", "Maps"]);
        assert!(saved(&layout).new.is_empty());
        assert_eq!(
            labels(self::holder(&fakes, &layout).state()),
            ["Mail", "Maps"]
        );
    }

    #[test]
    fn an_app_installed_while_meegone_was_closed_joins_the_end_with_a_dot() {
        // APPS-06, APPS-07, APPS-18
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail"), ("phone", "Phone")]);
        let layout = layout_store();
        holder(&fakes, &layout);

        install(
            &fakes,
            &[("atlas", "Atlas"), ("mail", "Mail"), ("phone", "Phone")],
        );
        let holder = holder(&fakes, &layout);

        assert_eq!(labels(holder.state()), ["Mail", "Phone", "Atlas•"]);
    }

    #[test]
    fn every_damaged_or_shortened_layout_still_shows_every_app() {
        // REL-02: whatever happened to the file, MeeGone starts, shows each app once and
        // replaces the file, so it can't fail the same way twice.
        let fakes = Fakes::default();
        *fakes.apps.profiles.lock().unwrap() = vec![meegone_core_data::Profile {
            id: ProfileId(0),
            work: false,
            paused: false,
            locked: false,
        }];
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps"), ("go", "Go")]);
        let (storage, layout) = storage_and_store();
        layout
            .save(&Layout {
                entries: vec![
                    LayoutEntry::Folder {
                        id: FolderId(1),
                        name: "Fun & games".into(),
                        apps: vec![app_id("go"), app_id("chess")],
                    },
                    LayoutEntry::App(app_id("maps")),
                    LayoutEntry::App(app_id("atlas")),
                    LayoutEntry::App(app_id("mail")),
                ],
                new: [app_id("mail")].into(),
                // Saved by this copy, still waiting for Atlas after a restore (APPS-21).
                installed_ms: Some(0),
                restore: Some(Restore {
                    until_ms: 1_000,
                    waiting: [app_id("atlas")].into(),
                }),
            })
            .unwrap();
        let bytes = storage.files.lock().unwrap()[&StoredFile::Layout].clone();

        let mut damaged_files: Vec<Vec<u8>> =
            (0..bytes.len()).map(|len| bytes[..len].to_vec()).collect();
        for i in 0..bytes.len() {
            for bit in 0..8 {
                let mut damaged = bytes.clone();
                damaged[i] ^= 1 << bit;
                damaged_files.push(damaged);
            }
        }
        for damaged in damaged_files {
            storage
                .files
                .lock()
                .unwrap()
                .insert(StoredFile::Layout, damaged.clone());

            let holder = holder(&fakes, &layout);

            let mut shown = labels(holder.state())
                .into_iter()
                .map(|label| label.trim_end_matches('•').to_owned())
                .collect::<Vec<_>>();
            shown.sort();
            assert_eq!(shown, ["Go", "Mail", "Maps"], "{damaged:?}");
            assert!(
                matches!(layout.load(), Ok(SavedLayout::Read(_))),
                "{damaged:?}"
            );
        }
    }

    /// A holder in edit mode, with Camera, Mail, Maps and Phone in that order.
    fn editing(fakes: &Fakes) -> (Arc<FakeStorage>, Arc<PersistentLayoutStore>, AppsHolder) {
        install(
            fakes,
            &[
                ("camera", "Camera"),
                ("mail", "Mail"),
                ("maps", "Maps"),
                ("phone", "Phone"),
            ],
        );
        let (storage, layout) = storage_and_store();
        let mut holder = holder(fakes, &layout);
        holder.handle(AppsIntent::EnterEditMode);
        storage.writes.lock().unwrap().clear();
        (storage, layout, holder)
    }

    fn saved_order(layout: &PersistentLayoutStore) -> Vec<String> {
        saved(layout)
            .entries
            .iter()
            .map(|entry| match entry {
                LayoutEntry::App(id) => id.package.clone(),
                LayoutEntry::Folder { name, .. } => format!("[{name}]"),
            })
            .collect()
    }

    #[test]
    fn a_dragged_app_moves_the_others_aside_and_is_saved_once_when_dropped() {
        // APPS-12, APPS-18, AC-07
        let fakes = Fakes::default();
        let (storage, layout, mut holder) = editing(&fakes);

        holder.handle(AppsIntent::Arrange {
            item: CellKey::App(app_id("maps")),
            to: 1,
        });
        assert_eq!(labels(holder.state()), ["Camera", "Maps", "Mail", "Phone"]);
        holder.handle(AppsIntent::Arrange {
            item: CellKey::App(app_id("maps")),
            to: 0,
        });
        assert_eq!(labels(holder.state()), ["Maps", "Camera", "Mail", "Phone"]);
        // Nothing is saved while the icon is still moving.
        assert!(storage.writes.lock().unwrap().is_empty());

        holder.handle(AppsIntent::Drop);

        assert_eq!(labels(holder.state()), ["Maps", "Camera", "Mail", "Phone"]);
        assert_eq!(*storage.writes.lock().unwrap(), [StoredFile::Layout]);
        assert_eq!(saved_order(&layout), ["maps", "camera", "mail", "phone"]);
        // The drop is final: a refresh, or another drop, changes nothing.
        holder.handle(AppsIntent::Refresh);
        holder.handle(AppsIntent::Drop);
        assert_eq!(labels(holder.state()), ["Maps", "Camera", "Mail", "Phone"]);
        assert_eq!(storage.writes.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_drag_that_isnt_dropped_puts_the_grid_back() {
        // APPS-12: Back, Done, or anything else that ends a drag but a drop.
        for end in [
            AppsIntent::CancelArrange,
            AppsIntent::ExitEditMode,
            AppsIntent::Refresh,
        ] {
            let fakes = Fakes::default();
            let (storage, _, mut holder) = editing(&fakes);
            holder.handle(AppsIntent::Arrange {
                item: CellKey::App(app_id("phone")),
                to: 0,
            });

            let refresh = matches!(end, AppsIntent::Refresh);
            holder.handle(end);

            if refresh {
                // A refresh keeps it: the finger is still there.
                assert_eq!(labels(holder.state()), ["Phone", "Camera", "Mail", "Maps"]);
            } else {
                assert_eq!(labels(holder.state()), ["Camera", "Mail", "Maps", "Phone"]);
            }
            assert!(storage.writes.lock().unwrap().is_empty());
        }

        let fakes = Fakes::default();
        let (_, _, mut holder) = editing(&fakes);
        holder.handle(AppsIntent::Arrange {
            item: CellKey::App(app_id("phone")),
            to: 0,
        });
        assert!(holder.on_back());
        assert!(!holder.state().edit_mode);
        assert_eq!(labels(holder.state()), ["Camera", "Mail", "Maps", "Phone"]);
    }

    #[test]
    fn a_drags_draft_keeps_its_order_as_apps_come_and_go() {
        // APPS-12, APPS-08, ADR 0015: an app before it is removed while it is dragged; it stays
        // among its neighbours, and the shell moves it under the finger again. If the dragged
        // app itself goes, the rest of the draft stays.
        let fakes = Fakes::default();
        let (_, layout, mut holder) = editing(&fakes);
        holder.handle(AppsIntent::Arrange {
            item: CellKey::App(app_id("phone")),
            to: 1,
        });

        install(
            &fakes,
            &[("mail", "Mail"), ("maps", "Maps"), ("phone", "Phone")],
        );
        holder.handle(AppsIntent::Refresh);
        assert_eq!(labels(holder.state()), ["Phone", "Mail", "Maps"]);

        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        holder.handle(AppsIntent::Refresh);
        holder.handle(AppsIntent::Drop);
        assert_eq!(labels(holder.state()), ["Mail", "Maps"]);
        assert_eq!(saved_order(&layout), ["mail", "maps"]);
    }

    #[test]
    fn moving_is_an_arrangement_and_a_drop_at_once() {
        // A11Y-05: a screen reader's "Move to…" actions.
        let fakes = Fakes::default();
        let (_, layout, mut holder) = editing(&fakes);

        holder.handle(AppsIntent::Move {
            item: CellKey::App(app_id("camera")),
            to: 99,
        });

        assert_eq!(labels(holder.state()), ["Mail", "Maps", "Phone", "Camera"]);
        assert_eq!(saved_order(&layout), ["mail", "maps", "phone", "camera"]);
    }

    #[test]
    fn nothing_moves_outside_edit_mode_or_when_the_layout_couldnt_be_read() {
        // APPS-12, REL-02: an arrangement nothing can save isn't offered.
        let fakes = Fakes::default();
        let (storage, layout, mut done) = editing(&fakes);
        done.handle(AppsIntent::ExitEditMode);
        done.handle(AppsIntent::Move {
            item: CellKey::App(app_id("phone")),
            to: 0,
        });
        assert_eq!(labels(done.state()), ["Camera", "Mail", "Maps", "Phone"]);

        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail"), ("maps", "Maps")]);
        layout
            .save(&Layout {
                entries: vec![LayoutEntry::App(app_id("maps"))],
                ..Layout::default()
            })
            .unwrap();
        *storage.error.lock().unwrap() = Some(DataError::Failed("Disk busy".into()));
        storage.writes.lock().unwrap().clear();
        let mut holder = holder(&fakes, &layout);
        holder.handle(AppsIntent::EnterEditMode);
        holder.handle(AppsIntent::Move {
            item: CellKey::App(app_id("maps")),
            to: 0,
        });
        assert_eq!(labels(holder.state()), ["Mail", "Maps"]);
        assert!(storage.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn in_edit_mode_a_tap_launches_nothing() {
        // APPS-12
        let fakes = Fakes::default();
        let (_, _, mut holder) = editing(&fakes);

        holder.handle(AppsIntent::Launch {
            app: app_id("mail"),
            from: None,
        });
        assert!(fakes.apps.launched.lock().unwrap().is_empty());

        holder.handle(AppsIntent::ExitEditMode);
        holder.handle(AppsIntent::Launch {
            app: app_id("mail"),
            from: None,
        });
        assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
    }

    #[test]
    fn the_cross_asks_the_system_to_uninstall_only_apps_the_user_can_uninstall() {
        // APPS-13, AC-09: MeeGone never uninstalls anything itself.
        let fakes = Fakes::default();
        let mut camera = app("camera", "Camera");
        camera.can_uninstall = false;
        *fakes.apps.apps.lock().unwrap() = vec![camera, app("mail", "Mail")];
        let mut holder = holder(&fakes, &layout_store());

        // Not in edit mode: there is no ✕ to tap.
        holder.handle(AppsIntent::RequestUninstall(app_id("mail")));
        assert!(fakes.apps.uninstall_requested.lock().unwrap().is_empty());

        holder.handle(AppsIntent::EnterEditMode);
        holder.handle(AppsIntent::RequestUninstall(app_id("camera")));
        holder.handle(AppsIntent::RequestUninstall(app_id("gone")));
        holder.handle(AppsIntent::RequestUninstall(app_id("mail")));

        assert_eq!(
            *fakes.apps.uninstall_requested.lock().unwrap(),
            [app_id("mail")]
        );
        assert_eq!(labels(holder.state()), ["Camera", "Mail"]);
        assert!(holder.state().edit_mode);

        // The system refusing is a problem shown briefly.
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed("No".into()));
        holder.handle(AppsIntent::RequestUninstall(app_id("mail")));
        assert_eq!(holder.state().problem.as_deref(), Some("No"));
    }

    #[test]
    fn done_leaves_edit_mode_and_closes_the_menu() {
        // APPS-12
        let fakes = Fakes::default();
        let (_, _, mut holder) = editing(&fakes);
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));

        holder.handle(AppsIntent::ExitEditMode);

        assert!(!holder.state().edit_mode);
        assert_eq!(holder.state().menu, None);
        assert!(!holder.on_back());
    }

    /// A phone's grid: 4 columns of 1001 px in all, at 2.75 px per dp, rows 300 px apart,
    /// 100 px of safe area above and 50 below, 2000 px tall.
    fn phone_grid() -> GridLayout {
        GridLayout {
            width_px: 1001.0,
            row_height_px: 300.0,
            density: 2.75,
            top_px: 100.0,
            bottom_px: 50.0,
            height_px: 2000.0,
            left_px: 0.0,
            view_top_px: 0.0,
            icon_px: 0.0,
            icon_top_px: 0.0,
        }
    }

    #[test]
    fn cell_places_are_worked_out_as_the_host_lays_them_out() {
        // ADR 0014: AppsGrid spreads the leftover pixels, `width * column / columns`.
        let grid = phone_grid();
        assert_eq!(grid.columns(), 4);
        let lefts: Vec<f64> = (0..5).map(|place| grid.cell_left(place, false)).collect();
        assert_eq!(lefts, [0.0, 250.0, 500.0, 750.0, 0.0]);
        assert_eq!(grid.cell_width(3, false), 251.0);
        assert_eq!(grid.cell_top(5), 300.0);
        // Right to left mirrors the columns (L10N-02).
        assert_eq!(grid.cell_left(0, true), 750.0);
        assert_eq!(grid.cell_width(0, true), 251.0);
    }

    #[test]
    fn the_place_under_a_point_is_the_nearest_cell() {
        // APPS-12
        let grid = phone_grid();
        assert_eq!(grid.place_at(10.0, 10.0, 10, false), Some(0));
        assert_eq!(grid.place_at(260.0, 310.0, 10, false), Some(5));
        assert_eq!(grid.place_at(260.0, 310.0, 10, true), Some(6));
        // Past the ends of a row, above the first row and below the last cell.
        assert_eq!(grid.place_at(-50.0, -50.0, 10, false), Some(0));
        assert_eq!(grid.place_at(5_000.0, 310.0, 10, false), Some(7));
        assert_eq!(grid.place_at(10.0, 5_000.0, 10, false), Some(9));
        assert_eq!(grid.place_at(10.0, 10.0, 0, false), None);
    }

    #[test]
    fn the_grid_scrolls_as_far_as_its_last_row() {
        // 10 rows of 300 px and the safe area, in 2000 px.
        assert_eq!(phone_grid().max_scroll(10), 1150.0);
        assert_eq!(phone_grid().max_scroll(2), 0.0);
    }

    #[test]
    fn a_layout_with_negative_room_changes_nothing() {
        let fakes = Fakes::default();
        let mut holder = holder(&fakes, &layout_store());
        holder.handle(AppsIntent::Layout(GridLayout {
            top_px: -1.0,
            ..phone_grid()
        }));
        assert_eq!(holder.state().layout, None);
        holder.handle(AppsIntent::Layout(phone_grid()));
        assert_eq!(holder.state().layout, Some(phone_grid()));
    }

    /// In edit mode, with Camera, Mail, Maps and Phone, and the store they are saved in.
    fn folder_of_two(fakes: &Fakes) -> (Arc<FakeStorage>, Arc<PersistentLayoutStore>, AppsHolder) {
        let (storage, layout, mut holder) = editing(fakes);
        holder.handle(AppsIntent::Group {
            app: app_id("maps"),
            with: CellKey::App(app_id("mail")),
        });
        storage.writes.lock().unwrap().clear();
        (storage, layout, holder)
    }

    fn folder_id(state: &AppsState) -> FolderId {
        state
            .items
            .iter()
            .find_map(|item| match item {
                GridItem::Folder { id, .. } => Some(*id),
                GridItem::App { .. } => None,
            })
            .expect("a folder")
    }

    #[test]
    fn grouping_two_apps_makes_a_folder_named_folder_and_saves_it() {
        // FOLD-01, FOLD-03, A11Y-05: an unnamed folder shows as "Folder" in the user's language.
        let fakes = Fakes::default();
        let (_, layout, holder) = folder_of_two(&fakes);

        assert_eq!(labels(holder.state()), ["Camera", "[]", "Phone"]);
        assert_eq!(saved_order(&layout), ["camera", "[]", "phone"]);
        assert_eq!(holder.state().folders_made, 1);
        let GridItem::Folder { apps, .. } = &holder.state().items[1] else {
            panic!("not a folder");
        };
        let apps: Vec<&str> = apps.iter().map(|app| app.label.as_str()).collect();
        assert_eq!(apps, ["Mail", "Maps"]);
    }

    #[test]
    fn a_folder_opens_shows_its_apps_and_closes_on_back_before_edit_mode() {
        // FOLD-04, HOME-08
        let fakes = Fakes::default();
        let (_, _, mut holder) = folder_of_two(&fakes);
        let folder = folder_id(holder.state());

        holder.handle(AppsIntent::OpenFolder(folder));
        let state = holder.state();
        assert_eq!(state.open_folder, Some(folder));
        assert_eq!(state.folder_rows(), 1);
        let shown: Vec<&str> = state.folder_items.iter().map(GridItem::label).collect();
        assert_eq!(shown, ["Mail", "Maps"]);
        assert_eq!(
            state.place_of(Surface::Folder, &CellKey::App(app_id("maps"))),
            Some(1)
        );

        assert!(holder.on_back());
        assert_eq!(holder.state().open_folder, None);
        assert!(holder.state().folder_items.is_empty());
        assert!(holder.state().edit_mode);
        assert!(holder.on_back());
        assert!(!holder.state().edit_mode);

        // A folder that isn't there doesn't open.
        holder.handle(AppsIntent::OpenFolder(FolderId(99)));
        assert_eq!(holder.state().open_folder, None);
    }

    #[test]
    fn dragging_an_app_out_of_a_folder_of_two_turns_it_back_into_the_other() {
        // FOLD-05, FOLD-06, AC-08: mid-drag the folder keeps its one app; the drop tidies it.
        let fakes = Fakes::default();
        let (storage, layout, mut holder) = folder_of_two(&fakes);
        holder.handle(AppsIntent::OpenFolder(folder_id(holder.state())));

        holder.handle(AppsIntent::TakeOut {
            app: app_id("maps"),
            to: 0,
        });
        assert_eq!(labels(holder.state()), ["Maps", "Camera", "[]", "Phone"]);
        assert_eq!(holder.state().open_folder, None);
        assert!(storage.writes.lock().unwrap().is_empty());

        holder.handle(AppsIntent::Drop);
        assert_eq!(labels(holder.state()), ["Maps", "Camera", "Mail", "Phone"]);
        assert_eq!(saved_order(&layout), ["maps", "camera", "mail", "phone"]);
    }

    #[test]
    fn a_drag_into_a_folder_is_kept_only_when_dropped() {
        // FOLD-05
        let fakes = Fakes::default();
        let (storage, layout, mut holder) = folder_of_two(&fakes);
        let folder = folder_id(holder.state());

        holder.handle(AppsIntent::Combine {
            app: app_id("camera"),
            onto: CellKey::Folder(folder),
        });
        assert_eq!(labels(holder.state()), ["[]", "Phone"]);
        holder.handle(AppsIntent::CancelArrange);
        assert_eq!(labels(holder.state()), ["Camera", "[]", "Phone"]);
        assert!(storage.writes.lock().unwrap().is_empty());

        holder.handle(AppsIntent::Combine {
            app: app_id("camera"),
            onto: CellKey::Folder(folder),
        });
        holder.handle(AppsIntent::Drop);
        assert_eq!(saved_order(&layout), ["[]", "phone"]);
        // It joined a folder: none was made.
        assert_eq!(holder.state().folders_made, 1);
    }

    #[test]
    fn a_drags_draft_keeps_a_folder_of_one_through_a_refresh() {
        // ADR 0015: a refresh mid-drag mustn't collapse the folder the finger is taking an app
        // out of.
        let fakes = Fakes::default();
        let (_, _, mut holder) = folder_of_two(&fakes);
        holder.handle(AppsIntent::OpenFolder(folder_id(holder.state())));
        holder.handle(AppsIntent::TakeOut {
            app: app_id("maps"),
            to: 3,
        });

        holder.handle(AppsIntent::Refresh);

        assert_eq!(labels(holder.state()), ["Camera", "[]", "Phone", "Maps"]);
    }

    #[test]
    fn apps_move_within_an_open_folder_and_out_of_it() {
        // FOLD-05, A11Y-05
        let fakes = Fakes::default();
        let (_, layout, mut holder) = folder_of_two(&fakes);
        let folder = folder_id(holder.state());
        holder.handle(AppsIntent::OpenFolder(folder));

        holder.handle(AppsIntent::MoveInFolder {
            app: app_id("maps"),
            to: 0,
        });
        let shown: Vec<&str> = holder
            .state()
            .folder_items
            .iter()
            .map(GridItem::label)
            .collect();
        assert_eq!(shown, ["Maps", "Mail"]);

        holder.handle(AppsIntent::MoveOutOfFolder(app_id("maps")));
        assert_eq!(labels(holder.state()), ["Camera", "Mail", "Maps", "Phone"]);
        assert_eq!(saved_order(&layout), ["camera", "mail", "maps", "phone"]);
    }

    #[test]
    fn folders_change_only_in_edit_mode_and_with_a_layout_that_was_read() {
        // APPS-12, REL-02
        let fakes = Fakes::default();
        let (_, _, mut holder) = editing(&fakes);
        holder.handle(AppsIntent::ExitEditMode);
        holder.handle(AppsIntent::Group {
            app: app_id("maps"),
            with: CellKey::App(app_id("mail")),
        });
        assert_eq!(labels(holder.state()), ["Camera", "Mail", "Maps", "Phone"]);
        assert_eq!(holder.state().folders_made, 0);
    }

    #[test]
    fn renaming_trims_caps_and_saves_the_name() {
        // FOLD-03
        let fakes = Fakes::default();
        let (_, layout, mut holder) = folder_of_two(&fakes);
        let folder = folder_id(holder.state());

        holder.handle(AppsIntent::StartRename(folder));
        assert_eq!(holder.state().renaming, Some(folder));
        holder.handle(AppsIntent::RenameFolder {
            folder,
            name: "  Mail & maps  ".into(),
        });
        assert_eq!(holder.state().renaming, None);
        assert_eq!(labels(holder.state())[1], "[Mail & maps]");
        assert_eq!(saved_order(&layout)[1], "[Mail & maps]");

        holder.handle(AppsIntent::RenameFolder {
            folder,
            name: "é".repeat(100),
        });
        assert_eq!(
            holder.state().items[1].label().chars().count(),
            MAX_FOLDER_NAME
        );

        // Blank goes back to "Folder"; a folder that has gone is left alone.
        holder.handle(AppsIntent::RenameFolder {
            folder,
            name: "   ".into(),
        });
        assert_eq!(labels(holder.state())[1], "[]");
        holder.handle(AppsIntent::RenameFolder {
            folder: FolderId(99),
            name: "Nope".into(),
        });
        holder.handle(AppsIntent::StartRename(FolderId(99)));
        assert_eq!(holder.state().renaming, None);
        holder.handle(AppsIntent::StartRename(folder));
        holder.handle(AppsIntent::CancelRename);
        assert_eq!(holder.state().renaming, None);
    }

    #[test]
    fn a_rename_during_a_drag_survives_the_drop() {
        let fakes = Fakes::default();
        let (_, layout, mut holder) = folder_of_two(&fakes);
        let folder = folder_id(holder.state());
        holder.handle(AppsIntent::Arrange {
            item: CellKey::Folder(folder),
            to: 0,
        });

        holder.handle(AppsIntent::RenameFolder {
            folder,
            name: "Post".into(),
        });
        holder.handle(AppsIntent::Drop);

        assert_eq!(saved_order(&layout), ["[Post]", "camera", "phone"]);
    }

    #[test]
    fn a_folders_menu_offers_rename_and_edit_applications() {
        // ADR 0015
        let fakes = Fakes::default();
        let (_, _, mut holder) = folder_of_two(&fakes);
        let folder = folder_id(holder.state());
        holder.handle(AppsIntent::ExitEditMode);

        holder.handle(AppsIntent::OpenMenu(CellKey::Folder(folder)));
        let menu = holder.state().menu.clone().expect("a menu");
        assert_eq!(
            menu.items,
            [AppMenuItem::RenameFolder, AppMenuItem::EditApplications]
        );

        holder.handle(AppsIntent::ChooseMenuItem(AppMenuItem::RenameFolder));
        assert_eq!(holder.state().renaming, Some(folder));
        assert_eq!(holder.state().menu, None);

        // App info means nothing for a folder; a folder that isn't there has no menu.
        holder.handle(AppsIntent::OpenMenu(CellKey::Folder(folder)));
        holder.handle(AppsIntent::ChooseMenuItem(AppMenuItem::AppInfo));
        assert!(holder.state().menu.is_some());
        holder.handle(AppsIntent::CloseMenu);
        holder.handle(AppsIntent::OpenMenu(CellKey::Folder(FolderId(99))));
        assert_eq!(holder.state().menu, None);
    }

    #[test]
    fn the_cross_works_on_an_app_in_the_open_folder() {
        // APPS-13
        let fakes = Fakes::default();
        let (_, _, mut holder) = folder_of_two(&fakes);
        holder.handle(AppsIntent::RequestUninstall(app_id("maps")));
        assert!(fakes.apps.uninstall_requested.lock().unwrap().is_empty());

        holder.handle(AppsIntent::OpenFolder(folder_id(holder.state())));
        holder.handle(AppsIntent::RequestUninstall(app_id("maps")));
        assert_eq!(
            *fakes.apps.uninstall_requested.lock().unwrap(),
            [app_id("maps")]
        );
    }

    #[test]
    fn a_folder_that_goes_closes_and_stops_being_renamed() {
        // FOLD-06: its apps were uninstalled.
        let fakes = Fakes::default();
        let (_, _, mut holder) = folder_of_two(&fakes);
        let folder = folder_id(holder.state());
        holder.handle(AppsIntent::OpenFolder(folder));
        holder.handle(AppsIntent::StartRename(folder));

        install(
            &fakes,
            &[("camera", "Camera"), ("mail", "Mail"), ("phone", "Phone")],
        );
        holder.handle(AppsIntent::Refresh);

        let state = holder.state();
        assert_eq!(labels(state), ["Camera", "Mail", "Phone"]);
        assert_eq!((state.open_folder, state.renaming), (None, None));
    }

    #[test]
    fn the_folders_grid_is_reported_with_its_panel() {
        let fakes = Fakes::default();
        let mut holder = holder(&fakes, &layout_store());
        let bounds = Bounds {
            left: 10.0,
            top: 20.0,
            right: 30.0,
            bottom: 40.0,
        };
        holder.handle(AppsIntent::FolderLayout {
            layout: phone_grid(),
            bounds,
        });
        holder.handle(AppsIntent::FolderScrolled(42.0));
        holder.handle(AppsIntent::FolderScrolled(f64::NAN));
        let state = holder.state();
        assert_eq!(state.layout_of(Surface::Folder), Some(phone_grid()));
        assert_eq!(state.folder_bounds, Some(bounds));
        assert_eq!(state.scroll_of(Surface::Folder), 42.0);
        assert!(bounds.contains((10.0, 39.0)) && !bounds.contains((30.0, 20.0)));

        // Bounds turned inside out change nothing.
        holder.handle(AppsIntent::FolderLayout {
            layout: phone_grid(),
            bounds: Bounds {
                right: 0.0,
                ..bounds
            },
        });
        assert_eq!(holder.state().folder_bounds, Some(bounds));
    }

    #[test]
    fn an_icon_centre_follows_edit_modes_scale_around_the_cells_centre() {
        // ADR 0015: rows of 300 px, the icon 154 px from 22 px down.
        let grid = GridLayout {
            icon_px: 154.0,
            icon_top_px: 22.0,
            left_px: 40.0,
            view_top_px: 5.0,
            ..phone_grid()
        };
        assert_eq!(grid.icon_centre(5, false, 1.0), (375.0, 300.0 + 99.0));
        // 150 px is the cell's middle; the icon's centre is 51 px above it, then 92 % of that.
        let (_, y) = grid.icon_centre(5, false, 0.92);
        assert!((y - (450.0 - 51.0 * 0.92)).abs() < 1e-9);
        assert_eq!(grid.origin(30.0), (40.0, 5.0 + 100.0 - 30.0));

        assert!(grid.over_icon(5, false, 1.0, (375.0 + 60.0, 399.0)));
        assert!(!grid.over_icon(5, false, 1.0, (375.0 + 63.0, 399.0)));
        // With no icon reported, nothing is over an icon; the centre is the cell's.
        assert!(!phone_grid().over_icon(5, false, 1.0, (375.0, 450.0)));
        assert_eq!(phone_grid().icon_centre(5, false, 0.92), (375.0, 450.0));
    }

    #[test]
    fn preview_slots_go_in_reading_order() {
        // FOLD-02
        let (x0, y0, size) = preview_slot(0, false);
        let (x3, y3, _) = preview_slot(3, false);
        assert!(x0 < 0.5 && y0 < 0.5 && x3 > 0.5 && y3 > 0.5);
        assert!((size - 0.33).abs() < 1e-9);
        assert_eq!(preview_slot(0, true).0, preview_slot(1, false).0);
    }

    #[test]
    fn a_panel_four_grid_cells_wide_always_has_four_columns() {
        // FOLD-04: whatever the grid's columns, its cells are at most 128 dp wide.
        for tenths in 2_000..=25_600 {
            let width_dp = f64::from(tenths) / 10.0;
            let cell = width_dp / columns_for(width_dp) as f64;
            assert_eq!(columns_for(4.0 * cell), FOLDER_COLUMNS, "{width_dp} dp");
        }
    }

    fn searching(fakes: &Fakes) -> AppsHolder {
        install(
            fakes,
            &[
                ("gmail", "Gmail"),
                ("mail", "Mail"),
                ("maps", "Maps"),
                ("cafe", "Café"),
            ],
        );
        let mut holder = holder(fakes, &layout_store());
        holder.handle(AppsIntent::OpenSearch);
        holder
    }

    fn results(state: &AppsState) -> Vec<&str> {
        state.results.iter().map(GridItem::label).collect()
    }

    #[test]
    fn search_opens_empty_and_its_results_follow_each_keystroke() {
        // SRCH-01, SRCH-02
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        assert_eq!(holder.state().search.as_deref(), Some(""));
        assert!(!holder.state().has_query());
        assert!(holder.state().results.is_empty());

        holder.handle(AppsIntent::Search("M".into()));
        assert_eq!(results(holder.state()), ["Mail", "Maps", "Gmail"]);
        holder.handle(AppsIntent::Search("Ma".into()));
        assert_eq!(results(holder.state()), ["Mail", "Maps", "Gmail"]);
        holder.handle(AppsIntent::Search("Map".into()));
        assert_eq!(results(holder.state()), ["Maps"]);
        assert!(holder.state().has_query());

        // The platform folded each query as it folds the labels (L10N-04).
        assert_eq!(*fakes.apps.search_keys.lock().unwrap(), ["M", "Ma", "Map"]);
        holder.handle(AppsIntent::Search("CAFE".into()));
        assert_eq!(results(holder.state()), ["Café"]);
    }

    #[test]
    fn the_same_query_again_changes_nothing() {
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Search("ma".into()));
        holder.handle(AppsIntent::ResultsScrolled(300.0));
        let before = holder.state().results.clone();

        holder.handle(AppsIntent::Search("ma".into()));

        assert!(Arc::ptr_eq(&before, &holder.state().results));
        assert_eq!(holder.state().results_scroll_px, 300.0);
        assert_eq!(fakes.apps.search_keys.lock().unwrap().len(), 1);
    }

    #[test]
    fn each_new_query_shows_its_results_from_the_top() {
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Search("m".into()));
        holder.handle(AppsIntent::ResultsScrolled(300.0));
        holder.handle(AppsIntent::ResultsScrolled(f64::NAN));
        assert_eq!(holder.state().scroll_of(Surface::Search), 300.0);

        holder.handle(AppsIntent::Search("ma".into()));

        assert_eq!(holder.state().results_scroll_px, 0.0);
    }

    #[test]
    fn typing_with_search_closed_does_nothing() {
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout_store());

        holder.handle(AppsIntent::Search("ma".into()));
        holder.handle(AppsIntent::SubmitSearch { from: None });

        assert_eq!(holder.state().search, None);
        assert!(holder.state().results.is_empty());
        assert!(fakes.apps.launched.lock().unwrap().is_empty());
    }

    #[test]
    fn the_results_follow_apps_that_come_and_go() {
        // APPS-06, APPS-08: a refresh while search is open ranks again.
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Search("ma".into()));
        install(&fakes, &[("maps", "Maps"), ("matter", "Matter")]);

        holder.handle(AppsIntent::Refresh);

        assert_eq!(results(holder.state()), ["Maps", "Matter"]);
    }

    #[test]
    fn apps_in_folders_are_found_too() {
        let fakes = Fakes::default();
        let (_, _, mut holder) = folder_of_two(&fakes);
        holder.handle(AppsIntent::ExitEditMode);
        holder.handle(AppsIntent::OpenSearch);

        holder.handle(AppsIntent::Search("ma".into()));

        assert_eq!(results(holder.state()), ["Mail", "Maps"]);
    }

    #[test]
    fn the_action_key_launches_the_first_result_from_its_icon_and_closes_search() {
        // SRCH-03, SRCH-04, ANIM-08: coming back shows the grid where it was.
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Scrolled(0.0));
        holder.handle(AppsIntent::Search("ma".into()));

        holder.handle(AppsIntent::SubmitSearch { from: Some(ICON) });

        assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
        assert_eq!(*fakes.apps.launched_from.lock().unwrap(), [Some(ICON)]);
        assert_eq!(holder.state().search, None);
        assert!(holder.state().results.is_empty());
    }

    #[test]
    fn launching_a_result_closes_search_and_keeps_the_grids_scroll() {
        // SRCH-04: search never moves the grid, even opened by a screen reader mid-grid.
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::CloseSearch);
        holder.handle(AppsIntent::Scrolled(640.0));
        holder.handle(AppsIntent::OpenSearch);
        holder.handle(AppsIntent::Search("gm".into()));

        holder.handle(AppsIntent::Launch {
            app: app_id("gmail"),
            from: None,
        });

        assert_eq!(holder.state().search, None);
        assert_eq!(holder.state().scroll_px, 640.0);
    }

    #[test]
    fn a_launch_the_system_refuses_keeps_search_open() {
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Search("ma".into()));
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed("Can't open".into()));

        holder.handle(AppsIntent::SubmitSearch { from: None });

        assert_eq!(holder.state().problem.as_deref(), Some("Can't open"));
        assert_eq!(holder.state().search.as_deref(), Some("ma"));
    }

    #[test]
    fn with_nothing_typed_the_action_key_does_nothing() {
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Search("   ".into()));

        holder.handle(AppsIntent::SubmitSearch { from: None });
        holder.handle(AppsIntent::SearchWeb);

        assert!(fakes.apps.launched.lock().unwrap().is_empty());
        assert!(fakes.system.web_searches.lock().unwrap().is_empty());
        assert!(holder.state().search.is_some());
    }

    #[test]
    fn a_query_nothing_matches_offers_a_web_search() {
        // SRCH-05: the action key and the offer both search with the device's default handler.
        for intent in [
            AppsIntent::SubmitSearch { from: None },
            AppsIntent::SearchWeb,
        ] {
            let fakes = Fakes::default();
            let mut holder = searching(&fakes);
            assert!(holder.state().web_search_offered);
            holder.handle(AppsIntent::Search(" zebra crossing ".into()));
            assert!(holder.state().results.is_empty());

            holder.handle(intent);

            assert_eq!(
                *fakes.system.web_searches.lock().unwrap(),
                ["zebra crossing"]
            );
            assert_eq!(holder.state().search, None);
        }
    }

    #[test]
    fn the_web_is_searched_only_when_nothing_matches_and_a_browser_is_installed() {
        // SRCH-05
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Search("ma".into()));
        holder.handle(AppsIntent::SearchWeb);
        assert!(fakes.system.web_searches.lock().unwrap().is_empty());

        let fakes = Fakes::default();
        *fakes.system.browser.lock().unwrap() = false;
        let mut holder = searching(&fakes);
        assert!(!holder.state().web_search_offered);
        holder.handle(AppsIntent::Search("zebra".into()));
        holder.handle(AppsIntent::SubmitSearch { from: None });
        holder.handle(AppsIntent::SearchWeb);
        assert!(fakes.system.web_searches.lock().unwrap().is_empty());
        assert_eq!(holder.state().search.as_deref(), Some("zebra"));
    }

    #[test]
    fn a_web_search_the_system_refuses_becomes_a_short_message() {
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Search("zebra".into()));
        *fakes.system.error.lock().unwrap() = Some(DataError::Unsupported("No search".into()));

        holder.handle(AppsIntent::SearchWeb);

        assert_eq!(holder.state().problem.as_deref(), Some("No search"));
        assert_eq!(holder.state().search.as_deref(), Some("zebra"));
    }

    #[test]
    fn back_closes_the_menu_then_search() {
        // HOME-08, SRCH-04
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::Search("ma".into()));

        assert!(holder.on_back());
        assert_eq!(holder.state().search, None);
        assert!(holder.state().results.is_empty());
        assert!(!holder.on_back());
    }

    #[test]
    fn search_doesnt_open_in_edit_mode_or_over_an_open_folder() {
        let fakes = Fakes::default();
        let (_, _, mut holder) = folder_of_two(&fakes);
        holder.handle(AppsIntent::OpenSearch);
        assert_eq!(holder.state().search, None);

        holder.handle(AppsIntent::ExitEditMode);
        let folder = folder_id(holder.state());
        holder.handle(AppsIntent::OpenFolder(folder));
        holder.handle(AppsIntent::OpenSearch);
        assert_eq!(holder.state().search, None);
    }

    #[test]
    fn opening_a_folder_a_menu_or_edit_mode_closes_search() {
        let fakes = Fakes::default();
        let (_, _, mut holder) = folder_of_two(&fakes);
        holder.handle(AppsIntent::ExitEditMode);
        let folder = folder_id(holder.state());
        let opens: [AppsIntent; 3] = [
            AppsIntent::OpenFolder(folder),
            AppsIntent::OpenMenu(CellKey::App(app_id("camera"))),
            AppsIntent::EnterEditMode,
        ];
        for open in opens {
            holder.handle(AppsIntent::OpenSearch);
            holder.handle(AppsIntent::Search("ca".into()));
            assert!(holder.state().search.is_some());

            holder.handle(open);

            assert_eq!(holder.state().search, None);
            assert!(holder.state().results.is_empty());
            holder.on_back();
        }
    }

    #[test]
    fn opening_search_closes_the_menu_and_opening_it_again_keeps_the_query() {
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout_store());
        holder.handle(AppsIntent::OpenMenu(CellKey::App(app_id("mail"))));

        holder.handle(AppsIntent::OpenSearch);
        assert!(holder.state().menu.is_none());
        holder.handle(AppsIntent::Search("ma".into()));
        holder.handle(AppsIntent::OpenSearch);

        assert_eq!(holder.state().search.as_deref(), Some("ma"));
        assert_eq!(results(holder.state()), ["Mail"]);
    }

    #[test]
    fn the_results_grid_is_reported_with_the_fields_band() {
        // ANIM-16
        let fakes = Fakes::default();
        let mut holder = holder(&fakes, &layout_store());
        assert_eq!(holder.state().search_band_px(), None);
        let band = Bounds {
            left: 0.0,
            top: 100.0,
            right: 1080.0,
            bottom: 298.0,
        };

        holder.handle(AppsIntent::SearchLayout {
            layout: phone_grid(),
            band,
        });

        let state = holder.state();
        assert_eq!(state.layout_of(Surface::Search), Some(phone_grid()));
        assert_eq!(state.search_band_px(), Some(198.0));
        // Bounds turned inside out change nothing.
        holder.handle(AppsIntent::SearchLayout {
            layout: phone_grid(),
            band: Bounds {
                bottom: 0.0,
                ..band
            },
        });
        assert_eq!(holder.state().search_band, Some(band));
    }

    #[test]
    fn the_results_grid_has_no_empty_last_row() {
        let fakes = Fakes::default();
        let mut holder = searching(&fakes);
        holder.handle(AppsIntent::SearchLayout {
            layout: phone_grid(),
            band: Bounds::default(),
        });
        assert_eq!(holder.state().rows_of(Surface::Search), 0);

        holder.handle(AppsIntent::Search("a".into()));

        // Café, Gmail, Mail and Maps fill one row of four.
        assert_eq!(holder.state().results.len(), 4);
        assert_eq!(holder.state().rows_of(Surface::Search), 1);
        assert_eq!(
            holder
                .state()
                .place_of(Surface::Search, &CellKey::App(app_id("maps"))),
            Some(3)
        );
    }

    /// A layout as a backup restores it (APPS-21): Phone, Maps, then Mail, saved by another copy
    /// of MeeGone, installed at 1.
    fn restored(layout: &PersistentLayoutStore) {
        layout
            .save(&Layout {
                entries: vec![
                    LayoutEntry::App(app_id("phone")),
                    LayoutEntry::App(app_id("maps")),
                    LayoutEntry::App(app_id("mail")),
                ],
                installed_ms: Some(1),
                ..Layout::default()
            })
            .unwrap();
    }

    #[test]
    fn a_restored_layout_waits_for_its_apps_and_is_saved_as_this_copys() {
        // APPS-21: Maps isn't installed yet. Notes, which the backup doesn't hold, joins the
        // end without a dot.
        let fakes = Fakes::default();
        *fakes.system.installed_ms.lock().unwrap() = 5;
        *fakes.system.now_ms.lock().unwrap() = 100;
        let (_, layout) = storage_and_store();
        restored(&layout);
        install(
            &fakes,
            &[("mail", "Mail"), ("phone", "Phone"), ("notes", "Notes")],
        );

        let mut holder = holder(&fakes, &layout);

        assert_eq!(labels(holder.state()), ["Phone", "Mail", "Notes"]);
        let kept = saved(&layout);
        assert_eq!(kept.installed_ms, Some(5));
        assert_eq!(
            kept.restore,
            Some(Restore {
                until_ms: 100 + RESTORE_WAIT_MS,
                waiting: [app_id("maps")].into(),
            })
        );

        fakes.apps.apps.lock().unwrap().push(app("maps", "Maps"));
        holder.handle(AppsIntent::Refresh);

        assert_eq!(labels(holder.state()), ["Phone", "Maps", "Mail", "Notes"]);
        assert_eq!(saved(&layout).restore, None);
    }

    #[test]
    fn a_layout_is_restored_only_once() {
        // APPS-21: after a restart, an app that is gone leaves as usual (APPS-08).
        let fakes = Fakes::default();
        *fakes.system.installed_ms.lock().unwrap() = 5;
        let (_, layout) = storage_and_store();
        restored(&layout);
        install(
            &fakes,
            &[("mail", "Mail"), ("phone", "Phone"), ("maps", "Maps")],
        );
        drop(holder(&fakes, &layout));

        install(&fakes, &[("mail", "Mail"), ("phone", "Phone")]);
        let restarted = holder(&fakes, &layout);

        assert_eq!(labels(restarted.state()), ["Phone", "Mail"]);
        assert_eq!(
            saved(&layout),
            Layout {
                entries: vec![
                    LayoutEntry::App(app_id("phone")),
                    LayoutEntry::App(app_id("mail")),
                ],
                installed_ms: Some(5),
                ..Layout::default()
            }
        );
    }

    #[test]
    fn apps_still_missing_when_the_wait_ends_leave() {
        // APPS-21: at the first refresh after it. Installed later, an app is new (APPS-06).
        let fakes = Fakes::default();
        *fakes.system.installed_ms.lock().unwrap() = 5;
        let (_, layout) = storage_and_store();
        restored(&layout);
        install(&fakes, &[("mail", "Mail"), ("phone", "Phone")]);
        let mut holder = holder(&fakes, &layout);

        *fakes.system.now_ms.lock().unwrap() = RESTORE_WAIT_MS - 1;
        holder.handle(AppsIntent::Refresh);
        assert_eq!(
            saved(&layout).restore.map(|restore| restore.waiting),
            Some([app_id("maps")].into())
        );

        *fakes.system.now_ms.lock().unwrap() = RESTORE_WAIT_MS;
        holder.handle(AppsIntent::Refresh);
        assert_eq!(
            saved(&layout),
            Layout {
                entries: vec![
                    LayoutEntry::App(app_id("phone")),
                    LayoutEntry::App(app_id("mail")),
                ],
                installed_ms: Some(5),
                ..Layout::default()
            }
        );

        fakes.apps.apps.lock().unwrap().push(app("maps", "Maps"));
        holder.handle(AppsIntent::Refresh);
        assert_eq!(labels(holder.state()), ["Phone", "Mail", "Maps•"]);
    }

    #[test]
    fn an_app_first_seen_as_the_wait_ends_keeps_its_place_and_the_restore_is_over() {
        // APPS-21: nothing refreshed between its install and the end of the wait.
        let fakes = Fakes::default();
        *fakes.system.installed_ms.lock().unwrap() = 5;
        let (_, layout) = storage_and_store();
        restored(&layout);
        install(&fakes, &[("mail", "Mail"), ("phone", "Phone")]);
        let mut holder = holder(&fakes, &layout);

        fakes.apps.apps.lock().unwrap().push(app("maps", "Maps"));
        *fakes.system.now_ms.lock().unwrap() = RESTORE_WAIT_MS;
        holder.handle(AppsIntent::Refresh);

        assert_eq!(labels(holder.state()), ["Phone", "Maps", "Mail"]);
        assert_eq!(saved(&layout).restore, None);
    }

    #[test]
    fn a_hidden_app_a_restore_waits_for_stays_hidden() {
        // APPS-19, APPS-21: the backed-up settings hide Maps, which isn't installed yet.
        let fakes = Fakes::default();
        *fakes.system.installed_ms.lock().unwrap() = 5;
        let (_, layout) = storage_and_store();
        restored(&layout);
        PersistentSettingsStore::new(fakes.storage.clone())
            .update(&mut |settings| settings.hidden_apps = [app_id("maps")].into())
            .unwrap();
        install(&fakes, &[("mail", "Mail"), ("phone", "Phone")]);
        let mut holder = holder(&fakes, &layout);
        assert_eq!(saved_hidden(&fakes), [app_id("maps")].into());

        fakes.apps.apps.lock().unwrap().push(app("maps", "Maps"));
        holder.handle(AppsIntent::Refresh);

        assert_eq!(labels(holder.state()), ["Phone", "Mail"]);
        assert_eq!(hidden_labels(&holder), ["Maps"]);
    }
}
