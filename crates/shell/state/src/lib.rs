//! Home shell state (SRS 4.1): the carousel of views, the home menu, Home and Back, coming back
//! to the home, and the theme.
//!
//! [`HomeShell::compose`] is the only place features are put together (ARCH-03). Each feature
//! runs in its own [`Store`], so a failing feature never takes down the shell (ARCH-09).

mod carousel;
mod home_menu;
mod return_home;

use std::sync::Arc;
use std::time::Duration;

use meegone_apps_data::PersistentLayoutStore;
use meegone_apps_state::{AppsHolder, AppsIntent};
use meegone_core_data::{
    HomeTarget, PersistentSettingsStore, Platform, SettingsStore, SystemConfigAdapter,
};
use meegone_core_state::{Store, ThemeHolder, ThemeIntent};
use meegone_events_state::{EventsHolder, EventsIntent};
use meegone_lockimage_data::PersistentImageCache;
use meegone_lockimage_state::{LockImageHolder, LockImageIntent, LockImageState};
use meegone_recent_state::{RecentHolder, RecentIntent};
use meegone_settings_state::{SettingsHolder, SettingsIntent, SettingsState};

use carousel::{Carousel, Release};
pub use carousel::{CarouselState, Layout, PointerEvent, PointerKind};
pub use home_menu::{HomeMenu, MenuItem};
pub use meegone_settings_state::SettingsGroup;
use return_home::ReturnHome;
pub use return_home::ReturnHomeState;

/// The three home views, left to right (HOME-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeView {
    Events,
    /// Shown after setup and after a restart (HOME-05).
    #[default]
    Applications,
    Recent,
}

/// The direction of a view swipe in a left-to-right layout. Right-to-left layouts mirror it
/// (L10N-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The finger moves right to left, showing the view to the right.
    Next,
    /// The finger moves left to right, showing the view to the left.
    Previous,
}

impl HomeView {
    /// Every view, left to right in a left-to-right layout (HOME-02).
    pub const ALL: [HomeView; 3] = [HomeView::Events, HomeView::Applications, HomeView::Recent];

    /// The view's place in [`HomeView::ALL`], from 0.
    pub fn index(self) -> usize {
        match self {
            HomeView::Events => 0,
            HomeView::Applications => 1,
            HomeView::Recent => 2,
        }
    }

    /// The view at `index` in [`HomeView::ALL`], if there is one.
    pub fn from_index(index: usize) -> Option<HomeView> {
        HomeView::ALL.get(index).copied()
    }

