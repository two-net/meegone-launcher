//! Applications state (SRS 4.2): the grid, its layout and scroll position, an app's long-press
//! menu, edit mode with its arrangement and the open folder. [`reconcile`] keeps the layout in
//! step with the installed apps (ADR 0012).

mod reconcile;

use std::sync::Arc;

use meegone_apps_data::{Layout, LayoutStore, SavedLayout};
use meegone_core_data::{AppsAdapter, DataError, Profile};
use meegone_core_state::{App, AppId, Rect, Shortcut, StateHolder};

pub use meegone_apps_data::FolderId;
use reconcile::{Start, arrange, follow_order, grid, reconcile};
pub use reconcile::{cells_moved, previous_places};

/// Columns on a phone (APPS-02).
pub const PHONE_COLUMNS: usize = 4;
/// On a wider grid, columns are added so no cell is wider than this, in dp (APPS-02)…
pub const MAX_CELL_DP: f64 = 128.0;
/// …and none narrower than this, which holds for any grid at least four of them wide.
pub const MIN_CELL_DP: f64 = 96.0;

/// How many columns a grid `width_dp` wide has, inside its margins (APPS-02).
///
/// Four, unless four cells would be wider than [`MAX_CELL_DP`]: then as few columns as keep
/// every cell at most that wide. A phone's grid is under 512 dp, so it always has four. A
/// screen 600 dp wide or more leaves a grid of at least 384 dp inside its safe area and
/// margins, where every cell is between [`MIN_CELL_DP`] and [`MAX_CELL_DP`].
pub fn columns_for(width_dp: f64) -> usize {
    if !width_dp.is_finite() || width_dp <= 0.0 {
        return PHONE_COLUMNS;
    }
    PHONE_COLUMNS.max((width_dp / MAX_CELL_DP).ceil() as usize)
}

/// An app's long-press menu lists at most this many of its shortcuts, as the system's own
/// launcher does (APPS-11, ADR 0013).
pub const MAX_SHORTCUTS: usize = 4;

/// One item of an app's long-press menu (APPS-11), in the order it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMenuItem {
    /// Opens the system's App info screen.
    AppInfo,
    /// Opens the system's uninstall confirmation (APPS-13). Only for apps the user can
    /// uninstall.
    Uninstall,
    /// Enters edit mode. Not offered while edit mode is on.
    EditApplications,
    /// Starts one of the app's shortcuts. Offered only while MeeGone is the default home app.
    Shortcut(Shortcut),
}

/// An app's long-press menu (APPS-11). It covers the home views while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppMenu {
    /// The app it is for. The host opens it at this app's icon.
    pub app: AppId,
    /// What it offers, in order: App info, Uninstall, Edit applications, then the shortcuts.
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

/// How the host laid the grid out, in physical pixels.
///
/// The cells start `top_px` below the grid's top, which is the home views' top, and scroll under
/// the system bars (ADR 0010). Cell places are worked out exactly as the host lays them out, in
/// whole pixels, so the shell can tell which place is under a dragged icon (ADR 0014).
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
}

