//! Settings → Applications → Hidden apps (APPS-19, SET-02; ADR 0017).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use meegone_apps_state::{AppsIntent, AppsState};
use meegone_core_state::App;
use meegone_core_ui::{Dispatch, Latest, Slot, Tokens};

/// Settings → Applications → Hidden apps (APPS-19, SET-02): the apps the user hid, each with
/// Unhide. Applications provides it, and the shell puts it into Settings (ADR 0002).
///
/// The host draws it as it draws the grid (ADR 0010). [`HiddenAppsSection::revision`] changes
/// whenever the list does, the host then reads [`HiddenAppsSection::apps`], and it names an app
/// by its place in them. A tap on a list that changed meanwhile does nothing.
///
/// Clones share what the host was last told, so the shell can keep one for the host while
/// Settings holds another.
#[derive(Clone)]
pub struct HiddenAppsSection {
    latest: Latest<AppsState>,
    dispatch: Dispatch<AppsIntent>,
    told: Rc<RefCell<Told>>,
}

/// The list the host was last told about, and its revision.
struct Told {
    apps: Arc<[App]>,
    revision: u32,
}

impl HiddenAppsSection {
    pub fn new(latest: Latest<AppsState>, dispatch: Dispatch<AppsIntent>) -> Self {
        Self {
            latest,
            dispatch,
            told: Rc::new(RefCell::new(Told {
                apps: Arc::new([]),
                revision: 0,
            })),
        }
    }

    /// The list's revision, which changes whenever the hidden apps do, so the host reads
    /// [`HiddenAppsSection::apps`] again.
    pub fn revision(&self) -> u32 {
        let hidden = Arc::clone(&self.latest.get().hidden_apps);
        let mut told = self.told.borrow_mut();
        if !Arc::ptr_eq(&told.apps, &hidden) {
            told.apps = hidden;
            told.revision = told.revision.wrapping_add(1);
        }
        told.revision
    }

    /// The hidden apps, in the locale's alphabetical order, as of the last
    /// [`HiddenAppsSection::revision`].
    pub fn apps(&self) -> Arc<[App]> {
        Arc::clone(&self.told.borrow().apps)
    }

    /// Unhide was tapped on the app at `index` of the list at `revision`: it shows in the grid
    /// again, in its old place (APPS-19). Returns whether that was asked; a list that changed
    /// meanwhile, or a place that isn't in it, asks nothing.
    pub fn unhide_tapped(&self, revision: u32, index: usize) -> bool {
        let app = {
            let told = self.told.borrow();
            match told.apps.get(index) {
                Some(app) if told.revision == revision => app.id.clone(),
                _ => return false,
            }
        };
        self.dispatch.send(AppsIntent::Unhide(app));
        true
    }
}

impl Slot for HiddenAppsSection {
    fn render(&mut self, _tokens: &Tokens) {
        // The host draws the list from `revision` and `apps` until the Settings screen, #34,
        // draws the rest (ADR 0006, ADR 0017).
    }
}
