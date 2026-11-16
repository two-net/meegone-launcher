//! Home shell UI (SRS 4.1). It puts the feature views together, connects each to its state
//! holder through the shell and places content one feature shows inside another (ADR 0002).
//! Only the shell depends on other features (ARCH-03).
//!
//! It also tells the host what to draw for the home views in each frame, as a [`HomeFrame`]:
//! where the carousel's views are, how the view indicator looks, and how the content comes back
//! from an app (ADR 0006, ADR 0007).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use meegone_apps_ui::AppsView;
use meegone_core_ui::{Dispatch, Latest, Rgb, Slot, View, tokens};
use meegone_events_ui::EventsView;
use meegone_lockimage_ui::{ImageCard, ImageSettingsSection};
use meegone_recent_ui::RecentView;
use meegone_settings_ui::{HomeBanner, SettingsView};
use meegone_shell_state::{HomeShell, HomeView, ShellIntent};

type Outbox = Rc<RefCell<VecDeque<ShellIntent>>>;

/// What the host draws for the home views in one frame (ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeFrame {
    /// How far each view, in [`HomeView::ALL`] order, is drawn from where it fills the screen,
    /// in whole pixels. A view a full width or more away is off screen (HOME-02 to HOME-04).
    pub pages: [i32; 3],
    /// The view indicator's opacity, from 0 to 255 (HOME-09).
    pub indicator_alpha: u8,
    /// The segment the indicator highlights: the view nearest the carousel's position.
    pub indicator_selected: HomeView,
    /// The view on screen, or the one the carousel is settling on, which screen readers
    /// announce (A11Y-08).
    pub current: HomeView,
    /// The home content's scale in ten-thousandths: 10 000 at rest, from 9 400 while it comes
    /// back from an app (ANIM-09).
    pub content_scale: u16,
    /// The home content's opacity, from 0 to 255: 255 at rest (ANIM-09). The background stays
    /// opaque.
    pub content_alpha: u8,
    /// Something is still moving or fading, so the host should draw another frame.
    pub animating: bool,
    /// The home views' solid background (HOME-11, THEME-04).
    pub background: Rgb,
    /// Text, and the indicator's highlighted segment.
    pub foreground: Rgb,
    /// The indicator's other segments.
    pub dim: Rgb,
}

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

    /// What the host draws for the home views now, in the current theme (THEME-03).
    pub fn frame(&self, shell: &HomeShell) -> HomeFrame {
        let tokens = tokens(shell.theme().state().theme);
        let state = shell.state();
        HomeFrame {
            pages: HomeView::ALL.map(|view| state.page_offset_px(view).round() as i32),
            indicator_alpha: (state.carousel.indicator.clamp(0.0, 1.0) * 255.0).round() as u8,
            indicator_selected: state.nearest_view(),
            current: state.current,
            content_scale: (state.return_home.scale.clamp(0.0, 1.0) * 10_000.0).round() as u16,
            content_alpha: (state.return_home.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            animating: state.carousel.animating || state.return_home.animating,
            background: tokens.background,
            foreground: tokens.text_primary,
            dim: tokens.text_secondary,
        }
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