impl GridLayout {
    fn is_valid(&self) -> bool {
        let sizes = [self.width_px, self.row_height_px, self.density];
        let room = [self.top_px, self.bottom_px, self.height_px];
        sizes.iter().all(|value| value.is_finite() && *value > 0.0)
            && room.iter().all(|value| value.is_finite() && *value >= 0.0)
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
    /// An app's long-press menu, while it is open (APPS-11). Shared, so the UI can tell when it
    /// changed.
    pub menu: Option<Arc<AppMenu>>,
    /// Edit mode (APPS-12): ✕ badges on the apps the user can uninstall, icons that can be
    /// dragged, and a Done button. A tap on an icon launches nothing.
    pub edit_mode: bool,
    pub open_folder: Option<FolderId>,
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

    /// The place of `app`'s own cell, if it has one: an app in a folder has none.
    pub fn place_of(&self, app: &AppId) -> Option<usize> {
        self.items
            .iter()
            .position(|item| matches!(item, GridItem::App { app: shown, .. } if shown.id == *app))
    }
}

#[derive(Debug)]
pub enum AppsIntent {
    /// Rebuilds the grid from the installed apps and the saved layout.
    Refresh,
    /// Launches an app, growing its window from `from`, the tapped icon (APPS-10, ANIM-08). An
    /// unavailable app is launched too, so the system explains why it can't open (APPS-09). In
    /// edit mode, nothing launches.
    Launch {
        app: AppId,
        from: Option<Rect>,
    },
    /// The host laid the grid out, at start or after a size, density or font change.
    Layout(GridLayout),
    /// Opens `app`'s long-press menu (APPS-11).
    OpenMenu(AppId),
    /// Closes the app menu, if it is open.
    CloseMenu,
    /// An item of the app menu was chosen. The menu closes, then does what the item says. An
    /// item it doesn't offer is ignored.
    ChooseMenuItem(AppMenuItem),
    /// Enters edit mode (APPS-12). An open app menu closes.
    EnterEditMode,
    /// Done: leaves edit mode (APPS-12). An arrangement not dropped yet is undone.
    ExitEditMode,
    /// In edit mode, shows `app`'s own cell at place `to`, or the last place, while the others
    /// move aside, as a dragged icon goes (APPS-12). Nothing is saved until [`AppsIntent::Drop`].
    /// A refresh keeps it.
    Arrange {
        app: AppId,
        to: usize,
    },
    /// Keeps the arrangement shown, and saves the layout in that order (APPS-12, APPS-18).
    Drop,
    /// Shows the saved order again, as when a drag ends any way but a drop.
    CancelArrange,
    /// In edit mode, moves `app`'s own cell to place `to` and saves it: an arrangement and a
    /// drop at once, as a screen reader's "Move to…" actions do (A11Y-05).
    Move {
        app: AppId,
        to: usize,
    },
    /// The ✕ on `app` was tapped, in edit mode: the system asks the user to confirm its
    /// uninstall (APPS-13). Nothing happens for an app the user can't uninstall.
    RequestUninstall(AppId),
    OpenFolder(FolderId),
    /// The host scrolled the grid to this many pixels from its top, or MeeGone restores where
    /// it was (REL-05).
    Scrolled(f64),
    /// Home on Applications with nothing open (HOME-07).
    ScrollToTop,
}

pub struct AppsHolder {
    apps: Arc<dyn AppsAdapter>,
    store: Arc<dyn LayoutStore>,
    /// The layout, kept once the store has been read. Applications is its only writer, so the
    /// file is read at start, or until a read succeeds, and written after each change.
    layout: Option<Layout>,
    /// The kept layout's last save failed, so the next refresh saves it again.
    unsaved: bool,
    /// The apps the system listed at the last refresh, which the cells show.
    installed: Vec<App>,
    /// In edit mode, an app's own cell shown at a place, while the others move aside, until it
    /// is dropped (APPS-12). The place stays through a refresh: it is where the finger is.
    arranging: Option<(AppId, usize)>,
    state: AppsState,
}

impl AppsHolder {
    pub fn new(apps: Arc<dyn AppsAdapter>, layout: Arc<dyn LayoutStore>) -> Self {
        Self {
            apps,
            store: layout,
            layout: None,
            unsaved: false,
            installed: Vec::new(),
            arranging: None,
            state: AppsState::default(),
        }
    }

