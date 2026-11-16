//! Applications state (SRS 4.2): the grid, its scroll position, edit mode and the open folder.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use meegone_apps_data::{Layout, LayoutEntry, LayoutStore};
use meegone_core_data::AppsAdapter;
use meegone_core_state::{App, AppId, StateHolder};

pub use meegone_apps_data::FolderId;

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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppsState {
    pub items: Vec<GridItem>,
    /// How far the grid is scrolled from its top, in pixels, as the host reports it. Home sets
    /// it back to 0 (HOME-07), and it is kept when the system ends MeeGone's process (REL-05).
    /// The host scrolls its grid to this position whenever it differs from its own (ADR 0007).
    pub scroll_px: f64,
    pub edit_mode: bool,
    pub open_folder: Option<FolderId>,
    /// The last problem, in plain words. The grid keeps working (REL-01).
    pub problem: Option<String>,
}

#[derive(Debug)]
pub enum AppsIntent {
    /// Rebuilds the grid from the installed apps and the saved layout.
    Refresh,
    Launch(AppId),
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
    state: AppsState,
}

impl AppsHolder {
    pub fn new(apps: Arc<dyn AppsAdapter>, layout: Arc<dyn LayoutStore>) -> Self {
        Self {
            apps,
            layout,
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
        self.state.items = grid(&layout, &installed);
    }

    fn launch(&mut self, app: &AppId) {
        self.state.problem = self.apps.launch(app).err().map(|error| error.to_string());
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
            AppsIntent::Launch(app) => self.launch(&app),
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
    use meegone_core_data::fakes::{FakeStorage, Fakes, app, app_id};

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

    #[test]
    fn launching_goes_through_the_apps_adapter() {
        // APPS-10; the side effect runs in the data layer (ARCH-05).
        let fakes = Fakes::default();
        install(&fakes, &[("mail", "Mail")]);
        let mut holder = holder(&fakes, &layout_store());

        holder.handle(AppsIntent::Launch(app_id("mail")));

        assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
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
