//! Home shell UI (SRS 4.1). It puts the feature views together, connects each to its state
//! holder through the shell and places content one feature shows inside another (ADR 0002).
//! Only the shell depends on other features (ARCH-03).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use meegone_apps_ui::AppsView;
use meegone_core_ui::{Dispatch, Latest, Slot, View, tokens};
use meegone_events_ui::EventsView;
use meegone_lockimage_ui::{ImageCard, ImageSettingsSection};
use meegone_recent_ui::RecentView;
use meegone_settings_ui::{HomeBanner, SettingsView};
use meegone_shell_state::{HomeShell, HomeView, ShellIntent};

type Outbox = Rc<RefCell<VecDeque<ShellIntent>>>;

pub struct ShellView {
    outbox: Outbox,
    apps: AppsView,
    events: EventsView,
    recent: RecentView,
    settings: SettingsView,
    home_banner: HomeBanner,
}

impl ShellView {
    pub fn new(shell: &mut HomeShell) -> Self {
        let outbox = Outbox::default();

        // The lock-screen image shows its card in Events and its section in Settings. Both read
        // the latest image state, which the shell keeps current.
        let image = Latest::new(shell.lockimage().state());
        shell.subscribe_lockimage({
            let image = image.clone();
            move |state| image.set(Arc::clone(state))
        });

        let mut events = EventsView::new(dispatch(&outbox, ShellIntent::Events));
        events.add_card(Box::new(ImageCard::new(image.clone())));

        let mut settings = SettingsView::new(dispatch(&outbox, ShellIntent::Settings));
        settings.add_section(Box::new(ImageSettingsSection::new(
            image,
            dispatch(&outbox, ShellIntent::LockImage),
        )));

        // Settings and setup offers to make MeeGone the default home app on the home views
        // (ONB-04).
        let settings_state = Latest::new(shell.settings().state());
        shell.subscribe_settings({
            let settings_state = settings_state.clone();
            move |state| settings_state.set(Arc::clone(state))
        });
        let home_banner = HomeBanner::new(settings_state, dispatch(&outbox, ShellIntent::Settings));

        Self {
            apps: AppsView::new(dispatch(&outbox, ShellIntent::Apps)),
            recent: RecentView::new(dispatch(&outbox, ShellIntent::Recent)),
            events,
            settings,
            home_banner,
            outbox,
        }
    }

    /// The intents the views sent since the last call, in order, for the shell to apply.
    pub fn take_intents(&self) -> Vec<ShellIntent> {
        self.outbox.borrow_mut().drain(..).collect()
    }

    /// Draws what is on screen in the current theme (THEME-03).
    pub fn render(&mut self, shell: &HomeShell) {
        let tokens = tokens(shell.theme().state().theme);
        if shell.state().settings_open {
            self.settings.render(&shell.settings().state(), tokens);
            return;
        }
        match shell.state().current {
            HomeView::Events => self.events.render(&shell.events().state(), tokens),
            HomeView::Applications => self.apps.render(&shell.apps().state(), tokens),
            HomeView::Recent => self.recent.render(&shell.recent().state(), tokens),
        }
        self.home_banner.render(tokens);
    }

    pub fn apps(&self) -> &AppsView {
        &self.apps
    }

    pub fn events(&self) -> &EventsView {
        &self.events
    }

    pub fn recent(&self) -> &RecentView {
        &self.recent
    }

    pub fn settings(&self) -> &SettingsView {
        &self.settings
    }

    /// Shown over the home views, never over Settings (ONB-04).
    pub fn home_banner(&self) -> &HomeBanner {
        &self.home_banner
    }
}

/// A dispatcher that queues a view's intents, wrapped for the shell.
fn dispatch<I: 'static>(outbox: &Outbox, wrap: fn(I) -> ShellIntent) -> Dispatch<I> {
    let outbox = Rc::clone(outbox);
    Dispatch::new(move |intent| outbox.borrow_mut().push_back(wrap(intent)))
}
