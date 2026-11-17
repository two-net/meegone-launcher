//! Events state (SRS 4.3): the header with the date and the next alarm, and live notifications
//! grouped by app. Memory only (EVT-13).
//!
//! The platform writes dates and times in the device's language and 12- or 24-hour setting;
//! Events decides which to show (L10N-03, ADR 0019). Each group takes its app's label, accent
//! and icon from the app catalogue (APPS-17, ADR 0002, ADR 0020).

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use meegone_core_data::{AppsAdapter, NotificationsAdapter, SystemConfigAdapter, TimeStyle};
use meegone_core_state::StateHolder;
pub use meegone_core_state::{Access, App, AppId, Notification, ProfileId};

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

/// Where each of `now`'s rows was in `before`, if it kept its row there; `None` for a row that
/// came in (ANIM-20). A row of `before` that no row of `now` keeps has gone.
///
/// The rows are in order of their newest notification, and a kept row's newest notification is
/// the same as before, so kept rows stay in the order they were in.
pub fn rows_kept(before: &[GroupKey], now: &[GroupKey]) -> Vec<Option<usize>> {
    let places: HashMap<&GroupKey, usize> = before
        .iter()
        .enumerate()
        .map(|(at, key)| (key, at))
        .collect();
    now.iter().map(|key| places.get(key).copied()).collect()
}

/// Whether a row came in or went between `before` and `now`, so the list moves (ANIM-20).
pub fn rows_came_or_went(before: &[GroupKey], now: &[GroupKey]) -> bool {
    let kept = rows_kept(before, now);
    kept.iter().any(Option::is_none) || kept.len() != before.len()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventsState {
    pub header: EventsHeader,
    /// Without notification access, Events shows its explainer card (EVT-03).
    pub access: Access,
    /// The group with the newest notification comes first (EVT-04). Shared, so the shell can
    /// tell cheaply when it changed.
    pub groups: Arc<[NotificationGroup]>,
    pub problem: Option<String>,
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
                self.state.problem = Some(error.to_string());
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

    /// Keeps the shared groups when nothing changed, so the shell sees no change.
    fn set_groups(&mut self, groups: Vec<NotificationGroup>) {
        if *self.state.groups != *groups {
            self.state.groups = groups.into();
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
            .map(|error| error.to_string());
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
                    .map(|error| error.to_string());
            }
        }
    }
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
    // counts only when nothing else of its app shows.
    for group in &mut groups {
        if group.notifications.iter().any(|n| !n.summary) {
            group.notifications.retain(|n| !n.summary);
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

    fn keys(holder: &EventsHolder) -> Vec<GroupKey> {
        holder
            .state()
            .groups
            .iter()
            .map(NotificationGroup::key)
            .collect()
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
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed("No apps".into()));
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
        *fakes.apps.error.lock().unwrap() = Some(DataError::Failed("No apps".into()));
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

        *fakes.notifications.error.lock().unwrap() = Some(DataError::Failed("Busy".into()));
        holder.handle(EventsIntent::RefreshNotifications);

        assert!(holder.state().groups.is_empty());
        assert_eq!(holder.state().problem.as_deref(), Some("Busy"));
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
        *fakes.system.error.lock().unwrap() = Some(DataError::Unsupported("No clock".into()));

        holder.handle(EventsIntent::OpenAlarm);

        assert_eq!(holder.state().problem.as_deref(), Some("No clock"));
    }

    #[test]
    fn allow_opens_the_system_access_screen() {
        let fakes = Fakes::default();
        let mut holder = holder(&fakes);

        holder.handle(EventsIntent::OpenAccessSettings);

        assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
    }
}
