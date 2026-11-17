//! Recent view (SRS 4.4). It renders [`RecentState`] and turns input into [`RecentIntent`]s.
//!
//! The host draws what Recent shows from what [`RecentView::show_cards`] last told it: the
//! explainer card, "No recent apps" or the cards (ADR 0023). It reads the cards again whenever
//! that revision changes, and names a tapped card by the revision and its place, so a card that
//! changed meanwhile opens nothing.

use meegone_core_state::{Access, AppId, RecentDensity, Rect};
use meegone_core_ui::{Dispatch, Rgb, Tokens, View};
pub use meegone_recent_state::Age;
use meegone_recent_state::{RecentCard, RecentIntent, RecentState};

/// What Recent shows below its title (REC-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecentMode {
    /// Without usage access: a card says what Recent reads and why, with Allow. No placeholder
    /// cards show (REC-02, ONB-02).
    #[default]
    Explainer,
    /// With access, but no app used in the window: "No recent apps" (REC-10).
    Empty,
    /// The cards, newest top-left (REC-03).
    Cards,
}

/// A card as the host draws it (REC-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownCard {
    /// Which app, for its icon and for opening it.
    pub app: AppId,
    pub label: String,
    /// The app is in the work profile, which screen readers say (APPS-20).
    pub work: bool,
    /// False while the app can't open, which the host shows as it does in Applications
    /// (APPS-09).
    pub available: bool,
    /// The app's accent, at 3:1 or more against the background (APPS-17, THEME-07).
    pub fill: Rgb,
    /// How long ago the app was last used.
    pub age: Age,
}

/// What the host was last told, and what it was made from.
struct Told {
    /// The access, cards and theme the cards were made from, so they are made again only when
    /// one of them changes.
    access: Access,
    from: Vec<RecentCard>,
    tokens: Option<Tokens>,
    mode: RecentMode,
    cards: Vec<ShownCard>,
    revision: u32,
}

pub struct RecentView {
    dispatch: Dispatch<RecentIntent>,
    told: Told,
    columns: u8,
}

impl RecentView {
    pub fn new(dispatch: Dispatch<RecentIntent>) -> Self {
        Self {
            dispatch,
            told: Told {
                access: Access::Missing,
                from: Vec::new(),
                tokens: None,
                mode: RecentMode::Explainer,
                cards: Vec::new(),
                revision: 0,
            },
            columns: columns(RecentDensity::default()),
        }
    }

    /// Takes on what `state` shows in the theme of `tokens`, and returns its revision, which
    /// changes only when the mode or the cards do. Called for every frame, whichever view
    /// shows, so Recent is current as it slides in; the cards are made again only when the
    /// access, the cards or the theme change.
    pub fn show_cards(&mut self, state: &RecentState, tokens: &Tokens) -> u32 {
        self.columns = columns(state.density);
        let told = &mut self.told;
        if told.access == state.access
            && told.from == state.cards
            && told.tokens.as_ref() == Some(tokens)
        {
            return told.revision;
        }
        told.access = state.access;
        told.from.clone_from(&state.cards);
        told.tokens = Some(*tokens);

        let mode = match state.access {
            Access::Missing => RecentMode::Explainer,
            Access::Granted if state.cards.is_empty() => RecentMode::Empty,
            Access::Granted => RecentMode::Cards,
        };
        let cards: Vec<ShownCard> = match mode {
            RecentMode::Cards => state.cards.iter().map(|card| shown(card, tokens)).collect(),
            RecentMode::Explainer | RecentMode::Empty => Vec::new(),
        };
        if mode != told.mode || cards != told.cards {
            told.revision = told.revision.wrapping_add(1);
        }
        told.mode = mode;
        told.cards = cards;
        told.revision
    }

    /// What shows as of the last [`RecentView::show_cards`].
    pub fn mode(&self) -> RecentMode {
        self.told.mode
    }

    /// The cards as of the last [`RecentView::show_cards`], newest first, filling rows left to
    /// right and top to bottom (REC-03). None unless the mode is [`RecentMode::Cards`].
    pub fn cards(&self) -> &[ShownCard] {
        &self.told.cards
    }

    /// How many cards each row holds: three, or two after a pinch (REC-05).
    pub fn columns(&self) -> u8 {
        self.columns
    }

    /// Card `index` of the cards at `revision` was tapped, and is at `from` on the screen: its
    /// app comes to the front, growing from the card (REC-06, ANIM-08). Returns whether it does
    /// anything: not when the cards changed since the host drew them.
    pub fn card_tapped(&self, revision: u32, index: usize, from: Rect) -> bool {
        if revision != self.told.revision {
            return false;
        }
        let Some(card) = self.told.cards.get(index) else {
            return false;
        };
        let from = (!from.is_empty()).then_some(from);
        self.dispatch.send(RecentIntent::Launch {
            app: card.app.clone(),
            from,
        });
        true
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

fn columns(density: RecentDensity) -> u8 {
    match density {
        RecentDensity::ThreeColumns => 3,
        RecentDensity::TwoColumns => 2,
    }
}

fn shown(card: &RecentCard, tokens: &Tokens) -> ShownCard {
    ShownCard {
        app: card.app.id.clone(),
        label: card.app.label.clone(),
        work: card.app.work,
        available: card.app.available,
        fill: tokens.app_accent(card.app.accent),
        age: card.age,
    }
}

impl View for RecentView {
    type State = RecentState;

    fn render(&mut self, _state: &RecentState, _tokens: &Tokens) {
        // The host draws the cards (ADR 0023).
    }
}

#[cfg(test)]
mod tests;