    /// The view a swipe shows. Past either end it wraps around when `wrap` is on, and otherwise
    /// stays put (HOME-04).
    pub fn neighbour(self, direction: Direction, wrap: bool) -> Option<HomeView> {
        use Direction::{Next, Previous};
        use HomeView::{Applications, Events, Recent};
        match (self, direction) {
            (Events, Next) => Some(Applications),
            (Applications, Next) => Some(Recent),
            (Recent, Next) => wrap.then_some(Events),
            (Events, Previous) => wrap.then_some(Recent),
            (Applications, Previous) => Some(Events),
            (Recent, Previous) => Some(Applications),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShellState {
    /// The view on screen, or the one the carousel is settling on.
    pub current: HomeView,
    /// Settings covers the home views.
    pub settings_open: bool,
    /// The group Settings opened at, or `None` for its top.
    pub settings_group: Option<SettingsGroup>,
    /// The home menu, open over the current view (HOME-10). It covers the home views: they get
    /// no touches while it is open.
    pub menu: Option<HomeMenu>,
    pub carousel: CarouselState,
    /// The home content's scale and opacity while it comes back from an app (ANIM-09).
    pub return_home: ReturnHomeState,
}

impl ShellState {
    /// How far `view` is drawn from where it fills the screen, in pixels.
    pub fn page_offset_px(&self, view: HomeView) -> f64 {
        self.carousel.page_offset_px(self.current, view)
    }

    /// The view nearest to the carousel's position, which the view indicator highlights
    /// (HOME-09).
    pub fn nearest_view(&self) -> HomeView {
        self.carousel.nearest(self.current)
    }

    /// Whether the current view offers the home menu now, which it can open without a gesture
    /// (A11Y-05): Events and Applications do, while nothing covers them.
    pub fn menu_offered(&self) -> bool {
        !self.settings_open && self.menu.is_none() && HomeMenu::items_for(self.current).is_some()
    }
}

/// What the home keeps while the system may end MeeGone's process, so it comes back as it was
/// (REL-05). Open folders, menus, search, edit mode and close mode aren't kept, so ending the
/// process cancels them cleanly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedHome {
    pub view: HomeView,
    /// How far Applications was scrolled from its top, in pixels.
    pub apps_scroll_px: f64,
}

#[derive(Debug)]
pub enum ShellIntent {
    /// Shows the neighbouring view without a gesture, such as from a screen reader (A11Y-05).
    Swipe(Direction),
    /// The host laid the home views out, at start or after a size or direction change.
    Layout(Layout),
    /// A finger on the home views, which may drag the carousel (HOME-03).
    Pointer(PointerEvent),
    /// The display is about to draw a frame at this time on the host's monotonic clock.
    Frame(Duration),
    /// The system Home action while the home is on screen (HOME-07).
    Home,
    /// The system Back action (HOME-08).
    Back,
    /// The finger that is down has stayed on empty space for the platform's long-press time
    /// (HOME-10). The host sends it only where no element under the finger has its own
    /// long-press; the shell takes it only if the finger hasn't moved past the touch slop. In
    /// Events and Applications it opens the home menu, and in Recent it enters close mode
    /// (REC-07).
    LongPress,
    /// Opens the home menu without a gesture, from a screen-reader action (A11Y-05).
    OpenMenu,
    /// A touch outside the home menu closes it.
    CloseMenu,
    /// An item of the home menu was chosen (HOME-10).
    ChooseMenuItem(MenuItem),
    /// Opens Settings at its top (SET-01).
    OpenSettings,
    /// MeeGone came to the foreground: at start, after the system's role dialog, or back from
    /// another app or a settings screen. The default home app or the day may have changed
    /// (HOME-01, ONB-04).
    Resumed,
    /// The home is back on screen after an app or a system screen covered it (HOME-06). `home`
    /// is true when the system Home action brought it back, and false when the app closed or
    /// the user switched back another way. Sent after `Resumed`.
    Returned {
        home: bool,
    },
    /// The system ended MeeGone's process while it was out of sight, and this is how the home
    /// was (REL-05). Sent once, right after start, never after a device restart (HOME-05).
    Restore(SavedHome),
    /// The device's dark-theme setting changed (THEME-02).
    SystemThemeChanged,
    /// An app was installed, updated or removed, or a profile was paused or resumed
    /// (APPS-06, APPS-08, APPS-09).
    AppsChanged,
    /// A notification was posted or removed, or notification access changed (EVT-10).
    NotificationsChanged,
    Apps(AppsIntent),
    Events(EventsIntent),
    Recent(RecentIntent),
    LockImage(LockImageIntent),
    Settings(SettingsIntent),
}

/// One Home action closes at most this many open elements in a view, such as a folder inside
/// edit mode.
const MAX_OPEN_ELEMENTS: usize = 8;

/// Puts the features together and routes intents between them.
pub struct HomeShell {
    state: ShellState,
    settings_store: Arc<dyn SettingsStore>,
    system: Arc<dyn SystemConfigAdapter>,
    carousel: Carousel,
    return_home: ReturnHome,
    theme: Store<ThemeHolder>,
    apps: Store<AppsHolder>,
    events: Store<EventsHolder>,
    recent: Store<RecentHolder>,
    lockimage: Store<LockImageHolder>,
    settings: Store<SettingsHolder>,
}

impl HomeShell {
    /// Builds every feature's stores and state holders from the platform adapters. The three
    /// persistent stores share the platform's storage (SRS 7.1, REL-03).
    pub fn compose(platform: Platform) -> Self {
        let settings_store: Arc<dyn SettingsStore> =
            Arc::new(PersistentSettingsStore::new(platform.storage.clone()));
        let mut shell = Self {
            state: ShellState::default(),
            system: platform.system.clone(),
            carousel: Carousel::new(true, 1.0),
            return_home: ReturnHome::default(),
            theme: Store::new(ThemeHolder::new(
                settings_store.clone(),
                platform.system.clone(),
            )),
            apps: Store::new(AppsHolder::new(
                platform.apps.clone(),
                Arc::new(PersistentLayoutStore::new(platform.storage.clone())),
            )),
            events: Store::new(EventsHolder::new(platform.notifications.clone())),
            recent: Store::new(RecentHolder::new(
                platform.usage.clone(),
                platform.apps.clone(),
                settings_store.clone(),
                platform.system.clone(),
            )),
            lockimage: Store::new(LockImageHolder::new(
                settings_store.clone(),
                Arc::new(PersistentImageCache::new(platform.storage.clone())),
                platform.wallpaper.clone(),
                platform.scheduler.clone(),
            )),
            settings: Store::new(SettingsHolder::new(
                settings_store.clone(),
                platform.notifications.clone(),
                platform.usage.clone(),
                platform.system.clone(),
            )),
            settings_store,
        };
        // Loading happens inside each store, so a feature that fails to load is contained
        // (ARCH-09).
        shell.theme.dispatch(ThemeIntent::Refresh);
        shell.apps.dispatch(AppsIntent::Refresh);
        shell.events.dispatch(EventsIntent::Refresh);
        shell.recent.dispatch(RecentIntent::Refresh);
        shell.lockimage.dispatch(LockImageIntent::Refresh);
        shell.settings.dispatch(SettingsIntent::Refresh);
        shell.configure_carousel();
        shell
    }

    pub fn state(&self) -> &ShellState {
        &self.state
    }

    /// What to keep in case the system ends MeeGone's process (REL-05).
    pub fn saved(&self) -> SavedHome {
        SavedHome {
            view: self.state.current,
            apps_scroll_px: self.apps.state().scroll_px,
        }
    }

    pub fn theme(&self) -> &Store<ThemeHolder> {
        &self.theme
    }

    pub fn apps(&self) -> &Store<AppsHolder> {
        &self.apps
    }

    pub fn events(&self) -> &Store<EventsHolder> {
        &self.events
    }

    pub fn recent(&self) -> &Store<RecentHolder> {
        &self.recent
    }

    pub fn lockimage(&self) -> &Store<LockImageHolder> {
        &self.lockimage
    }

    pub fn settings(&self) -> &Store<SettingsHolder> {
        &self.settings
    }

    /// Calls `listener` with every new lock-screen image state, for its content shown inside
    /// Events and Settings (ADR 0002).
    pub fn subscribe_lockimage(&mut self, listener: impl FnMut(&Arc<LockImageState>) + 'static) {
        self.lockimage.subscribe(listener);
    }

    /// Calls `listener` with every new settings state, for the banner that offers to make
    /// MeeGone the default home app on the home views (ONB-04, ADR 0002).
    pub fn subscribe_settings(&mut self, listener: impl FnMut(&Arc<SettingsState>) + 'static) {
        self.settings.subscribe(listener);
    }

    pub fn dispatch(&mut self, intent: ShellIntent) {
        match intent {
            ShellIntent::Swipe(direction) => self.swipe(direction),
            ShellIntent::Layout(layout) => {
                if layout != self.state.carousel.layout {
                    // Folding, rotating or resizing keeps what is open (COMP-03). The menu
                    // opened where the finger was, which means nothing in the new layout, so
                    // it moves to the centre.
                    if let Some(menu) = &mut self.state.menu {
                        menu.anchor = None;
                    }
                }
                let release = self.carousel.layout(layout, self.state.current);
                self.settle(release);
            }
            ShellIntent::Pointer(event) => {
                if event.kind == PointerKind::Down {
                    // A touch never waits for the return motion (ANIM-03).
                    self.return_home.finish();
                }
                // Settings and the home menu cover the home views, but a finger that was
                // already down may still lift.
                let covered = self.state.settings_open || self.state.menu.is_some();
                if !covered || matches!(event.kind, PointerKind::Up | PointerKind::Cancel) {
                    let release = self.carousel.pointer(event, self.state.current);
                    self.settle(release);
                }
            }
            ShellIntent::Frame(at) => {
                self.carousel.frame(at);
                self.return_home.frame(at);
            }
            ShellIntent::Home => self.home(),
            ShellIntent::Back => self.back(),
            ShellIntent::LongPress => self.long_press(),
            ShellIntent::OpenMenu => self.open_menu(None),
            ShellIntent::CloseMenu => self.state.menu = None,
            ShellIntent::ChooseMenuItem(item) => self.choose(item),
            ShellIntent::OpenSettings => self.open_settings(None),
            ShellIntent::Resumed => {
                self.settings.dispatch(SettingsIntent::Refresh);
                // The animation speed may have changed in system settings (ANIM-05).
                self.configure_carousel();
            }
            ShellIntent::Returned { home } => self.returned(home),
            ShellIntent::Restore(saved) => self.restore(saved),
            ShellIntent::SystemThemeChanged => self.theme.dispatch(ThemeIntent::Refresh),
            ShellIntent::AppsChanged => {
                self.apps.dispatch(AppsIntent::Refresh);
                self.recent.dispatch(RecentIntent::Refresh);
            }
            ShellIntent::NotificationsChanged => self.events.dispatch(EventsIntent::Refresh),
            ShellIntent::Apps(intent) => self.apps.dispatch(intent),
            ShellIntent::Events(intent) => self.events.dispatch(intent),
            ShellIntent::Recent(intent) => self.recent.dispatch(intent),
            ShellIntent::LockImage(intent) => self.lockimage.dispatch(intent),
            ShellIntent::Settings(intent) => {
                self.settings.dispatch(intent);
                // The theme choice and wrap-around live in the settings store; take them on
                // (THEME-06, HOME-04).
                self.theme.dispatch(ThemeIntent::Refresh);
                self.configure_carousel();
            }
        }
        self.state.carousel = self.carousel.state();
        self.state.return_home = self.return_home.state();
    }

    /// Takes on the wrap-around setting and the system animation speed, which the carousel
    /// keeps rather than reading them on every touch.
    fn configure_carousel(&mut self) {
        let wrap = self.settings_store.get().wrap_around;
        self.carousel.configure(wrap, self.system.animation_scale());
        self.state.carousel = self.carousel.state();
    }

    fn swipe(&mut self, direction: Direction) {
        let wrap = self.state.carousel.wrap;
        if let Some(view) = self.state.current.neighbour(direction, wrap) {
            self.move_to(view, 0.0, None);
        }
    }

    /// A drag ended: settle on the view it chose.
    fn settle(&mut self, release: Option<Release>) {
        if let Some(Release {
            target,
            velocity,
            at,
        }) = release
        {
            self.move_to(target, velocity, Some(at));
        }
    }

    /// Moves the carousel to `view` from wherever it is, and shows it.
    fn move_to(&mut self, view: HomeView, velocity: f64, at: Option<Duration>) {
        self.carousel.settle(self.state.current, view, velocity, at);
        if view != self.state.current {
            self.show(view);
        }
    }

    /// HOME-10: a long-press on empty space opens the home menu where the finger is, or, in
    /// Recent, enters close mode (REC-07). Either way it ticks (ANIM-24).
    fn long_press(&mut self) {
        if self.state.settings_open || self.state.menu.is_some() {
            return;
        }
        let Some(at) = self.carousel.long_press() else {
            return;
        };
        match self.state.current {
            HomeView::Recent => self.recent.dispatch(RecentIntent::EnterCloseMode),
            HomeView::Events | HomeView::Applications => self.open_menu(Some(at)),
        }
        self.system.haptic_tick();
    }

    /// Opens the current view's home menu at `anchor`, or centred, if it has one (HOME-10).
    fn open_menu(&mut self, anchor: Option<(f64, f64)>) {
        if self.state.settings_open || self.state.menu.is_some() {
            return;
        }
        let Some(items) = HomeMenu::items_for(self.state.current) else {
            return;
        };
        // The menu covers the home views, so a drag on them ends.
        let release = self.carousel.end_gesture(self.state.current);
        self.settle(release);
        self.state.menu = Some(HomeMenu { items, anchor });
    }

    /// Closes the home menu and does what its item says (HOME-10, SET-01).
    fn choose(&mut self, item: MenuItem) {
        if !self.state.menu.is_some_and(|menu| menu.offers(item)) {
            return;
        }
        self.state.menu = None;
        match item {
            MenuItem::LauncherSettings => self.open_settings(None),
            MenuItem::LockScreenImage => self.open_settings(Some(SettingsGroup::LockScreenImage)),
            MenuItem::EditApplications => self.apps.dispatch(AppsIntent::EnterEditMode),
        }
    }

    /// Opens Settings over the home views, at `group` or at its top.
    fn open_settings(&mut self, group: Option<SettingsGroup>) {
        // Settings covers the home views, so a drag on them ends and the menu closes.
        let release = self.carousel.end_gesture(self.state.current);
        self.settle(release);
        self.state.menu = None;
        self.settings.dispatch(SettingsIntent::Refresh);
        self.state.settings_open = true;
        self.state.settings_group = group;
    }

    /// Closes Settings. Returns whether it was open.
    fn close_settings(&mut self) -> bool {
        self.state.settings_group = None;
        std::mem::take(&mut self.state.settings_open)
    }

    /// HOME-07: close what is open; otherwise go to Applications, or, on Applications, scroll
    /// it to the top.
    fn home(&mut self) {
        if self.close_settings() {
            return;
        }
        // The menu and everything open in the view close together.
        let menu_closed = self.state.menu.take().is_some();
        if self.close_everything_in(self.state.current) || menu_closed {
            return;
        }
        if self.state.current != HomeView::Applications || self.carousel.state().dragging {
            // A drag in progress ends too.
            self.move_to(HomeView::Applications, 0.0, None);
        } else {
            // At rest on Applications, or settling onto it, so a second quick Home after
            // leaving another view scrolls too.
            self.apps.dispatch(AppsIntent::ScrollToTop);
        }
    }

    /// HOME-08: close the topmost open element (Settings, then the home menu, then the view's
    /// own); otherwise do nothing.
    fn back(&mut self) {
        if !self.close_settings() && self.state.menu.take().is_none() {
            self.close_topmost(self.state.current);
        }
    }

    /// HOME-06: the home shows the view the user left. When Home brought it back and the Home
    /// target is Applications, it shows Applications instead, with nothing open. Recent
    /// refreshes (REC-09), and the content comes in with its own motion unless the system
    /// animates the return (ANIM-09).
    fn returned(&mut self, home: bool) {
        if home && self.settings_store.get().home_target == HomeTarget::Applications {
            self.close_settings();
            self.state.menu = None;
            for view in HomeView::ALL {
                self.close_everything_in(view);
            }
            self.jump_to(HomeView::Applications);
        }
        self.recent.dispatch(RecentIntent::Refresh);
        if self.system.animates_return_home() {
            self.return_home.finish();
        } else {
            self.return_home.start(self.carousel.animation_scale());
        }
    }

    /// REL-05: the view and the Applications scroll position as they were. Nothing else was
    /// kept, so any edit or close mode is cancelled.
    fn restore(&mut self, saved: SavedHome) {
        self.jump_to(saved.view);
        self.apps
            .dispatch(AppsIntent::Scrolled(saved.apps_scroll_px));
    }

    /// Shows `view` at once, with nothing moving, while the home is out of sight.
    fn jump_to(&mut self, view: HomeView) {
        self.carousel.reset();
        if view != self.state.current {
            self.show(view);
        }
    }

    /// Closes everything open in `view`, topmost first. Returns whether anything was.
    fn close_everything_in(&mut self, view: HomeView) -> bool {
        let mut closed_any = false;
        for _ in 0..MAX_OPEN_ELEMENTS {
            if !self.close_topmost(view) {
                break;
            }
            closed_any = true;
        }
        closed_any
    }

    fn close_topmost(&mut self, view: HomeView) -> bool {
        match view {
            HomeView::Events => self.events.back(),
            HomeView::Applications => self.apps.back(),
            HomeView::Recent => self.recent.back(),
        }
    }

    fn show(&mut self, view: HomeView) {
        self.state.current = view;
        // The menu belongs to the view it opened over.
        self.state.menu = None;
        // Recent refreshes whenever it becomes visible (REC-09); Events does too, to stay live.
        match view {
            HomeView::Events => self.events.dispatch(EventsIntent::Refresh),
            HomeView::Recent => self.recent.dispatch(RecentIntent::Refresh),
            HomeView::Applications => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_apps_data::{Layout as SavedLayout, LayoutEntry, LayoutStore};
    use meegone_apps_state::{FolderId, GridItem};
    use meegone_core_data::fakes::{FakeApps, Fakes, app, app_id};
    use meegone_core_data::{App, AppId, AppsAdapter, DataError, Profile, RecentUse, Shortcut};
    use meegone_core_state::{Access, Health, Theme, ThemeChoice};

    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// A shell whose Applications grid holds one folder, with Maps and Mail.
    fn with_a_folder() -> (Fakes, HomeShell) {
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![app("maps", "Maps"), app("mail", "Mail")];
        PersistentLayoutStore::new(fakes.storage.clone())
            .save(&SavedLayout {
                entries: vec![LayoutEntry::Folder {
                    id: FolderId(1),
                    name: "Folder".into(),
                    apps: vec![app_id("maps"), app_id("mail")],
                }],
            })
            .unwrap();
        let shell = HomeShell::compose(fakes.platform());
        (fakes, shell)
    }

    fn always_applications(shell: &mut HomeShell) {
        shell.dispatch(ShellIntent::Settings(SettingsIntent::SetHomeTarget(
            HomeTarget::Applications,
        )));
    }

    fn swipes(shell: &mut HomeShell, direction: Direction, count: usize) -> Vec<HomeView> {
        (0..count)
            .map(|_| {
                shell.dispatch(ShellIntent::Swipe(direction));
                shell.state().current
            })
            .collect()
    }

    #[test]
    fn starts_on_applications() {
        // HOME-05
        let shell = HomeShell::compose(Fakes::default().platform());

        assert_eq!(shell.state().current, HomeView::Applications);
    }

    #[test]
    fn views_wrap_around_in_both_directions() {
        // HOME-02, HOME-04, AC-02
        let mut shell = HomeShell::compose(Fakes::default().platform());

        assert_eq!(
            swipes(&mut shell, Direction::Next, 3),
            [HomeView::Recent, HomeView::Events, HomeView::Applications]
        );
        assert_eq!(
            swipes(&mut shell, Direction::Previous, 3),
            [HomeView::Events, HomeView::Recent, HomeView::Applications]
        );
    }

    #[test]
    fn with_wrap_around_off_the_ends_stay_put() {
        // HOME-04
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Settings(SettingsIntent::SetWrapAround(false)));

        assert_eq!(
            swipes(&mut shell, Direction::Next, 2),
            [HomeView::Recent, HomeView::Recent]
        );
    }

    #[test]
    fn home_goes_to_applications() {
        // HOME-07, AC-04
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        assert_eq!(shell.state().current, HomeView::Events);

        shell.dispatch(ShellIntent::Home);

        assert_eq!(shell.state().current, HomeView::Applications);
    }

    #[test]
    fn home_first_closes_everything_open_in_the_current_view() {
        // HOME-07
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        shell.dispatch(ShellIntent::Recent(RecentIntent::EnterCloseMode));

        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().current, HomeView::Recent);
        assert!(!shell.recent().state().close_mode);

        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().current, HomeView::Applications);
    }

    #[test]
    fn back_closes_the_topmost_element_and_otherwise_does_nothing() {
        // HOME-08
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        shell.dispatch(ShellIntent::OpenSettings);

        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().settings_open);
        assert!(shell.apps().state().edit_mode);

        shell.dispatch(ShellIntent::Back);
        assert!(!shell.apps().state().edit_mode);

        shell.dispatch(ShellIntent::Back);
        assert_eq!(*shell.state(), ShellState::default());
    }

