//! Applications state (SRS 4.2): the grid, its layout and scroll position, edit mode and the
//! open folder.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use meegone_apps_data::{Layout, LayoutEntry, LayoutStore};
use meegone_core_data::AppsAdapter;
use meegone_core_state::{App, AppId, Rect, StateHolder};

pub use meegone_apps_data::FolderId;

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
    App(App),
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
            GridItem::App(app) => &app.label,
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
    layout: Arc<dyn LayoutStore>,
    /// The grid's last layout, if the host has laid it out yet.
    grid: Option<GridLayout>,
    state: AppsState,
}

impl AppsHolder {
    pub fn new(apps: Arc<dyn AppsAdapter>, layout: Arc<dyn LayoutStore>) -> Self {
        Self {
            apps,
            layout,
            grid: None,
            state: AppsState::default(),
        }
    }

    fn refresh(&mut self) {
        let installed = match self.apps.launchable_apps() {
            Ok(installed) => installed,
            Err(error) => {
                self.state.problem = Some(error.to_string());
                return;
            }
        };
        let saved = self.layout.load();
        let layout = reconcile(&saved, &installed);
        self.state.problem = None;
        if layout != saved {
            if let Err(error) = self.layout.save(&layout) {
                self.state.problem = Some(error.to_string());
            }
        }
        let items = grid(&layout, &installed);
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

/// Keeps the saved order, drops apps that are gone and adds new apps at the end in
/// alphabetical order (APPS-04, APPS-06, APPS-18). A folder left with one app becomes that app;
/// an empty folder is deleted (FOLD-06).
fn reconcile(saved: &Layout, installed: &[App]) -> Layout {
    let present: HashSet<&AppId> = installed.iter().map(|app| &app.id).collect();
    let mut placed: HashSet<AppId> = HashSet::new();
    let mut keep = |id: &AppId| present.contains(id) && placed.insert(id.clone());
    let mut entries = Vec::new();

    for entry in &saved.entries {
        match entry {
            LayoutEntry::App(id) => {
                if keep(id) {
                    entries.push(entry.clone());
                }
            }
            LayoutEntry::Folder { id, name, apps } => {
                let kept: Vec<AppId> = apps.iter().filter(|app| keep(app)).cloned().collect();
                if kept.len() > 1 {
                    entries.push(LayoutEntry::Folder {
                        id: *id,
                        name: name.clone(),
                        apps: kept,
                    });
                } else if let Some(only) = kept.into_iter().next() {
                    entries.push(LayoutEntry::App(only));
                }
            }
        }
    }

    // Alphabetical by label for now; locale-aware ordering comes with L10N-04.
    let mut new: Vec<&App> = installed.iter().filter(|app| keep(&app.id)).collect();
    new.sort_by_cached_key(|app| app.label.to_lowercase());
    entries.extend(new.into_iter().map(|app| LayoutEntry::App(app.id.clone())));

    Layout { entries }
}

fn grid(layout: &Layout, installed: &[App]) -> Vec<GridItem> {
    let by_id: HashMap<&AppId, &App> = installed.iter().map(|app| (&app.id, app)).collect();
    let app = |id: &AppId| by_id.get(id).map(|app| (*app).clone());
    layout
        .entries
        .iter()
        .filter_map(|entry| match entry {
            LayoutEntry::App(id) => app(id).map(GridItem::App),
            LayoutEntry::Folder { id, name, apps } => Some(GridItem::Folder {
                id: *id,
                name: name.clone(),
                apps: apps.iter().filter_map(app).collect(),
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use meegone_apps_data::PersistentLayoutStore;
    use meegone_core_data::DataError;
    use meegone_core_data::fakes::{FakeStorage, Fakes, app, app_id, work_app};

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
                GridItem::App(app) => app.label.clone(),
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
        assert_eq!(layout.load().entries.len(), 3);
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
            })
            .unwrap();
        install(
            &fakes,
            &[("mail", "Mail"), ("phone", "Phone"), ("atlas", "Atlas")],
        );

        let holder = holder(&fakes, &layout);

        assert_eq!(labels(holder.state()), ["Phone", "Mail", "Atlas"]);
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
        assert_eq!(*holder.state().items, [GridItem::App(paused.clone())]);

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
                GridItem::App(app) => (app.label.as_str(), app.work),
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
            })
            .unwrap();
        install(&fakes, &[("maps", "Maps"), ("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout);
        holder.handle(AppsIntent::Scrolled(480.0));
        holder.handle(AppsIntent::OpenFolder(FolderId(1)));

        assert!(holder.on_back());
        assert_eq!(holder.state().scroll_px, 480.0);
    }
}