    /// Reconciles the layout with the installed apps, saves it if it changed, and shows it
    /// (APPS-06, APPS-08, APPS-18). If the system can't list the apps, the grid stays as it is.
    fn refresh(&mut self) {
        let (installed, profiles) = match self.catalogue() {
            Ok(catalogue) => catalogue,
            Err(error) => {
                self.state.problem = Some(error.to_string());
                return;
            }
        };
        self.state.problem = None;
        let reconciled = |saved: &Layout, start: &Start| {
            let layout = reconcile(saved, &installed, &profiles, start);
            let changed = layout != *saved;
            (layout, changed)
        };
        let (layout, changed) = match self.layout.take() {
            Some(kept) => reconciled(&kept, &Start::Saved),
            None => match self.store.load() {
                Ok(SavedLayout::Missing) => {
                    let defaults = self.apps.default_apps();
                    (
                        reconciled(&Layout::default(), &Start::First { defaults }).0,
                        true,
                    )
                }
                Ok(SavedLayout::Read(saved)) => reconciled(&saved, &Start::Saved),
                // Saved at once, so the damaged file is replaced before anything else can read
                // it (REL-02).
                Ok(SavedLayout::Recovered(recovered)) => {
                    (reconciled(&recovered, &Start::Rebuilt).0, true)
                }
                Err(error) => {
                    // Shown but never saved: the file may be fine, so the next refresh reads it
                    // again instead of replacing it (REL-02). Nothing can be arranged meanwhile.
                    let shown = reconciled(&Layout::default(), &Start::Rebuilt).0;
                    self.installed = installed;
                    self.arranging = None;
                    self.show(&shown);
                    self.state.problem = Some(error.to_string());
                    return;
                }
            },
        };
        if changed || self.unsaved {
            self.save(&layout);
        }
        self.installed = installed;
        self.show(&layout);
        self.layout = Some(layout);
        // The menu of an app that was just removed closes.
        let gone = |menu: &Arc<AppMenu>| self.app(&menu.app).is_none();
        if self.state.menu.as_ref().is_some_and(gone) {
            self.state.menu = None;
        }
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

    /// Shows `layout`'s cells, with the arrangement in progress if there is one. This is the
    /// only place the cells are made, so they never go stale. They are replaced only when they
    /// change, so the host rebinds nothing it doesn't need to.
    fn show(&mut self, layout: &Layout) {
        let mut items = grid(layout, &self.installed);
        if let Some((app, to)) = &self.arranging {
            // An app that has gone, or now sits in a folder, ends its arrangement.
            if !arrange(&mut items, app, *to) {
                self.arranging = None;
            }
        }
        if *self.state.items != *items {
            self.state.items = items.into();
        }
    }

    /// Shows the kept layout again, after the arrangement changed.
    fn show_kept(&mut self) {
        if let Some(layout) = self.layout.take() {
            self.show(&layout);
            self.layout = Some(layout);
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
        }
    }

    /// An app opened from MeeGone loses its dot, for good (APPS-07).
    fn opened(&mut self, app: &AppId) {
        let Some(mut layout) = self.layout.take() else {
            return;
        };
        if layout.new.remove(app) {
            self.save(&layout);
            self.show(&layout);
        }
        self.layout = Some(layout);
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

    fn open_folder(&mut self, folder: FolderId) {
        let exists = self
            .state
            .items
            .iter()
            .any(|item| matches!(item, GridItem::Folder { id, .. } if *id == folder));
        if exists {
            self.state.open_folder = Some(folder);
        }
    }

    /// The grid's app `id`, in a cell of its own or in a folder.
    fn app(&self, id: &AppId) -> Option<&App> {
        self.state.items.iter().find_map(|item| match item {
            GridItem::App { app, .. } => (app.id == *id).then_some(app),
            GridItem::Folder { apps, .. } => apps.iter().find(|app| app.id == *id),
        })
    }

    /// APPS-11: App info, Uninstall when the user can uninstall the app (APPS-13), Edit
    /// applications unless edit mode is on, then the app's first shortcuts. The system offers
    /// shortcuts only while MeeGone is the default home app; otherwise, or if it can't say, the
    /// menu has none.
    fn open_menu(&mut self, id: AppId) {
        let Some(app) = self.app(&id) else {
            return;
        };
        let mut items = vec![AppMenuItem::AppInfo];
        if app.can_uninstall {
            items.push(AppMenuItem::Uninstall);
        }
        if !self.state.edit_mode {
            items.push(AppMenuItem::EditApplications);
        }
        let shortcuts = self.apps.shortcuts(&id).unwrap_or_default();
        items.extend(
            shortcuts
                .into_iter()
                .take(MAX_SHORTCUTS)
                .map(AppMenuItem::Shortcut),
        );
        self.state.menu = Some(Arc::new(AppMenu { app: id, items }));
    }

    /// Closes the menu, then does what `item` says. Leaving MeeGone for a system screen or a
    /// shortcut can fail; the failure is the problem, shown briefly.
    fn choose(&mut self, item: AppMenuItem) {
        let Some(menu) = self.state.menu.take_if(|menu| menu.offers(&item)) else {
            return;
        };
        let done = match item {
            AppMenuItem::AppInfo => self.apps.open_app_info(&menu.app),
            // MeeGone never uninstalls anything itself: the system asks the user (APPS-13).
            AppMenuItem::Uninstall => self.apps.request_uninstall(&menu.app),
            AppMenuItem::EditApplications => {
                self.state.edit_mode = true;
                Ok(())
            }
            AppMenuItem::Shortcut(shortcut) => self.apps.launch_shortcut(&shortcut),
        };
        self.state.problem = done.err().map(|error| error.to_string());
    }

    /// Leaves edit mode, undoing an arrangement that wasn't dropped. Returns whether it was on.
    fn exit_edit_mode(&mut self) -> bool {
        self.cancel_arrangement();
        std::mem::take(&mut self.state.edit_mode)
    }

    /// APPS-12: shows `app`'s own cell at `to` while the others move aside. Only in edit mode,
    /// and only while the layout could be read, since nothing could be saved otherwise
    /// (REL-02).
    fn arrange(&mut self, app: AppId, to: usize) {
        if !self.state.edit_mode || self.layout.is_none() || self.state.place_of(&app).is_none() {
            return;
        }
        self.arranging = Some((app, to));
        self.show_kept();
    }

    /// APPS-12, APPS-18: the arrangement shown becomes the layout, saved once. A failed save is
    /// the problem, and the next refresh saves again (ADR 0012).
    fn drop_arrangement(&mut self) {
        if self.arranging.take().is_none() {
            return;
        }
        let Some(layout) = self.layout.take() else {
            return;
        };
        let arranged = follow_order(&layout, &self.state.items);
        if arranged != layout {
            self.save(&arranged);
        }
        self.layout = Some(arranged);
    }

    fn cancel_arrangement(&mut self) {
        if self.arranging.take().is_some() {
            self.show_kept();
        }
    }

    /// APPS-13: the ✕ asks the system, which asks the user. MeeGone never uninstalls anything
    /// itself, and apps the user can't uninstall have no ✕.
    fn request_uninstall(&mut self, id: &AppId) {
        let can = self.state.edit_mode
            && self.state.place_of(id).is_some()
            && self.app(id).is_some_and(|app| app.can_uninstall);
        if can {
            self.state.problem = self
                .apps
                .request_uninstall(id)
                .err()
                .map(|error| error.to_string());
        }
    }
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
            AppsIntent::OpenMenu(app) => self.open_menu(app),
            AppsIntent::CloseMenu => self.state.menu = None,
            AppsIntent::ChooseMenuItem(item) => self.choose(item),
            AppsIntent::EnterEditMode => {
                self.state.menu = None;
                self.state.edit_mode = true;
            }
            AppsIntent::ExitEditMode => {
                self.state.menu = None;
                self.exit_edit_mode();
            }
            AppsIntent::Arrange { app, to } => self.arrange(app, to),
            AppsIntent::Drop => self.drop_arrangement(),
            AppsIntent::CancelArrange => self.cancel_arrangement(),
            AppsIntent::Move { app, to } => {
                self.arrange(app, to);
                self.drop_arrangement();
            }
            AppsIntent::RequestUninstall(app) => self.request_uninstall(&app),
            AppsIntent::OpenFolder(folder) => self.open_folder(folder),
            AppsIntent::Scrolled(px) => {
                if px.is_finite() {
                    self.state.scroll_px = px.max(0.0);
                }
            }
            AppsIntent::ScrollToTop => self.state.scroll_px = 0.0,
        }
    }

