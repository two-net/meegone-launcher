//! Events state (SRS 4.3): the header with the date and the next alarm, and live notifications
//! grouped by app, which the user opens, expands, dismisses and clears. Memory only (EVT-13).
//!
//! The platform writes dates and times in the device's language and 12- or 24-hour setting;
//! Events decides which to show (L10N-03, ADR 0019). Each group takes its app's label, accent
//! and icon from the app catalogue (APPS-17, ADR 0002, ADR 0020). Opening a notification,
//! removing one meant to disappear when opened, and what Dismiss and Clear take are decided
//! here, and carried out through the adapter (ADR 0021).

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use meegone_core_data::{AppsAdapter, NotificationsAdapter, SystemConfigAdapter, TimeStyle};
pub use meegone_core_state::{Access, App, AppId, Notification, ProfileId, Rect};
use meegone_core_state::{Problem, StateHolder};

/// An alarm that rings within this long shows only its time; a later one also shows its
/// weekday, so a time alone always means the next time the clock shows it (EVT-02).
pub const ALARM_TIME_ONLY_MS: u64 = 24 * 60 * 60 * 1000;

/// The top of Events: today's date, and the next alarm if one is set (EVT-01, EVT-02). It never
/// needs notification access (EVT-03).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventsHeader {
    /// Today's weekday, such as "Tuesday".
    pub weekday: String,
    /// Today's day and month, such as "17 November".
    pub date: String,
    /// When the next alarm rings, such as "07:00", or "Wed 07:00" when it is more than a day
    /// away. `None` hides the alarm row.
    pub alarm: Option<String>,
}

/// Which app's group: its profile and package (APPS-20), whatever notifications it holds. A
/// group keeps it while its row comes and goes, so it names the group that is expanded or that
/// a menu is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GroupId {
    pub profile: ProfileId,
    pub package: String,
}

/// One app's notifications (EVT-04). Its `Debug` output, like each [`Notification`]'s, leaves
/// out what they say (EVT-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationGroup {
    /// The app that posted them: its profile and package (APPS-20).
    pub profile: ProfileId,
    pub package: String,
    /// The app's name: its label in the grid, or the system's name for an app with no icon
    /// there, such as Android System.
    pub label: String,
    /// The app in the grid, whose icon and accent the group shows (APPS-17). `None` for an app
    /// with no icon there: the system's icon for it shows, with a neutral marker (ADR 0020).
    pub app: Option<App>,
    /// In a work profile, so screen readers say so (APPS-20, A11Y-01).
    pub work: bool,
    /// Newest first, and never empty. A summary the app posts for a bundle of them is left out
    /// while any of the bundle shows.
    pub notifications: Vec<Notification>,
    /// The keys of the clearable summaries left out while their bundle shows. They go with the
    /// last of what the group shows, so a summary never shows alone after MeeGone removed the
    /// rest (ADR 0021).
    pub hidden_summaries: Vec<String>,
}

impl NotificationGroup {
    /// The newest notification, whose title and text the group shows (EVT-04).
    pub fn latest(&self) -> &Notification {
        &self.notifications[0]
    }

    /// How many notifications the group holds (EVT-04).
    pub fn count(&self) -> usize {
        self.notifications.len()
    }

    /// Which app's group it is.
    pub fn id(&self) -> GroupId {
        GroupId {
            profile: self.profile,
            package: self.package.clone(),
        }
    }

    /// What tells this group's row apart from the one it had before (ANIM-20).
    pub fn key(&self) -> GroupKey {
        let latest = self.latest();
        GroupKey {
            profile: self.profile,
            package: self.package.clone(),
            latest: latest.key.clone(),
            posted_at_ms: latest.posted_at_ms,
        }
    }

    /// The notification with `key`, if the group shows it.
    pub fn notification(&self, key: &str) -> Option<&Notification> {
        self.notifications.iter().find(|n| n.key == key)
    }

    fn is(&self, id: &GroupId) -> bool {
        self.profile == id.profile && self.package == id.package
    }
}

/// A group's row as the list shows it: the app, and its newest notification. A row keeps its
/// place while its key stays the same; a newer notification makes it a new row at the top
/// (ANIM-20).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GroupKey {
    pub profile: ProfileId,
    pub package: String,
    /// The newest notification's key, and when it was posted, as an app that posts again with
    /// the same key moves its group to the top.
    pub latest: String,
    pub posted_at_ms: u64,
}

/// One row of the Events list (EVT-04, EVT-06): a group's, or one notification's below the row
/// of the expanded group.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowKey {
    Group(GroupKey),
    /// A notification of the expanded group. Its group's key is part of it, so it comes and goes
    /// with its group's row, and the rows that stay keep their order.
    Notification {
        group: GroupKey,
        key: String,
    },
}

/// What a row stands for: a whole group, or one notification of the expanded group. A tap or a
/// menu acts on it (EVT-06, EVT-08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowTarget {
    pub group: GroupId,
    /// The notification's key, or `None` for the whole group.
    pub notification: Option<String>,
}

/// One row of the list as [`EventsState::rows`] makes it: the group, and for a notification's
/// row, the notification.
#[derive(Debug, Clone, Copy)]
pub struct ListRow<'a> {
    pub group: &'a NotificationGroup,
    /// `None` for the group's own row.
    pub notification: Option<&'a Notification>,
    /// The group's row while it is expanded.
    pub expanded: bool,
}

impl ListRow<'_> {
    pub fn key(&self) -> RowKey {
        let group = self.group.key();
        match self.notification {
            Some(notification) => RowKey::Notification {
                group,
                key: notification.key.clone(),
            },
            None => RowKey::Group(group),
        }
    }

    pub fn target(&self) -> RowTarget {
        RowTarget {
            group: self.group.id(),
            notification: self.notification.map(|n| n.key.clone()),
        }
    }
}

/// Where each of `now`'s rows was in `before`, if it kept its row there; `None` for a row that
/// came in (ANIM-20). A row of `before` that no row of `now` keeps has gone.
///
/// The groups are in order of their newest notification, a kept group's newest notification is
/// the same as before, and a notification's row comes and goes with its group's, so kept rows
/// stay in the order they were in.
pub fn rows_kept(before: &[RowKey], now: &[RowKey]) -> Vec<Option<usize>> {
    let places: HashMap<&RowKey, usize> = before
        .iter()
        .enumerate()
        .map(|(at, key)| (key, at))
        .collect();
    now.iter().map(|key| places.get(key).copied()).collect()
}

/// Whether a row came in or went between `before` and `now`, so the list moves (ANIM-20).
pub fn rows_came_or_went(before: &[RowKey], now: &[RowKey]) -> bool {
    let kept = rows_kept(before, now);
    kept.iter().any(Option::is_none) || kept.len() != before.len()
}

/// One item of a notification's or a group's long-press menu (EVT-08), in the order it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationMenuItem {
    /// Removes the notification, or every clearable one of the group. Offered only if one is
    /// clearable.
    Dismiss,
    /// Opens the system's notification settings for the app.
    NotificationSettings,
}

