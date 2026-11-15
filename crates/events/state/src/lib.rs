//! Events state (SRS 4.3): live notifications grouped by app. Memory only (EVT-13).

use std::cmp::Reverse;
use std::sync::Arc;

use meegone_core_data::NotificationsAdapter;
use meegone_core_state::{Access, Notification, StateHolder};

/// One app's notifications, newest first (EVT-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationGroup {
    pub package: String,
    pub notifications: Vec<Notification>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventsState {
    /// Without notification access, Events shows its explainer card (EVT-03).
    pub access: Access,
    /// The group with the newest notification comes first (EVT-04).
    pub groups: Vec<NotificationGroup>,
    pub problem: Option<String>,
}

#[derive(Debug)]
pub enum EventsIntent {
    Refresh,
    /// The user tapped Allow on the explainer card (EVT-03).
    OpenAccessSettings,
}

pub struct EventsHolder {
    notifications: Arc<dyn NotificationsAdapter>,
    state: EventsState,
}

impl EventsHolder {
    pub fn new(notifications: Arc<dyn NotificationsAdapter>) -> Self {
        Self {
            notifications,
            state: EventsState::default(),
        }
    }

    fn refresh(&mut self) {
        let access = self.notifications.access();
        if access == Access::Missing {
            self.state = EventsState::default();
            return;
        }
        self.state = match self.notifications.active() {
            Ok(active) => EventsState {
                access,
                groups: group(active),
                problem: None,
            },
            Err(error) => EventsState {
                access,
                groups: Vec::new(),
                problem: Some(error.to_string()),
            },
        };
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
    use meegone_core_data::fakes::Fakes;

    use super::*;

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
        let mut holder = EventsHolder::new(fakes.notifications.clone());

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
        let mut holder = EventsHolder::new(fakes.notifications.clone());

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
    fn allow_opens_the_system_access_screen() {
        let fakes = Fakes::default();
        let mut holder = EventsHolder::new(fakes.notifications.clone());

        holder.handle(EventsIntent::OpenAccessSettings);

        assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
    }
}