    #[test]
    fn a_theme_chosen_in_settings_is_applied_by_the_shell() {
        // THEME-01, THEME-06; Settings and the shell share theme through core (ADR 0002).
        let mut shell = HomeShell::compose(Fakes::default().platform());

        shell.dispatch(ShellIntent::Settings(SettingsIntent::SetTheme(
            ThemeChoice::Dark,
        )));

        assert_eq!(shell.theme().state().theme, Theme::Dark);
    }

    #[test]
    fn the_system_theme_is_followed_by_default() {
        // THEME-02
        let fakes = Fakes::default();
        let mut shell = HomeShell::compose(fakes.platform());

        *fakes.system.dark.lock().unwrap() = true;
        shell.dispatch(ShellIntent::SystemThemeChanged);

        assert_eq!(shell.theme().state().theme, Theme::Dark);
    }

    #[test]
    fn an_installed_app_reaches_applications_and_a_removed_one_leaves() {
        // APPS-06, APPS-08: the system reports the change and the shell refreshes.
        let fakes = Fakes::default();
        fakes.apps.apps.lock().unwrap().push(app("mail", "Mail"));
        let mut shell = HomeShell::compose(fakes.platform());

        *fakes.apps.apps.lock().unwrap() = vec![app("maps", "Maps")];
        shell.dispatch(ShellIntent::AppsChanged);

        assert!(matches!(
            &shell.apps().state().items[..],
            [GridItem::App(app)] if app.label == "Maps"
        ));
    }

