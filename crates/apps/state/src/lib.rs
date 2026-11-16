//! Applications state (SRS 4.2): the grid, its layout and scroll position, edit mode and the
//! open folder. [`reconcile`] keeps the layout in step with the installed apps (ADR 0012).

mod reconcile;

use std::sync::Arc;

use meegone_apps_data::{Layout, LayoutStore, SavedLayout};
use meegone_core_data::{AppsAdapter, DataError, Profile};
use meegone_core_state::{App, AppId, Rect, StateHolder};

pub use meegone_apps_data::FolderId;
use reconcile::{Start, grid, reconcile};
pub use reconcile::{cells_came_or_went, previous_places};

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    /// The grid's width inside its margins and the safe area.
    pub width_px: f64,
    /// From the top of one row to the top of the next: a cell and the gap below it. Cells grow
    /// taller with the system font size (APPS-16).
    pub row_height_px: f64,
    /// Physical pixels per dp.
    pub density: f64,
}

impl GridLayout {
    fn is_valid(&self) -> bool {
        [self.width_px, self.row_height_px, self.density]
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
    }

    fn columns(&self) -> usize {
        columns_for(self.width_px / self.density)
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
}

#[derive(Debug)]
pub enum AppsIntent {
    /// Rebuilds the grid from the installed apps and the saved layout.
    Refresh,
    /// Launches an app, growing its window from `from`, the tapped icon (APPS-10, ANIM-08). An
    /// unavailable app is launched too, so the system explains why it can't open (APPS-09).
    Launch {
        app: AppId,
        from: Option<Rect>,
    },
    /// The host laid the grid out, at start or after a size, density or font change.
    Layout(GridLayout),
    EnterEditMode,
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
    /// The grid's last layout, if the host has laid it out yet.
    grid: Option<GridLayout>,
    state: AppsState,
}

impl AppsHolder {
    pub fn new(apps: Arc<dyn AppsAdapter>, layout: Arc<dyn LayoutStore>) -> Self {
        Self {
            apps,
            store: layout,
            layout: None,
            unsaved: false,
            grid: None,
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
                    // again instead of replacing it (REL-02).
                    let shown = reconciled(&Layout::default(), &Start::Rebuilt).0;
                    self.show(&shown, &installed);
                    self.state.problem = Some(error.to_string());
                    return;
                }
            },
        };
        if changed || self.unsaved {
            self.save(&layout);
        }
        self.show(&layout, &installed);
        self.layout = Some(layout);
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

    fn show(&mut self, layout: &Layout, installed: &[App]) {
        let items = grid(layout, installed);
        if *self.state.items != *items {
            self.state.items = items.into();
        }
    }

    fn launch(&mut self, app: &AppId, from: Option<Rect>) {
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
            let items: Vec<GridItem> = self
                .state
                .items
                .iter()
                .map(|item| match item {
                    GridItem::App { app: shown, .. } if shown.id == *app => GridItem::App {
                        app: shown.clone(),
                        new: false,
                    },
                    other => other.clone(),
                })
                .collect();
            self.state.items = items.into();
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
        if let Some(before) = self.grid {
            if before.row_height_px != grid.row_height_px || self.state.columns != columns {
                let row = (self.state.scroll_px / before.row_height_px).floor() as usize;
                let first = row * self.state.columns;
                self.state.scroll_px = (first / columns) as f64 * grid.row_height_px;
            }
        }
        self.grid = Some(grid);
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
            AppsIntent::EnterEditMode => self.state.edit_mode = true,
            AppsIntent::OpenFolder(folder) => self.open_folder(folder),
            AppsIntent::Scrolled(px) => {
                if px.is_finite() {
                    self.state.scroll_px = px.max(0.0);
                }
            }
            AppsIntent::ScrollToTop => self.state.scroll_px = 0.0,
        }
    }

    fn on_back(&mut self) -> bool {
        if self.state.open_folder.take().is_some() {
            return true;
        }
        std::mem::take(&mut self.state.edit_mode)
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
}
