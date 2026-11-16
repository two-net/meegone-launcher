//! Home shell UI (SRS 4.1). It puts the feature views together, connects each to its state
//! holder through the shell and places content one feature shows inside another (ADR 0002).
//! Only the shell depends on other features (ARCH-03).
//!
//! It also tells the host what to draw for the home views in each frame, as a [`HomeFrame`]:
//! where the carousel's views are, how the view indicator looks, how the content comes back
//! from an app, the home menu, the Applications grid, the pressed icon, the grid's cells as
//! apps come, go and move, an app's long-press menu, the dragged icon and edit mode (ADR 0006
//! to ADR 0008, ADR 0010, ADR 0012 to ADR 0014).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use meegone_apps_ui::AppsView;
pub use meegone_apps_ui::{AppMenuFrame, AppMenuItem, AppsFrame, Cell, GridLayout};
use meegone_core_ui::{Dispatch, Latest, Rgb, Slot, View, tokens};
use meegone_events_ui::EventsView;
use meegone_lockimage_ui::{ImageCard, ImageSettingsSection};
use meegone_recent_ui::RecentView;
use meegone_settings_ui::{HomeBanner, SettingsView};
pub use meegone_shell_state::BADGE_ROWS;
use meegone_shell_state::{HomeShell, HomeView, MenuItem, SettingsGroup, ShellIntent};

type Outbox = Rc<RefCell<VecDeque<ShellIntent>>>;

/// The home menu as the host draws it (HOME-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuFrame {
    /// What it offers, in order.
    pub items: &'static [MenuItem],
    /// Where the finger was held, in whole pixels from the home views' top left, or `None` to
    /// centre it, when a screen reader opened it (A11Y-05).
    pub anchor: Option<(i32, i32)>,
}

/// How the Applications grid's cells come, go and move as apps are installed and removed, in
/// ten-thousandths (ANIM-21). Each cell says where it was, so the host knows which values apply
/// to it (ADR 0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridChangeFrame {
    /// The scale of a cell that just joined: 5 000 at first, 10 000 at rest.
    pub appear_scale: u16,
    /// Its opacity: 0 at first, 10 000 at rest.
    pub appear_alpha: u16,
    /// The scale of a cell that left: 10 000 at first, 5 000 at the end.
    pub leave_scale: u16,
    /// Its opacity: 10 000 at first, 0 at the end, when the host drops it.
    pub leave_alpha: u16,
    /// How far every other cell has come from its old place to its new one: 0 at first,
    /// 10 000 at rest.
    pub shift: u16,
}

/// The app icon a drag after a long-press picked up, as the host draws it (APPS-11, APPS-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiftFrame {
    /// Its cell's place in the Applications grid's cells now.
    pub cell: usize,
    /// How far it is drawn from its cell's place, in whole pixels, so it stays under the
    /// finger.
    pub dx: i32,
    pub dy: i32,
    /// Its scale in ten-thousandths: 11 000 while lifted (ANIM-13), springing to edit mode's
    /// 9 200 as it settles. It takes the place of edit mode's scale for this cell.
    pub scale: u16,
    /// The finger has lifted, and it is settling into its cell (ANIM-13).
    pub settling: bool,
}

