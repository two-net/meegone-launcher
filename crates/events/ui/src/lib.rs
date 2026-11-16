//! Events view (SRS 4.3). It renders [`EventsState`], turns input into [`EventsIntent`]s and
//! shows below the notifications any cards the shell adds, such as the image of the day
//! (EVT-12, ADR 0002).

use meegone_core_ui::{Dispatch, Slot, Tokens, View};
use meegone_events_state::{EventsIntent, EventsState};

pub struct EventsView {
    dispatch: Dispatch<EventsIntent>,
    cards: Vec<Box<dyn Slot>>,
}

impl EventsView {
    pub fn new(dispatch: Dispatch<EventsIntent>) -> Self {
        Self {
            dispatch,
            cards: Vec::new(),
        }
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
        // Drawing the header and notifications arrives with #18 and #19 (ADR 0006).
        for card in &mut self.cards {
            card.render(tokens);
        }
    }
}
