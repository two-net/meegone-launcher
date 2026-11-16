//! Events view (SRS 4.3). It renders [`EventsState`], turns input into [`EventsIntent`]s and
//! shows below the notifications any cards the shell adds, such as the image of the day
//! (EVT-12, ADR 0002).
//!
//! The host draws the header from what [`EventsView::header`] last told it, and reads it again
//! whenever the revision from [`EventsView::show_header`] changes (ADR 0019).

use meegone_core_ui::{Dispatch, Slot, Tokens, View};
pub use meegone_events_state::EventsHeader;
use meegone_events_state::{EventsIntent, EventsState};

pub struct EventsView {
    dispatch: Dispatch<EventsIntent>,
    cards: Vec<Box<dyn Slot>>,
    /// The header the host was last told about, and its revision.
    header: EventsHeader,
    header_revision: u32,
}

impl EventsView {
    pub fn new(dispatch: Dispatch<EventsIntent>) -> Self {
        Self {
            dispatch,
            cards: Vec::new(),
            header: EventsHeader::default(),
            header_revision: 0,
        }
    }

    /// Takes on `state`'s header, and returns its revision, which changes only when the header
    /// does (EVT-01, EVT-02). Called for every frame, whichever view shows, so the header is
    /// current as Events slides in.
    pub fn show_header(&mut self, state: &EventsState) -> u32 {
        if state.header != self.header {
            self.header = state.header.clone();
            self.header_revision = self.header_revision.wrapping_add(1);
        }
        self.header_revision
    }

    /// The header as of the last [`EventsView::show_header`].
    pub fn header(&self) -> &EventsHeader {
        &self.header
    }

    /// The user tapped the alarm row (EVT-02).
    pub fn alarm_tapped(&self) {
        self.dispatch.send(EventsIntent::OpenAlarm);
    }

    /// Adds a card below the notifications. Only the shell calls this (ARCH-03).
    pub fn add_card(&mut self, card: Box<dyn Slot>) {
        self.cards.push(card);
    }

    /// The user tapped Allow on the notification-access card (EVT-03).
    pub fn allow_access_tapped(&self) {
        self.dispatch.send(EventsIntent::OpenAccessSettings);
    }
}

impl View for EventsView {
    type State = EventsState;

    fn render(&mut self, _state: &EventsState, tokens: &Tokens) {
        // The host draws the header (ADR 0019); the notifications arrive with #19.
        for card in &mut self.cards {
            card.render(tokens);
        }
    }
}
