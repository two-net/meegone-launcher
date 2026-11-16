//! Events state (SRS 4.3): the header with the date and the next alarm, and live notifications
//! grouped by app. Memory only (EVT-13).
//!
//! The platform writes dates and times in the device's language and 12- or 24-hour setting;
//! Events decides which to show (L10N-03, ADR 0019).

use std::cmp::Reverse;
use std::sync::Arc;

use meegone_core_data::{NotificationsAdapter, SystemConfigAdapter, TimeStyle};
use meegone_core_state::{Access, Notification, StateHolder};

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

/// One app's notifications, newest first (EVT-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationGroup {
    pub package: String,
    pub notifications: Vec<Notification>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventsState {
    pub header: EventsHeader,
    /// Without notification access, Events shows its explainer card (EVT-03).
    pub access: Access,
    /// The group with the newest notification comes first (EVT-04).
    pub groups: Vec<NotificationGroup>,
    pub problem: Option<String>,
}

#[derive(Debug)]
pub enum EventsIntent {
    /// Reads the header and the notifications again.
    Refresh,
    /// The date, the time, the time zone, the 12- or 24-hour setting, the language or the next
    /// alarm may have changed (EVT-01, EVT-02, L10N-03).
    RefreshHeader,
    /// The user tapped the alarm row (EVT-02).
    OpenAlarm,
    /// The user tapped Allow on the explainer card (EVT-03).
    OpenAccessSettings,
}

pub struct EventsHolder {
    notifications: Arc<dyn NotificationsAdapter>,
    system: Arc<dyn SystemConfigAdapter>,
    state: EventsState,
}

impl EventsHolder {
    pub fn new(
        notifications: Arc<dyn NotificationsAdapter>,
        system: Arc<dyn SystemConfigAdapter>,
    ) -> Self {
        Self {
            notifications,
            system,
            state: EventsState::default(),
        }
    }

    fn refresh(&mut self) {
        self.refresh_header();
        self.refresh_notifications();
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
            self.state.groups = Vec::new();
            self.state.problem = None;
            return;
        }
        match self.notifications.active() {
            Ok(active) => {
                self.state.groups = group(active);
                self.state.problem = None;
            }
            Err(error) => {
                self.state.groups = Vec::new();
                self.state.problem = Some(error.to_string());
            }
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
fn group(active: Vec<Notification>) -> Vec<NotificationGroup> {
    let mut shown: Vec<Notification> = active.into_iter().filter(|n| !n.ongoing).collect();
    shown.sort_by_key(|notification| Reverse(notification.posted_at_ms));

    let mut groups: Vec<NotificationGroup> = Vec::new();
    for notification in shown {
        match groups
            .iter_mut()
            .find(|group| group.package == notification.package)
        {
            Some(group) => group.notifications.push(notification),
            None => groups.push(NotificationGroup {
                package: notification.package.clone(),
                notifications: vec![notification],
            }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use meegone_core_data::DataError;
    use meegone_core_data::fakes::Fakes;

    use super::*;

    const HOUR_MS: u64 = 60 * 60 * 1000;
    /// Tuesday 17 November 2026, 00:00 UTC.
    const TUESDAY: u64 = 1_794_873_600_000;

    fn holder(fakes: &Fakes) -> EventsHolder {
        EventsHolder::new(fakes.notifications.clone(), fakes.system.clone())
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
            key: key.into(),
            package: package.into(),
            title: "Title".into(),
            text: "Text".into(),
            posted_at_ms,
            clearable: !ongoing,
            ongoing,
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

        let groups: Vec<(&str, Vec<&str>)> = holder
            .state()
            .groups
            .iter()
            .map(|group| {
                let keys = group.notifications.iter().map(|n| n.key.as_str()).collect();
                (group.package.as_str(), keys)
            })
            .collect();
        assert_eq!(groups, [("mail", vec!["m2", "m1"]), ("chat", vec!["c1"])]);
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