/// A notification's or a group's long-press menu (EVT-08). It covers the home views while it is
/// open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationMenu {
    /// The row it is for. The host opens it there.
    pub target: RowTarget,
    /// What it offers, in order: Dismiss if something is clearable, then Notification settings.
    pub items: Vec<NotificationMenuItem>,
}

impl NotificationMenu {
    /// Whether the menu offers `item`.
    pub fn offers(&self, item: NotificationMenuItem) -> bool {
        self.items.contains(&item)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventsState {
    pub header: EventsHeader,
    /// Without notification access, Events shows its explainer card (EVT-03).
    pub access: Access,
    /// The group with the newest notification comes first (EVT-04). Shared, so the shell can
    /// tell cheaply when it changed.
    pub groups: Arc<[NotificationGroup]>,
    /// The group showing each of its notifications below its row (EVT-06). Only a group of
    /// several, and one at a time.
    pub expanded: Option<GroupId>,
    /// A notification's or a group's long-press menu, while it is open (EVT-08).
    pub menu: Option<NotificationMenu>,
    pub problem: Option<Problem>,
}

impl EventsState {
    /// The list's rows, in order: each group's, and below the expanded group's, one for each of
    /// its notifications (EVT-04, EVT-06).
    pub fn rows(&self) -> Vec<ListRow<'_>> {
        let mut rows = Vec::with_capacity(self.groups.len());
        for group in self.groups.iter() {
            let expanded = self.expanded.as_ref().is_some_and(|id| group.is(id));
            rows.push(ListRow {
                group,
                notification: None,
                expanded,
            });
            if expanded {
                rows.extend(group.notifications.iter().map(|notification| ListRow {
                    group,
                    notification: Some(notification),
                    expanded: false,
                }));
            }
        }
        rows
    }

    /// The keys of [`EventsState::rows`], to tell which rows came and went (ANIM-20).
    pub fn row_keys(&self) -> Vec<RowKey> {
        self.rows().iter().map(ListRow::key).collect()
    }

    /// Whether anything shown can be cleared, so Clear is offered (EVT-07).
    pub fn clearable(&self) -> bool {
        self.groups
            .iter()
            .any(|group| group.notifications.iter().any(|n| n.clearable))
    }

    /// The group `id` names, if it shows.
    pub fn group(&self, id: &GroupId) -> Option<&NotificationGroup> {
        self.groups.iter().find(|group| group.is(id))
    }
}

#[derive(Debug)]
pub enum EventsIntent {
    /// Reads the header and the notifications again.
    Refresh,
    /// The date, the time, the time zone, the 12- or 24-hour setting, the language or the next
    /// alarm may have changed (EVT-01, EVT-02, L10N-03).
    RefreshHeader,
    /// A notification was posted or removed, or notification access changed: reads the
    /// notifications again (EVT-10).
    RefreshNotifications,
    /// Apps were installed, removed or changed, or their accents were read: the groups take on
    /// their new labels, icons and accents (APPS-17).
    AppsChanged,
    /// The user tapped the alarm row (EVT-02).
    OpenAlarm,
    /// The user tapped Allow on the explainer card (EVT-03).
    OpenAccessSettings,
    /// The user tapped a row, which is at `from` on the screen (EVT-06). A group of one opens
    /// its notification; a group of several expands, or collapses if it is expanded; a
    /// notification of the expanded group opens. A notification meant to disappear when opened
    /// is then dismissed.
    Tap {
        row: RowTarget,
        from: Option<Rect>,
    },
    /// Collapses the expanded group, as when the user leaves Events.
    Collapse,
    /// Opens a row's long-press menu (EVT-08). The shell decides whether a long-press counts.
    OpenMenu(RowTarget),
    CloseMenu,
    /// Closes the menu and does what its item says (EVT-08). An item it doesn't offer does
    /// nothing.
    ChooseMenuItem(NotificationMenuItem),
    /// The user tapped Clear: every clearable notification shown goes (EVT-07). Ongoing ones
    /// were never shown, so they stay (EVT-05, AC-12).
    Clear,
}

pub struct EventsHolder {
    notifications: Arc<dyn NotificationsAdapter>,
    apps: Arc<dyn AppsAdapter>,
    system: Arc<dyn SystemConfigAdapter>,
    /// The app catalogue as last read, for each group's label, icon and accent. `None` until it
    /// is first needed, after a read failed, and after the apps changed.
    catalogue: Option<Vec<App>>,
    state: EventsState,
}

impl EventsHolder {
    pub fn new(
        notifications: Arc<dyn NotificationsAdapter>,
        apps: Arc<dyn AppsAdapter>,
        system: Arc<dyn SystemConfigAdapter>,
    ) -> Self {
        Self {
            notifications,
            apps,
            system,
            catalogue: None,
            state: EventsState::default(),
        }
    }

    fn refresh(&mut self) {
        self.refresh_header();
        self.refresh_notifications();
    }

    /// The groups read again with the new catalogue, if any show; otherwise the catalogue is
    /// read when a notification next needs it.
    fn apps_changed(&mut self) {
        self.catalogue = None;
        if !self.state.groups.is_empty() {
            self.refresh_notifications();
        }
    }

    fn refresh_header(&mut self) {
        let now = self.system.now_ms();
        // A time the platform couldn't write hides the row, as no alarm does.
        let alarm = self
            .system
            .next_alarm_ms()
            .map(|at| {
                let style = if at.saturating_sub(now) < ALARM_TIME_ONLY_MS {
                    TimeStyle::Time
                } else {
                    TimeStyle::WeekdayTime
                };
                self.system.format_time(at, style)
            })
            .filter(|time| !time.is_empty());
        self.state.header = EventsHeader {
            weekday: self.system.format_time(now, TimeStyle::Weekday),
            date: self.system.format_time(now, TimeStyle::DayMonth),
            alarm,
        };
    }

    /// Without access there is nothing to read, but the header stays (EVT-03).
    fn refresh_notifications(&mut self) {
        let access = self.notifications.access();
        self.state.access = access;
        if access == Access::Missing {
            self.set_groups(Vec::new());
            self.state.problem = None;
            return;
        }
        match self.notifications.active() {
            Ok(active) => {
                let catalogue = if active.is_empty() {
                    &[][..]
                } else {
                    self.catalogue()
                };
                let groups = group(active, catalogue);
                self.set_groups(groups);
                self.state.problem = None;
            }
            Err(error) => {
                self.set_groups(Vec::new());
                self.state.problem = Some(error.problem());
            }
        }
    }

    /// The catalogue, read if it isn't yet. If it can't be read, the groups show the system's
    /// names for their apps, and Events still works without Applications (REL-01).
    fn catalogue(&mut self) -> &[App] {
        if self.catalogue.is_none() {
            self.catalogue = self.apps.launchable_apps().ok();
        }
        self.catalogue.as_deref().unwrap_or_default()
    }

