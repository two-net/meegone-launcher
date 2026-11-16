//! Home shell state (SRS 4.1): the carousel of views, Home and Back, and the theme.
//!
//! [`HomeShell::compose`] is the only place features are put together (ARCH-03). Each feature
//! runs in its own [`Store`], so a failing feature never takes down the shell (ARCH-09).

mod carousel;

use std::sync::Arc;
use std::time::Duration;

use meegone_apps_data::PersistentLayoutStore;
use meegone_apps_state::{AppsHolder, AppsIntent};
use meegone_core_data::{PersistentSettingsStore, Platform, SettingsStore, SystemConfigAdapter};
use meegone_core_state::{Store, ThemeHolder, ThemeIntent};
use meegone_events_state::{EventsHolder, EventsIntent};
use meegone_lockimage_data::PersistentImageCache;
use meegone_lockimage_state::{LockImageHolder, LockImageIntent, LockImageState};
use meegone_recent_state::{RecentHolder, RecentIntent};
use meegone_settings_state::{SettingsHolder, SettingsIntent, SettingsState};

use carousel::{Carousel, Release};
pub use carousel::{CarouselState, Layout, PointerEvent, PointerKind};

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
    pub settings_open: bool,
    pub carousel: CarouselState,
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
    /// The system Home action (HOME-07).
    Home,
    /// The system Back action (HOME-08).
    Back,
    /// Chosen from the home menu (HOME-10, SET-01).
    OpenSettings,
    /// MeeGone came to the foreground: at start, after the system's role dialog, or back from
    /// another app or a settings screen. The default home app or the day may have changed
    /// (HOME-01, ONB-04).
    Resumed,
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

/// One Home action closes at most this many open elements, such as a folder inside edit mode.
const MAX_OPEN_ELEMENTS: usize = 8;

/// Puts the features together and routes intents between them.
pub struct HomeShell {
    state: ShellState,
    settings_store: Arc<dyn SettingsStore>,
    system: Arc<dyn SystemConfigAdapter>,
    carousel: Carousel,
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
                let release = self.carousel.layout(layout, self.state.current);
                self.settle(release);
            }
            ShellIntent::Pointer(event) => {
                // Settings covers the home views.
                if !self.state.settings_open {
                    let release = self.carousel.pointer(event, self.state.current);
                    self.settle(release);
                }
            }
            ShellIntent::Frame(at) => self.carousel.frame(at),
            ShellIntent::Home => self.home(),
            ShellIntent::Back => self.back(),
            ShellIntent::OpenSettings => {
                // Settings covers the home views, so a drag on them ends.
                let release = self.carousel.end_gesture(self.state.current);
                self.settle(release);
                self.settings.dispatch(SettingsIntent::Refresh);
                self.state.settings_open = true;
            }
            ShellIntent::Resumed => {
                self.settings.dispatch(SettingsIntent::Refresh);
                // The animation speed may have changed in system settings (ANIM-05).
                self.configure_carousel();
            }
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

    /// HOME-07: close what is open; otherwise go to Applications.
    fn home(&mut self) {
        if std::mem::take(&mut self.state.settings_open) {
            return;
        }
        let mut closed_any = false;
        for _ in 0..MAX_OPEN_ELEMENTS {
            if !self.close_topmost() {
                break;
            }
            closed_any = true;
        }
        // A drag in progress ends too.
        let carousel = self.carousel.state();
        let moving = carousel.dragging || carousel.offset != 0.0;
        if !closed_any && (self.state.current != HomeView::Applications || moving) {
            self.move_to(HomeView::Applications, 0.0, None);
        }
        // On Applications with nothing open, Home scrolls to the top; scrolling arrives with
        // the grid (#6, #9).
    }

    /// HOME-08: close the topmost open element; otherwise do nothing.
    fn back(&mut self) {
        if !std::mem::take(&mut self.state.settings_open) {
            self.close_topmost();
        }
    }

    fn close_topmost(&mut self) -> bool {
        match self.state.current {
            HomeView::Events => self.events.back(),
            HomeView::Applications => self.apps.back(),
            HomeView::Recent => self.recent.back(),
        }
    }

    fn show(&mut self, view: HomeView) {
        self.state.current = view;
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
    use meegone_apps_state::GridItem;
    use meegone_core_data::fakes::{FakeApps, Fakes, app, app_id};
    use meegone_core_data::{App, AppId, AppsAdapter, DataError, Profile, Shortcut};
    use meegone_core_state::{Access, Health, Theme, ThemeChoice};

    use super::*;

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
}
