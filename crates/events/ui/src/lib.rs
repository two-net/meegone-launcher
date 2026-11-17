//! Events view (SRS 4.3). It renders [`EventsState`], turns input into [`EventsIntent`]s and
//! shows below the notifications any cards the shell adds, such as the image of the day
//! (EVT-12, ADR 0002).
//!
//! The host draws the header from what [`EventsView::header`] last told it, and reads it again
//! whenever the revision from [`EventsView::show_header`] changes (ADR 0019). It draws what
//! shows below the header the same way: the explainer card, "No new events" or the rows, from
//! [`EventsView::show_list`] and [`EventsView::rows`] (ADR 0020). It names a tapped or
//! long-pressed row by that revision and its place, so a row that changed meanwhile does
//! nothing (ADR 0021).

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use meegone_core_ui::{Dispatch, Rgb, Slot, Tokens, View};
pub use meegone_events_state::EventsHeader;
use meegone_events_state::{
    Access, EventsIntent, EventsState, GroupId, ListRow, NotificationGroup, ProfileId, RowKey,
    rows_kept,
};
pub use meegone_events_state::{NotificationMenuItem, Rect, RowTarget};

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

/// One row as the host draws it: an app's group (EVT-04, EVT-09), or one notification of the
/// expanded group, below its group's row (EVT-06). Its `Debug` output leaves out what the
/// notification says (EVT-13).
#[derive(Clone, PartialEq, Eq)]
pub struct EventsRow {
    /// Where the row was in the rows the host last read, if it keeps its view; `None` for a row
    /// that comes in. A row the host had that no row keeps has gone (ANIM-20).
    pub from: Option<usize>,
    /// One notification of the expanded group, rather than a group.
    pub notification: bool,
    /// A group's row while it shows each of its notifications below it. It shows only the
    /// app's name and count then.
    pub expanded: bool,
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
    /// How many notifications the group holds; 1 for a notification's row.
    pub count: usize,
    /// What the row says, which the host shows in at most two lines: a group's newest
    /// notification's title and text, empty while it is expanded, or the notification's own.
    pub title: String,
    pub text: String,
}

impl fmt::Debug for EventsRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventsRow")
            .field("from", &self.from)
            .field("notification", &self.notification)
            .field("expanded", &self.expanded)
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

/// A notification's or a group's long-press menu as the host draws it (EVT-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventsMenuFrame {
    /// It offers Dismiss first, as something it is for can be cleared. Notification settings
    /// is always offered.
    pub dismiss: bool,
    /// The row it opens at, in the rows the host last read: the notification's, or else its
    /// group's. `None` centres it.
    pub row: Option<usize>,
}

/// What the host was last told about the list, and what it was made from.
struct Told {
    /// The groups, expanded group, access and theme the rows were made from, so they are made
    /// again only when one of them changes.
    groups: Arc<[NotificationGroup]>,
    expanded: Option<GroupId>,
    access: Access,
    tokens: Option<Tokens>,
    mode: EventsListMode,
    rows: Vec<EventsRow>,
    keys: Vec<RowKey>,
    /// What each row stands for, which a tap or a long-press on it acts on.
    targets: Vec<RowTarget>,
    /// Something shown can be cleared, so the section header offers Clear (EVT-07).
    clearable: bool,
    /// The keys of the rows the host last read. Two revisions can come between its reads, so
    /// each row says where it was in these, not in the revision before.
    read: RefCell<Vec<RowKey>>,
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
                expanded: None,
                access: Access::Missing,
                tokens: None,
                mode: EventsListMode::Explainer,
                rows: Vec::new(),
                keys: Vec::new(),
                targets: Vec::new(),
                clearable: false,
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
    /// revision, which changes only when the mode or the rows do (EVT-04, EVT-06, EVT-10).
    /// Called for every frame, whichever view shows, like [`EventsView::show_header`]; the rows
    /// are made again only when the groups, the expanded group, the access or the theme change.
    pub fn show_list(&mut self, state: &EventsState, tokens: &Tokens) -> u32 {
        let told = &mut self.told;
        if Arc::ptr_eq(&told.groups, &state.groups)
            && told.expanded == state.expanded
            && told.access == state.access
            && told.tokens.as_ref() == Some(tokens)
        {
            return told.revision;
        }
        told.groups = Arc::clone(&state.groups);
        told.expanded = state.expanded.clone();
        told.access = state.access;
        told.tokens = Some(*tokens);

        let (mode, list) = match state.access {
            Access::Missing => (EventsListMode::Explainer, Vec::new()),
            Access::Granted if state.groups.is_empty() => (EventsListMode::Empty, Vec::new()),
            Access::Granted => (EventsListMode::List, state.rows()),
        };
        let keys: Vec<RowKey> = list.iter().map(ListRow::key).collect();
        let rows: Vec<EventsRow> = rows_kept(&told.read.borrow(), &keys)
            .into_iter()
            .zip(&list)
            .map(|(from, list_row)| row(from, list_row, tokens))
            .collect();
        if mode != told.mode || rows != told.rows {
            told.revision = told.revision.wrapping_add(1);
        }
        told.mode = mode;
        told.rows = rows;
        told.keys = keys;
        told.targets = list.iter().map(ListRow::target).collect();
        told.clearable = mode == EventsListMode::List && state.clearable();
        told.revision
    }