    /// Keeps the shared groups when nothing changed, so the shell sees no change. The expanded
    /// group and the menu follow the new groups.
    fn set_groups(&mut self, groups: Vec<NotificationGroup>) {
        if *self.state.groups != *groups {
            self.state.groups = groups.into();
        }
        // A group collapses once it holds fewer than two (EVT-06).
        let still_expandable =
            |id: &GroupId| self.state.group(id).is_some_and(|group| group.count() > 1);
        if !self.state.expanded.as_ref().is_none_or(still_expandable) {
            self.state.expanded = None;
        }
        // The menu closes when what it is for went, and offers Dismiss only while that can go.
        if let Some(target) = self.state.menu.as_ref().map(|menu| menu.target.clone()) {
            self.state.menu =
                menu_items(&self.state, &target).map(|items| NotificationMenu { target, items });
        }
    }

    /// Does nothing while no alarm shows, as there is no row to tap.
    fn open_alarm(&mut self) {
        if self.state.header.alarm.is_none() {
            return;
        }
        self.state.problem = self
            .system
            .open_next_alarm()
            .err()
            .map(|error| error.problem());
    }

    /// EVT-06: a row that no longer shows does nothing.
    fn tap(&mut self, row: &RowTarget, from: Option<Rect>) {
        self.state.problem = None;
        let groups = Arc::clone(&self.state.groups);
        let Some(group) = groups.iter().find(|group| group.is(&row.group)) else {
            return;
        };
        let notification = match &row.notification {
            Some(key) => group.notification(key),
            None if group.count() == 1 => Some(group.latest()),
            None => {
                // One group is expanded at a time.
                let expanded = self.state.expanded.take();
                if expanded.as_ref() != Some(&row.group) {
                    self.state.expanded = Some(row.group.clone());
                }
                return;
            }
        };
        if let Some(notification) = notification {
            self.open(group, notification, from);
        }
    }

    /// Opens `notification`'s content, and dismisses it if it is meant to disappear when opened
    /// (EVT-06), with the summaries hidden behind it if it was the last of its group.
    fn open(&mut self, group: &NotificationGroup, notification: &Notification, from: Option<Rect>) {
        if let Err(error) = self.notifications.open(&notification.key, from) {
            self.state.problem = Some(error.problem());
            return;
        }
        if notification.auto_cancel {
            self.dismiss(dismissal(group, |n| n.key == notification.key));
        }
    }

    /// Asks the system to remove `keys`, at once. Their rows go when it reports that they went
    /// (`RefreshNotifications`), so nothing comes back if it refused (ANIM-20).
    fn dismiss(&mut self, keys: Vec<String>) {
        if keys.is_empty() {
            return;
        }
        self.state.problem = self
            .notifications
            .dismiss(&keys)
            .err()
            .map(|error| error.problem());
    }

    /// EVT-08: a row that no longer shows has no menu.
    fn open_menu(&mut self, target: RowTarget) {
        if let Some(items) = menu_items(&self.state, &target) {
            self.state.menu = Some(NotificationMenu { target, items });
        }
    }

    fn choose(&mut self, item: NotificationMenuItem) {
        self.state.problem = None;
        let Some(menu) = self.state.menu.take_if(|menu| menu.offers(item)) else {
            return;
        };
        let target = menu.target;
        match item {
            NotificationMenuItem::Dismiss => {
                let groups = Arc::clone(&self.state.groups);
                if let Some(group) = groups.iter().find(|group| group.is(&target.group)) {
                    let only = target.notification.as_deref();
                    self.dismiss(dismissal(group, |n| only.is_none_or(|key| n.key == key)));
                }
            }
            NotificationMenuItem::NotificationSettings => {
                self.state.problem = self
                    .notifications
                    .open_app_settings(target.group.profile, &target.group.package)
                    .err()
                    .map(|error| error.problem());
            }
        }
    }

    /// EVT-07: everything clearable that shows, in one request.
    fn clear(&mut self) {
        self.state.problem = None;
        let groups = Arc::clone(&self.state.groups);
        let keys = groups
            .iter()
            .flat_map(|group| dismissal(group, |_| true))
            .collect();
        self.dismiss(keys);
    }
}

impl StateHolder for EventsHolder {
    type State = EventsState;
    type Intent = EventsIntent;

    fn state(&self) -> &EventsState {
        &self.state
    }

    fn handle(&mut self, intent: EventsIntent) {
        match intent {
            EventsIntent::Refresh => self.refresh(),
            EventsIntent::RefreshHeader => self.refresh_header(),
            EventsIntent::RefreshNotifications => self.refresh_notifications(),
            EventsIntent::AppsChanged => self.apps_changed(),
            EventsIntent::OpenAlarm => self.open_alarm(),
            EventsIntent::OpenAccessSettings => {
                self.state.problem = self
                    .notifications
                    .open_access_settings()
                    .err()
                    .map(|error| error.problem());
            }
            EventsIntent::Tap { row, from } => self.tap(&row, from),
            EventsIntent::Collapse => self.state.expanded = None,
            EventsIntent::OpenMenu(target) => self.open_menu(target),
            EventsIntent::CloseMenu => self.state.menu = None,
            EventsIntent::ChooseMenuItem(item) => self.choose(item),
            EventsIntent::Clear => self.clear(),
        }
    }

