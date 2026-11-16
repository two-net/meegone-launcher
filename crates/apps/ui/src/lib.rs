//! Applications view (SRS 4.2). It renders [`AppsState`] and turns input into [`AppsIntent`]s.

use meegone_apps_state::{AppsIntent, AppsState, FolderId};
use meegone_core_state::AppId;
use meegone_core_ui::{Dispatch, Tokens, View};

pub struct AppsView {
    dispatch: Dispatch<AppsIntent>,
}

impl AppsView {
    pub fn new(dispatch: Dispatch<AppsIntent>) -> Self {
        Self { dispatch }
    }

    /// APPS-10
    pub fn icon_tapped(&self, app: AppId) {
        self.dispatch.send(AppsIntent::Launch(app));
    }

    /// Dragging after a long-press picks the icon up and enters edit mode (APPS-11).
    pub fn icon_dragged(&self) {
        self.dispatch.send(AppsIntent::EnterEditMode);
    }

    /// FOLD-04
    pub fn folder_tapped(&self, folder: FolderId) {
        self.dispatch.send(AppsIntent::OpenFolder(folder));
    }
}

impl View for AppsView {
    type State = AppsState;

    fn render(&mut self, _state: &AppsState, _tokens: &Tokens) {
        // Drawing the grid arrives with #9, in Android's views (ADR 0006).
    }
}