/// Edit mode as the host draws it (APPS-12, ANIM-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditFrame {
    /// Edit mode is on: a tap on a ✕ asks to uninstall, a tap on an icon does nothing, and
    /// Done and the screen-reader actions are offered.
    pub on: bool,
    /// Every cell's scale in ten-thousandths: 10 000 out of edit mode, 9 200 in it.
    pub scale: u16,
    /// How far into edit mode the cells are, from 0 to 10 000. Done fades with it.
    pub progress: u16,
    /// The grid's first visible row, which `badges[0]` is for.
    pub first_row: u16,
    /// The ✕ scale, in ten-thousandths, of each visible row from `first_row`; rows further
    /// down take the last one. 0 while none shows, and a little over 10 000 as one pops in.
    pub badges: [u16; BADGE_ROWS],
}

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
    /// The Applications grid (APPS-01, APPS-02), with an app's long-press menu (APPS-11).
    pub apps: AppsFrame,
    /// The item of the current view under the finger, by its place in the view's cells
    /// (ANIM-10).
    pub pressed: Option<usize>,
    /// The pressed item's icon scale in ten-thousandths: 10 000 at rest, 9 200 held (ANIM-10).
    pub press_scale: u16,
    /// The grid's cells as apps come and go (ANIM-21).
    pub grid_change: GridChangeFrame,
    /// The picked-up icon, while one is, or settles into its cell (APPS-11, APPS-12).
    pub lifted: Option<LiftFrame>,
    /// Edit mode and its motion (APPS-12, ANIM-12).
    pub edit: EditFrame,
    /// Something is still moving or fading, so the host should draw another frame.
    pub animating: bool,
    /// The home menu, while it is open (HOME-10).
    pub menu: Option<MenuFrame>,
    /// The current view offers the home menu and nothing covers it, so screen readers can open
    /// it with an action (A11Y-05).
    pub menu_offered: bool,
    /// Settings covers the home views.
    pub settings_open: bool,
    /// The group Settings opened at, or `None` for its top.
    pub settings_group: Option<SettingsGroup>,
    /// The home views' solid background (HOME-11, THEME-04).
    pub background: Rgb,
    /// Menus and Settings (THEME-03).
    pub surface: Rgb,
    /// Text, and the indicator's highlighted segment.
    pub foreground: Rgb,
    /// The indicator's other segments.
    pub dim: Rgb,
    /// The dot on a newly installed app (APPS-07).
    pub accent: Rgb,
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

    /// What the host draws for the home views now, in the current theme (THEME-03). The
    /// Applications grid takes on its latest cells.
    pub fn frame(&mut self, shell: &HomeShell) -> HomeFrame {
        let tokens = tokens(shell.theme().state().theme);
        let state = shell.state();
        let apps = self.apps.frame(&shell.apps().state());
        let lifted = state.lift.app.as_ref().and_then(|app| {
            Some(LiftFrame {
                cell: self.apps.place_of(app)?,
                dx: state.lift.dx.round() as i32,
                dy: state.lift.dy.round() as i32,
                scale: scale(state.lift.scale),
                settling: state.lift.settling,
            })
        });
        let mode = &state.mode;
        HomeFrame {
            pages: HomeView::ALL.map(|view| state.page_offset_px(view).round() as i32),
            indicator_alpha: (state.carousel.indicator.clamp(0.0, 1.0) * 255.0).round() as u8,
            indicator_selected: state.nearest_view(),
            current: state.current,
            content_scale: (state.return_home.scale.clamp(0.0, 1.0) * 10_000.0).round() as u16,
            content_alpha: (state.return_home.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            apps,
            pressed: state.press.item,
            // A spring back overshoots full size a little.
            press_scale: scale(state.press.scale),
            grid_change: GridChangeFrame {
                appear_scale: ten_thousandths(state.grid_change.appear_scale),
                appear_alpha: ten_thousandths(state.grid_change.appear_alpha),
                leave_scale: ten_thousandths(state.grid_change.leave_scale),
                leave_alpha: ten_thousandths(state.grid_change.leave_alpha),
                shift: ten_thousandths(state.grid_change.shift),
            },
            lifted,
            edit: EditFrame {
                on: mode.on,
                scale: scale(mode.scale),
                progress: ten_thousandths(mode.progress),
                first_row: u16::try_from(mode.first_row).unwrap_or(u16::MAX),
                badges: mode.badges.map(scale),
            },
            animating: state.carousel.animating
                || state.return_home.animating
                || state.press.animating
                || state.grid_change.animating
                || state.lift.animating
                || mode.animating,
            menu: state.menu.map(|menu| MenuFrame {
                items: menu.items,
                anchor: menu
                    .anchor
                    .map(|(x, y)| (x.round() as i32, y.round() as i32)),
            }),
            menu_offered: state.menu_offered(),
            settings_open: state.settings_open,
            settings_group: state.settings_group,
            background: tokens.background,
            surface: tokens.surface,
            foreground: tokens.text_primary,
            dim: tokens.text_secondary,
            accent: tokens.accent,
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

/// A value from 0 to 1 in ten-thousandths.
fn ten_thousandths(value: f64) -> u16 {
    (value.clamp(0.0, 1.0) * 10_000.0).round() as u16
}

/// A scale in ten-thousandths. Springs and pops go a little past their ends.
fn scale(value: f64) -> u16 {
    (value.clamp(0.0, 1.5) * 10_000.0).round() as u16
}

/// A dispatcher that queues a view's intents, wrapped for the shell.
fn dispatch<I: 'static>(outbox: &Outbox, wrap: fn(I) -> ShellIntent) -> Dispatch<I> {
    let outbox = Rc::clone(outbox);
    Dispatch::new(move |intent| outbox.borrow_mut().push_back(wrap(intent)))
}
