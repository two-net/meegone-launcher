//! Recent view (SRS 4.4). It renders [`RecentState`] and turns input into [`RecentIntent`]s.

use meegone_core_state::AppId;
use meegone_core_ui::{Dispatch, Tokens, View};
use meegone_recent_state::{RecentIntent, RecentState};

pub struct RecentView {
    dispatch: Dispatch<RecentIntent>,
}

impl RecentView {
    pub fn new(dispatch: Dispatch<RecentIntent>) -> Self {
        Self { dispatch }
    }

    /// The user tapped Allow on the usage-access card (REC-02).
    pub fn allow_access_tapped(&self) {
        self.dispatch.send(RecentIntent::OpenAccessSettings);
    }

    /// A long-press anywhere on the grid enters close mode (REC-07).
    pub fn grid_long_pressed(&self) {
        self.dispatch.send(RecentIntent::EnterCloseMode);
    }

    /// REC-08
    pub fn remove_tapped(&self, app: AppId) {
        self.dispatch.send(RecentIntent::Remove(app));
    }
}

impl View for RecentView {
    type State = RecentState;

    fn render(&mut self, _state: &RecentState, _tokens: &Tokens) {
        // Drawing the cards arrives with #22, in Android's views (ADR 0006).
    }
}
