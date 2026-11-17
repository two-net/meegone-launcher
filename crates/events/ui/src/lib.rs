//! Events view (SRS 4.3). It renders [`EventsState`], turns input into [`EventsIntent`]s and
//! shows below the notifications any cards the shell adds, such as the image of the day
//! (EVT-12, ADR 0002).
//!
//! The host draws the header from what [`EventsView::header`] last told it, and reads it again
//! whenever the revision from [`EventsView::show_header`] changes (ADR 0019). It draws what
//! shows below the header the same way: the explainer card, "No new events" or the app groups,
//! from [`EventsView::show_list`] and [`EventsView::rows`] (ADR 0020).

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use meegone_core_ui::{Dispatch, Rgb, Slot, Tokens, View};
pub use meegone_events_state::EventsHeader;
use meegone_events_state::{
    Access, EventsIntent, EventsState, GroupKey, NotificationGroup, ProfileId, rows_kept,
};

/// What shows below the Events header (EVT-03, EVT-04, EVT-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventsListMode {
    /// Without notification access: a card says what Events reads and why, with Allow (EVT-03,
    /// ONB-02).
    #[default]
    Explainer,
    /// Nothing to show: "No new events" (EVT-11).
    Empty,
    /// The "Notifications" section and its app groups (EVT-04).
    List,
}

/// One app's group as the host draws it (EVT-04, EVT-09). Its `Debug` output leaves out what
/// the notification says (EVT-13).
#[derive(Clone, PartialEq, Eq)]
pub struct EventsRow {
    /// Where the row was in the rows the host last read, if it keeps its view; `None` for a row
    /// that comes in. A row the host had that no row keeps has gone (ANIM-20).
    pub from: Option<usize>,
    pub profile: ProfileId,
    pub package: String,
    /// The app's launcher activity, for its icon in the grid; `None` for an app with no icon
    /// there, which shows the system's icon for it.
    pub activity: Option<String>,
    /// The app's name.
    pub label: String,
    /// In a work profile, so screen readers say so (APPS-20, A11Y-01).
    pub work: bool,
    /// The marker's colour: the app's accent as this theme draws it, at 3:1 or more against the
    /// background, or the secondary text colour for an app with no icon in the grid (EVT-09,
    /// THEME-07).
    pub accent: Rgb,
    /// How many notifications the group holds.
    pub count: usize,
    /// The newest notification's title and text, which the host shows in at most two lines.
    pub title: String,
    pub text: String,
}

impl fmt::Debug for EventsRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventsRow")
            .field("from", &self.from)
            .field("profile", &self.profile)
            .field("package", &self.package)
            .field("activity", &self.activity)
            .field("label", &self.label)
            .field("work", &self.work)
            .field("accent", &self.accent)
            .field("count", &self.count)
            .field("title", &"<redacted>")
            .field("text", &"<redacted>")
            .finish()
    }
}

/// What the host was last told about the list, and what it was made from.
struct Told {
    /// The groups, access and theme the rows were made from, so they are made again only when
    /// one of them changes.
    groups: Arc<[NotificationGroup]>,
    access: Access,
    tokens: Option<Tokens>,
    mode: EventsListMode,
    rows: Vec<EventsRow>,
    keys: Vec<GroupKey>,
    /// The keys of the rows the host last read. Two revisions can come between its reads, so
    /// each row says where it was in these, not in the revision before.
    read: RefCell<Vec<GroupKey>>,
    revision: u32,
}

pub struct EventsView {
    dispatch: Dispatch<EventsIntent>,
    cards: Vec<Box<dyn Slot>>,
    /// The header the host was last told about, and its revision.
    header: EventsHeader,
    header_revision: u32,
    told: Told,
}

impl EventsView {
    pub fn new(dispatch: Dispatch<EventsIntent>) -> Self {
        Self {
            dispatch,
            cards: Vec::new(),
            header: EventsHeader::default(),
            header_revision: 0,
            told: Told {
                groups: Arc::new([]),
                access: Access::Missing,
                tokens: None,
                mode: EventsListMode::Explainer,
                rows: Vec::new(),
                keys: Vec::new(),
                read: RefCell::new(Vec::new()),
                revision: 0,
            },
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

    /// Takes on what shows below `state`'s header in the theme of `tokens`, and returns its
    /// revision, which changes only when the mode or the rows do (EVT-04, EVT-10). Called for
    /// every frame, whichever view shows, like [`EventsView::show_header`]; the rows are made
    /// again only when the groups, the access or the theme change.
    pub fn show_list(&mut self, state: &EventsState, tokens: &Tokens) -> u32 {
        let told = &mut self.told;
        if Arc::ptr_eq(&told.groups, &state.groups)
            && told.access == state.access
            && told.tokens.as_ref() == Some(tokens)
        {
            return told.revision;
        }
        told.groups = Arc::clone(&state.groups);
        told.access = state.access;
        told.tokens = Some(*tokens);

        let (mode, groups): (_, &[NotificationGroup]) = match state.access {
            Access::Missing => (EventsListMode::Explainer, &[]),
            Access::Granted if state.groups.is_empty() => (EventsListMode::Empty, &[]),
            Access::Granted => (EventsListMode::List, &state.groups),
        };
        let keys: Vec<GroupKey> = groups.iter().map(NotificationGroup::key).collect();
        let rows: Vec<EventsRow> = rows_kept(&told.read.borrow(), &keys)
            .into_iter()
            .zip(groups)
            .map(|(from, group)| row(from, group, tokens))
            .collect();
        if mode != told.mode || rows != told.rows {
            told.revision = told.revision.wrapping_add(1);
        }
        told.mode = mode;
        told.rows = rows;
        told.keys = keys;
        told.revision
    }

    /// What shows below the header as of the last [`EventsView::show_list`].
    pub fn mode(&self) -> EventsListMode {
        self.told.mode
    }

    /// The app groups, newest first, as of the last [`EventsView::show_list`]. None unless the
    /// mode is [`EventsListMode::List`]. The host reads them when the revision changes; each row
    /// of a later revision says where it was in these.
    pub fn rows(&self) -> &[EventsRow] {
        self.told.read.replace(self.told.keys.clone());
        &self.told.rows
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

/// `group`'s row, which was at `from` in the rows the host last read.
fn row(from: Option<usize>, group: &NotificationGroup, tokens: &Tokens) -> EventsRow {
    let latest = group.latest();
    EventsRow {
        from,
        profile: group.profile,
        package: group.package.clone(),
        activity: group.app.as_ref().map(|app| app.id.activity.clone()),
        label: group.label.clone(),
        work: group.work,
        accent: group
            .app
            .as_ref()
            .map_or(tokens.text_secondary, |app| tokens.app_accent(app.accent)),
        count: group.count(),
        title: latest.title.clone(),
        text: latest.text.clone(),
    }
}

impl View for EventsView {
    type State = EventsState;

    fn render(&mut self, _state: &EventsState, tokens: &Tokens) {
        // The host draws the header and the list (ADR 0019, ADR 0020).
        for card in &mut self.cards {
            card.render(tokens);
        }
    }
}

#[cfg(test)]
mod tests;