    /// What shows below the header as of the last [`EventsView::show_list`].
    pub fn mode(&self) -> EventsListMode {
        self.told.mode
    }

    /// The rows, as of the last [`EventsView::show_list`]: each group, newest first, and below
    /// the expanded group's row, each of its notifications. None unless the mode is
    /// [`EventsListMode::List`]. The host reads them when the revision changes; each row of a
    /// later revision says where it was in these.
    pub fn rows(&self) -> &[EventsRow] {
        self.told.read.replace(self.told.keys.clone());
        &self.told.rows
    }

    /// Whether the section header offers Clear, as of the last [`EventsView::show_list`]:
    /// something shown can be cleared (EVT-07).
    pub fn clearable(&self) -> bool {
        self.told.clearable
    }

    /// `state`'s menu as the host draws it, at its row as of the last
    /// [`EventsView::show_list`], or `None` while it is closed (EVT-08).
    pub fn menu(&self, state: &EventsState) -> Option<EventsMenuFrame> {
        let menu = state.menu.as_ref()?;
        let at = |target: &RowTarget| self.told.targets.iter().position(|t| t == target);
        let group = RowTarget {
            notification: None,
            ..menu.target.clone()
        };
        Some(EventsMenuFrame {
            dismiss: menu.offers(NotificationMenuItem::Dismiss),
            row: at(&menu.target).or_else(|| at(&group)),
        })
    }

    /// What row `index` of the rows at `revision` stands for, if they are still the rows of
    /// the last [`EventsView::show_list`].
    pub fn row_target(&self, revision: u32, index: usize) -> Option<RowTarget> {
        if revision != self.told.revision {
            return None;
        }
        self.told.targets.get(index).cloned()
    }

    /// Row `index` of the rows at `revision` was tapped, and is at `from` on the screen
    /// (EVT-06). Returns whether it does anything: not when the rows changed since the host
    /// drew them.
    pub fn row_tapped(&self, revision: u32, index: usize, from: Rect) -> bool {
        let Some(row) = self.row_target(revision, index) else {
            return false;
        };
        let from = (!from.is_empty()).then_some(from);
        self.dispatch.send(EventsIntent::Tap { row, from });
        true
    }

    /// An item of the open menu was chosen (EVT-08). One it doesn't offer does nothing.
    pub fn menu_item_chosen(&self, item: NotificationMenuItem) {
        self.dispatch.send(EventsIntent::ChooseMenuItem(item));
    }

    /// The user tapped Clear on the section header (EVT-07).
    pub fn clear_tapped(&self) {
        self.dispatch.send(EventsIntent::Clear);
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

/// `list_row`'s row, which was at `from` in the rows the host last read.
fn row(from: Option<usize>, list_row: &ListRow<'_>, tokens: &Tokens) -> EventsRow {
    let group = list_row.group;
    // A group's row says what its newest notification says, unless its notifications show
    // below it.
    let (count, said) = match list_row.notification {
        Some(notification) => (1, Some(notification)),
        None if list_row.expanded => (group.count(), None),
        None => (group.count(), Some(group.latest())),
    };
    EventsRow {
        from,
        notification: list_row.notification.is_some(),
        expanded: list_row.expanded,
        profile: group.profile,
        package: group.package.clone(),
        activity: group.app.as_ref().map(|app| app.id.activity.clone()),
        label: group.label.clone(),
        work: group.work,
        accent: group
            .app
            .as_ref()
            .map_or(tokens.text_secondary, |app| tokens.app_accent(app.accent)),
        count,
        title: said.map(|n| n.title.clone()).unwrap_or_default(),
        text: said.map(|n| n.text.clone()).unwrap_or_default(),
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
