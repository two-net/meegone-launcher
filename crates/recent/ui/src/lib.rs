//! Recent view (SRS 4.4). It renders [`RecentState`] and turns input into [`RecentIntent`]s.
//!
//! The host draws what Recent shows from what [`RecentView::show_cards`] last told it: the
//! explainer card, "No recent apps" or the cards (ADR 0023). It reads the cards again whenever
//! that revision changes, and names a tapped card or ✕ by the revision and its place, so a card
//! that changed meanwhile opens or removes nothing. Each card says where it was in the cards the
//! host last read, so a card that went shrinks away where it was and the others close the gap
//! (ANIM-18, ADR 0025).

use std::cell::RefCell;
use std::collections::HashMap;

use meegone_core_state::{Access, AppId, RecentDensity, Rect};
use meegone_core_ui::{Dispatch, Rgb, Tokens, View};
pub use meegone_recent_state::Age;
use meegone_recent_state::{RecentCard, RecentIntent, RecentState, phone_columns};

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
    /// Where the card was in the cards the host last read, if it keeps its view; `None` for a
    /// card that comes in. A card the host had that no card keeps has gone (ANIM-18).
    pub from: Option<usize>,
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
    /// The apps of the cards the host last read. Two revisions can come between its reads, so
    /// each card says where it was in these, not in the revision before.
    read: RefCell<Vec<AppId>>,
    revision: u32,
}

pub struct RecentView {
    dispatch: Dispatch<RecentIntent>,
    told: Told,
    columns: u8,
    density: RecentDensity,
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
                read: RefCell::new(Vec::new()),
                revision: 0,
            },
            columns: phone_columns(RecentDensity::default()),
            density: RecentDensity::default(),
        }
    }

    /// Takes on what `state` shows in the theme of `tokens`, and returns its revision, which
    /// changes only when the mode or the cards do. Called for every frame, whichever view
    /// shows, so Recent is current as it slides in; the cards are made again only when the
    /// access, the cards or the theme change.
    pub fn show_cards(&mut self, state: &RecentState, tokens: &Tokens) -> u32 {
        self.columns = state.columns();
        self.density = state.density;
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
            RecentMode::Cards => {
                let read = told.read.borrow();
                let places: HashMap<&AppId, usize> =
                    read.iter().enumerate().map(|(at, app)| (app, at)).collect();
                state
                    .cards
                    .iter()
                    .map(|card| shown(places.get(&card.app.id).copied(), card, tokens))
                    .collect()
            }
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
    /// right and top to bottom (REC-03). None unless the mode is [`RecentMode::Cards`]. The host
    /// reads them when the revision changes; each card of a later revision says where it was in
    /// these.
    pub fn cards(&self) -> &[ShownCard] {
        self.told.read.replace(
            self.told
                .cards
                .iter()
                .map(|card| card.app.clone())
                .collect(),
        );
        &self.told.cards
    }

    /// How many cards each row holds: three, or two after a pinch, and more of each on a large
    /// screen (REC-05).
    pub fn columns(&self) -> u8 {
        self.columns
    }

    /// The density as of the last [`RecentView::show_cards`]: more cards or bigger ones.
    pub fn density(&self) -> RecentDensity {
        self.density
    }

    /// How many rows of cards one phone screen shows: three, or two after a pinch (REC-05).
    pub fn rows_per_screen(&self) -> u8 {
        phone_columns(self.density)
    }

    /// The host laid the cards out `width_px` wide inside their margins, at `px_per_dp`, so
    /// Recent picks the columns (REC-05). Says nothing while the size isn't known yet.
    pub fn laid_out(&self, width_px: f64, px_per_dp: f64) {
        if width_px > 0.0 && px_per_dp > 0.0 {
            self.dispatch.send(RecentIntent::Layout {
                width_dp: width_px / px_per_dp,
            });
        }
    }

    /// More cards or bigger ones were chosen without a pinch, from a screen reader (A11Y-05).
    pub fn density_chosen(&self, density: RecentDensity) {
        self.dispatch.send(RecentIntent::SetDensity(density));
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

    /// A screen reader asked for close mode, as a long-press on the grid enters it (REC-07,
    /// A11Y-05).
    pub fn close_mode_requested(&self) {
        self.dispatch.send(RecentIntent::EnterCloseMode);
    }

    /// A tap on empty space, in close mode, leaves it (REC-07).
    pub fn empty_tapped(&self) {
        self.dispatch.send(RecentIntent::ExitCloseMode);
    }

    /// The ✕ of card `index` of the cards at `revision` was tapped, or a screen reader chose
    /// "Remove ‹app› from Recent": the card goes from Recent, and the app keeps running
    /// (REC-08). Returns whether it does anything: not when the cards changed since the host
    /// drew them.
    pub fn badge_tapped(&self, revision: u32, index: usize) -> bool {
        if revision != self.told.revision {
            return false;
        }
        let Some(card) = self.told.cards.get(index) else {
            return false;
        };
        self.dispatch.send(RecentIntent::Remove(card.app.clone()));
        true
    }

    /// Clear all removes every card from Recent; no app is stopped (REC-08, AC-14).
    pub fn clear_all_tapped(&self) {
        self.dispatch.send(RecentIntent::ClearAll);
    }
}

fn shown(from: Option<usize>, card: &RecentCard, tokens: &Tokens) -> ShownCard {
    ShownCard {
        from,
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