    /// HOME-08: the menu, then the expanded group.
    fn on_back(&mut self) -> bool {
        self.state.menu.take().is_some() || self.state.expanded.take().is_some()
    }
}

/// What `target`'s menu offers, or `None` if it no longer shows (EVT-08).
fn menu_items(state: &EventsState, target: &RowTarget) -> Option<Vec<NotificationMenuItem>> {
    let group = state.group(&target.group)?;
    let clearable = match &target.notification {
        Some(key) => group.notification(key)?.clearable,
        None => group.notifications.iter().any(|n| n.clearable),
    };
    let mut items = Vec::with_capacity(2);
    if clearable {
        items.push(NotificationMenuItem::Dismiss);
    }
    items.push(NotificationMenuItem::NotificationSettings);
    Some(items)
}

/// The keys to dismiss for the clearable notifications of `group` that `chosen` picks. With the
/// last of what the group shows go the summaries hidden behind it, so none is left showing
/// alone (ADR 0021).
fn dismissal(group: &NotificationGroup, chosen: impl Fn(&Notification) -> bool) -> Vec<String> {
    let mut keys: Vec<String> = group
        .notifications
        .iter()
        .filter(|n| n.clearable && chosen(n))
        .map(|n| n.key.clone())
        .collect();
    if !keys.is_empty() && keys.len() == group.count() {
        keys.extend(group.hidden_summaries.iter().cloned());
    }
    keys
}

/// Groups notifications by app, newest group first, leaving out ongoing ones (EVT-04, EVT-05).
/// Each group takes its app's label and accent from `catalogue`, or the system's name for an
/// app that isn't in it.
fn group(active: Vec<Notification>, catalogue: &[App]) -> Vec<NotificationGroup> {
    let mut shown: Vec<Notification> = active.into_iter().filter(|n| !n.ongoing).collect();
    // Newest first; the keys keep notifications posted at the same moment in one order.
    shown.sort_by(|a, b| (Reverse(a.posted_at_ms), &a.key).cmp(&(Reverse(b.posted_at_ms), &b.key)));

    let mut groups: Vec<NotificationGroup> = Vec::new();
    for notification in shown {
        let app = (notification.profile, notification.package.as_str());
        match groups
            .iter_mut()
            .find(|group| (group.profile, group.package.as_str()) == app)
        {
            Some(group) => group.notifications.push(notification),
            None => groups.push(new_group(notification, catalogue)),
        }
    }
    // A summary stands for the app's bundle of notifications, which show for themselves, so it
    // counts only when nothing else of its app shows. It is kept aside, to go with the last of
    // them.
    for group in &mut groups {
        if group.notifications.iter().any(|n| !n.summary) {
            let (summaries, shown) = std::mem::take(&mut group.notifications)
                .into_iter()
                .partition::<Vec<_>, _>(|n| n.summary);
            group.notifications = shown;
            group.hidden_summaries = summaries
                .into_iter()
                .filter(|n| n.clearable)
                .map(|n| n.key)
                .collect();
        }
    }
    groups.sort_by(|a, b| {
        let order = |group: &NotificationGroup| {
            (
                Reverse(group.latest().posted_at_ms),
                group.package.clone(),
                group.profile,
            )
        };
        order(a).cmp(&order(b))
    });
    groups
}

/// A group holding only `first`, named as the catalogue names its app.
fn new_group(first: Notification, catalogue: &[App]) -> NotificationGroup {
    let app = catalogue
        .iter()
        .find(|app| app.id.profile == first.profile && app.id.package == first.package)
        .cloned();
    let label = match &app {
        Some(app) => app.label.clone(),
        None if !first.app_label.is_empty() => first.app_label.clone(),
        None => first.package.clone(),
    };
    // An app outside the catalogue is in a work profile if the catalogue's apps there are.
    let work = catalogue
        .iter()
        .any(|app| app.id.profile == first.profile && app.work);
    NotificationGroup {
        profile: first.profile,
        package: first.package.clone(),
        label,
        app,
        work,
        notifications: vec![first],
        hidden_summaries: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use meegone_core_data::fakes::{self, Fakes, WORK_PROFILE, app, work_app};
    use meegone_core_data::{DataError, Rgb};

    use super::*;

    const HOUR_MS: u64 = 60 * 60 * 1000;
    /// Tuesday 17 November 2026, 00:00 UTC.
    const TUESDAY: u64 = 1_794_873_600_000;

    fn holder(fakes: &Fakes) -> EventsHolder {
        EventsHolder::new(
            fakes.notifications.clone(),
            fakes.apps.clone(),
            fakes.system.clone(),
        )
    }

    /// Notification access granted, and `active` the notifications the system holds.
    fn showing(fakes: &Fakes, active: Vec<Notification>) {
        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        *fakes.notifications.active.lock().unwrap() = active;
    }

    /// Each group's package and its notifications' keys, in order.
    fn shown(holder: &EventsHolder) -> Vec<(String, Vec<String>)> {
        holder
            .state()
            .groups
            .iter()
            .map(|group| {
                let keys = group.notifications.iter().map(|n| n.key.clone()).collect();
                (group.package.clone(), keys)
            })
            .collect()
    }

    fn keys(holder: &EventsHolder) -> Vec<RowKey> {
        holder.state().row_keys()
    }

    fn at(fakes: &Fakes, now_ms: u64) {
        *fakes.system.now_ms.lock().unwrap() = now_ms;
    }

    fn header(weekday: &str, date: &str, alarm: Option<&str>) -> EventsHeader {
        EventsHeader {
            weekday: weekday.into(),
            date: date.into(),
            alarm: alarm.map(Into::into),
        }
    }

    fn notification(key: &str, package: &str, posted_at_ms: u64, ongoing: bool) -> Notification {
        Notification {
            posted_at_ms,
            clearable: !ongoing,
            ongoing,
            ..fakes::notification(key, package)
        }
    }

    #[test]
    fn without_access_events_shows_the_explainer_and_no_notifications() {
        // EVT-03
        let fakes = Fakes::default();
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        assert_eq!(holder.state().access, Access::Missing);
        assert!(holder.state().groups.is_empty());
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn notifications_are_grouped_newest_first_without_ongoing_ones() {
        // EVT-04, EVT-05
        let fakes = Fakes::default();
        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        *fakes.notifications.active.lock().unwrap() = vec![
            notification("m1", "mail", 10, false),
            notification("c1", "chat", 20, false),
            notification("p1", "player", 30, true),
            notification("m2", "mail", 40, false),
        ];
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        assert_eq!(
            shown(&holder),
            [
                ("mail".into(), vec!["m2".into(), "m1".into()]),
                ("chat".into(), vec!["c1".into()]),
            ]
        );
        assert_eq!(holder.state().groups[0].count(), 2);
        assert_eq!(holder.state().groups[0].latest().key, "m2");
    }

    #[test]
    fn a_group_takes_its_apps_label_and_accent_from_the_catalogue() {
        // EVT-04, EVT-09, APPS-17
        let fakes = Fakes::default();
        let mail = App {
            accent: Rgb::hex(0xEA4335),
            ..app("mail", "Mail")
        };
        *fakes.apps.apps.lock().unwrap() = vec![mail.clone()];
        showing(&fakes, vec![notification("m1", "mail", 10, false)]);
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        let group = &holder.state().groups[0];
        assert_eq!(group.label, "Mail");
        assert_eq!(group.app, Some(mail));
        assert!(!group.work);
    }

    #[test]
    fn an_app_with_no_icon_in_the_grid_is_named_as_the_system_names_it() {
        // EVT-04, ADR 0020: such as Android System.
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
        showing(
            &fakes,
            vec![
                Notification {
                    app_label: "Android System".into(),
                    ..notification("s1", "android", 20, false)
                },
                Notification {
                    app_label: String::new(),
                    ..notification("u1", "com.example.updater", 10, false)
                },
            ],
        );
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        let groups = &holder.state().groups;
        assert_eq!(groups[0].label, "Android System");
        assert_eq!(groups[0].app, None);
        assert_eq!(groups[1].label, "com.example.updater", "never blank");
    }

    #[test]
    fn the_same_app_in_the_work_profile_is_a_group_of_its_own() {
        // EVT-04, APPS-20
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), work_app("mail", "Mail")];
        showing(
            &fakes,
            vec![
                notification("p1", "mail", 10, false),
                Notification {
                    profile: WORK_PROFILE,
                    ..notification("w1", "mail", 20, false)
                },
                Notification {
                    profile: WORK_PROFILE,
                    app_label: "Work Updater".into(),
                    ..notification("u1", "updater", 5, false)
                },
            ],
        );
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        let groups = &holder.state().groups;
        let profiles: Vec<(ProfileId, bool)> = groups.iter().map(|g| (g.profile, g.work)).collect();
        assert_eq!(
            profiles,
            [
                (WORK_PROFILE, true),
                (ProfileId(0), false),
                // Outside the catalogue, but in the work profile.
                (WORK_PROFILE, true),
            ]
        );
        assert_eq!(groups[0].app.as_ref().map(|app| app.work), Some(true));
    }

    #[test]
    fn a_summary_counts_only_when_nothing_else_of_its_app_shows() {
        // EVT-04: a bundle of four messages is four, not five.
        let fakes = Fakes::default();
        let summary = |key: &str, package: &str, posted_at_ms| Notification {
            summary: true,
            ..notification(key, package, posted_at_ms, false)
        };
        showing(
            &fakes,
            vec![
                summary("cs", "chat", 50),
                notification("c1", "chat", 10, false),
                notification("c2", "chat", 20, false),
                summary("ms", "mail", 30),
            ],
        );
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        // Chat's newest is now c2, posted before mail's summary.
        assert_eq!(
            shown(&holder),
            [
                ("mail".into(), vec!["ms".into()]),
                ("chat".into(), vec!["c2".into(), "c1".into()]),
            ]
        );
    }

    #[test]
    fn groups_posted_at_the_same_moment_keep_one_order() {
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![
                notification("b1", "b", 10, false),
                notification("a1", "a", 10, false),
            ],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        assert_eq!(shown(&holder)[0].0, "a");

        fakes.notifications.active.lock().unwrap().reverse();
        holder.handle(EventsIntent::RefreshNotifications);
        assert_eq!(shown(&holder)[0].0, "a");
    }

    #[test]
    fn notifications_are_read_again_without_reading_the_catalogue_again() {
        // EVT-10: the catalogue changes only with the apps.
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
        showing(&fakes, vec![notification("m1", "mail", 10, false)]);
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Post")];

        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .push(notification("m2", "mail", 20, false));
        holder.handle(EventsIntent::RefreshNotifications);
        assert_eq!(holder.state().groups[0].count(), 2);
        assert_eq!(holder.state().groups[0].label, "Mail");

        holder.handle(EventsIntent::AppsChanged);
        assert_eq!(holder.state().groups[0].label, "Post");
    }

    #[test]
    fn with_nothing_showing_the_catalogue_waits_for_a_notification() {
        let fakes = Fakes::default();
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed(Problem::AppListUnavailable));
        showing(&fakes, Vec::new());
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);