    #[test]
    fn coming_back_to_the_foreground_rechecks_the_default_home_app() {
        // HOME-01, ONB-04: the role may have changed in the system dialog or in system settings.
        let fakes = Fakes::default();
        let mut shell = HomeShell::compose(fakes.platform());
        assert!(shell.settings().state().home_banner);

        *fakes.system.default_home.lock().unwrap() = true;
        shell.dispatch(ShellIntent::Resumed);

        assert!(shell.settings().state().default_home);
        assert!(!shell.settings().state().home_banner);
    }

    #[test]
    fn notification_changes_refresh_events() {
        // EVT-10
        let fakes = Fakes::default();
        let mut shell = HomeShell::compose(fakes.platform());
        *fakes.notifications.access.lock().unwrap() = Access::Granted;

        shell.dispatch(ShellIntent::NotificationsChanged);

        assert_eq!(shell.events().state().access, Access::Granted);
    }

    /// Works like [`FakeApps`], except that it panics when asked to launch an app.
    struct BrokenLauncher(FakeApps);

    impl AppsAdapter for BrokenLauncher {
        fn launchable_apps(&self) -> Result<Vec<App>, DataError> {
            self.0.launchable_apps()
        }

        fn profiles(&self) -> Result<Vec<Profile>, DataError> {
            self.0.profiles()
        }