    /// HOME-08: the app menu, then the open folder, then edit mode.
    fn on_back(&mut self) -> bool {
        if self.state.menu.take().is_some() || self.state.open_folder.take().is_some() {
            return true;
        }
        self.exit_edit_mode()
    }
}

#[cfg(test)]
mod tests {
    use meegone_apps_data::{LayoutEntry, PersistentLayoutStore};
    use meegone_core_data::fakes::{FakeStorage, Fakes, app, app_id, work_app};
    use meegone_core_data::storage::format::seal;
    use meegone_core_data::{ProfileId, StoredFile};

    use super::*;

    fn holder(fakes: &Fakes, layout: &Arc<PersistentLayoutStore>) -> AppsHolder {
        let mut holder = AppsHolder::new(fakes.apps.clone(), layout.clone());
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
    fn an_apps_menu_offers_app_info_uninstall_and_edit_applications() {
        // APPS-11, APPS-13: a system app can't be uninstalled, so its menu doesn't offer it.
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

        holder.handle(AppsIntent::OpenMenu(app_id("mail")));
        assert_eq!(
            holder.state().menu.as_deref(),
            Some(&AppMenu {
                app: app_id("mail"),
                items: vec![
                    AppMenuItem::AppInfo,
                    AppMenuItem::Uninstall,
                    AppMenuItem::EditApplications,
                ],
            })
        );

        holder.handle(AppsIntent::OpenMenu(app_id("settings")));
        assert_eq!(
            menu(&holder),
            Some(vec![AppMenuItem::AppInfo, AppMenuItem::EditApplications])
        );

        // An app that isn't in the grid has no menu.
        holder.handle(AppsIntent::CloseMenu);
        holder.handle(AppsIntent::OpenMenu(app_id("gone")));
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

        holder.handle(AppsIntent::OpenMenu(app_id("mail")));
        assert_eq!(menu(&holder).unwrap().len(), 3);

        *fakes.apps.shortcuts_allowed.lock().unwrap() = true;
        holder.handle(AppsIntent::OpenMenu(app_id("mail")));
        let items = menu(&holder).unwrap();
        assert_eq!(
            items[..3],
            [
                AppMenuItem::AppInfo,
                AppMenuItem::Uninstall,
                AppMenuItem::EditApplications
            ]
        );
        assert_eq!(
            items[3..],
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
            holder.handle(AppsIntent::OpenMenu(app_id("mail")));
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
        holder.handle(AppsIntent::OpenMenu(app_id("settings")));
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
        holder.handle(AppsIntent::OpenMenu(app_id("mail")));
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
        holder.handle(AppsIntent::OpenMenu(app_id("maps")));
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
        holder.handle(AppsIntent::OpenMenu(app_id("mail")));

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
        holder.handle(AppsIntent::OpenMenu(app_id("mail")));

        holder.handle(AppsIntent::EnterEditMode);
        assert_eq!(holder.state().menu, None);

        holder.handle(AppsIntent::OpenMenu(app_id("mail")));
        assert_eq!(
            menu(&holder),
            Some(vec![AppMenuItem::AppInfo, AppMenuItem::Uninstall])
        );
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
                    LayoutEntry::App(app_id("mail")),
                ],
                new: [app_id("mail")].into(),
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
            app: app_id("maps"),
            to: 1,
        });
        assert_eq!(labels(holder.state()), ["Camera", "Maps", "Mail", "Phone"]);
        holder.handle(AppsIntent::Arrange {
            app: app_id("maps"),
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
                app: app_id("phone"),
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
            app: app_id("phone"),
            to: 0,
        });
        assert!(holder.on_back());
        assert!(!holder.state().edit_mode);
        assert_eq!(labels(holder.state()), ["Camera", "Mail", "Maps", "Phone"]);
    }

    #[test]
    fn an_arrangement_stays_where_the_finger_is_as_apps_come_and_go() {
        // APPS-12, APPS-08: an app above it is removed while it is dragged; it keeps its place.
        // If the dragged app itself goes, so does its arrangement.
        let fakes = Fakes::default();
        let (_, layout, mut holder) = editing(&fakes);
        holder.handle(AppsIntent::Arrange {
            app: app_id("phone"),
            to: 1,
        });

        install(
            &fakes,
            &[("mail", "Mail"), ("maps", "Maps"), ("phone", "Phone")],
        );
        holder.handle(AppsIntent::Refresh);
        assert_eq!(labels(holder.state()), ["Mail", "Phone", "Maps"]);

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
            app: app_id("camera"),
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
            app: app_id("phone"),
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
            app: app_id("maps"),
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
        holder.handle(AppsIntent::OpenMenu(app_id("mail")));

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
}