        holder.handle(EventsIntent::AppsChanged);
        *fakes.apps.error.lock().unwrap() = None;
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
        showing(&fakes, vec![notification("m1", "mail", 10, false)]);
        holder.handle(EventsIntent::RefreshNotifications);

        assert_eq!(holder.state().groups[0].label, "Mail");
    }

    #[test]
    fn without_the_catalogue_groups_still_show_with_the_systems_names() {
        // REL-01: Events never depends on Applications.
        let fakes = Fakes::default();
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed(Problem::AppListUnavailable));
        showing(
            &fakes,
            vec![Notification {
                app_label: "Mail".into(),
                ..notification("m1", "mail", 10, false)
            }],
        );
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        assert_eq!(holder.state().groups[0].label, "Mail");
        assert_eq!(holder.state().groups[0].app, None);
        assert_eq!(holder.state().problem, None);

        // Read again once it can be.
        *fakes.apps.error.lock().unwrap() = None;
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
        holder.handle(EventsIntent::RefreshNotifications);
        assert!(holder.state().groups[0].app.is_some());
    }

    #[test]
    fn revoked_access_empties_the_list_without_a_problem() {
        // EVT-03, REL-01, AC-21
        let fakes = Fakes::default();
        showing(&fakes, vec![notification("m1", "mail", 10, false)]);
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        assert_eq!(holder.state().groups.len(), 1);

        *fakes.notifications.access.lock().unwrap() = Access::Missing;
        holder.handle(EventsIntent::RefreshNotifications);

        assert_eq!(holder.state().access, Access::Missing);
        assert!(holder.state().groups.is_empty());
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn notifications_that_cant_be_read_become_the_problem() {
        // ARCH-09
        let fakes = Fakes::default();
        showing(&fakes, vec![notification("m1", "mail", 10, false)]);
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);

        *fakes.notifications.error.lock().unwrap() = Some(DataError::Failed(Problem::SystemFailed));
        holder.handle(EventsIntent::RefreshNotifications);

        assert!(holder.state().groups.is_empty());
        assert_eq!(holder.state().problem, Some(Problem::SystemFailed));
    }

    #[test]
    fn unchanged_groups_stay_the_same_shared_list() {
        // So the shell sees no change, and nothing moves (ANIM-20).
        let fakes = Fakes::default();
        showing(&fakes, vec![notification("m1", "mail", 10, false)]);
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        let before = Arc::clone(&holder.state().groups);

        holder.handle(EventsIntent::RefreshNotifications);

        assert!(Arc::ptr_eq(&before, &holder.state().groups));
    }

    #[test]
    fn rows_keep_their_place_until_a_newer_notification_moves_them_to_the_top() {
        // ANIM-20
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![
                notification("m1", "mail", 30, false),
                notification("c1", "chat", 20, false),
                notification("n1", "news", 10, false),
            ],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        let first = keys(&holder);

        // Nothing changed.
        assert_eq!(rows_kept(&first, &first), [Some(0), Some(1), Some(2)]);
        assert!(!rows_came_or_went(&first, &first));

        // An older notification of mail's: only its count changes.
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .push(notification("m0", "mail", 5, false));
        holder.handle(EventsIntent::RefreshNotifications);
        let counted = keys(&holder);
        assert_eq!(rows_kept(&first, &counted), [Some(0), Some(1), Some(2)]);
        assert!(!rows_came_or_went(&first, &counted));

        // A new message in news moves its group to the top: its old row goes, and a new one
        // comes in.
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .push(notification("n2", "news", 40, false));
        holder.handle(EventsIntent::RefreshNotifications);
        let moved = keys(&holder);
        assert_eq!(rows_kept(&counted, &moved), [None, Some(0), Some(1)]);
        assert!(rows_came_or_went(&counted, &moved));

        // Chat's only notification goes.
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .retain(|n| n.key != "c1");
        holder.handle(EventsIntent::RefreshNotifications);
        let gone = keys(&holder);
        assert_eq!(rows_kept(&moved, &gone), [Some(0), Some(1)]);
        assert!(rows_came_or_went(&moved, &gone));
    }

    #[test]
    fn an_app_posting_again_with_the_same_key_moves_its_group_to_the_top() {
        // ANIM-20: a chat app updates one notification per conversation.
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![
                notification("m1", "mail", 20, false),
                notification("c1", "chat", 10, false),
            ],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        let before = keys(&holder);

        showing(
            &fakes,
            vec![
                notification("m1", "mail", 20, false),
                notification("c1", "chat", 30, false),
            ],
        );
        holder.handle(EventsIntent::RefreshNotifications);

        assert_eq!(rows_kept(&before, &keys(&holder)), [None, Some(0)]);
    }

    #[test]
    fn the_header_shows_the_date_without_notification_access() {
        // EVT-01, EVT-03
        let fakes = Fakes::default();
        at(&fakes, TUESDAY + 10 * HOUR_MS);
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        assert_eq!(holder.state().access, Access::Missing);
        assert_eq!(
            holder.state().header,
            header("Tuesday", "17 November", None)
        );
    }

    #[test]
    fn an_alarm_within_a_day_shows_its_time_in_the_devices_hour_format() {
        // EVT-02, L10N-03
        let fakes = Fakes::default();
        at(&fakes, TUESDAY + 22 * HOUR_MS);
        *fakes.system.next_alarm_ms.lock().unwrap() = Some(TUESDAY + 31 * HOUR_MS);
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);
        assert_eq!(holder.state().header.alarm.as_deref(), Some("7:00 AM"));

        *fakes.system.hour_24.lock().unwrap() = true;
        holder.handle(EventsIntent::RefreshHeader);
        assert_eq!(holder.state().header.alarm.as_deref(), Some("07:00"));
    }

    #[test]
    fn an_alarm_a_day_or_more_away_also_shows_its_weekday() {
        // EVT-02: a time alone would read as the next time the clock shows it.
        let fakes = Fakes::default();
        *fakes.system.hour_24.lock().unwrap() = true;
        at(&fakes, TUESDAY + 7 * HOUR_MS);
        *fakes.system.next_alarm_ms.lock().unwrap() = Some(TUESDAY + 31 * HOUR_MS);
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::Refresh);

        assert_eq!(holder.state().header.alarm.as_deref(), Some("Wed 07:00"));
    }

    #[test]
    fn without_an_alarm_the_row_is_hidden() {
        // EVT-02, SRS 8.1
        let fakes = Fakes::default();
        *fakes.system.next_alarm_ms.lock().unwrap() = Some(TUESDAY);
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        assert!(holder.state().header.alarm.is_some());

        *fakes.system.next_alarm_ms.lock().unwrap() = None;
        holder.handle(EventsIntent::RefreshHeader);

        assert_eq!(holder.state().header.alarm, None);
    }

    #[test]
    fn the_header_follows_midnight_and_the_time_zone_and_keeps_the_notifications() {
        // EVT-01
        let fakes = Fakes::default();
        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        *fakes.notifications.active.lock().unwrap() = vec![notification("m1", "mail", 1, false)];
        at(&fakes, TUESDAY - 60_000);
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        assert_eq!(holder.state().header, header("Monday", "16 November", None));

        at(&fakes, TUESDAY);
        fakes.notifications.active.lock().unwrap().clear();
        holder.handle(EventsIntent::RefreshHeader);
        assert_eq!(
            holder.state().header,
            header("Tuesday", "17 November", None)
        );
        assert_eq!(
            holder.state().groups.len(),
            1,
            "only the header was read again"
        );

        *fakes.system.utc_offset_ms.lock().unwrap() = -(HOUR_MS as i64);
        holder.handle(EventsIntent::RefreshHeader);
        assert_eq!(holder.state().header, header("Monday", "16 November", None));
    }

    #[test]
    fn tapping_the_alarm_opens_the_app_that_set_it() {
        // EVT-02
        let fakes = Fakes::default();
        *fakes.system.next_alarm_ms.lock().unwrap() = Some(TUESDAY + 7 * HOUR_MS);
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);

        holder.handle(EventsIntent::OpenAlarm);

        assert_eq!(*fakes.system.alarm_opened.lock().unwrap(), 1);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn without_an_alarm_showing_a_tap_opens_nothing() {
        let fakes = Fakes::default();
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        // Set after the header was read, so no row shows yet.
        *fakes.system.next_alarm_ms.lock().unwrap() = Some(TUESDAY);

        holder.handle(EventsIntent::OpenAlarm);

        assert_eq!(*fakes.system.alarm_opened.lock().unwrap(), 0);
    }

    #[test]
    fn an_alarm_that_cannot_open_becomes_the_problem() {
        // ARCH-09
        let fakes = Fakes::default();
        *fakes.system.next_alarm_ms.lock().unwrap() = Some(TUESDAY);
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        *fakes.system.error.lock().unwrap() =
            Some(DataError::Unsupported(Problem::AlarmUnavailable));

        holder.handle(EventsIntent::OpenAlarm);

        assert_eq!(holder.state().problem, Some(Problem::AlarmUnavailable));
    }

    #[test]
    fn allow_opens_the_system_access_screen() {
        let fakes = Fakes::default();
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::OpenAccessSettings);

        assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
    }

    fn id(package: &str) -> GroupId {
        GroupId {
            profile: ProfileId(0),
            package: package.into(),
        }
    }

    fn group_row(package: &str) -> RowTarget {
        RowTarget {
            group: id(package),
            notification: None,
        }
    }

    fn notification_row(package: &str, key: &str) -> RowTarget {
        RowTarget {
            group: id(package),
            notification: Some(key.into()),
        }
    }

    fn tap(holder: &mut EventsHolder, row: RowTarget) {
        holder.handle(EventsIntent::Tap { row, from: None });
    }

    fn opened(fakes: &Fakes) -> Vec<String> {
        fakes.notifications.opened.lock().unwrap().clone()
    }

    fn dismissed(fakes: &Fakes) -> Vec<String> {
        fakes.notifications.dismissed.lock().unwrap().clone()
    }

    /// Mail with three notifications and chat with one, read.
    fn mail_and_chat(fakes: &Fakes) -> EventsHolder {
        showing(
            fakes,
            vec![
                notification("m1", "mail", 10, false),
                notification("m2", "mail", 20, false),
                notification("m3", "mail", 30, false),
                notification("c1", "chat", 25, false),
            ],
        );
        let mut holder = holder(fakes);
        holder.handle(EventsIntent::Refresh);
        holder
    }

    #[test]
    fn tapping_a_group_of_one_opens_its_notification_from_the_row() {
        // EVT-06, ANIM-08
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        let from = Rect {
            left: 0,
            top: 300,
            right: 1080,
            bottom: 480,
        };

        holder.handle(EventsIntent::Tap {
            row: group_row("chat"),
            from: Some(from),
        });

        assert_eq!(opened(&fakes), ["c1"]);
        assert_eq!(
            *fakes.notifications.opened_from.lock().unwrap(),
            [Some(from)]
        );
        assert!(dismissed(&fakes).is_empty(), "it isn't meant to disappear");
        assert_eq!(holder.state().expanded, None);
    }

    #[test]
    fn a_notification_meant_to_disappear_when_opened_is_dismissed() {
        // EVT-06: MeeGone removes it, as the system's shade does.
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![Notification {
                auto_cancel: true,
                ..notification("c1", "chat", 10, false)
            }],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);

        tap(&mut holder, group_row("chat"));

        assert_eq!(opened(&fakes), ["c1"]);
        assert_eq!(dismissed(&fakes), ["c1"]);
        // Its row goes when the system says it went.
        assert_eq!(holder.state().groups.len(), 1);
        holder.handle(EventsIntent::RefreshNotifications);
        assert!(holder.state().groups.is_empty());
    }

    #[test]
    fn one_that_cant_be_cleared_stays_after_opening() {
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![Notification {
                auto_cancel: true,
                clearable: false,
                ..notification("c1", "chat", 10, false)
            }],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);

        tap(&mut holder, group_row("chat"));

        assert_eq!(opened(&fakes), ["c1"]);
        assert!(dismissed(&fakes).is_empty());
    }

    #[test]
    fn tapping_a_group_of_several_expands_it_and_again_collapses_it() {
        // EVT-06
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);

        tap(&mut holder, group_row("mail"));
        assert_eq!(holder.state().expanded, Some(id("mail")));
        assert!(opened(&fakes).is_empty());

        tap(&mut holder, group_row("mail"));
        assert_eq!(holder.state().expanded, None);
        assert!(opened(&fakes).is_empty());
    }

    #[test]
    fn one_group_is_expanded_at_a_time() {
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .push(notification("c2", "chat", 5, false));
        holder.handle(EventsIntent::RefreshNotifications);

        tap(&mut holder, group_row("mail"));
        tap(&mut holder, group_row("chat"));

        assert_eq!(holder.state().expanded, Some(id("chat")));
    }

    #[test]
    fn the_expanded_groups_notifications_follow_its_row() {
        // EVT-06, ANIM-20: expanding brings their rows in below the group's, which stays.
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        let before = keys(&holder);

        tap(&mut holder, group_row("mail"));

        let rows = holder.state().rows();
        let shown: Vec<(&str, Option<&str>, bool)> = rows
            .iter()
            .map(|row| {
                (
                    row.group.package.as_str(),
                    row.notification.map(|n| n.key.as_str()),
                    row.expanded,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("mail", None, true),
                ("mail", Some("m3"), false),
                ("mail", Some("m2"), false),
                ("mail", Some("m1"), false),
                ("chat", None, false),
            ]
        );
        let expanded = keys(&holder);
        assert_eq!(
            rows_kept(&before, &expanded),
            [Some(0), None, None, None, Some(1)]
        );
        assert!(rows_came_or_went(&before, &expanded));
        assert_eq!(rows[2].target(), notification_row("mail", "m2"));

        // A notification of the group's that goes takes only its row; collapsing takes the rest.
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .retain(|n| n.key != "m2");
        holder.handle(EventsIntent::RefreshNotifications);
        let fewer = keys(&holder);
        assert_eq!(
            rows_kept(&expanded, &fewer),
            [Some(0), Some(1), Some(3), Some(4)]
        );
        holder.handle(EventsIntent::Collapse);
        assert_eq!(rows_kept(&fewer, &keys(&holder)), [Some(0), Some(3)]);
    }

    #[test]
    fn a_newer_notification_brings_the_expanded_group_in_again_at_the_top() {
        // ANIM-20: its rows go and come together, so the rows that stay keep their order.
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .push(notification("c2", "chat", 15, false));
        holder.handle(EventsIntent::RefreshNotifications);
        tap(&mut holder, group_row("chat"));
        let before = keys(&holder);

        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .push(notification("c3", "chat", 40, false));
        holder.handle(EventsIntent::RefreshNotifications);

        assert_eq!(holder.state().expanded, Some(id("chat")));
        // Before: mail, then chat with c1 and c2. Now chat and its three are new at the top, and
        // mail stays.
        assert_eq!(
            rows_kept(&before, &keys(&holder)),
            [None, None, None, None, Some(0)]
        );
    }

    #[test]
    fn an_expanded_group_collapses_once_it_holds_fewer_than_two() {
        // EVT-06
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        tap(&mut holder, group_row("mail"));

        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .retain(|n| n.key != "m1" && n.key != "m2");
        holder.handle(EventsIntent::RefreshNotifications);

        assert_eq!(holder.state().expanded, None);
    }

    #[test]
    fn a_notification_of_the_expanded_group_opens() {
        // EVT-06, the issue's acceptance: tapping one item opens it.
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        tap(&mut holder, group_row("mail"));

        tap(&mut holder, notification_row("mail", "m2"));

        assert_eq!(opened(&fakes), ["m2"]);
        assert_eq!(holder.state().expanded, Some(id("mail")));
    }

    #[test]
    fn a_row_that_no_longer_shows_does_nothing() {
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);

        tap(&mut holder, group_row("news"));
        tap(&mut holder, notification_row("mail", "gone"));
        holder.handle(EventsIntent::OpenMenu(group_row("news")));

        assert!(opened(&fakes).is_empty());
        assert_eq!(holder.state().expanded, None);
        assert_eq!(holder.state().menu, None);
    }

    #[test]
    fn a_notification_that_cant_open_becomes_the_problem() {
        // ARCH-09: the host shows it briefly.
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![Notification {
                auto_cancel: true,
                ..notification("c1", "chat", 10, false)
            }],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        *fakes.notifications.error.lock().unwrap() =
            Some(DataError::Failed(Problem::NotificationUnavailable));

        tap(&mut holder, group_row("chat"));

        assert_eq!(
            holder.state().problem,
            Some(Problem::NotificationUnavailable)
        );
        assert!(dismissed(&fakes).is_empty(), "nothing opened, so it stays");
    }

    #[test]
    fn the_menu_offers_dismiss_only_for_what_can_be_cleared() {
        // EVT-08
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![
                notification("m1", "mail", 10, false),
                Notification {
                    clearable: false,
                    ..notification("m2", "mail", 20, false)
                },
                Notification {
                    clearable: false,
                    ..notification("s1", "system", 30, false)
                },
            ],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        let items = |holder: &EventsHolder| holder.state().menu.as_ref().map(|m| m.items.clone());
        use NotificationMenuItem::{Dismiss, NotificationSettings};

        holder.handle(EventsIntent::OpenMenu(group_row("mail")));
        assert_eq!(items(&holder), Some(vec![Dismiss, NotificationSettings]));

        holder.handle(EventsIntent::OpenMenu(notification_row("mail", "m2")));
        assert_eq!(items(&holder), Some(vec![NotificationSettings]));

        holder.handle(EventsIntent::OpenMenu(group_row("system")));
        assert_eq!(items(&holder), Some(vec![NotificationSettings]));
        assert_eq!(
            holder.state().menu.as_ref().unwrap().target,
            group_row("system")
        );
    }

    #[test]
    fn dismiss_takes_a_groups_clearable_notifications_or_just_one() {
        // EVT-08
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);

        holder.handle(EventsIntent::OpenMenu(notification_row("mail", "m2")));
        holder.handle(EventsIntent::ChooseMenuItem(NotificationMenuItem::Dismiss));
        assert_eq!(dismissed(&fakes), ["m2"]);
        assert_eq!(holder.state().menu, None);

        holder.handle(EventsIntent::RefreshNotifications);
        holder.handle(EventsIntent::OpenMenu(group_row("mail")));
        holder.handle(EventsIntent::ChooseMenuItem(NotificationMenuItem::Dismiss));
        assert_eq!(dismissed(&fakes), ["m2", "m3", "m1"]);
        assert_eq!(*fakes.notifications.dismiss_requests.lock().unwrap(), 2);

        holder.handle(EventsIntent::RefreshNotifications);
        assert_eq!(shown(&holder), [("chat".into(), vec!["c1".into()])]);
    }

    #[test]
    fn the_last_of_a_bundle_takes_the_summaries_hidden_behind_it() {
        // ADR 0021: no summary is left showing alone.
        let fakes = Fakes::default();
        let summary = |key: &str, posted_at_ms| Notification {
            summary: true,
            ..notification(key, "chat", posted_at_ms, false)
        };
        showing(
            &fakes,
            vec![
                summary("cs", 50),
                Notification {
                    clearable: false,
                    ..summary("fixed", 40)
                },
                notification("c1", "chat", 10, false),
                notification("c2", "chat", 20, false),
            ],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        assert_eq!(holder.state().groups[0].hidden_summaries, ["cs"]);

        // Not the last: the summary stays hidden behind c1.
        holder.handle(EventsIntent::OpenMenu(notification_row("chat", "c2")));
        holder.handle(EventsIntent::ChooseMenuItem(NotificationMenuItem::Dismiss));
        assert_eq!(dismissed(&fakes), ["c2"]);

        holder.handle(EventsIntent::RefreshNotifications);
        holder.handle(EventsIntent::OpenMenu(notification_row("chat", "c1")));
        holder.handle(EventsIntent::ChooseMenuItem(NotificationMenuItem::Dismiss));
        assert_eq!(dismissed(&fakes), ["c2", "c1", "cs"]);
    }

    #[test]
    fn notification_settings_open_for_the_app_in_its_profile() {
        // EVT-08, APPS-20
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![Notification {
                profile: WORK_PROFILE,
                ..notification("w1", "mail", 10, false)
            }],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        let work_mail = RowTarget {
            group: GroupId {
                profile: WORK_PROFILE,
                package: "mail".into(),
            },
            notification: None,
        };

        holder.handle(EventsIntent::OpenMenu(work_mail));
        holder.handle(EventsIntent::ChooseMenuItem(
            NotificationMenuItem::NotificationSettings,
        ));

        assert_eq!(
            *fakes.notifications.app_settings_opened.lock().unwrap(),
            [(WORK_PROFILE, "mail".to_owned())]
        );
        assert_eq!(holder.state().menu, None);
        assert!(dismissed(&fakes).is_empty());
    }

    #[test]
    fn an_item_the_menu_doesnt_offer_does_nothing() {
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![Notification {
                clearable: false,
                ..notification("s1", "system", 10, false)
            }],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);

        holder.handle(EventsIntent::ChooseMenuItem(NotificationMenuItem::Dismiss));
        holder.handle(EventsIntent::OpenMenu(group_row("system")));
        holder.handle(EventsIntent::ChooseMenuItem(NotificationMenuItem::Dismiss));

        assert!(dismissed(&fakes).is_empty());
        assert!(holder.state().menu.is_some(), "it stays open");
    }

    #[test]
    fn a_failed_menu_item_becomes_the_problem() {
        // ARCH-09
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        holder.handle(EventsIntent::OpenMenu(group_row("chat")));
        *fakes.notifications.error.lock().unwrap() = Some(DataError::Unsupported(
            Problem::NotificationSettingsUnavailable,
        ));

        holder.handle(EventsIntent::ChooseMenuItem(
            NotificationMenuItem::NotificationSettings,
        ));

        assert_eq!(
            holder.state().problem,
            Some(Problem::NotificationSettingsUnavailable)
        );
    }

    #[test]
    fn the_menu_closes_when_what_it_is_for_goes() {
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        tap(&mut holder, group_row("mail"));
        holder.handle(EventsIntent::OpenMenu(notification_row("mail", "m1")));

        // Another of mail's going keeps it.
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .retain(|n| n.key != "m2");
        holder.handle(EventsIntent::RefreshNotifications);
        assert!(holder.state().menu.is_some());

        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .retain(|n| n.key != "m1");
        holder.handle(EventsIntent::RefreshNotifications);
        assert_eq!(holder.state().menu, None);
    }

    #[test]
    fn back_closes_the_menu_then_collapses_the_group() {
        // HOME-08
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        tap(&mut holder, group_row("mail"));
        holder.handle(EventsIntent::OpenMenu(group_row("mail")));

        assert!(holder.on_back());
        assert_eq!(holder.state().menu, None);
        assert_eq!(holder.state().expanded, Some(id("mail")));

        assert!(holder.on_back());
        assert_eq!(holder.state().expanded, None);

        assert!(!holder.on_back());
    }

    #[test]
    fn clear_takes_every_clearable_notification_in_one_request() {
        // EVT-07, AC-12: ongoing ones were never shown, and those that can't be cleared stay.
        let fakes = Fakes::default();
        showing(
            &fakes,
            vec![
                notification("m1", "mail", 10, false),
                notification("m2", "mail", 20, false),
                Notification {
                    summary: true,
                    ..notification("ms", "mail", 30, false)
                },
                notification("c1", "chat", 40, false),
                Notification {
                    clearable: false,
                    ..notification("s1", "system", 50, false)
                },
                notification("p1", "player", 60, true),
            ],
        );
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);
        assert!(holder.state().clearable());

        holder.handle(EventsIntent::Clear);

        let mut cleared = dismissed(&fakes);
        cleared.sort();
        assert_eq!(cleared, ["c1", "m1", "m2", "ms"]);
        assert_eq!(*fakes.notifications.dismiss_requests.lock().unwrap(), 1);
        holder.handle(EventsIntent::RefreshNotifications);
        assert_eq!(shown(&holder), [("system".into(), vec!["s1".into()])]);
        assert!(!holder.state().clearable(), "Clear is offered no more");
        let left: Vec<String> = fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .iter()
            .map(|n| n.key.clone())
            .collect();
        assert_eq!(left, ["s1", "p1"]);
    }

    #[test]
    fn clear_with_nothing_clearable_asks_nothing() {
        let fakes = Fakes::default();
        showing(&fakes, Vec::new());
        let mut holder = holder(&fakes);
        holder.handle(EventsIntent::Refresh);

        holder.handle(EventsIntent::Clear);

        assert_eq!(*fakes.notifications.dismiss_requests.lock().unwrap(), 0);
        assert_eq!(holder.state().problem, None);
    }

    #[test]
    fn a_failed_clear_becomes_the_problem() {
        // ARCH-09
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        *fakes.notifications.error.lock().unwrap() =
            Some(DataError::Failed(Problem::NotificationsDismissUnavailable));

        holder.handle(EventsIntent::Clear);

        assert_eq!(
            holder.state().problem,
            Some(Problem::NotificationsDismissUnavailable)
        );
    }

    #[test]
    fn revoking_access_collapses_the_group_and_closes_the_menu() {
        // AC-21
        let fakes = Fakes::default();
        let mut holder = mail_and_chat(&fakes);
        tap(&mut holder, group_row("mail"));
        holder.handle(EventsIntent::OpenMenu(group_row("mail")));

        *fakes.notifications.access.lock().unwrap() = Access::Missing;
        holder.handle(EventsIntent::RefreshNotifications);

        assert_eq!(holder.state().expanded, None);
        assert_eq!(holder.state().menu, None);
    }
}