        fn launch(&self, _app: &AppId) -> Result<(), DataError> {
            panic!("launcher bug");
        }

        fn shortcuts(&self, app: &AppId) -> Result<Vec<Shortcut>, DataError> {
            self.0.shortcuts(app)
        }

        fn launch_shortcut(&self, shortcut: &Shortcut) -> Result<(), DataError> {
            self.0.launch_shortcut(shortcut)
        }

        fn request_uninstall(&self, app: &AppId) -> Result<(), DataError> {
            self.0.request_uninstall(app)
        }

        fn open_app_info(&self, app: &AppId) -> Result<(), DataError> {
            self.0.open_app_info(app)
        }
    }

    #[test]
    fn a_failing_feature_never_takes_down_the_shell_or_the_others() {
        // ARCH-09, REL-01
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        let broken = FakeApps::default();
        broken.apps.lock().unwrap().push(app("mail", "Mail"));
        platform.apps = Arc::new(BrokenLauncher(broken));
        let mut shell = HomeShell::compose(platform);

        shell.dispatch(ShellIntent::Apps(AppsIntent::Launch(app_id("mail"))));

        assert_eq!(
            *shell.apps().health(),
            Health::Failed("launcher bug".into())
        );
        assert_eq!(*shell.events().health(), Health::Ready);
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        assert_eq!(shell.state().current, HomeView::Events);
        shell.dispatch(ShellIntent::Events(EventsIntent::OpenAccessSettings));
        assert_eq!(*fakes.notifications.access_screen_opened.lock().unwrap(), 1);
        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().current, HomeView::Applications);
        shell.dispatch(ShellIntent::Back);
        assert_eq!(shell.state().current, HomeView::Applications);
    }

    #[test]
    fn home_on_applications_scrolls_it_to_the_top() {
        // HOME-07, AC-04
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(900.0)));

        shell.dispatch(ShellIntent::Home);

        assert_eq!(shell.state().current, HomeView::Applications);
        assert_eq!(shell.apps().state().scroll_px, 0.0);
    }

    #[test]
    fn a_second_quick_home_scrolls_while_the_views_still_settle() {
        // HOME-07, AC-04: Home on Events moves to Applications; Home again scrolls it to the
        // top, even before the row has come to rest.
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 1000.0,
            density: 2.0,
            touch_slop_px: 16.0,
            rtl: false,
        }));
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(900.0)));
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        for frame in 0..100 {
            shell.dispatch(ShellIntent::Frame(ms(frame * 16)));
        }
        assert_eq!(shell.state().current, HomeView::Events);

        shell.dispatch(ShellIntent::Home);
        shell.dispatch(ShellIntent::Frame(ms(2_000)));
        shell.dispatch(ShellIntent::Frame(ms(2_050)));
        assert_eq!(shell.state().current, HomeView::Applications);
        assert_ne!(shell.state().carousel.offset, 0.0);
        assert_eq!(shell.apps().state().scroll_px, 900.0);

        shell.dispatch(ShellIntent::Home);

        assert_eq!(shell.apps().state().scroll_px, 0.0);
        assert!(shell.state().carousel.animating);
    }

    #[test]
    fn home_with_an_open_folder_closes_it_and_applications_stays_where_it_was() {
        // HOME-07, issue #6
        let (_, mut shell) = with_a_folder();
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(480.0)));
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenFolder(FolderId(1))));
        assert_eq!(shell.apps().state().open_folder, Some(FolderId(1)));

        shell.dispatch(ShellIntent::Home);

        let apps = shell.apps().state();
        assert_eq!(apps.open_folder, None);
        assert_eq!(apps.scroll_px, 480.0);
        assert_eq!(shell.state().current, HomeView::Applications);
    }

    #[test]
    fn back_with_nothing_open_does_nothing() {
        // HOME-08, issue #6
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(480.0)));
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        let before = shell.state().clone();

        shell.dispatch(ShellIntent::Back);

        assert_eq!(*shell.state(), before);
        assert_eq!(shell.apps().state().scroll_px, 480.0);
    }

    #[test]
    fn coming_back_shows_the_view_the_user_left_as_it_was() {
        // HOME-06: by default, Home from an app shows the last view, with what was open.
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        shell.dispatch(ShellIntent::Returned { home: true });
        assert_eq!(shell.state().current, HomeView::Applications);
        assert!(shell.apps().state().edit_mode);

        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        shell.dispatch(ShellIntent::Returned { home: true });
        assert_eq!(shell.state().current, HomeView::Recent);
    }

    #[test]
    fn with_always_applications_home_from_an_app_shows_applications_with_nothing_open() {
        // HOME-06: shown at once, with nothing moving, and the grid where it was.
        let (_, mut shell) = with_a_folder();
        always_applications(&mut shell);
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(480.0)));
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenFolder(FolderId(1))));
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        shell.dispatch(ShellIntent::Recent(RecentIntent::EnterCloseMode));
        shell.dispatch(ShellIntent::OpenSettings);

        shell.dispatch(ShellIntent::Returned { home: true });

        let state = shell.state();
        assert_eq!(state.current, HomeView::Applications);
        assert!(!state.settings_open);
        assert_eq!(state.carousel.offset, 0.0);
        assert!(!state.carousel.animating);
        let apps = shell.apps().state();
        assert_eq!(apps.open_folder, None);
        assert!(!apps.edit_mode);
        assert_eq!(apps.scroll_px, 480.0);
        assert!(!shell.recent().state().close_mode);
    }

    #[test]
    fn coming_back_without_home_keeps_the_last_view_even_with_always_applications() {
        // HOME-06, ADR 0007: the setting is "Home button opens", so Back from an app, such as
        // the system screen that grants notification access, returns to the view it left.
        let mut shell = HomeShell::compose(Fakes::default().platform());
        always_applications(&mut shell);
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));

        shell.dispatch(ShellIntent::Returned { home: false });

        assert_eq!(shell.state().current, HomeView::Events);
    }

    #[test]
    fn coming_back_refreshes_recent() {
        // REC-09: Recent refreshes when MeeGone returns to the foreground.
        let fakes = Fakes::default();
        *fakes.usage.access.lock().unwrap() = Access::Granted;
        *fakes.apps.apps.lock().unwrap() = vec![app("maps", "Maps"), app("mail", "Mail")];
        *fakes.system.now_ms.lock().unwrap() = 1_000_000;
        let used = |package: &str, at: u64| RecentUse {
            app: app_id(package),
            last_used_ms: at,
        };
        *fakes.usage.uses.lock().unwrap() = vec![used("maps", 900_000)];
        let mut shell = HomeShell::compose(fakes.platform());

        fakes.usage.uses.lock().unwrap().push(used("mail", 999_000));
        shell.dispatch(ShellIntent::Returned { home: false });

        let cards = &shell.recent().state().cards;
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].app.label, "Mail");
    }

    #[test]
    fn the_home_content_comes_back_with_its_own_motion_unless_the_system_animates_it() {
        // ANIM-09
        let fakes = Fakes::default();
        let mut shell = HomeShell::compose(fakes.platform());
        assert_eq!(shell.state().return_home, ReturnHomeState::default());

        shell.dispatch(ShellIntent::Returned { home: true });
        let first = shell.state().return_home;
        assert!(first.animating);
        assert_eq!((first.scale, first.alpha), (0.94, 0.0));
        shell.dispatch(ShellIntent::Frame(ms(1_000)));
        shell.dispatch(ShellIntent::Frame(ms(1_125)));
        assert!(shell.state().return_home.alpha > 0.5);
        shell.dispatch(ShellIntent::Frame(ms(1_250)));
        assert_eq!(shell.state().return_home, ReturnHomeState::default());

        *fakes.system.animates_return_home.lock().unwrap() = true;
        shell.dispatch(ShellIntent::Returned { home: true });
        assert_eq!(shell.state().return_home, ReturnHomeState::default());
    }

    #[test]
    fn with_animations_removed_the_content_comes_back_at_once() {
        // ANIM-05, ANIM-09
        let fakes = Fakes::default();
        *fakes.system.animation_scale.lock().unwrap() = 0.0;
        let mut shell = HomeShell::compose(fakes.platform());

        shell.dispatch(ShellIntent::Returned { home: false });

        assert_eq!(shell.state().return_home, ReturnHomeState::default());
    }

    #[test]
    fn a_touch_shows_the_returning_content_at_once() {
        // ANIM-03: no animation blocks input.
        let mut shell = HomeShell::compose(Fakes::default().platform());
        shell.dispatch(ShellIntent::Returned { home: true });
        shell.dispatch(ShellIntent::Frame(ms(0)));

        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Down,
            x: 100.0,
            y: 100.0,
            at: ms(40),
        }));

        assert_eq!(shell.state().return_home, ReturnHomeState::default());
    }

    #[test]
    fn after_the_system_ends_the_process_the_home_comes_back_as_it_was() {
        // REL-05: the view and the scroll position come back; edit and close mode don't.
        let fakes = Fakes::default();
        let mut before = HomeShell::compose(fakes.platform());
        before.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(720.0)));
        before.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        before.dispatch(ShellIntent::Swipe(Direction::Next));
        before.dispatch(ShellIntent::Recent(RecentIntent::EnterCloseMode));
        let saved = before.saved();
        assert_eq!(
            saved,
            SavedHome {
                view: HomeView::Recent,
                apps_scroll_px: 720.0
            }
        );

        let mut after = HomeShell::compose(fakes.platform());
        after.dispatch(ShellIntent::Restore(saved));

        let state = after.state();
        assert_eq!(state.current, HomeView::Recent);
        assert_eq!(state.carousel.offset, 0.0);
        assert!(!state.carousel.animating);
        assert_eq!(after.apps().state().scroll_px, 720.0);
        assert!(!after.apps().state().edit_mode);
        assert!(!after.recent().state().close_mode);
    }

    #[test]
    fn a_restart_shows_applications_whatever_showed_before() {
        // HOME-05: after a device restart nothing is restored.
        let fakes = Fakes::default();
        let mut before = HomeShell::compose(fakes.platform());
        before.dispatch(ShellIntent::Swipe(Direction::Next));

        let after = HomeShell::compose(fakes.platform());

        assert_eq!(after.state().current, HomeView::Applications);
    }

    /// A phone 1000 px wide, laid out.
    fn laid_out(fakes: &Fakes) -> HomeShell {
        let mut shell = HomeShell::compose(fakes.platform());
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 1000.0,
            density: 2.0,
            touch_slop_px: 16.0,
            rtl: false,
        }));
        shell
    }

    /// Shows the neighbouring view and lets the row come to rest.
    fn go(shell: &mut HomeShell, direction: Direction) {
        shell.dispatch(ShellIntent::Swipe(direction));
        for frame in 0..100 {
            shell.dispatch(ShellIntent::Frame(ms(frame * 16)));
        }
        assert!(!shell.state().carousel.animating);
    }

    /// A finger held still at `(x, y)` until the host's long-press time has passed.
    fn hold(shell: &mut HomeShell, x: f64, y: f64) {
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Down,
            x,
            y,
            at: ms(10_000),
        }));
        shell.dispatch(ShellIntent::LongPress);
    }

    fn lift(shell: &mut HomeShell) {
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Up,
            x: 0.0,
            y: 0.0,
            at: ms(10_500),
        }));
    }

    fn ticks(fakes: &Fakes) -> u32 {
        *fakes.system.haptic_ticks.lock().unwrap()
    }

    #[test]
    fn a_long_press_on_applications_opens_the_home_menu_where_the_finger_is() {
        // HOME-10, ANIM-24
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes);

        hold(&mut shell, 300.0, 1200.0);

        assert_eq!(
            shell.state().menu,
            Some(HomeMenu {
                items: &MenuItem::ALL,
                anchor: Some((300.0, 1200.0)),
            })
        );
        assert_eq!(ticks(&fakes), 1);
        // The rest of the gesture goes nowhere, so lifting taps nothing.
        assert!(shell.state().carousel.held);
        lift(&mut shell);
        assert!(!shell.state().carousel.held);
        assert!(shell.state().menu.is_some());
    }

    #[test]
    fn on_events_the_home_menu_has_no_edit_applications() {
        // HOME-10, the issue's acceptance criteria
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes);
        go(&mut shell, Direction::Previous);

        hold(&mut shell, 500.0, 900.0);

        let menu = shell.state().menu.expect("the menu is open");
        assert_eq!(
            menu.items,
            [MenuItem::LauncherSettings, MenuItem::LockScreenImage]
        );
    }

    #[test]
    fn in_recent_a_long_press_enters_close_mode_instead() {
        // HOME-10, REC-07, ANIM-24
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes);
        go(&mut shell, Direction::Next);

        hold(&mut shell, 500.0, 900.0);

        assert_eq!(shell.state().menu, None);
        assert!(shell.recent().state().close_mode);
        assert_eq!(ticks(&fakes), 1);
    }

    #[test]
    fn under_settings_or_the_menu_a_long_press_does_nothing() {
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes);
        shell.dispatch(ShellIntent::OpenSettings);
        hold(&mut shell, 500.0, 900.0);
        assert_eq!(shell.state().menu, None);
        lift(&mut shell);

        shell.dispatch(ShellIntent::Back);
        shell.dispatch(ShellIntent::OpenMenu);
        hold(&mut shell, 500.0, 900.0);
        assert_eq!(shell.state().menu.map(|menu| menu.anchor), Some(None));
        assert!(!shell.state().carousel.held);

        assert_eq!(ticks(&fakes), 0);
    }

    #[test]
    fn back_closes_the_menu_before_edit_mode() {
        // HOME-08
        let mut shell = laid_out(&Fakes::default());
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        shell.dispatch(ShellIntent::OpenMenu);

        shell.dispatch(ShellIntent::Back);
        assert_eq!(shell.state().menu, None);
        assert!(shell.apps().state().edit_mode);

        shell.dispatch(ShellIntent::Back);
        assert!(!shell.apps().state().edit_mode);
    }

    #[test]
    fn home_closes_the_menu_and_everything_open_with_it_and_goes_no_further() {
        // HOME-07: Home first closes any open menu, and only then moves or scrolls.
        let mut shell = laid_out(&Fakes::default());
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(480.0)));
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        hold(&mut shell, 500.0, 900.0);
        lift(&mut shell);

        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().menu, None);
        assert!(!shell.apps().state().edit_mode);
        assert_eq!(shell.apps().state().scroll_px, 480.0);
        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.apps().state().scroll_px, 0.0);

        go(&mut shell, Direction::Previous);
        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().menu, None);
        assert_eq!(shell.state().current, HomeView::Events);
        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().current, HomeView::Applications);
    }

    #[test]
    fn each_menu_item_does_what_it_says() {
        // HOME-10, SET-01
        let mut shell = laid_out(&Fakes::default());

        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::ChooseMenuItem(MenuItem::LauncherSettings));
        let state = shell.state();
        assert_eq!(state.menu, None);
        assert!(state.settings_open);
        assert_eq!(state.settings_group, None);
        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().settings_open);

        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::ChooseMenuItem(MenuItem::LockScreenImage));
        assert!(shell.state().settings_open);
        assert_eq!(
            shell.state().settings_group,
            Some(SettingsGroup::LockScreenImage)
        );
        shell.dispatch(ShellIntent::Home);
        assert!(!shell.state().settings_open);
        assert_eq!(shell.state().settings_group, None);

        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
        assert_eq!(shell.state().menu, None);
        assert!(shell.apps().state().edit_mode);
    }

    #[test]
    fn an_item_the_menu_doesnt_offer_or_with_no_menu_open_does_nothing() {
        let mut shell = laid_out(&Fakes::default());
        shell.dispatch(ShellIntent::ChooseMenuItem(MenuItem::LauncherSettings));
        assert!(!shell.state().settings_open);

        go(&mut shell, Direction::Previous);
        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));

        assert!(shell.state().menu.is_some());
        assert!(!shell.apps().state().edit_mode);
    }

    #[test]
    fn a_screen_reader_opens_the_menu_in_events_and_applications_only() {
        // A11Y-05: centred, since no finger says where; no long-press, so no tick.
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes);
        assert!(shell.state().menu_offered());

        shell.dispatch(ShellIntent::OpenMenu);
        assert_eq!(
            shell.state().menu,
            Some(HomeMenu {
                items: &MenuItem::ALL,
                anchor: None,
            })
        );
        assert!(!shell.state().menu_offered());
        shell.dispatch(ShellIntent::CloseMenu);
        assert_eq!(shell.state().menu, None);

        go(&mut shell, Direction::Next);
        assert!(!shell.state().menu_offered());
        shell.dispatch(ShellIntent::OpenMenu);
        assert_eq!(shell.state().menu, None);
        assert_eq!(ticks(&fakes), 0);
    }

    #[test]
    fn while_the_menu_is_open_a_drag_moves_nothing() {
        // The menu covers the home views.
        let mut shell = laid_out(&Fakes::default());
        shell.dispatch(ShellIntent::OpenMenu);

        for (kind, x, at) in [
            (PointerKind::Down, 800.0, 0),
            (PointerKind::Move, 600.0, 16),
            (PointerKind::Move, 300.0, 32),
        ] {
            shell.dispatch(ShellIntent::Pointer(PointerEvent {
                kind,
                x,
                y: 900.0,
                at: ms(at),
            }));
        }

        assert!(!shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, 0.0);
        assert!(shell.state().menu.is_some());
    }

    #[test]
    fn opening_the_menu_ends_a_drag() {
        let mut shell = laid_out(&Fakes::default());
        for (kind, x, at) in [
            (PointerKind::Down, 800.0, 0),
            (PointerKind::Move, 600.0, 16),
        ] {
            shell.dispatch(ShellIntent::Pointer(PointerEvent {
                kind,
                x,
                y: 900.0,
                at: ms(at),
            }));
        }
        assert!(shell.state().carousel.dragging);

        shell.dispatch(ShellIntent::OpenMenu);

        assert!(!shell.state().carousel.dragging);
        assert!(shell.state().menu.is_some());
    }

    #[test]
    fn the_menu_closes_when_the_view_changes() {
        let mut shell = laid_out(&Fakes::default());
        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        assert_eq!(shell.state().menu, None);
    }

    #[test]
    fn the_menu_stays_open_and_centred_when_the_layout_changes() {
        // COMP-03, issue #8: folding, rotating or resizing keeps it open, but the finger's
        // place means nothing in the new layout.
        let mut shell = laid_out(&Fakes::default());
        hold(&mut shell, 300.0, 700.0);
        lift(&mut shell);
        let layout = shell.state().carousel.layout;

        shell.dispatch(ShellIntent::Layout(layout));
        assert_eq!(
            shell.state().menu.and_then(|menu| menu.anchor),
            Some((300.0, 700.0))
        );

        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 2000.0,
            ..layout
        }));
        assert_eq!(
            shell.state().menu,
            Some(HomeMenu {
                items: &MenuItem::ALL,
                anchor: None,
            })
        );
        assert_eq!(shell.state().current, HomeView::Applications);
    }

    #[test]
    fn a_new_layout_keeps_the_open_folder_edit_mode_and_scroll_position() {
        // COMP-03, issue #8: a foldable showing an open folder in edit mode is unfolded, then
        // folded again.
        let (_, mut shell) = with_a_folder();
        let folded = Layout {
            width_px: 1080.0,
            density: 2.625,
            touch_slop_px: 21.0,
            rtl: false,
        };
        let unfolded = Layout {
            width_px: 2208.0,
            density: 2.375,
            touch_slop_px: 19.0,
            rtl: false,
        };
        shell.dispatch(ShellIntent::Layout(folded));
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(480.0)));
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenFolder(FolderId(1))));

        for layout in [unfolded, folded] {
            shell.dispatch(ShellIntent::Layout(layout));

            assert_eq!(shell.state().carousel.layout, layout);
            assert_eq!(shell.state().current, HomeView::Applications);
            let apps = shell.apps().state();
            assert_eq!(apps.open_folder, Some(FolderId(1)));
            assert!(apps.edit_mode);
            assert_eq!(apps.scroll_px, 480.0);
        }
    }

    #[test]
    fn coming_back_keeps_the_menu_unless_home_opens_applications() {
        // HOME-06: by default the home comes back as it was; with Always Applications, Home
        // from an app closes everything.
        let mut shell = laid_out(&Fakes::default());
        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::Returned { home: true });
        assert!(shell.state().menu.is_some());

        always_applications(&mut shell);
        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::Returned { home: true });
        assert_eq!(shell.state().menu, None);
    }

    #[test]
    fn views_are_found_by_their_place() {
        assert_eq!(HomeView::from_index(0), Some(HomeView::Events));
        assert_eq!(HomeView::from_index(2), Some(HomeView::Recent));
        assert_eq!(HomeView::from_index(3), None);
        for view in HomeView::ALL {
            assert_eq!(HomeView::from_index(view.index()), Some(view));
        }
    }
}
