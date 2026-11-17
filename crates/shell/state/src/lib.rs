//! Home shell state (SRS 4.1): the carousel of views, the home menu, which long-press counts,
//! Home and Back, coming back to the home, the icon press, the dragged icon, the grid's motion
//! as apps come, go and move, edit mode's motion, the open folder's motion, the search field's
//! pull, the Events list's motion as notifications come and go, the image details screen and
//! the image-of-the-day card's cross-fade, Recent's pinch and its cards' reflow, Recent's close
//! mode and its cards leaving, and the theme.
//!
//! [`HomeShell::compose`] is the only place features are put together (ARCH-03). Each feature
//! runs in its own [`Store`], so a failing feature never takes down the shell (ARCH-09).

mod carousel;
mod events_change;
mod folder;
mod grid_change;
mod home_menu;
mod image_change;
mod lift;
mod mode;
mod pinch;
mod press;
mod recent_change;
mod return_home;
mod search;
mod timed;

use std::sync::Arc;
use std::time::Duration;

use meegone_apps_data::PersistentLayoutStore;
use meegone_apps_state::{
    AppsHolder, AppsIntent, AppsState, CellKey, FolderId, Surface, cells_moved,
};
use meegone_core_data::{
    HomeTarget, PersistentSettingsStore, Platform, SettingsStore, SystemConfigAdapter,
};
use meegone_core_state::motion::{CARD_REMOVE, MODE_SCALE, RECENT_PINCH, VIEW_FLING_VELOCITY_DP};
use meegone_core_state::{
    Access, App, AppId, Health, RecentDensity, Store, ThemeHolder, ThemeIntent,
};
use meegone_events_state::{
    EventsHolder, EventsIntent, GroupId, NotificationGroup, RowKey, RowTarget, rows_came_or_went,
};
use meegone_lockimage_data::PersistentImageCache;
use meegone_lockimage_state::{LockImageHolder, LockImageIntent, LockImageState};
use meegone_recent_state::{MAX_ROWS, RecentHolder, RecentIntent, other_density};
use meegone_settings_state::{SettingsHolder, SettingsIntent, SettingsState};

use carousel::{Carousel, Release};
pub use carousel::{CarouselState, Layout, PointerEvent, PointerKind};
use events_change::EventsChange;
pub use events_change::EventsChangeState;
use folder::FolderMotion;
pub use folder::FolderState;
use grid_change::GridChange;
pub use grid_change::GridChangeState;
pub use home_menu::{HomeMenu, MenuItem};
use image_change::ImageChange;
pub use image_change::ImageChangeState;
use lift::{DragSurfaces, DropAt, Lift, LiftEvent, SurfaceView};
pub use lift::{HoverState, LiftState, MergeState};
pub use meegone_apps_state::{FOLDER_COLUMNS, GridItem, PREVIEW_APPS, preview_slot};
/// How many rows of Recent's cards there are at most: each has its own ✕ scale in close mode,
/// and its own progress as its cards leave (ANIM-12, ANIM-18).
pub use meegone_recent_state::MAX_ROWS as RECENT_ROWS;
pub use meegone_settings_state::SettingsGroup;
use mode::Mode;
pub use mode::{BADGE_ROWS, ModeState};
// Close mode's badges come from a `ModeState`, which has a scale for this many rows.
const _: () = assert!(RECENT_ROWS <= BADGE_ROWS);
use pinch::{Ended, PinchMotion};
pub use pinch::{PinchEvent, PinchKind, PinchState};
use press::Press;
pub use press::PressState;
use recent_change::RecentChange;
pub use recent_change::RecentChangeState;
use return_home::ReturnHome;
pub use return_home::ReturnHomeState;
pub use search::SearchState;
use search::{Change, Geometry, Pull, SearchMotion};

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
    /// The image details screen covers the home views (WALL-15). The image-of-the-day card in
    /// Events opens it (EVT-12, ADR 0022).
    pub image_details_open: bool,
    /// The home menu, open over the current view (HOME-10). It covers the home views: they get
    /// no touches while it is open.
    pub menu: Option<HomeMenu>,
    /// An app's or a folder's long-press menu is open over Applications (APPS-11). Its content
    /// is Applications' own. It covers the home views as the home menu does. A failed
    /// Applications never counts (ARCH-09).
    pub app_menu_open: bool,
    /// A notification's or a group's long-press menu is open over Events (EVT-08). Its content
    /// is Events' own. It covers the home views as the home menu does. A failed Events never
    /// counts (ARCH-09).
    pub events_menu_open: bool,
    /// A folder is open over Applications (FOLD-04): the carousel stays put, and only the
    /// folder's panel takes touches. A failed Applications never counts (ARCH-09).
    pub folder_open: bool,
    pub carousel: CarouselState,
    /// The home content's scale and opacity while it comes back from an app (ANIM-09).
    pub return_home: ReturnHomeState,
    /// The item of the current view under the finger, which shrinks while pressed (ANIM-10).
    pub press: PressState,
    /// How far the Applications grid has come in showing apps that came and went (ANIM-21).
    pub grid_change: GridChangeState,
    /// The app icon a drag after a long-press picked up, which follows the finger and settles
    /// into its cell when dropped (APPS-11, APPS-12).
    pub lift: LiftState,
    /// Edit mode as Applications shows it, with its motion: the cells' size, the ✕ badges and
    /// Done (APPS-12, ANIM-12).
    pub mode: ModeState,
    /// The icon a dragged app is held over, growing and pulsing (ANIM-14).
    pub hover: HoverState,
    /// A folder taking in a dropped app (ANIM-14).
    pub merge: MergeState,
    /// The open folder's panel, growing from its icon or shrinking back (ANIM-15).
    pub folder: FolderState,
    /// The folder whose new name the host asks the user for (FOLD-03). A failed Applications
    /// never asks (ARCH-09).
    pub renaming: Option<FolderId>,
    /// The search field, pulled down or springing open or away (SRCH-01, ANIM-16). While it is
    /// open the carousel stays put, and only the results, or the grid below an empty field,
    /// take touches. A failed Applications never shows it (ARCH-09).
    pub search: SearchState,
    /// How far the Events list has come in showing notifications that came and went
    /// (ANIM-20).
    pub events_change: EventsChangeState,
    /// How far the image-of-the-day card's new thumbnail has faded in (ANIM-22).
    pub image_change: ImageChangeState,
    /// Recent's cards as two fingers pinch them, and as they reflow to their places (REC-05,
    /// ANIM-17).
    pub pinch: PinchState,
    /// Recent shows its cards with nothing covering them, out of close mode, so a pinch, or a
    /// screen reader, can switch between more cards and bigger ones (REC-05, A11Y-05). A failed
    /// Recent never offers it (ARCH-09).
    pub density_offered: bool,
    /// Recent's close mode, with its motion: the cards' size, the ✕ badges, one row of cards at
    /// a time, and Clear all (REC-07, ANIM-12). A failed Recent never shows it (ARCH-09).
    pub close: ModeState,
    /// Recent shows its cards with nothing covering them, out of close mode, so a screen reader
    /// can enter it (REC-07, A11Y-05). A failed Recent never offers it (ARCH-09).
    pub close_offered: bool,
    /// Cards removed from Recent shrinking and fading where they were, row by row (ANIM-18).
    pub recent_change: RecentChangeState,
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
        !self.settings_open
            && !self.image_details_open
            && self.menu.is_none()
            && !self.app_menu_open
            && !self.events_menu_open
            && !self.folder_open
            && !self.search.open
            && HomeMenu::items_for(self.current, false).is_some()
    }

    /// Whether Applications offers search now, which a screen reader can open without a pull
    /// (A11Y-05): on Applications, with nothing covering it or open over it, outside edit mode,
    /// and search not open already.
    pub fn search_offered(&self) -> bool {
        self.current == HomeView::Applications
            && !self.settings_open
            && !self.image_details_open
            && self.menu.is_none()
            && !self.app_menu_open
            && !self.folder_open
            && !self.mode.on
            && !self.search.open
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
    /// Two fingers on the home views, which may pinch Recent's cards between more and bigger
    /// ones (REC-05, ANIM-17). The host sends it alongside the first finger's `Pointer` events.
    Pinch(PinchEvent),
    /// The display is about to draw a frame at this time on the host's monotonic clock.
    Frame(Duration),
    /// A finger went down on an item of the current view, such as an app icon, at this time on
    /// the host's monotonic clock. `item` is its place in the view's cells, or in the open
    /// folder's on that `surface`. Its icon shrinks (ANIM-10).
    Press {
        surface: Surface,
        item: usize,
        at: Duration,
    },
    /// The finger lifted or left the pressed item, which springs back (ANIM-10).
    Release(Duration),
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
    /// The finger that is down has stayed on an app's or a folder's icon in Applications, or on
    /// an app's in the open folder, for the platform's long-press time (APPS-11, ADR 0015). The
    /// shell takes it only if the finger hasn't moved past the touch slop and nothing covers
    /// that surface: the menu opens and ticks (ANIM-24), and the rest of the gesture is the
    /// shell's. Lifting the finger then taps nothing, and dragging on picks the icon up and
    /// enters edit mode.
    LongPressCell {
        surface: Surface,
        key: CellKey,
    },
    /// Opens the home menu without a gesture, from a screen-reader action (A11Y-05).
    OpenMenu,
    /// Opens an app's or a folder's menu without a gesture, from a screen-reader action
    /// (A11Y-05).
    OpenCellMenu(CellKey),
    /// The finger that is down has stayed on a notification's or a group's row in Events for
    /// the platform's long-press time (EVT-08). The shell takes it only if the finger hasn't
    /// moved past the touch slop and nothing covers the views: the menu opens and ticks
    /// (ANIM-24), and the rest of the gesture is the shell's, so lifting the finger opens
    /// nothing.
    LongPressNotification(RowTarget),
    /// Opens a notification's or a group's menu without a gesture, from a screen-reader action
    /// (A11Y-05).
    OpenNotificationMenu(RowTarget),
    /// A touch outside the home menu, an app's menu or a notification's menu closes it.
    CloseMenu,
    /// An item of the home menu was chosen (HOME-10).
    ChooseMenuItem(MenuItem),
    /// Opens Settings at its top (SET-01).
    OpenSettings,
    /// MeeGone came to the foreground: at start, after the system's role dialog, or back from
    /// another app or a settings screen. The default home app, the day or the language may have
    /// changed (HOME-01, ONB-04, EVT-01).
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
    /// Usage access was granted or revoked, even while Recent shows (REC-02, ADR 0023).
    UsageAccessChanged,
    /// The date, the time, the time zone, the 12- or 24-hour setting or the next alarm changed
    /// (EVT-01, EVT-02, L10N-03, ADR 0019).
    ClockChanged,
    /// The host decoded the image-of-the-day card's new thumbnail and is about to draw it over
    /// the one it showed, so the cross-fade starts (ANIM-22, ADR 0022).
    NewImageShown,
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
    press: Press,
    lift: Lift,
    grid_change: GridChange,
    mode: Mode,
    /// The Applications cells as of the last intent, to tell when they came, went or moved.
    seen_cells: Arc<[GridItem]>,
    /// An icon was lifted after the last intent.
    was_lifted: bool,
    /// Edit mode was just entered by picking an icon up, whose long-press already ticked.
    picked_up: bool,
    folder: FolderMotion,
    /// The open folder's cells as of the last intent, and which folder they were.
    seen_folder: (Option<FolderId>, Arc<[GridItem]>),
    /// How many folders Applications had made as of the last intent (ANIM-24).
    seen_folders_made: u32,
    /// The apps Applications had hidden as of the last intent, so Recent follows (APPS-19).
    seen_hidden: Arc<[App]>,
    search: SearchMotion,
    pinch: PinchMotion,
    /// Recent's density as of the last intent, so the cards reflow when it changes (ANIM-17).
    seen_density: RecentDensity,
    /// Recent's close mode (ANIM-12).
    close: Mode,
    recent_change: RecentChange,
    /// The apps of Recent's cards as of the last intent, and its columns then, to tell which
    /// rows cards left (ANIM-18).
    seen_recent: (Vec<AppId>, usize),
    events_change: EventsChange,
    image_change: ImageChange,
    /// Events' groups and expanded group as of the last intent, to tell cheaply whether its
    /// rows may have changed, and the keys of those rows, to tell which came or went (ANIM-20).
    seen_groups: Arc<[NotificationGroup]>,
    seen_expanded: Option<GroupId>,
    seen_rows: Vec<RowKey>,
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
            press: Press::default(),
            lift: Lift::default(),
            grid_change: GridChange::default(),
            mode: Mode::default(),
            seen_cells: Arc::new([]),
            was_lifted: false,
            picked_up: false,
            folder: FolderMotion::default(),
            seen_folder: (None, Arc::new([])),
            seen_folders_made: 0,
            seen_hidden: Arc::new([]),
            search: SearchMotion::default(),
            pinch: PinchMotion::default(),
            seen_density: RecentDensity::default(),
            close: Mode::default(),
            recent_change: RecentChange::default(),
            seen_recent: (Vec::new(), 0),
            events_change: EventsChange::default(),
            image_change: ImageChange::default(),
            seen_groups: Arc::new([]),
            seen_expanded: None,
            seen_rows: Vec::new(),
            theme: Store::new(ThemeHolder::new(
                settings_store.clone(),
                platform.system.clone(),
            )),
            apps: Store::new(AppsHolder::new(
                platform.apps.clone(),
                platform.system.clone(),
                Arc::new(PersistentLayoutStore::new(platform.storage.clone())),
                // Applications writes the hidden apps there (APPS-19, ADR 0002).
                settings_store.clone(),
            )),
            events: Store::new(EventsHolder::new(
                platform.notifications.clone(),
                // Each group's label, icon and accent come from the catalogue (ADR 0002).
                platform.apps.clone(),
                platform.system.clone(),
            )),
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
        // The grid as it starts is shown as it is: nothing moves (ANIM-21).
        shell.seen_cells = shell.apps.state().items.clone();
        // Recent read the hidden apps and its density as it loaded: nothing reflows, and no
        // card leaves.
        shell.seen_hidden = shell.apps.state().hidden_apps.clone();
        shell.seen_density = shell.recent.state().density;
        shell.seen_recent = shell.recent_cards();
        // Events' list as it starts is shown as it is too (ANIM-20).
        let events = shell.events.state();
        shell.seen_groups = events.groups.clone();
        shell.seen_expanded = events.expanded.clone();
        shell.seen_rows = events.row_keys();
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

    /// Whether Applications works. Its content shown elsewhere, such as Hidden apps in
    /// Settings, is offered only then (ARCH-09).
    pub fn apps_ready(&self) -> bool {
        *self.apps.health() == Health::Ready
    }

    /// Whether Events works. Its header is drawn only then (ARCH-09).
    pub fn events_ready(&self) -> bool {
        *self.events.health() == Health::Ready
    }

    /// Whether Recent works. Its cards are drawn only then (ARCH-09).
    pub fn recent_ready(&self) -> bool {
        *self.recent.health() == Health::Ready
    }

    /// Whether the lock-screen image works. Its card in Events, its section in Settings and
    /// its details screen are offered only then (ARCH-09).
    pub fn lockimage_ready(&self) -> bool {
        *self.lockimage.health() == Health::Ready
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

    /// Calls `listener` with every new Applications state, for its Hidden apps section shown
    /// inside Settings (APPS-19, ADR 0002).
    pub fn subscribe_apps(&mut self, listener: impl FnMut(&Arc<AppsState>) + 'static) {
        self.apps.subscribe(listener);
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
                    // it moves to the centre. An app's menu stays at its icon. A lifted icon
                    // goes back to its cell, since the finger's place means nothing either.
                    if let Some(menu) = &mut self.state.menu {
                        menu.anchor = None;
                    }
                    self.lift.cancel();
                    self.search.end_gesture();
                    self.pinch.end_gesture(self.carousel.animation_scale());
                }
                let release = self.carousel.layout(layout, self.state.current);
                self.settle(release);
            }
            ShellIntent::Pointer(event) => {
                if event.kind == PointerKind::Down {
                    // A touch never waits for the return motion (ANIM-03).
                    self.return_home.finish();
                    // A new gesture, which no pinch holds yet.
                    self.pinch.new_gesture(self.carousel.animation_scale());
                }
                // A long-press on an icon keeps the rest of its gesture, even under its menu:
                // dragging on picks the icon up and enters edit mode, which closes the menu
                // (APPS-11). Lifting the finger drops it (APPS-12, FOLD-01, FOLD-05).
                self.drag(&event);
                // At the top of Applications, pulling down reveals the search field, and with
                // nothing typed, pushing up puts it away (SRCH-01, SRCH-04).
                self.pull(event);
                // Settings and the menus cover the home views, but a finger that was already
                // down may still lift. An open folder or search takes touches itself: the row
                // stays put, but a press there can still become a long-press (FOLD-04). After
                // a pinch, the finger left on the views moves nothing (ANIM-17).
                self.carousel
                    .lock(self.folder_shown() || self.search_shown() || self.pinch.holding());
                if !self.covered() || matches!(event.kind, PointerKind::Up | PointerKind::Cancel) {
                    let release = self.carousel.pointer(event, self.state.current);
                    self.settle(release);
                }
                if self.carousel.state().dragging {
                    // The views move, so the icon under the finger lets go (ANIM-10).
                    self.press
                        .release(event.at, self.carousel.animation_scale());
                }
            }
            ShellIntent::Pinch(event) => self.follow_pinch(event),
            ShellIntent::Frame(at) => {
                self.carousel.frame(at);
                self.pinch.frame(at);
                self.return_home.frame(at);
                self.press.frame(at);
                self.grid_change.frame(at);
                self.events_change.frame(at);
                self.image_change.frame(at);
                self.mode.frame(at);
                self.close.frame(at);
                self.recent_change.frame(at);
                self.folder.frame(at);
                self.search.frame(at);
                self.drag_frame(at);
            }
            ShellIntent::Press { surface, item, at } => self.press_item(surface, item, at),
            ShellIntent::Release(at) => {
                self.press.release(at, self.carousel.animation_scale());
            }
            ShellIntent::Home => self.home(),
            ShellIntent::Back => self.back(),
            ShellIntent::LongPress => self.long_press(),
            ShellIntent::LongPressCell { surface, key } => self.long_press_cell(surface, key),
            ShellIntent::OpenMenu => self.open_menu(None),
            ShellIntent::OpenCellMenu(key) => self.open_cell_menu(key),
            ShellIntent::LongPressNotification(target) => self.long_press_notification(target),
            ShellIntent::OpenNotificationMenu(target) => self.open_notification_menu(target),
            ShellIntent::CloseMenu => {
                self.state.menu = None;
                self.close_app_menu();
                self.close_events_menu();
            }
            ShellIntent::ChooseMenuItem(item) => self.choose(item),
            ShellIntent::OpenSettings => self.open_settings(None),
            ShellIntent::Resumed => {
                self.settings.dispatch(SettingsIntent::Refresh);
                // The day or the language may have changed while MeeGone was away (EVT-01), and
                // notification access may have been granted or revoked in system settings
                // (EVT-03, AC-11, AC-21).
                self.events.dispatch(EventsIntent::RefreshHeader);
                self.events.dispatch(EventsIntent::RefreshNotifications);
                // The daily image may have changed while MeeGone was away (EVT-12, ANIM-22).
                self.lockimage.dispatch(LockImageIntent::Refresh);
                // The animation speed may have changed in system settings (ANIM-05).
                self.configure_carousel();
            }
            ShellIntent::Returned { home } => self.returned(home),
            ShellIntent::Restore(saved) => self.restore(saved),
            ShellIntent::SystemThemeChanged => self.theme.dispatch(ThemeIntent::Refresh),
            ShellIntent::AppsChanged => self.apps_changed(),
            ShellIntent::NotificationsChanged => {
                self.events.dispatch(EventsIntent::RefreshNotifications);
            }
            ShellIntent::UsageAccessChanged => self.recent.dispatch(RecentIntent::Refresh),
            ShellIntent::ClockChanged => self.events.dispatch(EventsIntent::RefreshHeader),
            ShellIntent::NewImageShown => {
                self.image_change.start(self.carousel.animation_scale());
            }
            ShellIntent::Apps(AppsIntent::OpenSearch) => self.open_search(),
            ShellIntent::Apps(intent) => self.apps.dispatch(intent),
            ShellIntent::Events(intent) => self.events.dispatch(intent),
            ShellIntent::Recent(intent) => self.recent.dispatch(intent),
            ShellIntent::LockImage(LockImageIntent::OpenDetails) => self.open_image_details(),
            ShellIntent::LockImage(intent) => self.lockimage.dispatch(intent),
            ShellIntent::Settings(intent) => {
                self.settings.dispatch(intent);
                // The theme choice and wrap-around live in the settings store; take them on
                // (THEME-06, HOME-04).
                self.theme.dispatch(ThemeIntent::Refresh);
                self.configure_carousel();
            }
        }
        self.keep_folder_in_step();
        self.keep_search_in_step();
        self.keep_lift_in_step();
        self.keep_mode_in_step();
        self.keep_cells_in_step();
        self.keep_events_in_step();
        self.keep_recent_in_step();
        self.keep_close_in_step();
        self.lift.sync(&self.drag_surfaces());
        self.state.carousel = self.carousel.state();
        self.state.return_home = self.return_home.state();
        self.state.press = self.press.state();
        self.state.lift = self.lift.state();
        self.state.hover = self.lift.hover();
        self.state.merge = self.lift.merge_state();
        self.state.grid_change = self.grid_change.state();
        self.state.events_change = self.events_change.state();
        self.state.image_change = self.image_change.state();
        self.state.mode = self.mode_state();
        self.state.app_menu_open = self.app_menu_shown();
        self.state.events_menu_open = self.events_menu_shown();
        self.state.folder_open = self.folder_shown();
        self.state.folder = self.folder.state();
        self.state.renaming = (*self.apps.health() == Health::Ready)
            .then(|| self.apps.state().renaming)
            .flatten();
        self.state.search = self.search.state();
        self.state.pinch = self.pinch.state();
        self.state.density_offered = self.density_offered();
        self.state.close = self.close_state();
        self.state.close_offered = self.close_offered();
        self.state.recent_change = self.recent_change.state();
    }

    /// Whether Settings, the image details or a menu covers the home views, so they get no new
    /// touches.
    fn covered(&self) -> bool {
        self.state.settings_open
            || self.state.image_details_open
            || self.state.menu.is_some()
            || self.app_menu_shown()
            || self.events_menu_shown()
    }

    /// Whether a notification's menu is open. A failed Events can't close its menu, so it never
    /// counts: the home must never be left covered for good (ARCH-09).
    fn events_menu_shown(&self) -> bool {
        *self.events.health() == Health::Ready && self.events.state().menu.is_some()
    }

    fn close_events_menu(&mut self) {
        if self.events_menu_shown() {
            self.events.dispatch(EventsIntent::CloseMenu);
        }
    }

    /// Whether an app's menu is open. A failed Applications can't close its menu, so it never
    /// counts: the home must never be left covered for good (ARCH-09).
    fn app_menu_shown(&self) -> bool {
        *self.apps.health() == Health::Ready && self.apps.state().menu.is_some()
    }

    fn close_app_menu(&mut self) {
        if self.app_menu_shown() {
            self.apps.dispatch(AppsIntent::CloseMenu);
        }
    }

    fn close_folder(&mut self) {
        if self.folder_shown() {
            self.apps.dispatch(AppsIntent::CloseFolder);
        }
    }

    /// Whether Applications shows edit mode. A failed Applications can't leave it, so it never
    /// counts: no badge, Done or drag is left on screen for good (ARCH-09).
    fn editing_shown(&self) -> bool {
        *self.apps.health() == Health::Ready && self.apps.state().edit_mode
    }

    /// Whether a folder is open over Applications. A failed Applications can't close it, so it
    /// never counts (ARCH-09).
    fn folder_shown(&self) -> bool {
        *self.apps.health() == Health::Ready && self.apps.state().open_folder.is_some()
    }

    /// Whether search is open on Applications. A failed Applications can't close it, so it
    /// never counts (ARCH-09).
    fn search_shown(&self) -> bool {
        *self.apps.health() == Health::Ready && self.apps.state().search.is_some()
    }

    fn close_search(&mut self) {
        if self.search_shown() {
            self.apps.dispatch(AppsIntent::CloseSearch);
        }
    }

    /// A11Y-05: opens search without a pull, only where it is offered: on Applications, with
    /// nothing covering it or open over it.
    fn open_search(&mut self) {
        if self.state.current != HomeView::Applications
            || self.covered()
            || self.folder_shown()
            || self.editing_shown()
        {
            return;
        }
        self.apps.dispatch(AppsIntent::OpenSearch);
    }

    /// What a vertical drag on the home views may do now (SRCH-01, SRCH-04). Only on
    /// Applications at rest, with nothing covering it, open over it or lifted, once the host
    /// has laid the field out: pull search open at the top of the grid, or push it away while
    /// nothing is typed.
    fn pull_offer(&self) -> Pull {
        let apps = self.apps.state();
        let carousel = self.carousel.state();
        let calm = self.state.current == HomeView::Applications
            && *self.apps.health() == Health::Ready
            && !self.covered()
            && !self.folder_shown()
            && !self.editing_shown()
            && self.lift.armed().is_none()
            && self.lift.held().is_none()
            && !carousel.dragging
            && carousel.offset == 0.0
            && apps.search_band_px().is_some();
        match &apps.search {
            _ if !calm => Pull::None,
            // Within a pixel of the top, as the host rounds its scroll position.
            None if apps.scroll_px < 1.0 => Pull::Open,
            Some(_) if !apps.has_query() => Pull::Away,
            _ => Pull::None,
        }
    }

    /// Follows the finger on the search field (ANIM-16). A release that opens or closes search
    /// tells Applications, and a pull lets the pressed icon go (ANIM-10).
    fn pull(&mut self, event: PointerEvent) {
        let layout = self.carousel.state().layout;
        let size = Geometry {
            slop_px: layout.touch_slop_px,
            fling_px_per_s: VIEW_FLING_VELOCITY_DP * layout.density,
        };
        match self.search.pointer(event, self.pull_offer(), size) {
            Some(Change::Opened) => self.apps.dispatch(AppsIntent::OpenSearch),
            Some(Change::Closed) => self.apps.dispatch(AppsIntent::CloseSearch),
            None => {}
        }
        if self.search.pulling() {
            self.press
                .release(event.at, self.carousel.animation_scale());
        }
    }

    /// The field follows search as Applications has it: open, or closed by Back, Home, a
    /// launch or a screen reader (SRCH-01, SRCH-04).
    fn keep_search_in_step(&mut self) {
        let band = self.apps.state().search_band_px().unwrap_or(0.0);
        self.search
            .set(self.search_shown(), band, self.carousel.animation_scale());
    }

    /// The panel follows the open folder (ANIM-15), and a folder made ticks (ANIM-24).
    fn keep_folder_in_step(&mut self) {
        let open = self
            .folder_shown()
            .then(|| self.apps.state().open_folder)
            .flatten();
        self.folder.set(open, self.carousel.animation_scale());
        let made = self.apps.state().folders_made;
        if made != std::mem::replace(&mut self.seen_folders_made, made) {
            self.system.haptic_tick();
        }
    }

    /// A long-press can pick its icon up only while the menu it opened is still open, and an
    /// icon stays lifted or settling only in edit mode, while it has a cell on its surface: the
    /// grid, or a folder that is still open. A merging icon needs only edit mode. Otherwise,
    /// such as after Back, the icon is back in its cell at once. A drag that ends any way but a
    /// drop puts the grid back as it was (APPS-12).
    fn keep_lift_in_step(&mut self) {
        let apps = self.apps.state();
        let in_step = match (self.lift.armed(), self.lift.held()) {
            (Some((key, _)), _) => {
                self.app_menu_shown() && apps.menu.as_ref().is_some_and(|menu| menu.item == *key)
            }
            (None, Some((key, surface))) => {
                self.editing_shown()
                    && (surface == Surface::Grid || self.folder_shown())
                    && apps.place_of(surface, key).is_some()
            }
            (None, None) => !self.lift.is_merging() || self.editing_shown(),
        };
        if !in_step {
            self.lift.cancel();
        }
        let lifted = self.lift.is_lifted();
        if std::mem::replace(&mut self.was_lifted, lifted) && !lifted {
            // After a drop, there is nothing left to put back.
            self.apps.dispatch(AppsIntent::CancelArrange);
        }
    }

    /// Edit mode's motion follows Applications (ANIM-12), and entering it ticks (ANIM-24),
    /// unless picking an icon up entered it: its long-press just ticked.
    fn keep_mode_in_step(&mut self) {
        let editing = self.editing_shown();
        let picked_up = std::mem::take(&mut self.picked_up);
        if editing != self.mode.is_on() {
            self.mode.set(editing, self.carousel.animation_scale());
            if editing && !picked_up {
                self.system.haptic_tick();
            }
        }
    }

    /// When Applications' cells came, went or moved, the grid moves to its new cells (ANIM-21,
    /// ANIM-13), and a pressed icon lets go: it is named by its place, which may now hold
    /// another cell (ANIM-10). The open folder's cells moving moves them too (FOLD-05), but a
    /// folder opening or closing doesn't.
    fn keep_cells_in_step(&mut self) {
        let apps = self.apps.state();
        let mut moved = false;
        if !Arc::ptr_eq(&apps.items, &self.seen_cells) {
            moved |= cells_moved(&self.seen_cells, &apps.items);
            self.seen_cells = apps.items.clone();
        }
        let (folder, cells) = &self.seen_folder;
        if !Arc::ptr_eq(&apps.folder_items, cells) {
            moved |= *folder == apps.open_folder && cells_moved(cells, &apps.folder_items);
            self.seen_folder = (apps.open_folder, apps.folder_items.clone());
        }
        if moved {
            self.press.finish();
            self.grid_change.start(self.carousel.animation_scale());
        }
    }

    /// When Events' rows came or went, as notifications came and went or a group expanded or
    /// collapsed, its list moves to its new rows (ANIM-20, EVT-06). A group whose count alone
    /// changed moves nothing.
    fn keep_events_in_step(&mut self) {
        let events = self.events.state();
        if Arc::ptr_eq(&events.groups, &self.seen_groups) && events.expanded == self.seen_expanded {
            return;
        }
        let rows = events.row_keys();
        self.seen_groups = events.groups.clone();
        self.seen_expanded = events.expanded.clone();
        if rows_came_or_went(&self.seen_rows, &rows) {
            self.events_change.start(self.carousel.animation_scale());
        }
        self.seen_rows = rows;
    }

    /// Recent leaves out the apps Applications hides (APPS-19, REC-03). When they change, by a
    /// Hide, an Unhide or a hidden app that is gone, Recent reads them again at once, so a swipe
    /// never shows a hidden app's card on the next page. A failed save changes nothing, and a
    /// failed Applications changes nothing either.
    ///
    /// When Recent's density changes, by a pinch or from a screen reader, its cards reflow to
    /// their new columns (ANIM-17). A pinch ends without switching once nothing offers it, such
    /// as after Home or under a menu.
    fn keep_recent_in_step(&mut self) {
        let hidden = &self.apps.state().hidden_apps;
        if !Arc::ptr_eq(hidden, &self.seen_hidden) {
            self.seen_hidden = hidden.clone();
            self.recent.dispatch(RecentIntent::Refresh);
        }
        let scale = self.carousel.animation_scale();
        let density = self.recent.state().density;
        if density != std::mem::replace(&mut self.seen_density, density) {
            self.pinch.reflow(RECENT_PINCH, scale);
        }
        if self.pinch.pinching() && !self.density_offered() {
            self.pinch.end_gesture(scale);
        }
        self.keep_recent_cards_in_step();
    }

    /// When cards leave Recent while it shows, by a ✕, Clear all or a refresh, they shrink and
    /// fade where they were, row by row, and the others close the gap (ANIM-18). Cards that go
    /// because access went, or while Recent is out of sight, just go.
    fn keep_recent_cards_in_step(&mut self) {
        let (cards, columns) = self.recent_cards();
        if cards == self.seen_recent.0 {
            self.seen_recent.1 = columns;
            return;
        }
        let (seen, seen_columns) = std::mem::replace(&mut self.seen_recent, (cards, columns));
        let cards = &self.seen_recent.0;
        let shows = self.state.current == HomeView::Recent
            && self.recent_ready()
            && self.recent.state().access == Access::Granted;
        if !shows {
            return;
        }
        let rows: Vec<usize> = seen
            .iter()
            .enumerate()
            .filter(|(_, app)| !cards.contains(app))
            .map(|(at, _)| at / seen_columns.max(1))
            .collect();
        if rows.is_empty() {
            return;
        }
        let scale = self.carousel.animation_scale();
        self.recent_change.start(rows, scale);
        // A pinch reflows the cards as it ends.
        if !cards.is_empty() && !self.pinch.pinching() {
            self.pinch.reflow(CARD_REMOVE, scale);
        }
    }

    /// The apps of Recent's cards, and how many each row holds.
    fn recent_cards(&self) -> (Vec<AppId>, usize) {
        let recent = self.recent.state();
        let cards = recent
            .cards
            .iter()
            .map(|card| card.app.id.clone())
            .collect();
        (cards, usize::from(recent.columns()))
    }

    /// Close mode's motion follows Recent (ANIM-12). Entering it doesn't tick here: the
    /// long-press that entered it just did, and a screen reader's request doesn't (ANIM-24).
    fn keep_close_in_step(&mut self) {
        let closing = self.close_shown();
        if closing != self.close.is_on() {
            self.close.set(closing, self.carousel.animation_scale());
        }
    }

    /// Whether Recent is in close mode. A failed Recent never shows it (ARCH-09).
    fn close_shown(&self) -> bool {
        self.recent_ready() && self.recent.state().close_mode
    }

    /// Close mode's motion for Recent's rows of cards: the badges stagger from the first.
    fn close_state(&self) -> ModeState {
        let recent = self.recent.state();
        let columns = usize::from(recent.columns()).max(1);
        let rows = recent.cards.len().div_ceil(columns);
        self.close.state(0, rows.clamp(1, MAX_ROWS))
    }

    /// Whether Recent shows its cards on screen with nothing covering them, so its density can
    /// change (REC-05, A11Y-05). Not in close mode, whose cards are being tidied. A failed Recent
    /// never offers it (ARCH-09).
    fn density_offered(&self) -> bool {
        self.recent_cards_offered() && !self.close_shown()
    }

    /// Whether a screen reader can enter Recent's close mode, as a long-press on its cards
    /// does (REC-07, A11Y-05).
    fn close_offered(&self) -> bool {
        self.recent_cards_offered() && !self.close_shown()
    }

    /// Whether Recent shows its cards on screen with nothing covering them.
    fn recent_cards_offered(&self) -> bool {
        self.state.current == HomeView::Recent
            && self.recent_ready()
            && self.recent.state().shows_cards()
            && !self.covered()
    }

    /// How much bigger, as a scale, the other density's cards are than these, if a pinch may
    /// start now: on Recent's cards with nothing covering them, while the views are at rest and
    /// no long-press holds the finger.
    fn pinch_target(&self) -> Option<f64> {
        let carousel = self.carousel.state();
        if !self.density_offered() || carousel.dragging || carousel.held || carousel.offset != 0.0 {
            return None;
        }
        let recent = self.recent.state();
        let other = recent.columns_of(other_density(recent.density));
        Some(f64::from(recent.columns()) / f64::from(other.max(1)))
    }

    /// Follows two fingers on the home views (REC-05, ANIM-17). A pinch that starts takes the
    /// gesture: the row lets go of the first finger and stays put, and no long-press counts. A
    /// release past the threshold switches Recent's density, which saves it.
    fn follow_pinch(&mut self, event: PinchEvent) {
        let scale = self.carousel.animation_scale();
        let target = match event.kind {
            PinchKind::Start => self.pinch_target(),
            _ => None,
        };
        let was_holding = self.pinch.holding();
        let ended = self.pinch.pinch(event, target, scale);
        if !was_holding && self.pinch.holding() {
            let release = self.carousel.pointer(
                PointerEvent {
                    kind: PointerKind::Cancel,
                    x: event.x,
                    y: event.y,
                    at: event.at,
                },
                self.state.current,
            );
            self.settle(release);
            self.carousel.lock(true);
            self.press.release(event.at, scale);
        }
        if ended == Some(Ended::Switch) {
            let density = other_density(self.recent.state().density);
            self.recent.dispatch(RecentIntent::SetDensity(density));
        }
    }

    /// The grid and the open folder as a drag needs them now. The folder counts once its panel
    /// is all the way open, since until then its cells aren't where the host laid them out.
    fn drag_surfaces(&self) -> DragSurfaces {
        let apps = self.apps.state();
        let view = |surface: Surface| SurfaceView {
            layout: apps.layout_of(surface),
            // As the host draws it, in whole pixels.
            scroll_px: apps.scroll_of(surface).round(),
            cells: apps.cells(surface).clone(),
            rows: apps.rows_of(surface),
        };
        let mode = self.mode_state();
        DragSurfaces {
            grid: view(Surface::Grid),
            folder: (self.folder_shown() && self.folder.fully_open())
                .then(|| view(Surface::Folder)),
            folder_bounds: apps.folder_bounds,
            rtl: self.carousel.state().layout.rtl,
            scale: mode.scale,
            rest_scale: if mode.on { MODE_SCALE } else { 1.0 },
        }
    }

    /// Follows the finger that made a long-press on an icon: dragging picks the icon up and
    /// enters edit mode (APPS-11), dragging it out of the open folder takes it out (FOLD-05),
    /// and lifting the finger drops the icon where it is (APPS-12, FOLD-01).
    fn drag(&mut self, event: &PointerEvent) {
        let slop = self.carousel.state().layout.touch_slop_px;
        let surfaces = self.drag_surfaces();
        let scale = self.carousel.animation_scale();
        match self.lift.pointer(event, slop, &surfaces, scale) {
            Some(LiftEvent::PickedUp) => {
                self.picked_up = true;
                self.apps.dispatch(AppsIntent::EnterEditMode);
            }
            Some(LiftEvent::LeftFolder { app, to }) => {
                self.apps.dispatch(AppsIntent::TakeOut { app, to });
            }
            Some(LiftEvent::Dropped(at)) => self.drop(at),
            None => {}
        }
    }

    /// The finger lifted: the drag's changes are kept, then the icon settles into its cell, or
    /// shrinks into the folder it went into (ANIM-13, ANIM-14).
    fn drop(&mut self, at: DropAt) {
        let scale = self.carousel.animation_scale();
        let into = match at {
            DropAt::Grid { key, to } => {
                self.apps.dispatch(AppsIntent::Arrange { item: key, to });
                None
            }
            DropAt::Folder { app, to } => {
                self.apps.dispatch(AppsIntent::ArrangeInFolder { app, to });
                None
            }
            DropAt::Combine { app, onto } => {
                self.apps.dispatch(AppsIntent::Combine {
                    app: app.clone(),
                    onto: onto.clone(),
                });
                Some((app, onto))
            }
            DropAt::Back => None,
        };
        self.apps.dispatch(AppsIntent::Drop);
        let surfaces = self.drag_surfaces();
        let merged = into.and_then(|(app, onto)| {
            // The folder it is in now, which was made from the app it was dropped on.
            let apps = self.apps.state();
            apps.items.iter().find_map(|item| match item {
                GridItem::Folder { id, apps, .. } => {
                    let slot = apps.iter().position(|shown| shown.id == app)?;
                    Some((*id, slot, matches!(onto, CellKey::App(_))))
                }
                GridItem::App { .. } => None,
            })
        });
        match merged {
            Some((folder, slot, formed)) => {
                self.lift.merge(&surfaces, folder, slot, formed, scale);
            }
            // Refused, or not a folder drop: it settles into its cell.
            None => self.lift.settle(&surfaces, scale),
        }
    }

    /// While an icon is lifted, it takes the place it waited over, and scrolls its surface near
    /// the edges (APPS-12, FOLD-05).
    fn drag_frame(&mut self, at: Duration) {
        let surfaces = self.drag_surfaces();
        let step = self
            .lift
            .frame(at, &surfaces, self.carousel.animation_scale());
        match step.scroll_to {
            Some((Surface::Grid, px)) => self.apps.dispatch(AppsIntent::Scrolled(px)),
            Some((Surface::Folder, px)) => self.apps.dispatch(AppsIntent::FolderScrolled(px)),
            Some((Surface::Search, _)) | None => {}
        }
        match step.arrange {
            Some(DropAt::Grid { key, to }) => {
                self.apps.dispatch(AppsIntent::Arrange { item: key, to });
            }
            Some(DropAt::Folder { app, to }) => {
                self.apps.dispatch(AppsIntent::ArrangeInFolder { app, to });
            }
            Some(DropAt::Combine { .. } | DropAt::Back) | None => {}
        }
    }

    /// Edit mode's motion for the grid as it is scrolled now: the badges stagger from its first
    /// visible row.
    fn mode_state(&self) -> ModeState {
        let apps = self.apps.state();
        let (first_row, visible_rows) = apps.layout.map_or((0, BADGE_ROWS), |layout| {
            let row = layout.row_height_px.round().max(1.0);
            let visible = (layout.height_px - layout.top_px - layout.bottom_px).max(0.0);
            let first = (apps.scroll_px.round() / row).floor() as usize;
            (first, (visible / row).ceil() as usize + 1)
        });
        self.mode.state(first_row, visible_rows)
    }

    /// APPS-06, APPS-08: the grid and Recent take on the system's change. If apps came or went,
    /// the grid moves to its new cells (ANIM-21); a dragged icon keeps going unless its own app
    /// left.
    fn apps_changed(&mut self) {
        self.apps.dispatch(AppsIntent::Refresh);
        self.recent.dispatch(RecentIntent::Refresh);
        // Events' groups take on new labels, icons and accents (APPS-17).
        self.events.dispatch(EventsIntent::AppsChanged);
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

    /// ANIM-10: an item of the current view, or of the open folder, shrinks under the finger,
    /// unless the views are moving or something covers that surface.
    fn press_item(&mut self, surface: Surface, item: usize, at: Duration) {
        // In edit mode a tap launches nothing, so nothing shrinks to say it would (APPS-12).
        if self.covered()
            || !self.touchable(surface)
            || self.carousel.state().dragging
            || self.editing_shown()
        {
            return;
        }
        self.press
            .press(surface, item, at, self.carousel.animation_scale());
    }

    /// Whether `surface` takes touches: the open folder's panel while it is open (FOLD-04);
    /// while search is open, its results, or the grid below an empty field; otherwise the grid.
    fn touchable(&self, surface: Surface) -> bool {
        if self.folder_shown() {
            surface == Surface::Folder
        } else if self.search_shown() {
            surface == Surface::Search
                || (surface == Surface::Grid && !self.apps.state().has_query())
        } else {
            surface == Surface::Grid
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
    /// Recent, anywhere on its cards, enters close mode (REC-07). Either way it ticks
    /// (ANIM-24). In Recent it counts only while it can enter close mode, so a long-press with
    /// no card, or already in close mode, takes nothing.
    fn long_press(&mut self) {
        if self.covered() || self.folder_shown() || self.search_shown() {
            return;
        }
        if self.state.current == HomeView::Recent && !self.close_offered() {
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

    /// APPS-11: a long-press on an app's or a folder's icon opens its menu and ticks (ANIM-24).
    /// It counts only on Applications, while the gesture is still a press, nothing covers the
    /// views, and the icon's surface takes touches: in the open folder only once its panel is
    /// all the way open. Then the rest of the gesture is the shell's, so lifting the finger
    /// launches nothing, and dragging on picks the icon up.
    fn long_press_cell(&mut self, surface: Surface, key: CellKey) {
        let opening = surface == Surface::Folder && !self.folder.fully_open();
        // Search's cells have no menu, and neither does the grid below its field.
        if self.covered()
            || !self.touchable(surface)
            || self.search_shown()
            || opening
            || self.state.current != HomeView::Applications
            || self.carousel.pressed_at().is_none()
        {
            return;
        }
        self.apps.dispatch(AppsIntent::OpenMenu(key.clone()));
        if !self.app_menu_shown() {
            return;
        }
        let Some(origin) = self.carousel.long_press() else {
            return;
        };
        // The menu covers the views, so the pressed icon lets go.
        self.press.finish();
        self.lift.arm(key, surface, origin);
        self.system.haptic_tick();
    }

    /// EVT-08: a long-press on a notification's or a group's row opens its menu and ticks
    /// (ANIM-24). It counts only on Events, while the gesture is still a press and nothing
    /// covers the views. Then the rest of the gesture is the shell's, so lifting the finger
    /// opens nothing.
    fn long_press_notification(&mut self, target: RowTarget) {
        if self.covered()
            || self.state.current != HomeView::Events
            || self.carousel.pressed_at().is_none()
        {
            return;
        }
        self.events.dispatch(EventsIntent::OpenMenu(target));
        if !self.events_menu_shown() || self.carousel.long_press().is_none() {
            return;
        }
        // The menu covers the views, so nothing stays pressed.
        self.press.finish();
        self.system.haptic_tick();
    }

    /// A11Y-05: opens a notification's or a group's menu without a gesture, so with no tick.
    fn open_notification_menu(&mut self, target: RowTarget) {
        if self.covered() || self.state.current != HomeView::Events {
            return;
        }
        let release = self.carousel.end_gesture(self.state.current);
        self.settle(release);
        self.press.finish();
        self.events.dispatch(EventsIntent::OpenMenu(target));
    }

    /// A11Y-05: opens an app's or a folder's menu without a gesture, so with no tick, and
    /// nothing to pick up.
    fn open_cell_menu(&mut self, key: CellKey) {
        if self.covered() || self.state.current != HomeView::Applications {
            return;
        }
        let release = self.carousel.end_gesture(self.state.current);
        self.settle(release);
        self.press.finish();
        self.apps.dispatch(AppsIntent::OpenMenu(key));
    }

    /// Opens the current view's home menu at `anchor`, or centred, if it has one (HOME-10).
    fn open_menu(&mut self, anchor: Option<(f64, f64)>) {
        if self.covered() || self.search_shown() {
            return;
        }
        let editing = self.editing_shown();
        let Some(items) = HomeMenu::items_for(self.state.current, editing) else {
            return;
        };
        // The menu covers the home views, so a drag on them ends and a pressed icon lets go.
        let release = self.carousel.end_gesture(self.state.current);
        self.settle(release);
        self.press.finish();
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
        // Settings covers the home views, so a drag on them ends, the menus close and a pressed
        // or lifted icon lets go.
        let release = self.carousel.end_gesture(self.state.current);
        self.settle(release);
        self.press.finish();
        self.lift.cancel();
        self.search.end_gesture();
        self.state.menu = None;
        self.close_app_menu();
        self.close_events_menu();
        self.close_folder();
        self.close_search();
        self.settings.dispatch(SettingsIntent::Refresh);
        self.state.settings_open = true;
        self.state.settings_group = group;
    }

    /// Closes Settings. Returns whether it was open.
    fn close_settings(&mut self) -> bool {
        self.state.settings_group = None;
        std::mem::take(&mut self.state.settings_open)
    }

    /// Opens the image details screen over the home views (EVT-12, WALL-15), while the
    /// lock-screen image works (ARCH-09). It shows only its name until #30 draws it.
    fn open_image_details(&mut self) {
        if !self.lockimage_ready() {
            return;
        }
        // It covers the home views as Settings does.
        let release = self.carousel.end_gesture(self.state.current);
        self.settle(release);
        self.press.finish();
        self.lift.cancel();
        self.search.end_gesture();
        self.state.menu = None;
        self.close_app_menu();
        self.close_events_menu();
        self.close_folder();
        self.close_search();
        self.state.image_details_open = true;
    }

    /// Closes the image details screen. Returns whether it was open.
    fn close_image_details(&mut self) -> bool {
        std::mem::take(&mut self.state.image_details_open)
    }

    /// Closes Settings, or else the image details screen: whichever covers the home views.
    /// Returns whether either was open.
    fn close_screen(&mut self) -> bool {
        self.close_settings() || self.close_image_details()
    }

    /// HOME-07: close what is open; otherwise go to Applications, or, on Applications, scroll
    /// it to the top.
    fn home(&mut self) {
        if self.close_screen() {
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

    /// HOME-08: close the topmost open element (Settings, then the image details, then the home
    /// menu, then the view's own); otherwise do nothing.
    fn back(&mut self) {
        if !self.close_screen() && self.state.menu.take().is_none() {
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
            self.close_image_details();
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
        // The menus, the pressed or lifted item, edit mode and the expanded group belong to the
        // view they were in.
        self.state.menu = None;
        self.close_app_menu();
        self.close_events_menu();
        self.press.finish();
        self.lift.cancel();
        if view != HomeView::Events && self.events_ready() && self.events.state().expanded.is_some()
        {
            self.events.dispatch(EventsIntent::Collapse);
        }
        if view != HomeView::Applications {
            self.close_folder();
            self.close_search();
            if self.editing_shown() {
                self.apps.dispatch(AppsIntent::ExitEditMode);
            }
        }
        // Close mode, like edit mode, is never left out of sight (REC-07).
        if view != HomeView::Recent && self.close_shown() {
            self.recent.dispatch(RecentIntent::ExitCloseMode);
        }
        // Recent refreshes whenever it becomes visible (REC-09); Events does too, to stay live.
        match view {
            HomeView::Events => {
                self.events.dispatch(EventsIntent::Refresh);
                // The card below the notifications stays live too (EVT-12).
                self.lockimage.dispatch(LockImageIntent::Refresh);
            }
            HomeView::Recent => self.recent.dispatch(RecentIntent::Refresh),
            HomeView::Applications => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use meegone_apps_data::{Layout as SavedLayout, LayoutEntry, LayoutStore};
    use meegone_apps_state::{AppMenuItem, Bounds, FolderId, GridItem, GridLayout};
    use meegone_core_data::fakes::{
        FakeApps, FakeNotifications, Fakes, app, app_id, notification, used,
    };
    use meegone_core_data::{
        App, AppId, AppsAdapter, DataError, Notification, NotificationsAdapter, Profile, ProfileId,
        RecentUse, Rect, Shortcut, Size, UsageAdapter, WallpaperAdapter,
    };
    use meegone_core_state::motion::{DRAG_LIFT_SCALE, ICON_PRESS_SCALE};
    use meegone_core_state::{Access, Health, Theme, ThemeChoice};
    use meegone_events_state::NotificationMenuItem;
    use meegone_lockimage_data::ImageCache;
    use meegone_lockimage_state::ImageRecord;

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
                ..SavedLayout::default()
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
        let mut shell = with_recent_maps_and_mail(&Fakes::default());
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        shell.dispatch(ShellIntent::Recent(RecentIntent::EnterCloseMode));
        assert!(shell.recent().state().close_mode);

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
            [GridItem::App { app, .. }] if app.label == "Maps"
        ));
    }

    #[test]
    fn apps_coming_or_going_move_the_grid_and_nothing_else_does() {
        // ANIM-21: the start's grid and an update's new label appear as they are.
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("maps", "Maps")];
        let mut shell = HomeShell::compose(fakes.platform());
        assert!(!shell.state().grid_change.animating);

        fakes.apps.apps.lock().unwrap()[0].label = "Email".into();
        shell.dispatch(ShellIntent::AppsChanged);
        assert!(!shell.state().grid_change.animating);

        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 1,
            at: ms(1_000),
        });
        fakes.apps.apps.lock().unwrap().remove(0);
        shell.dispatch(ShellIntent::AppsChanged);
        let change = shell.state().grid_change;
        assert!(change.animating);
        assert_eq!((change.leave_alpha, change.shift), (1.0, 0.0));
        // The pressed place now holds another cell, so nothing stays pressed (ANIM-10).
        assert_eq!(shell.state().press.item, None);

        let mut at = 2_000;
        shell.dispatch(ShellIntent::Frame(ms(at)));
        while shell.state().grid_change.animating {
            at += 8;
            shell.dispatch(ShellIntent::Frame(ms(at)));
        }
        assert_eq!(at, 2_200);
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

    #[test]
    fn clock_changes_and_coming_back_refresh_the_events_header() {
        // EVT-01, EVT-02
        let fakes = Fakes::default();
        let mut shell = HomeShell::compose(fakes.platform());
        assert_eq!(shell.events().state().header.weekday, "Thursday");

        *fakes.system.now_ms.lock().unwrap() = 24 * 60 * 60 * 1000;
        *fakes.system.next_alarm_ms.lock().unwrap() = Some(25 * 60 * 60 * 1000);
        shell.dispatch(ShellIntent::ClockChanged);
        assert_eq!(shell.events().state().header.weekday, "Friday");
        assert_eq!(
            shell.events().state().header.alarm.as_deref(),
            Some("1:00 AM")
        );

        *fakes.system.now_ms.lock().unwrap() = 2 * 24 * 60 * 60 * 1000;
        shell.dispatch(ShellIntent::Resumed);
        assert_eq!(shell.events().state().header.weekday, "Saturday");
        assert!(shell.events_ready());
    }

    #[test]
    fn coming_back_reads_notification_access_again() {
        // EVT-03, AC-11: granted in system settings while MeeGone was away.
        let fakes = Fakes::default();
        let mut shell = HomeShell::compose(fakes.platform());
        assert_eq!(shell.events().state().access, Access::Missing);

        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        *fakes.notifications.active.lock().unwrap() = vec![notification("m1", "mail")];
        shell.dispatch(ShellIntent::Resumed);

        assert_eq!(shell.events().state().access, Access::Granted);
        assert_eq!(shell.events().state().groups.len(), 1);
    }

    #[test]
    fn app_changes_reach_the_events_groups() {
        // APPS-17: a new label, or an accent read late, shows in Events too.
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        *fakes.notifications.active.lock().unwrap() = vec![notification("m1", "mail")];
        let mut shell = HomeShell::compose(fakes.platform());
        assert_eq!(shell.events().state().groups[0].label, "Mail");

        fakes.apps.apps.lock().unwrap()[0].label = "Email".into();
        shell.dispatch(ShellIntent::AppsChanged);

        assert_eq!(shell.events().state().groups[0].label, "Email");
    }

    #[test]
    fn groups_coming_and_going_move_the_events_list_and_a_new_count_doesnt() {
        // ANIM-20: Events as it starts shows as it is.
        let fakes = Fakes::default();
        let posted = |key: &str, package: &str, posted_at_ms| Notification {
            posted_at_ms,
            ..notification(key, package)
        };
        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        *fakes.notifications.active.lock().unwrap() = vec![posted("m1", "mail", 10)];
        let mut shell = HomeShell::compose(fakes.platform());
        assert!(!shell.state().events_change.animating);

        // An older notification of mail's changes only its count.
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .push(posted("m0", "mail", 5));
        shell.dispatch(ShellIntent::NotificationsChanged);
        assert_eq!(shell.events().state().groups[0].count(), 2);
        assert!(!shell.state().events_change.animating);

        // A new group slides in.
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .push(posted("c1", "chat", 20));
        shell.dispatch(ShellIntent::NotificationsChanged);
        assert_eq!(
            shell.state().events_change,
            EventsChangeState {
                progress: 0.0,
                animating: true
            }
        );

        let mut at = 1_000;
        shell.dispatch(ShellIntent::Frame(ms(at)));
        while shell.state().events_change.animating {
            at += 8;
            shell.dispatch(ShellIntent::Frame(ms(at)));
        }
        assert_eq!(at, 1_200);

        // A group that goes collapses.
        fakes
            .notifications
            .active
            .lock()
            .unwrap()
            .retain(|n| n.package != "chat");
        shell.dispatch(ShellIntent::NotificationsChanged);
        assert!(shell.state().events_change.animating);
    }

    /// The daily image on, with `date`'s image cached (EVT-12).
    fn cache_image(fakes: &Fakes, date: &str) {
        PersistentImageCache::new(fakes.storage.clone())
            .put(
                ImageRecord {
                    date: date.into(),
                    region: "en-GB".into(),
                    title: format!("Image of {date}"),
                    attribution: "© Someone".into(),
                    info_link: None,
                    applied_at_ms: None,
                    image: None,
                },
                date.as_bytes(),
            )
            .unwrap();
    }

    #[test]
    fn the_image_card_opens_its_details_over_the_home_views() {
        // EVT-12, WALL-15: they cover the views as Settings does, and Back or Home closes them.
        let mut shell = laid_out(&Fakes::default());
        go(&mut shell, Direction::Previous);
        shell.dispatch(ShellIntent::OpenMenu);

        shell.dispatch(ShellIntent::LockImage(LockImageIntent::OpenDetails));
        assert!(shell.state().image_details_open);
        assert_eq!(shell.state().menu, None);
        assert!(!shell.state().menu_offered());
        hold(&mut shell, 500.0, 900.0);
        assert_eq!(shell.state().menu, None);
        lift(&mut shell);

        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().image_details_open);
        assert_eq!(shell.state().current, HomeView::Events);

        shell.dispatch(ShellIntent::LockImage(LockImageIntent::OpenDetails));
        shell.dispatch(ShellIntent::Home);
        assert!(!shell.state().image_details_open);
        assert_eq!(shell.state().current, HomeView::Events);
    }

    #[test]
    fn settings_opened_over_the_image_details_close_first() {
        // HOME-08: Back closes the topmost screen.
        let mut shell = laid_out(&Fakes::default());
        shell.dispatch(ShellIntent::LockImage(LockImageIntent::OpenDetails));
        shell.dispatch(ShellIntent::OpenSettings);

        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().settings_open);
        assert!(shell.state().image_details_open);

        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().image_details_open);
    }

    #[test]
    fn home_from_an_app_closes_the_image_details_for_always_applications() {
        // HOME-06
        let mut shell = laid_out(&Fakes::default());
        go(&mut shell, Direction::Previous);
        shell.dispatch(ShellIntent::Settings(SettingsIntent::SetHomeTarget(
            HomeTarget::Applications,
        )));
        shell.dispatch(ShellIntent::LockImage(LockImageIntent::OpenDetails));

        shell.dispatch(ShellIntent::Returned { home: true });

        assert!(!shell.state().image_details_open);
        assert_eq!(shell.state().current, HomeView::Applications);
    }

    /// Works like [`FakeWallpaper`], except that it panics when asked whether the lock screen
    /// can be set.
    struct BrokenWallpaper;

    impl WallpaperAdapter for BrokenWallpaper {
        fn lock_screen_support(&self) -> Result<(), DataError> {
            panic!("wallpaper bug");
        }

        fn lock_screen_size(&self) -> Size {
            Size::default()
        }

        fn set_lock_screen(&self, _image: &[u8]) -> Result<(), DataError> {
            panic!("wallpaper bug");
        }

        fn reset_lock_screen(&self) -> Result<(), DataError> {
            panic!("wallpaper bug");
        }

        fn open_wallpaper_settings(&self) -> Result<(), DataError> {
            panic!("wallpaper bug");
        }
    }

    #[test]
    fn a_failing_lock_screen_image_never_covers_the_home() {
        // ARCH-09: its details never open, and Events still works.
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        platform.wallpaper = Arc::new(BrokenWallpaper);
        let mut shell = HomeShell::compose(platform);
        assert!(matches!(shell.lockimage().health(), Health::Failed(_)));
        assert!(!shell.lockimage_ready());

        shell.dispatch(ShellIntent::LockImage(LockImageIntent::OpenDetails));
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        shell.dispatch(ShellIntent::Resumed);

        assert!(!shell.state().image_details_open);
        assert!(shell.state().menu_offered());
        assert!(shell.events_ready());
    }

    #[test]
    fn coming_back_and_showing_events_read_the_image_again() {
        // EVT-12: the daily job may have cached a new image while MeeGone was away.
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes);
        shell.dispatch(ShellIntent::LockImage(LockImageIntent::SetEnabled(true)));
        assert_eq!(shell.lockimage().state().current, None);

        cache_image(&fakes, "20261116");
        shell.dispatch(ShellIntent::Resumed);
        let date = |shell: &HomeShell| shell.lockimage().state().current.clone().unwrap().date;
        assert_eq!(date(&shell), "20261116");

        cache_image(&fakes, "20261117");
        go(&mut shell, Direction::Previous);
        assert_eq!(date(&shell), "20261117");
    }

    #[test]
    fn a_new_thumbnail_cross_fades_in_300_ms() {
        // ANIM-22
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes);
        assert_eq!(shell.state().image_change, ImageChangeState::default());

        shell.dispatch(ShellIntent::NewImageShown);
        assert!(shell.state().image_change.animating);
        assert_eq!(shell.state().image_change.progress, 0.0);

        let mut at = 7_000;
        shell.dispatch(ShellIntent::Frame(ms(at)));
        while shell.state().image_change.animating {
            at += 8;
            shell.dispatch(ShellIntent::Frame(ms(at)));
        }
        assert_eq!(at, 7_304);
        assert_eq!(shell.state().image_change, ImageChangeState::default());

        // ANIM-05: with animations removed, the new thumbnail shows at once.
        *fakes.system.animation_scale.lock().unwrap() = 0.0;
        shell.dispatch(ShellIntent::Resumed);
        shell.dispatch(ShellIntent::NewImageShown);
        assert_eq!(shell.state().image_change, ImageChangeState::default());
    }

    /// Works like [`FakeNotifications`], except that it panics when asked for the active
    /// notifications.
    struct BrokenListener(FakeNotifications);

    impl NotificationsAdapter for BrokenListener {
        fn access(&self) -> Access {
            self.0.access()
        }

        fn active(&self) -> Result<Vec<Notification>, DataError> {
            panic!("listener bug");
        }

        fn open(&self, key: &str, from: Option<Rect>) -> Result<(), DataError> {
            self.0.open(key, from)
        }

        fn dismiss(&self, keys: &[String]) -> Result<(), DataError> {
            self.0.dismiss(keys)
        }

        fn open_access_settings(&self) -> Result<(), DataError> {
            self.0.open_access_settings()
        }

        fn open_app_settings(&self, profile: ProfileId, package: &str) -> Result<(), DataError> {
            self.0.open_app_settings(profile, package)
        }
    }

    #[test]
    fn a_failing_events_never_stops_applications() {
        // REL-01, AC-21
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
        let mut platform = fakes.platform();
        let broken = FakeNotifications::default();
        *broken.access.lock().unwrap() = Access::Granted;
        platform.notifications = Arc::new(BrokenListener(broken));
        let mut shell = HomeShell::compose(platform);

        assert_eq!(
            *shell.events().health(),
            Health::Failed("listener bug".into())
        );
        assert!(!shell.events_ready());
        shell.dispatch(ShellIntent::NotificationsChanged);
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        assert_eq!(shell.state().current, HomeView::Events);
        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().current, HomeView::Applications);
        shell.dispatch(ShellIntent::Apps(AppsIntent::Launch {
            app: app_id("mail"),
            from: None,
        }));
        assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
        assert!(shell.apps_ready());
    }

    /// Works like [`FakeApps`], except that it panics when asked to launch or uninstall an app.
    struct BrokenLauncher(FakeApps);

    impl AppsAdapter for BrokenLauncher {
        fn launchable_apps(&self) -> Result<Vec<App>, DataError> {
            self.0.launchable_apps()
        }

        fn profiles(&self) -> Result<Vec<Profile>, DataError> {
            self.0.profiles()
        }

        fn default_apps(&self) -> Vec<String> {
            self.0.default_apps()
        }

        fn search_key(&self, text: &str) -> String {
            self.0.search_key(text)
        }

        fn launch(&self, _app: &AppId, _from: Option<Rect>) -> Result<(), DataError> {
            panic!("launcher bug");
        }

        fn shortcuts(&self, app: &AppId) -> Result<Vec<Shortcut>, DataError> {
            self.0.shortcuts(app)
        }

        fn launch_shortcut(&self, shortcut: &Shortcut) -> Result<(), DataError> {
            self.0.launch_shortcut(shortcut)
        }

        fn request_uninstall(&self, _app: &AppId) -> Result<(), DataError> {
            panic!("uninstaller bug");
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

        shell.dispatch(ShellIntent::Apps(AppsIntent::Launch {
            app: app_id("mail"),
            from: None,
        }));

        assert_eq!(
            *shell.apps().health(),
            Health::Failed("launcher bug".into())
        );
        assert!(!shell.apps_ready());
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
        *fakes.usage.uses.lock().unwrap() = vec![used("maps", 900_000)];
        let mut shell = HomeShell::compose(fakes.platform());

        fakes.usage.uses.lock().unwrap().push(used("mail", 999_000));
        shell.dispatch(ShellIntent::Returned { home: false });

        let cards = &shell.recent().state().cards;
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].app.label, "Mail");
    }

    /// Usage access, where Maps and then Mail were used.
    fn maps_and_mail_used(fakes: &Fakes) {
        *fakes.usage.access.lock().unwrap() = Access::Granted;
        *fakes.apps.apps.lock().unwrap() = vec![app("maps", "Maps"), app("mail", "Mail")];
        *fakes.system.now_ms.lock().unwrap() = 1_000_000;
        *fakes.usage.uses.lock().unwrap() = ["maps", "mail"]
            .iter()
            .zip([900_000, 999_000])
            .map(|(package, at)| used(package, at))
            .collect();
    }

    /// A shell on Applications, with usage access, where Maps and then Mail were used.
    fn with_recent_maps_and_mail(fakes: &Fakes) -> HomeShell {
        maps_and_mail_used(fakes);
        HomeShell::compose(fakes.platform())
    }

    #[test]
    fn recent_follows_usage_access_as_it_changes() {
        // REC-02: access revoked while Recent shows brings the explainer back, and granted
        // brings the cards.
        let fakes = Fakes::default();
        let mut shell = with_recent_maps_and_mail(&fakes);
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        assert_eq!(shell.state().current, HomeView::Recent);
        assert_eq!(recent_labels(&shell), ["Mail", "Maps"]);

        *fakes.usage.access.lock().unwrap() = Access::Missing;
        shell.dispatch(ShellIntent::UsageAccessChanged);
        assert_eq!(shell.recent().state().access, Access::Missing);
        assert!(recent_labels(&shell).is_empty());
        assert_eq!(shell.recent().state().problem, None);

        *fakes.usage.access.lock().unwrap() = Access::Granted;
        shell.dispatch(ShellIntent::UsageAccessChanged);
        assert_eq!(recent_labels(&shell), ["Mail", "Maps"]);
        assert!(shell.recent_ready());
    }

    /// Usage access that panics when asked what was used.
    struct BrokenUsage;

    impl UsageAdapter for BrokenUsage {
        fn access(&self) -> Access {
            Access::Granted
        }

        fn used_since(&self, _since_ms: u64) -> Result<Vec<RecentUse>, DataError> {
            panic!("usage bug");
        }

        fn open_access_settings(&self) -> Result<(), DataError> {
            Ok(())
        }
    }

    #[test]
    fn a_failing_recent_never_stops_applications() {
        // REL-01, ARCH-09
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
        let mut platform = fakes.platform();
        platform.usage = Arc::new(BrokenUsage);
        let mut shell = HomeShell::compose(platform);

        assert_eq!(*shell.recent().health(), Health::Failed("usage bug".into()));
        assert!(!shell.recent_ready());
        shell.dispatch(ShellIntent::UsageAccessChanged);
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        assert_eq!(shell.state().current, HomeView::Recent);
        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().current, HomeView::Applications);
        shell.dispatch(ShellIntent::Apps(AppsIntent::Launch {
            app: app_id("mail"),
            from: None,
        }));
        assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
        assert!(shell.apps_ready());
    }

    fn recent_labels(shell: &HomeShell) -> Vec<String> {
        let cards = &shell.recent().state().cards;
        cards.iter().map(|card| card.app.label.clone()).collect()
    }

    fn hide_maps(shell: &mut HomeShell) {
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenMenu(CellKey::App(
            app_id("maps"),
        ))));
        shell.dispatch(ShellIntent::Apps(AppsIntent::ChooseMenuItem(
            AppMenuItem::Hide,
        )));
    }

    #[test]
    fn hiding_or_unhiding_an_app_takes_it_out_of_recent_and_back_at_once() {
        // APPS-19, REC-03: without waiting for Recent to show, so a swipe never shows it.
        let fakes = Fakes::default();
        let mut shell = with_recent_maps_and_mail(&fakes);
        assert_eq!(recent_labels(&shell), ["Mail", "Maps"]);

        hide_maps(&mut shell);
        assert_eq!(shell.state().current, HomeView::Applications);
        assert_eq!(recent_labels(&shell), ["Mail"]);

        shell.dispatch(ShellIntent::Apps(AppsIntent::Unhide(app_id("maps"))));
        assert_eq!(recent_labels(&shell), ["Mail", "Maps"]);
    }

    #[test]
    fn a_hide_that_cant_be_saved_leaves_recent_as_it_was() {
        // APPS-19, REL-03
        let fakes = Fakes::default();
        let mut shell = with_recent_maps_and_mail(&fakes);
        *fakes.storage.error.lock().unwrap() = Some(DataError::Failed("disk full".into()));

        hide_maps(&mut shell);

        assert_eq!(shell.apps().state().problem.as_deref(), Some("disk full"));
        assert_eq!(recent_labels(&shell), ["Mail", "Maps"]);
    }

    #[test]
    fn hidden_apps_reach_recent_from_the_start() {
        // APPS-19: Recent reads them as it loads, and nothing refreshes it twice.
        let fakes = Fakes::default();
        hide_maps(&mut with_recent_maps_and_mail(&fakes));

        let restarted = HomeShell::compose(fakes.platform());

        assert_eq!(recent_labels(&restarted), ["Mail"]);
        assert!(Arc::ptr_eq(
            &restarted.seen_hidden,
            &restarted.apps().state().hidden_apps
        ));
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

    /// Recent with Mail and Maps, laid out on a phone 1000 px wide and showing.
    fn on_recent(fakes: &Fakes) -> HomeShell {
        let mut shell = with_recent_maps_and_mail(fakes);
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 1000.0,
            density: 2.0,
            touch_slop_px: 16.0,
            rtl: false,
        }));
        go(&mut shell, Direction::Next);
        assert_eq!(shell.state().current, HomeView::Recent);
        shell
    }

    fn touch_recent(shell: &mut HomeShell, kind: PointerKind, x: f64, at: u64) {
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x,
            y: 1_000.0,
            at: ms(at),
        }));
    }

    fn fingers(shell: &mut HomeShell, kind: PinchKind, span_px: f64, at: u64) {
        shell.dispatch(ShellIntent::Pinch(PinchEvent {
            kind,
            span_px,
            x: 500.0,
            y: 1_000.0,
            at: ms(at),
        }));
    }

    /// Two fingers 300 px apart move to `to` px apart and lift, the first one last.
    fn pinch(shell: &mut HomeShell, to: f64) {
        touch_recent(shell, PointerKind::Down, 350.0, 2_000);
        fingers(shell, PinchKind::Start, 300.0, 2_010);
        for step in 1..=10_u32 {
            let span = 300.0 + (to - 300.0) * f64::from(step) / 10.0;
            touch_recent(
                shell,
                PointerKind::Move,
                350.0 - (span - 300.0) / 2.0,
                2_010 + 8 * u64::from(step),
            );
            fingers(shell, PinchKind::Move, span, 2_010 + 8 * u64::from(step));
        }
        fingers(shell, PinchKind::End, to, 2_100);
        touch_recent(shell, PointerKind::Up, 350.0 - (to - 300.0) / 2.0, 2_110);
    }

    #[test]
    fn spreading_two_fingers_on_recent_shows_bigger_cards_and_saves_them() {
        // REC-05, ANIM-17
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        assert!(shell.state().density_offered);
        let reflow = shell.state().pinch.reflow;

        pinch(&mut shell, 450.0);

        assert_eq!(shell.recent().state().density, RecentDensity::TwoColumns);
        assert_eq!(shell.recent().state().columns(), 2);
        assert_ne!(shell.state().pinch.reflow, reflow);
        assert!(shell.state().pinch.animating, "the cards reflow");
        let restarted = HomeShell::compose(fakes.platform());
        assert_eq!(
            restarted.recent().state().density,
            RecentDensity::TwoColumns
        );
        assert!(
            !restarted.state().pinch.animating,
            "nothing reflows at start"
        );

        // Bringing the fingers together shows more cards again.
        pinch(&mut shell, 200.0);
        assert_eq!(shell.recent().state().density, RecentDensity::ThreeColumns);
    }

    #[test]
    fn a_small_pinch_lets_the_cards_go_back() {
        // ANIM-17
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        let writes = fakes.storage.writes.lock().unwrap().len();

        pinch(&mut shell, 320.0);

        assert_eq!(shell.recent().state().density, RecentDensity::ThreeColumns);
        assert!(shell.state().pinch.animating);
        assert_eq!(
            fakes.storage.writes.lock().unwrap().len(),
            writes,
            "nothing saved"
        );
        for frame in 0..40 {
            shell.dispatch(ShellIntent::Frame(ms(2_200 + frame * 16)));
        }
        assert!(!shell.state().pinch.animating);
        assert_eq!(shell.state().pinch.scale, 1.0);
    }

    #[test]
    fn the_cards_scale_with_the_fingers_and_the_views_stay_put() {
        // ANIM-01, ANIM-17, HOME-03
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        touch_recent(&mut shell, PointerKind::Down, 350.0, 2_000);
        fingers(&mut shell, PinchKind::Start, 300.0, 2_010);
        assert!(shell.state().pinch.holding);

        // The first finger moves far sideways as the two spread.
        touch_recent(&mut shell, PointerKind::Move, 100.0, 2_020);
        fingers(&mut shell, PinchKind::Move, 390.0, 2_020);
        assert!((shell.state().pinch.scale - 1.3).abs() < 1e-9);
        assert!(!shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, 0.0);

        // The finger left after the pinch moves no view either.
        fingers(&mut shell, PinchKind::End, 390.0, 2_030);
        touch_recent(&mut shell, PointerKind::Move, 900.0, 2_040);
        touch_recent(&mut shell, PointerKind::Up, 900.0, 2_050);
        assert_eq!(shell.state().current, HomeView::Recent);
        assert!(!shell.state().carousel.dragging);

        // The next gesture is the carousel's again.
        touch_recent(&mut shell, PointerKind::Down, 900.0, 3_000);
        assert!(!shell.state().pinch.holding);
        touch_recent(&mut shell, PointerKind::Move, 500.0, 3_016);
        assert!(shell.state().carousel.dragging);
    }

    #[test]
    fn a_long_press_never_counts_once_a_pinch_starts() {
        // HOME-10, REC-07
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        touch_recent(&mut shell, PointerKind::Down, 350.0, 2_000);
        fingers(&mut shell, PinchKind::Start, 300.0, 2_010);

        shell.dispatch(ShellIntent::LongPress);

        assert!(!shell.recent().state().close_mode);
    }

    #[test]
    fn no_pinch_starts_off_recent_under_a_menu_or_without_cards() {
        let starts = |shell: &mut HomeShell| {
            touch_recent(shell, PointerKind::Down, 350.0, 2_000);
            fingers(shell, PinchKind::Start, 300.0, 2_010);
            let holding = shell.state().pinch.holding;
            fingers(shell, PinchKind::End, 600.0, 2_020);
            touch_recent(shell, PointerKind::Up, 350.0, 2_030);
            holding
        };
        assert!(starts(&mut on_recent(&Fakes::default())));
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);

        // Other views.
        go(&mut shell, Direction::Previous);
        assert!(!shell.state().density_offered);
        assert!(!starts(&mut shell));

        // Settings over Recent.
        go(&mut shell, Direction::Next);
        shell.dispatch(ShellIntent::OpenSettings);
        assert!(!starts(&mut shell));
        shell.dispatch(ShellIntent::Back);

        // Without usage access: the explainer, no cards.
        *fakes.usage.access.lock().unwrap() = Access::Missing;
        shell.dispatch(ShellIntent::UsageAccessChanged);
        assert!(!shell.state().density_offered);
        assert!(!starts(&mut shell));
        assert_eq!(shell.recent().state().density, RecentDensity::ThreeColumns);
    }

    #[test]
    fn a_failing_recent_offers_no_pinch() {
        // ARCH-09
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        platform.usage = Arc::new(BrokenUsage);
        let mut shell = HomeShell::compose(platform);
        go(&mut shell, Direction::Next);

        assert!(!shell.state().density_offered);
        touch_recent(&mut shell, PointerKind::Down, 350.0, 2_000);
        fingers(&mut shell, PinchKind::Start, 300.0, 2_010);
        assert!(!shell.state().pinch.holding);
    }

    #[test]
    fn home_during_a_pinch_lets_the_cards_go_back() {
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        touch_recent(&mut shell, PointerKind::Down, 350.0, 2_000);
        fingers(&mut shell, PinchKind::Start, 300.0, 2_010);
        fingers(&mut shell, PinchKind::Move, 600.0, 2_020);

        shell.dispatch(ShellIntent::Home);
        fingers(&mut shell, PinchKind::End, 600.0, 2_030);

        assert_eq!(shell.recent().state().density, RecentDensity::ThreeColumns);
        assert_eq!(shell.state().pinch.scale, 1.0);
    }

    #[test]
    fn a_density_chosen_from_a_screen_reader_reflows_the_cards() {
        // A11Y-05, ANIM-17
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        let reflow = shell.state().pinch.reflow;

        shell.dispatch(ShellIntent::Recent(RecentIntent::SetDensity(
            RecentDensity::TwoColumns,
        )));

        assert_eq!(shell.recent().state().columns(), 2);
        assert_ne!(shell.state().pinch.reflow, reflow);
        assert!(shell.state().pinch.animating);
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

    /// Draws frames every 8 ms from `from_ms` until nothing moves.
    fn come_to_rest(shell: &mut HomeShell, from_ms: u64) {
        for frame in 0..200 {
            shell.dispatch(ShellIntent::Frame(ms(from_ms + frame * 8)));
        }
        let state = shell.state();
        assert!(!state.lift.animating && !state.mode.animating, "{state:?}");
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
        maps_and_mail_used(&fakes);
        let mut shell = laid_out(&fakes);
        go(&mut shell, Direction::Next);

        hold(&mut shell, 500.0, 900.0);

        assert_eq!(shell.state().menu, None);
        assert!(shell.recent().state().close_mode);
        assert_eq!(ticks(&fakes), 1);
    }

    /// A finger held still at `(x, y)` from `at` ms until the long-press time has passed, then
    /// lifted.
    fn hold_and_lift(shell: &mut HomeShell, x: f64, y: f64, at: u64) {
        let pointer = |kind, at| {
            ShellIntent::Pointer(PointerEvent {
                kind,
                x,
                y,
                at: ms(at),
            })
        };
        shell.dispatch(pointer(PointerKind::Down, at));
        shell.dispatch(ShellIntent::LongPress);
        shell.dispatch(pointer(PointerKind::Up, at + 500));
    }

    #[test]
    fn a_long_press_on_recent_counts_only_where_close_mode_can_start() {
        // REC-07, ANIM-24: with no card to remove, or in close mode already, it takes nothing,
        // and nothing ticks.
        let fakes = Fakes::default();
        let mut shell = laid_out(&fakes);
        go(&mut shell, Direction::Next);
        hold(&mut shell, 500.0, 900.0);
        assert!(!shell.state().carousel.held);
        assert!(!shell.recent().state().close_mode);
        assert_eq!(ticks(&fakes), 0);

        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        hold_and_lift(&mut shell, 500.0, 900.0, 10_000);
        assert!(shell.recent().state().close_mode);
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Down,
            x: 500.0,
            y: 900.0,
            at: ms(11_000),
        }));
        shell.dispatch(ShellIntent::LongPress);
        assert!(!shell.state().carousel.held);
        assert_eq!(ticks(&fakes), 1);
    }

    #[test]
    fn close_mode_shrinks_the_cards_and_pops_their_badges() {
        // REC-07, ANIM-12
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        assert!(shell.state().close_offered);
        assert!(shell.state().density_offered);

        hold_and_lift(&mut shell, 500.0, 900.0, 10_000);
        let state = shell.state();
        assert!(state.close.on && state.close.animating);
        assert!(!state.close_offered);
        assert!(!state.density_offered, "no pinch while removing cards");
        come_to_rest(&mut shell, 20_000);
        let close = shell.state().close;
        assert_eq!(close.scale, MODE_SCALE);
        assert_eq!(close.badges[0], 1.0);
        assert_eq!(
            shell.state().mode,
            ModeState::default(),
            "edit mode is Applications'"
        );

        // Reversed on exit.
        shell.dispatch(ShellIntent::Back);
        let close = shell.state().close;
        assert!(!close.on && close.animating);
        come_to_rest(&mut shell, 30_000);
        assert_eq!(shell.state().close, ModeState::default());
        assert!(shell.state().close_offered);
        assert_eq!(ticks(&fakes), 1);
    }

    #[test]
    fn a_tap_on_empty_space_back_home_or_another_view_leaves_close_mode() {
        // REC-07. A screen reader enters it without a tick (A11Y-05, ANIM-24).
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        let enter = |shell: &mut HomeShell| {
            shell.dispatch(ShellIntent::Recent(RecentIntent::EnterCloseMode));
            assert!(shell.state().close.on);
        };

        enter(&mut shell);
        shell.dispatch(ShellIntent::Recent(RecentIntent::ExitCloseMode));
        assert!(!shell.state().close.on);

        enter(&mut shell);
        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().close.on);

        enter(&mut shell);
        shell.dispatch(ShellIntent::Home);
        assert!(!shell.state().close.on);
        assert_eq!(shell.state().current, HomeView::Recent);

        enter(&mut shell);
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        assert!(!shell.recent().state().close_mode);
        assert!(!shell.state().close.on);
        assert_eq!(ticks(&fakes), 0);
    }

    #[test]
    fn a_removed_card_shrinks_away_and_the_others_close_the_gap() {
        // REC-08, ANIM-18
        let fakes = Fakes::default();
        let mut shell = on_recent(&fakes);
        shell.dispatch(ShellIntent::Recent(RecentIntent::EnterCloseMode));
        come_to_rest(&mut shell, 20_000);
        let reflow = shell.state().pinch.reflow;

        shell.dispatch(ShellIntent::Recent(RecentIntent::Remove(app_id("mail"))));

        let state = shell.state();
        assert_eq!(recent_labels(&shell), ["Maps"]);
        assert!(state.recent_change.animating);
        assert_eq!(state.recent_change.leave[0], 0.0);
        assert_ne!(state.pinch.reflow, reflow, "Maps closes the gap");
        assert!(state.close.on, "Maps can still be removed");
        come_to_rest(&mut shell, 30_000);
        assert_eq!(shell.state().recent_change, RecentChangeState::default());
        assert!(!shell.state().pinch.animating);
    }

    #[test]
    fn clear_all_collapses_the_cards_row_by_row() {
        // REC-08, ANIM-18, AC-14: seven cards, three rows of three.
        let fakes = Fakes::default();
        let packages = ["a", "b", "c", "d", "e", "f", "g"];
        *fakes.usage.access.lock().unwrap() = Access::Granted;
        *fakes.apps.apps.lock().unwrap() = packages.iter().map(|p| app(p, p)).collect();
        *fakes.system.now_ms.lock().unwrap() = 1_000_000;
        *fakes.usage.uses.lock().unwrap() = (0..)
            .zip(packages)
            .map(|(at, package)| used(package, 900_000 + at))
            .collect();
        let mut shell = laid_out(&fakes);
        go(&mut shell, Direction::Next);
        shell.dispatch(ShellIntent::Recent(RecentIntent::EnterCloseMode));
        let reflow = shell.state().pinch.reflow;

        shell.dispatch(ShellIntent::Recent(RecentIntent::ClearAll));

        let state = shell.state();
        assert!(recent_labels(&shell).is_empty());
        assert!(!state.close.on, "nothing is left to remove");
        assert!(state.recent_change.animating);
        assert_eq!(state.recent_change.leave[..3], [0.0; 3]);
        assert_eq!(state.recent_change.leave[3], 1.0, "only three rows");
        assert_eq!(state.pinch.reflow, reflow, "no card is left to move");
        // Within 400 ms.
        for frame in 0..=50 {
            shell.dispatch(ShellIntent::Frame(ms(20_000 + frame * 8)));
        }
        assert!(!shell.state().recent_change.animating);
    }

    #[test]
    fn cards_that_go_out_of_sight_just_go() {
        // ANIM-18: hiding Maps in Applications takes its card out of Recent, unseen.
        let fakes = Fakes::default();
        let mut shell = with_recent_maps_and_mail(&fakes);

        hide_maps(&mut shell);

        assert_eq!(recent_labels(&shell), ["Mail"]);
        assert!(!shell.state().recent_change.animating);
    }

    #[test]
    fn a_failing_recent_offers_no_close_mode() {
        // ARCH-09
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        platform.usage = Arc::new(BrokenUsage);
        let mut shell = HomeShell::compose(platform);
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 1000.0,
            density: 2.0,
            touch_slop_px: 16.0,
            rtl: false,
        }));
        go(&mut shell, Direction::Next);

        hold(&mut shell, 500.0, 900.0);

        let state = shell.state();
        assert!(!state.close_offered);
        assert_eq!(state.close, ModeState::default());
        assert!(!state.carousel.held);
        assert_eq!(ticks(&fakes), 0);
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
    fn a_pressed_icon_shrinks_and_springs_back_with_the_frames() {
        // ANIM-10: the host draws frames while it moves, and none while it is held.
        let mut shell = laid_out(&Fakes::default());

        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 5,
            at: ms(1_000),
        });
        assert_eq!(shell.state().press.item, Some(5));
        assert!(shell.state().press.animating);

        shell.dispatch(ShellIntent::Frame(ms(1_100)));
        assert_eq!(shell.state().press.scale, ICON_PRESS_SCALE);
        assert!(!shell.state().press.animating);

        shell.dispatch(ShellIntent::Release(ms(1_200)));
        assert!(shell.state().press.animating);
        for frame in 0..60 {
            shell.dispatch(ShellIntent::Frame(ms(1_200 + frame * 16)));
        }
        assert_eq!(shell.state().press, PressState::default());
    }

    #[test]
    fn a_drag_on_the_views_lets_the_pressed_icon_go() {
        // HOME-03: the views move instead, so the icon springs back.
        let mut shell = laid_out(&Fakes::default());
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Down,
            x: 800.0,
            y: 900.0,
            at: ms(0),
        }));
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 2,
            at: ms(0),
        });
        shell.dispatch(ShellIntent::Frame(ms(90)));
        assert_eq!(shell.state().press.scale, ICON_PRESS_SCALE);

        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Move,
            x: 600.0,
            y: 900.0,
            at: ms(100),
        }));
        assert!(shell.state().carousel.dragging);
        shell.dispatch(ShellIntent::Frame(ms(116)));

        assert!(shell.state().press.scale > ICON_PRESS_SCALE);
        // While the views move, nothing else is pressed.
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 3,
            at: ms(120),
        });
        assert_eq!(shell.state().press.item, Some(2));
    }

    #[test]
    fn the_menu_settings_or_another_view_let_the_pressed_icon_go_at_once() {
        let mut shell = laid_out(&Fakes::default());
        let press = |shell: &mut HomeShell| {
            shell.dispatch(ShellIntent::Press {
                surface: Surface::Grid,
                item: 1,
                at: ms(0),
            });
            assert_eq!(shell.state().press.item, Some(1));
        };

        press(&mut shell);
        shell.dispatch(ShellIntent::OpenMenu);
        assert_eq!(shell.state().press, PressState::default());

        // Under the menu or Settings, nothing is pressed.
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 1,
            at: ms(0),
        });
        assert_eq!(shell.state().press, PressState::default());
        shell.dispatch(ShellIntent::CloseMenu);

        press(&mut shell);
        shell.dispatch(ShellIntent::OpenSettings);
        assert_eq!(shell.state().press, PressState::default());
        shell.dispatch(ShellIntent::Back);

        press(&mut shell);
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        assert_eq!(shell.state().press, PressState::default());
    }

    /// A phone 1000 px wide, laid out, with Mail and Maps in Applications.
    fn with_mail(fakes: &Fakes) -> HomeShell {
        *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail"), app("maps", "Maps")];
        laid_out(fakes)
    }

    /// A finger held still on `package`'s icon at `(x, y)` until the host's long-press time has
    /// passed (APPS-11).
    fn hold_app(shell: &mut HomeShell, package: &str, x: f64, y: f64) {
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind: PointerKind::Down,
            x,
            y,
            at: ms(10_000),
        }));
        shell.dispatch(ShellIntent::LongPressCell {
            surface: Surface::Grid,
            key: CellKey::App(app_id(package)),
        });
    }

    fn finger(shell: &mut HomeShell, kind: PointerKind, x: f64, y: f64) {
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x,
            y,
            at: ms(10_200),
        }));
    }

    fn app_menu_of(shell: &HomeShell) -> Option<AppId> {
        shell
            .apps()
            .state()
            .menu
            .as_ref()
            .and_then(|menu| match &menu.item {
                CellKey::App(app) => Some(app.clone()),
                CellKey::Folder(_) => None,
            })
    }

    #[test]
    fn a_long_press_on_an_app_opens_its_menu_and_ticks() {
        // APPS-11, ANIM-24
        let fakes = Fakes::default();
        let mut shell = with_mail(&fakes);
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 0,
            at: ms(10_000),
        });

        hold_app(&mut shell, "mail", 300.0, 400.0);

        assert_eq!(app_menu_of(&shell), Some(app_id("mail")));
        let state = shell.state();
        assert!(state.app_menu_open);
        assert_eq!(state.menu, None);
        assert!(!state.menu_offered());
        assert_eq!(state.press, PressState::default());
        assert_eq!(ticks(&fakes), 1);
        // The rest of the gesture is the shell's, so lifting the finger launches nothing.
        assert!(state.carousel.held);
        lift(&mut shell);
        assert!(!shell.state().carousel.held);
        assert!(shell.state().app_menu_open);
        assert!(fakes.apps.launched.lock().unwrap().is_empty());
    }

    #[test]
    fn a_long_press_on_an_app_counts_only_while_nothing_covers_applications() {
        // APPS-11: no menu, no tick, and the gesture stays the content's.
        let not_taken = |shell: &HomeShell, fakes: &Fakes| {
            assert_eq!(app_menu_of(shell), None);
            assert!(!shell.state().carousel.held);
            assert_eq!(ticks(fakes), 0);
        };
        let fakes = Fakes::default();
        let mut shell = with_mail(&fakes);

        // The finger moved past the touch slop: a scroll that stopped.
        finger(&mut shell, PointerKind::Down, 300.0, 400.0);
        finger(&mut shell, PointerKind::Move, 300.0, 480.0);
        shell.dispatch(ShellIntent::LongPressCell {
            surface: Surface::Grid,
            key: CellKey::App(app_id("mail")),
        });
        not_taken(&shell, &fakes);
        lift(&mut shell);

        // Under Settings or the home menu.
        shell.dispatch(ShellIntent::OpenSettings);
        hold_app(&mut shell, "mail", 300.0, 400.0);
        not_taken(&shell, &fakes);
        lift(&mut shell);
        shell.dispatch(ShellIntent::Back);
        shell.dispatch(ShellIntent::OpenMenu);
        hold_app(&mut shell, "mail", 300.0, 400.0);
        not_taken(&shell, &fakes);
        lift(&mut shell);
        shell.dispatch(ShellIntent::CloseMenu);

        // An app that isn't in the grid has no menu.
        hold_app(&mut shell, "gone", 300.0, 400.0);
        not_taken(&shell, &fakes);
        lift(&mut shell);

        // Only Applications has app menus.
        go(&mut shell, Direction::Previous);
        hold_app(&mut shell, "mail", 300.0, 400.0);
        not_taken(&shell, &fakes);
    }

    #[test]
    fn dragging_after_a_long_press_lifts_the_icon_and_enters_edit_mode() {
        // APPS-11: the menu closes, edit mode starts, and the icon follows the finger. The
        // views stay put, even for a sideways drag.
        let fakes = Fakes::default();
        let mut shell = with_mail(&fakes);
        hold_app(&mut shell, "mail", 300.0, 400.0);

        // Within the touch slop, it is still a long-press.
        finger(&mut shell, PointerKind::Move, 310.0, 410.0);
        assert!(shell.state().app_menu_open);
        assert_eq!(shell.state().lift, LiftState::default());

        finger(&mut shell, PointerKind::Move, 520.0, 380.0);
        let state = shell.state();
        assert!(!state.app_menu_open);
        assert!(shell.apps().state().edit_mode);
        // The grid isn't laid out, so the icon's centre is the finger.
        let lifted = &state.lift;
        assert_eq!(
            (&lifted.key, lifted.x, lifted.y, lifted.settling),
            (&Some(CellKey::App(app_id("mail"))), 520.0, 380.0, false)
        );
        assert_eq!(state.carousel.offset, 0.0);
        assert!(!state.carousel.dragging);
        // ANIM-13: it lifts to 110 % on a spring.
        for frame in 0..40 {
            shell.dispatch(ShellIntent::Frame(ms(10_200 + frame * 8)));
        }
        assert!((shell.state().lift.scale - DRAG_LIFT_SCALE).abs() < 0.001);

        finger(&mut shell, PointerKind::Move, 700.0, 900.0);
        assert_eq!((shell.state().lift.x, shell.state().lift.y), (700.0, 900.0));

        // Lifting the finger drops it: with no grid laid out, there is no cell to settle into,
        // so it is back at once. Edit mode stays.
        lift(&mut shell);
        assert_eq!(shell.state().lift, LiftState::default());
        assert!(shell.apps().state().edit_mode);
        assert!(!shell.state().carousel.held);
        assert!(fakes.apps.launched.lock().unwrap().is_empty());
    }

    #[test]
    fn once_the_menu_is_closed_a_drag_picks_nothing_up() {
        // HOME-08: Back closes the menu while the finger is still down.
        let mut shell = with_mail(&Fakes::default());
        hold_app(&mut shell, "mail", 300.0, 400.0);

        shell.dispatch(ShellIntent::Back);
        finger(&mut shell, PointerKind::Move, 600.0, 400.0);

        assert_eq!(shell.state().lift, LiftState::default());
        assert!(!shell.apps().state().edit_mode);
    }

    #[test]
    fn a_lifted_icon_goes_back_on_back_a_new_layout_or_apps_coming_and_going() {
        let fakes = Fakes::default();
        let pick_up = |shell: &mut HomeShell| {
            hold_app(shell, "mail", 300.0, 400.0);
            finger(shell, PointerKind::Move, 300.0, 600.0);
            assert!(shell.state().lift.key.is_some());
        };

        let mut shell = with_mail(&fakes);
        pick_up(&mut shell);
        // Back ends edit mode, and the icon goes back.
        shell.dispatch(ShellIntent::Back);
        assert!(!shell.apps().state().edit_mode);
        assert_eq!(shell.state().lift, LiftState::default());
        lift(&mut shell);

        pick_up(&mut shell);
        let layout = shell.state().carousel.layout;
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 2000.0,
            ..layout
        }));
        assert_eq!(shell.state().lift, LiftState::default());
        lift(&mut shell);

        // An app coming leaves it lifted; its own app going puts it back.
        pick_up(&mut shell);
        fakes.apps.apps.lock().unwrap().push(app("atlas", "Atlas"));
        shell.dispatch(ShellIntent::AppsChanged);
        assert!(shell.state().lift.key.is_some());
        fakes
            .apps
            .apps
            .lock()
            .unwrap()
            .retain(|app| app.id != app_id("mail"));
        shell.dispatch(ShellIntent::AppsChanged);
        assert_eq!(shell.state().lift, LiftState::default());
        // A later move picks nothing up.
        finger(&mut shell, PointerKind::Move, 300.0, 700.0);
        assert_eq!(shell.state().lift, LiftState::default());
    }

    #[test]
    fn back_and_home_close_the_app_menu() {
        // HOME-07, HOME-08: Home goes no further than closing it.
        let mut shell = with_mail(&Fakes::default());
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(480.0)));
        hold_app(&mut shell, "mail", 300.0, 400.0);
        lift(&mut shell);

        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().app_menu_open);
        assert_eq!(shell.state().current, HomeView::Applications);

        hold_app(&mut shell, "mail", 300.0, 400.0);
        lift(&mut shell);
        shell.dispatch(ShellIntent::Home);
        assert!(!shell.state().app_menu_open);
        assert_eq!(shell.apps().state().scroll_px, 480.0);
        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.apps().state().scroll_px, 0.0);
    }

    #[test]
    fn a_touch_outside_another_view_or_settings_close_the_app_menu() {
        let mut shell = with_mail(&Fakes::default());
        let open = |shell: &mut HomeShell| {
            shell.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app_id("maps"))));
            assert!(shell.state().app_menu_open);
        };

        open(&mut shell);
        shell.dispatch(ShellIntent::CloseMenu);
        assert!(!shell.state().app_menu_open);

        open(&mut shell);
        shell.dispatch(ShellIntent::OpenSettings);
        assert!(!shell.state().app_menu_open);
        shell.dispatch(ShellIntent::Back);

        // A screen reader can still switch views while it is open.
        open(&mut shell);
        shell.dispatch(ShellIntent::Swipe(Direction::Next));
        assert!(!shell.state().app_menu_open);
        assert_eq!(app_menu_of(&shell), None);
    }

    #[test]
    fn a_screen_reader_opens_an_apps_menu_without_a_tick() {
        // A11Y-05: in Applications only, and with nothing to pick up.
        let fakes = Fakes::default();
        let mut shell = with_mail(&fakes);

        shell.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app_id("mail"))));
        assert_eq!(app_menu_of(&shell), Some(app_id("mail")));
        assert_eq!(ticks(&fakes), 0);
        assert!(!shell.state().carousel.held);
        // Another app's menu doesn't replace an open one.
        shell.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app_id("maps"))));
        assert_eq!(app_menu_of(&shell), Some(app_id("mail")));
        shell.dispatch(ShellIntent::CloseMenu);

        go(&mut shell, Direction::Next);
        shell.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app_id("mail"))));
        assert_eq!(app_menu_of(&shell), None);
    }

    #[test]
    fn while_an_apps_menu_is_open_the_views_get_no_touches() {
        // It covers the home views, as the home menu does.
        let fakes = Fakes::default();
        let mut shell = with_mail(&fakes);
        shell.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app_id("mail"))));

        finger(&mut shell, PointerKind::Down, 800.0, 900.0);
        finger(&mut shell, PointerKind::Move, 300.0, 900.0);
        assert!(!shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, 0.0);
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 1,
            at: ms(0),
        });
        assert_eq!(shell.state().press, PressState::default());
        shell.dispatch(ShellIntent::LongPress);
        shell.dispatch(ShellIntent::OpenMenu);
        assert_eq!(shell.state().menu, None);
        assert_eq!(ticks(&fakes), 0);
    }

    #[test]
    fn the_app_menu_stays_open_when_the_layout_changes() {
        // COMP-03: it is drawn at its icon, wherever that is now.
        let mut shell = with_mail(&Fakes::default());
        hold_app(&mut shell, "mail", 300.0, 400.0);
        lift(&mut shell);
        let layout = shell.state().carousel.layout;

        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 2000.0,
            ..layout
        }));

        assert_eq!(app_menu_of(&shell), Some(app_id("mail")));
    }

    #[test]
    fn a_failing_applications_never_leaves_the_home_covered() {
        // ARCH-09: a menu Applications can no longer close doesn't count.
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        let broken = FakeApps::default();
        broken.apps.lock().unwrap().push(app("mail", "Mail"));
        platform.apps = Arc::new(BrokenLauncher(broken));
        let mut shell = HomeShell::compose(platform);
        shell.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app_id("mail"))));
        assert!(shell.state().app_menu_open);

        shell.dispatch(ShellIntent::Apps(AppsIntent::Launch {
            app: app_id("mail"),
            from: None,
        }));

        assert!(matches!(shell.apps().health(), Health::Failed(_)));
        assert!(!shell.state().app_menu_open);
        assert!(shell.state().menu_offered());
        shell.dispatch(ShellIntent::OpenMenu);
        assert!(shell.state().menu.is_some());
    }

    /// Laid out on Events, with mail's three notifications and chat's one.
    fn on_events(fakes: &Fakes) -> HomeShell {
        let posted = |key: &str, package: &str, posted_at_ms| Notification {
            posted_at_ms,
            ..notification(key, package)
        };
        *fakes.notifications.access.lock().unwrap() = Access::Granted;
        *fakes.notifications.active.lock().unwrap() = vec![
            posted("m1", "mail", 10),
            posted("m2", "mail", 20),
            posted("m3", "mail", 30),
            posted("c1", "chat", 25),
        ];
        let mut shell = laid_out(fakes);
        go(&mut shell, Direction::Previous);
        assert_eq!(shell.state().current, HomeView::Events);
        shell
    }

    fn row_of(package: &str) -> RowTarget {
        RowTarget {
            group: GroupId {
                profile: ProfileId(0),
                package: package.into(),
            },
            notification: None,
        }
    }

    /// A finger held still on `target`'s row until the host's long-press time has passed
    /// (EVT-08).
    fn hold_row(shell: &mut HomeShell, target: RowTarget) {
        finger(shell, PointerKind::Down, 300.0, 900.0);
        shell.dispatch(ShellIntent::LongPressNotification(target));
    }

    fn events_menu_of(shell: &HomeShell) -> Option<RowTarget> {
        let menu = shell.events().state().menu.clone()?;
        Some(menu.target)
    }

    fn expand(shell: &mut HomeShell, package: &str) {
        shell.dispatch(ShellIntent::Events(EventsIntent::Tap {
            row: row_of(package),
            from: None,
        }));
    }

    #[test]
    fn a_long_press_on_a_notification_opens_its_menu_and_ticks() {
        // EVT-08, ANIM-24
        let fakes = Fakes::default();
        let mut shell = on_events(&fakes);

        hold_row(&mut shell, row_of("mail"));

        assert_eq!(events_menu_of(&shell), Some(row_of("mail")));
        let state = shell.state();
        assert!(state.events_menu_open);
        assert!(!state.menu_offered());
        assert_eq!(ticks(&fakes), 1);
        // The rest of the gesture is the shell's, so lifting the finger opens nothing.
        assert!(state.carousel.held);
        lift(&mut shell);
        assert!(shell.state().events_menu_open);
        assert!(fakes.notifications.opened.lock().unwrap().is_empty());
    }

    #[test]
    fn a_long_press_on_a_notification_counts_only_on_events_while_nothing_covers_it() {
        // EVT-08: no menu, no tick, and the gesture stays the content's.
        let not_taken = |shell: &HomeShell, fakes: &Fakes| {
            assert_eq!(events_menu_of(shell), None);
            assert!(!shell.state().carousel.held);
            assert_eq!(ticks(fakes), 0);
        };
        let fakes = Fakes::default();
        let mut shell = on_events(&fakes);

        // The finger moved past the touch slop: a scroll that stopped.
        finger(&mut shell, PointerKind::Down, 300.0, 900.0);
        finger(&mut shell, PointerKind::Move, 300.0, 980.0);
        shell.dispatch(ShellIntent::LongPressNotification(row_of("mail")));
        not_taken(&shell, &fakes);
        lift(&mut shell);

        // Under Settings or the home menu.
        shell.dispatch(ShellIntent::OpenSettings);
        hold_row(&mut shell, row_of("mail"));
        not_taken(&shell, &fakes);
        lift(&mut shell);
        shell.dispatch(ShellIntent::Back);
        shell.dispatch(ShellIntent::OpenMenu);
        hold_row(&mut shell, row_of("mail"));
        not_taken(&shell, &fakes);
        lift(&mut shell);
        shell.dispatch(ShellIntent::CloseMenu);

        // A group that no longer shows has no menu.
        hold_row(&mut shell, row_of("news"));
        not_taken(&shell, &fakes);
        lift(&mut shell);

        // Only on Events.
        go(&mut shell, Direction::Next);
        hold_row(&mut shell, row_of("mail"));
        not_taken(&shell, &fakes);
        shell.dispatch(ShellIntent::OpenNotificationMenu(row_of("mail")));
        assert_eq!(events_menu_of(&shell), None);
    }

    #[test]
    fn a_screen_reader_opens_a_notifications_menu_without_a_tick() {
        // A11Y-05
        let fakes = Fakes::default();
        let mut shell = on_events(&fakes);

        shell.dispatch(ShellIntent::OpenNotificationMenu(row_of("chat")));

        assert_eq!(events_menu_of(&shell), Some(row_of("chat")));
        assert!(shell.state().events_menu_open);
        assert_eq!(ticks(&fakes), 0);
    }

    #[test]
    fn the_notification_menu_covers_the_views_and_closes_like_the_others() {
        // EVT-08, HOME-07, HOME-08
        let fakes = Fakes::default();
        let mut shell = on_events(&fakes);
        shell.dispatch(ShellIntent::OpenNotificationMenu(row_of("mail")));

        // It covers the home views, as the home menu does.
        hold(&mut shell, 300.0, 900.0);
        assert_eq!(shell.state().menu, None);
        lift(&mut shell);

        shell.dispatch(ShellIntent::CloseMenu);
        assert!(!shell.state().events_menu_open);

        // Back closes the menu, then collapses the group.
        expand(&mut shell, "mail");
        shell.dispatch(ShellIntent::OpenNotificationMenu(row_of("mail")));
        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().events_menu_open);
        assert!(shell.events().state().expanded.is_some());
        shell.dispatch(ShellIntent::Back);
        assert_eq!(shell.events().state().expanded, None);

        // Home closes both, and Events stays.
        expand(&mut shell, "mail");
        shell.dispatch(ShellIntent::OpenNotificationMenu(row_of("mail")));
        shell.dispatch(ShellIntent::Home);
        assert!(!shell.state().events_menu_open);
        assert_eq!(shell.events().state().expanded, None);
        assert_eq!(shell.state().current, HomeView::Events);

        // Settings closes it too.
        shell.dispatch(ShellIntent::OpenNotificationMenu(row_of("mail")));
        shell.dispatch(ShellIntent::OpenSettings);
        assert!(!shell.state().events_menu_open);
    }

    #[test]
    fn leaving_events_collapses_its_group_and_closes_its_menu() {
        // EVT-06: as an open folder closes on leaving Applications.
        let fakes = Fakes::default();
        let mut shell = on_events(&fakes);
        expand(&mut shell, "mail");
        assert!(shell.events().state().expanded.is_some());

        go(&mut shell, Direction::Next);

        assert_eq!(shell.events().state().expanded, None);
    }

    #[test]
    fn expanding_and_collapsing_a_group_moves_the_events_list() {
        // EVT-06, ANIM-20
        let fakes = Fakes::default();
        let mut shell = on_events(&fakes);
        come_to_rest(&mut shell, 20_000);
        assert!(!shell.state().events_change.animating);

        expand(&mut shell, "mail");
        assert!(shell.state().events_change.animating);
        come_to_rest(&mut shell, 30_000);

        expand(&mut shell, "mail");
        assert_eq!(shell.events().state().expanded, None);
        assert!(shell.state().events_change.animating);
    }

    /// Works like [`FakeNotifications`], except that dismissing panics.
    struct BrokenDismiss(FakeNotifications);

    impl NotificationsAdapter for BrokenDismiss {
        fn access(&self) -> Access {
            self.0.access()
        }

        fn active(&self) -> Result<Vec<Notification>, DataError> {
            self.0.active()
        }

        fn open(&self, key: &str, from: Option<Rect>) -> Result<(), DataError> {
            self.0.open(key, from)
        }

        fn dismiss(&self, _keys: &[String]) -> Result<(), DataError> {
            panic!("dismiss bug");
        }

        fn open_access_settings(&self) -> Result<(), DataError> {
            self.0.open_access_settings()
        }

        fn open_app_settings(&self, profile: ProfileId, package: &str) -> Result<(), DataError> {
            self.0.open_app_settings(profile, package)
        }
    }

    #[test]
    fn a_failing_events_never_leaves_the_home_covered() {
        // ARCH-09: a menu Events can no longer close doesn't count.
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        let broken = FakeNotifications::default();
        *broken.access.lock().unwrap() = Access::Granted;
        *broken.active.lock().unwrap() = vec![notification("c1", "chat")];
        platform.notifications = Arc::new(BrokenDismiss(broken));
        let mut shell = HomeShell::compose(platform);
        shell.dispatch(ShellIntent::Swipe(Direction::Previous));
        shell.dispatch(ShellIntent::OpenNotificationMenu(row_of("chat")));
        assert!(shell.state().events_menu_open);

        shell.dispatch(ShellIntent::Events(EventsIntent::ChooseMenuItem(
            NotificationMenuItem::Dismiss,
        )));

        assert!(matches!(shell.events().health(), Health::Failed(_)));
        assert!(!shell.state().events_menu_open);
        assert!(shell.state().menu_offered());
        shell.dispatch(ShellIntent::OpenMenu);
        assert!(shell.state().menu.is_some());
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

    /// A grid 800 px wide at 2 px per dp: four columns of 200 px, rows 200 px apart, 100 px of
    /// safe area above and below, 1600 px tall.
    const GRID: GridLayout = GridLayout {
        width_px: 800.0,
        row_height_px: 200.0,
        density: 2.0,
        top_px: 100.0,
        bottom_px: 100.0,
        height_px: 1600.0,
        left_px: 0.0,
        view_top_px: 0.0,
        icon_px: 0.0,
        icon_top_px: 0.0,
    };

    /// `fakes`' apps in [`GRID`].
    fn in_a_grid(fakes: &Fakes) -> HomeShell {
        let mut shell = laid_out(fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::Layout(GRID)));
        fakes.storage.writes.lock().unwrap().clear();
        shell
    }

    /// Camera, Mail, Maps and Phone in [`GRID`].
    fn with_four(fakes: &Fakes) -> HomeShell {
        *fakes.apps.apps.lock().unwrap() = vec![
            app("camera", "Camera"),
            app("mail", "Mail"),
            app("maps", "Maps"),
            app("phone", "Phone"),
        ];
        in_a_grid(fakes)
    }

    fn grid_labels(shell: &HomeShell) -> Vec<String> {
        shell
            .apps()
            .state()
            .items
            .iter()
            .map(|item| item.label().to_owned())
            .collect()
    }

    fn pointer_at(shell: &mut HomeShell, kind: PointerKind, x: f64, y: f64, millis: u64) {
        shell.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x,
            y,
            at: ms(millis),
        }));
    }

    /// Maps, at place 2, picked up and held over the first place long enough to take it.
    fn maps_moved_first(shell: &mut HomeShell) {
        hold_app(shell, "maps", 500.0, 300.0);
        pointer_at(shell, PointerKind::Move, 100.0, 300.0, 10_050);
        shell.dispatch(ShellIntent::Frame(ms(10_060)));
        assert_eq!(grid_labels(shell), ["Camera", "Mail", "Maps", "Phone"]);
        shell.dispatch(ShellIntent::Frame(ms(10_200)));
        assert_eq!(grid_labels(shell), ["Maps", "Camera", "Mail", "Phone"]);
    }

    fn saved_first(fakes: &Fakes) -> Option<LayoutEntry> {
        match PersistentLayoutStore::new(fakes.storage.clone()).load() {
            Ok(meegone_apps_data::SavedLayout::Read(saved)) => saved.entries.first().cloned(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_dragged_icon_moves_the_others_aside_and_a_drop_keeps_the_new_order() {
        // APPS-12, APPS-18, ANIM-13, AC-07
        let fakes = Fakes::default();
        let mut shell = with_four(&fakes);

        maps_moved_first(&mut shell);

        // The others slide to their new places, and the icon is drawn from its own new one,
        // still under the finger.
        let state = shell.state();
        assert!(state.grid_change.animating);
        // Picked up 100 px below its cell's middle, at (500, 200).
        assert_eq!((state.lift.x, state.lift.y), (100.0, 200.0));
        assert!(fakes.storage.writes.lock().unwrap().is_empty());

        pointer_at(&mut shell, PointerKind::Up, 110.0, 290.0, 10_300);

        assert!(shell.state().lift.settling);
        assert_eq!((shell.state().lift.x, shell.state().lift.y), (110.0, 190.0));
        assert_eq!(saved_first(&fakes), Some(LayoutEntry::App(app_id("maps"))));
        come_to_rest(&mut shell, 10_300);
        assert_eq!(shell.state().lift, LiftState::default());
        assert_eq!(grid_labels(&shell), ["Maps", "Camera", "Mail", "Phone"]);
        assert!(shell.apps().state().edit_mode);
        assert!(fakes.apps.launched.lock().unwrap().is_empty());
    }

    #[test]
    fn a_drop_before_the_wait_is_over_still_takes_the_place_under_it() {
        // APPS-12
        let fakes = Fakes::default();
        let mut shell = with_four(&fakes);
        hold_app(&mut shell, "maps", 500.0, 300.0);
        pointer_at(&mut shell, PointerKind::Move, 100.0, 300.0, 10_050);

        pointer_at(&mut shell, PointerKind::Up, 100.0, 300.0, 10_060);

        assert_eq!(grid_labels(&shell), ["Maps", "Camera", "Mail", "Phone"]);
        assert_eq!(saved_first(&fakes), Some(LayoutEntry::App(app_id("maps"))));
    }

    #[test]
    fn a_drag_that_ends_any_way_but_a_drop_puts_the_grid_back() {
        // APPS-12: Back, a new layout, Settings or another view.
        let ends: [fn(&mut HomeShell); 4] = [
            |shell| shell.dispatch(ShellIntent::Back),
            |shell| {
                let layout = shell.state().carousel.layout;
                shell.dispatch(ShellIntent::Layout(Layout {
                    width_px: 2000.0,
                    ..layout
                }));
            },
            |shell| shell.dispatch(ShellIntent::OpenSettings),
            |shell| shell.dispatch(ShellIntent::Swipe(Direction::Next)),
        ];
        for end in ends {
            let fakes = Fakes::default();
            let mut shell = with_four(&fakes);
            maps_moved_first(&mut shell);

            end(&mut shell);

            assert_eq!(grid_labels(&shell), ["Camera", "Mail", "Maps", "Phone"]);
            assert_eq!(shell.state().lift, LiftState::default());
            assert!(fakes.storage.writes.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn near_the_bottom_a_dragged_icon_scrolls_the_grid() {
        // APPS-12: 40 apps in 11 rows of 200 px; 1400 px of grid show.
        let fakes = Fakes::default();
        *fakes.apps.apps.lock().unwrap() = (0..40)
            .map(|i| app(&format!("app{i:02}"), &format!("App {i:02}")))
            .collect();
        let mut shell = in_a_grid(&fakes);
        hold_app(&mut shell, "app01", 300.0, 300.0);
        // Its centre 1300 px down the visible grid: 128 px into the bottom zone of 128 px.
        pointer_at(&mut shell, PointerKind::Move, 300.0, 1500.0, 10_050);
        shell.dispatch(ShellIntent::Frame(ms(10_060)));
        shell.dispatch(ShellIntent::Frame(ms(10_076)));
        let scrolled = shell.apps().state().scroll_px;
        assert!(scrolled > 0.0, "{scrolled}");
        assert!(shell.state().lift.animating);
        // The icon stays under the finger.
        assert_eq!(shell.state().lift.y, 1400.0);
    }

    #[test]
    fn entering_edit_mode_ticks_once() {
        // ANIM-24: from a menu it ticks; picking an icon up ticks only for its long-press.
        let fakes = Fakes::default();
        let mut shell = with_mail(&fakes);
        shell.dispatch(ShellIntent::OpenMenu);
        shell.dispatch(ShellIntent::ChooseMenuItem(MenuItem::EditApplications));
        assert!(shell.state().mode.on);
        assert_eq!(ticks(&fakes), 1);
        shell.dispatch(ShellIntent::Back);
        assert!(!shell.state().mode.on);
        assert_eq!(ticks(&fakes), 1);

        hold_app(&mut shell, "mail", 300.0, 400.0);
        assert_eq!(ticks(&fakes), 2);
        finger(&mut shell, PointerKind::Move, 300.0, 600.0);
        assert!(shell.state().mode.on);
        assert_eq!(ticks(&fakes), 2);
    }

    #[test]
    fn edit_mode_shrinks_the_icons_and_pops_the_badges_in_then_back_out() {
        // ANIM-12
        let fakes = Fakes::default();
        let mut shell = with_four(&fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        assert!(shell.state().mode.animating);
        come_to_rest(&mut shell, 0);
        let mode = shell.state().mode;
        assert_eq!(mode.scale, meegone_core_state::motion::MODE_SCALE);
        assert_eq!(mode.badges, [1.0; BADGE_ROWS]);

        shell.dispatch(ShellIntent::Apps(AppsIntent::ExitEditMode));
        assert!(shell.state().mode.animating);
        come_to_rest(&mut shell, 2_000);
        assert_eq!(shell.state().mode, ModeState::default());
    }

    #[test]
    fn leaving_applications_ends_edit_mode() {
        // So Done is never left out of sight, and Home from Events shows a plain grid.
        let mut shell = with_mail(&Fakes::default());
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));

        go(&mut shell, Direction::Previous);
        assert!(!shell.apps().state().edit_mode);
        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.state().current, HomeView::Applications);
        assert!(!shell.apps().state().edit_mode);
    }

    #[test]
    fn in_edit_mode_an_icon_doesnt_shrink_under_the_finger() {
        // APPS-12: a tap launches nothing, so nothing says it would.
        let mut shell = with_mail(&Fakes::default());
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));

        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 0,
            at: ms(0),
        });

        assert_eq!(shell.state().press, PressState::default());
    }

    #[test]
    fn in_edit_mode_the_home_menu_has_no_edit_applications() {
        let mut shell = with_mail(&Fakes::default());
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));

        shell.dispatch(ShellIntent::OpenMenu);

        assert_eq!(
            shell.state().menu.map(|menu| menu.items),
            Some(&[MenuItem::LauncherSettings, MenuItem::LockScreenImage][..])
        );
    }

    #[test]
    fn moving_an_icon_moves_the_grid_but_a_refresh_in_the_same_order_doesnt() {
        // ANIM-13, ANIM-21: one rule for every change of the cells.
        let mut shell = with_four(&Fakes::default());
        shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode));
        come_to_rest(&mut shell, 0);

        shell.dispatch(ShellIntent::Apps(AppsIntent::Move {
            item: CellKey::App(app_id("phone")),
            to: 0,
        }));
        assert!(shell.state().grid_change.animating);
        come_to_rest(&mut shell, 2_000);

        shell.dispatch(ShellIntent::AppsChanged);
        assert!(!shell.state().grid_change.animating);
    }

    #[test]
    fn a_failing_applications_ends_edit_mode_and_the_drag() {
        // ARCH-09: nothing Applications can no longer end is left on screen.
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        let broken = FakeApps::default();
        broken.apps.lock().unwrap().push(app("mail", "Mail"));
        platform.apps = Arc::new(BrokenLauncher(broken));
        let mut shell = HomeShell::compose(platform);
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 1000.0,
            density: 2.0,
            touch_slop_px: 16.0,
            rtl: false,
        }));
        hold_app(&mut shell, "mail", 300.0, 400.0);
        finger(&mut shell, PointerKind::Move, 300.0, 600.0);
        assert!(shell.state().lift.key.is_some() && shell.state().mode.on);

        shell.dispatch(ShellIntent::Apps(AppsIntent::RequestUninstall(app_id(
            "mail",
        ))));

        assert!(matches!(shell.apps().health(), Health::Failed(_)));
        let state = shell.state();
        assert_eq!(state.lift, LiftState::default());
        assert!(!state.mode.on);
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 0,
            at: ms(0),
        });
        assert!(shell.state().press.item.is_some());
    }

    /// [`GRID`] with each icon 112 px from 16 px down its cell, so a dragged app's centre within
    /// 44.8 px of an icon's centre is over it.
    const GRID_ICONS: GridLayout = GridLayout {
        icon_px: 112.0,
        icon_top_px: 16.0,
        ..GRID
    };

    /// Where cell `place`'s icon centre is on screen in [`GRID_ICONS`], unscrolled.
    fn icon_at(place: usize) -> (f64, f64) {
        let (column, row) = ((place % 4) as f64, (place / 4) as f64);
        (column * 200.0 + 100.0, 100.0 + row * 200.0 + 72.0)
    }

    #[test]
    fn holding_an_app_over_another_and_dropping_it_makes_a_folder_and_ticks() {
        // FOLD-01, FOLD-03, ANIM-14, ANIM-24, AC-08
        let fakes = Fakes::default();
        let mut shell = with_four(&fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::Layout(GRID_ICONS)));
        let (mx, my) = icon_at(2);
        hold_app(&mut shell, "maps", mx, my);
        assert_eq!(ticks(&fakes), 1);
        let (x, y) = icon_at(1);
        pointer_at(&mut shell, PointerKind::Move, x + 10.0, y, 10_050);
        shell.dispatch(ShellIntent::Frame(ms(10_060)));
        let hover = &shell.state().hover;
        assert_eq!(hover.key, Some(CellKey::App(app_id("mail"))));
        assert!(!hover.armed);
        // It stays a hover: Mail never moves aside.
        shell.dispatch(ShellIntent::Frame(ms(10_300)));
        assert_eq!(grid_labels(&shell), ["Camera", "Mail", "Maps", "Phone"]);
        shell.dispatch(ShellIntent::Frame(ms(10_460)));
        assert!(shell.state().hover.armed);

        pointer_at(&mut shell, PointerKind::Up, x + 10.0, y, 10_470);

        // A folder named "Folder" holds both, Mail first, where Mail was.
        assert_eq!(grid_labels(&shell), ["Camera", "", "Phone"]);
        let GridItem::Folder { id, apps, .. } = &shell.apps().state().items[1] else {
            panic!("not a folder");
        };
        let held: Vec<&str> = apps.iter().map(|app| app.label.as_str()).collect();
        assert_eq!(held, ["Mail", "Maps"]);
        assert_eq!(ticks(&fakes), 2);
        let state = shell.state();
        assert!(state.lift.merging);
        assert_eq!(
            state.merge,
            MergeState {
                folder: Some(*id),
                slot: Some(1),
                formed: true,
                progress: 0.0
            }
        );
        assert_eq!(state.hover, HoverState::default());
        come_to_rest(&mut shell, 10_470);
        assert_eq!(shell.state().lift, LiftState::default());
        assert_eq!(
            saved_first(&fakes),
            Some(LayoutEntry::App(app_id("camera")))
        );
    }

    #[test]
    fn an_open_folder_keeps_the_views_still_and_takes_its_own_long_presses() {
        // FOLD-04, H1: a sideways drag on the panel never swipes the views.
        let (fakes, mut shell) = with_a_folder();
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 1000.0,
            density: 2.0,
            touch_slop_px: 16.0,
            rtl: false,
        }));
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenFolder(FolderId(1))));
        assert!(shell.state().folder_open);
        assert!(!shell.state().menu_offered());
        assert_eq!(shell.state().folder.folder, Some(FolderId(1)));

        pointer_at(&mut shell, PointerKind::Down, 300.0, 400.0, 0);
        pointer_at(&mut shell, PointerKind::Move, 700.0, 400.0, 50);
        assert!(!shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, 0.0);
        pointer_at(&mut shell, PointerKind::Up, 700.0, 400.0, 100);

        // A long-press in the panel waits for it to finish opening; one on the grid behind it
        // never counts.
        let maps = CellKey::App(app_id("maps"));
        let long_press = |shell: &mut HomeShell, surface: Surface| {
            pointer_at(shell, PointerKind::Down, 300.0, 400.0, 1_000);
            shell.dispatch(ShellIntent::LongPressCell {
                surface,
                key: maps.clone(),
            });
        };
        long_press(&mut shell, Surface::Folder);
        assert!(!shell.state().app_menu_open);
        pointer_at(&mut shell, PointerKind::Up, 300.0, 400.0, 1_100);
        come_to_rest(&mut shell, 2_000);
        long_press(&mut shell, Surface::Grid);
        assert!(!shell.state().app_menu_open);
        pointer_at(&mut shell, PointerKind::Up, 300.0, 400.0, 1_100);
        long_press(&mut shell, Surface::Folder);
        assert!(shell.state().app_menu_open);
        assert_eq!(ticks(&fakes), 1);

        // Presses count only in the panel.
        shell.dispatch(ShellIntent::CloseMenu);
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 0,
            at: ms(3_000),
        });
        assert_eq!(shell.state().press.item, None);
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Folder,
            item: 1,
            at: ms(3_000),
        });
        assert_eq!(
            (shell.state().press.item, shell.state().press.surface),
            (Some(1), Surface::Folder)
        );
    }

    #[test]
    fn a_folder_closes_with_back_another_view_or_settings_and_shrinks_away() {
        // FOLD-04, ANIM-15
        let ends: [fn(&mut HomeShell); 3] = [
            |shell| shell.dispatch(ShellIntent::Back),
            |shell| shell.dispatch(ShellIntent::Swipe(Direction::Next)),
            |shell| shell.dispatch(ShellIntent::OpenSettings),
        ];
        for end in ends {
            let (_, mut shell) = with_a_folder();
            shell.dispatch(ShellIntent::Apps(AppsIntent::OpenFolder(FolderId(1))));
            come_to_rest(&mut shell, 0);
            assert_eq!(shell.state().folder.progress, 1.0);

            end(&mut shell);

            let state = shell.state();
            assert!(!state.folder_open);
            assert_eq!(state.folder.folder, Some(FolderId(1)));
            assert!(!state.folder.open && state.folder.animating);
            come_to_rest(&mut shell, 2_000);
            assert_eq!(shell.state().folder, FolderState::default());
        }
    }

    #[test]
    fn a_failing_applications_never_leaves_a_folder_open() {
        // ARCH-09
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        let broken = FakeApps::default();
        *broken.apps.lock().unwrap() = vec![app("maps", "Maps"), app("mail", "Mail")];
        platform.apps = Arc::new(BrokenLauncher(broken));
        PersistentLayoutStore::new(fakes.storage.clone())
            .save(&SavedLayout {
                entries: vec![LayoutEntry::Folder {
                    id: FolderId(1),
                    name: String::new(),
                    apps: vec![app_id("maps"), app_id("mail")],
                }],
                ..SavedLayout::default()
            })
            .unwrap();
        let mut shell = HomeShell::compose(platform);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenFolder(FolderId(1))));
        assert!(shell.state().folder_open);

        shell.dispatch(ShellIntent::Apps(AppsIntent::Launch {
            app: app_id("mail"),
            from: None,
        }));

        assert!(matches!(shell.apps().health(), Health::Failed(_)));
        assert!(!shell.state().folder_open);
        assert!(shell.state().menu_offered());
    }

    /// The search field's band: 160 px tall, below 100 px of safe area.
    const BAND: Bounds = Bounds {
        left: 0.0,
        top: 100.0,
        right: 1000.0,
        bottom: 260.0,
    };

    /// Camera, Mail, Maps and Phone in [`GRID`], with the search field laid out above them.
    fn ready_to_search(fakes: &Fakes) -> HomeShell {
        let mut shell = with_four(fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::SearchLayout {
            layout: GRID,
            band: BAND,
        }));
        shell
    }

    /// A slow drag from `from` to `to` px down the views from `start` ms, without lifting.
    /// Returns when it ended.
    fn drag_down(shell: &mut HomeShell, from: f64, to: f64, start: u64) -> u64 {
        pointer_at(shell, PointerKind::Down, 500.0, from, start);
        let mut t = start;
        for step in 1..=20 {
            t += 16;
            let y = from + (to - from) * f64::from(step) / 20.0;
            pointer_at(shell, PointerKind::Move, 500.0, y, t);
        }
        t
    }

    /// [`drag_down`], then a pause, so the finger carries no speed, and lifting it.
    fn pull_field(shell: &mut HomeShell, from: f64, to: f64, start: u64) -> u64 {
        let t = drag_down(shell, from, to, start);
        pointer_at(shell, PointerKind::Move, 500.0, to, t + 50);
        pointer_at(shell, PointerKind::Up, 500.0, to, t + 100);
        t + 100
    }

    /// Draws frames every 8 ms from `from_ms` until the search field rests.
    fn field_rests(shell: &mut HomeShell, from_ms: u64) -> u64 {
        let mut t = from_ms;
        while shell.state().search.animating {
            t += 8;
            shell.dispatch(ShellIntent::Frame(ms(t)));
            assert!(t < from_ms + 5_000, "the field never rested");
        }
        t
    }

    fn query(shell: &HomeShell) -> Option<String> {
        shell.apps().state().search.clone()
    }

    #[test]
    fn pulling_down_at_the_top_of_applications_opens_search() {
        // SRCH-01, ANIM-16: the field shows in step with the pull, and the gesture is the
        // shell's, so the grid below gets none of it.
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        assert_eq!(shell.state().search, SearchState::default());

        let t = drag_down(&mut shell, 400.0, 400.0 + 16.0 + 100.0, 0);
        assert!(shell.state().search.pulling);
        assert!((shell.state().search.reveal_px - 100.0).abs() < 1e-9);
        assert_eq!(query(&shell), None);
        pointer_at(
            &mut shell,
            PointerKind::Move,
            500.0,
            400.0 + 16.0 + 200.0,
            t + 16,
        );
        pointer_at(
            &mut shell,
            PointerKind::Move,
            500.0,
            400.0 + 16.0 + 200.0,
            t + 66,
        );
        pointer_at(
            &mut shell,
            PointerKind::Up,
            500.0,
            400.0 + 16.0 + 200.0,
            t + 116,
        );

        let state = shell.state();
        assert!(state.search.open && !state.search.pulling && state.search.animating);
        assert_eq!(query(&shell).as_deref(), Some(""));
        field_rests(&mut shell, t + 116);
        assert_eq!(shell.state().search.reveal_px, 160.0);
        // Neither the views nor a long-press moved.
        assert_eq!(shell.state().carousel.offset, 0.0);
        assert!(shell.state().menu.is_none());
    }

    #[test]
    fn a_pull_short_of_the_whole_field_springs_back_and_opens_nothing() {
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);

        let t = pull_field(&mut shell, 400.0, 400.0 + 16.0 + 120.0, 0);

        assert!(!shell.state().search.open);
        field_rests(&mut shell, t);
        assert_eq!(shell.state().search.reveal_px, 0.0);
        assert_eq!(query(&shell), None);
    }

    #[test]
    fn a_pull_counts_only_at_the_top_of_applications_with_nothing_open() {
        // SRCH-01
        let cases: [fn(&mut HomeShell); 5] = [
            |shell| shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(300.0))),
            |shell| shell.dispatch(ShellIntent::Apps(AppsIntent::EnterEditMode)),
            |shell| shell.dispatch(ShellIntent::OpenMenu),
            |shell| shell.dispatch(ShellIntent::OpenSettings),
            |shell| go(shell, Direction::Next),
        ];
        for case in cases {
            let fakes = Fakes::default();
            let mut shell = ready_to_search(&fakes);
            case(&mut shell);

            drag_down(&mut shell, 400.0, 800.0, 0);

            assert!(!shell.state().search.pulling);
            assert_eq!(shell.state().search.reveal_px, 0.0);
        }

        // Nor before the host has laid the field out.
        let fakes = Fakes::default();
        let mut shell = with_four(&fakes);
        drag_down(&mut shell, 400.0, 800.0, 0);
        assert!(!shell.state().search.pulling);
    }

    #[test]
    fn a_pull_lets_the_pressed_icon_go_and_no_long_press_follows() {
        // ANIM-10, HOME-10
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        pointer_at(&mut shell, PointerKind::Down, 500.0, 400.0, 0);
        shell.dispatch(ShellIntent::Press {
            surface: Surface::Grid,
            item: 2,
            at: ms(0),
        });
        assert_eq!(shell.state().press.item, Some(2));

        pointer_at(&mut shell, PointerKind::Move, 500.0, 450.0, 16);
        shell.dispatch(ShellIntent::LongPress);

        assert!(shell.state().search.pulling);
        assert!(shell.state().press.animating);
        assert!(shell.state().menu.is_none());
    }

    #[test]
    fn open_search_keeps_the_views_still_and_offers_no_menus() {
        // SRCH-01: a sideways drag over the field or the results never swipes the views.
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        assert!(!shell.state().menu_offered());

        pointer_at(&mut shell, PointerKind::Down, 300.0, 600.0, 0);
        pointer_at(&mut shell, PointerKind::Move, 700.0, 600.0, 50);
        assert!(!shell.state().carousel.dragging);
        assert_eq!(shell.state().carousel.offset, 0.0);
        pointer_at(&mut shell, PointerKind::Up, 700.0, 600.0, 100);

        pointer_at(&mut shell, PointerKind::Down, 300.0, 600.0, 1_000);
        shell.dispatch(ShellIntent::LongPress);
        assert!(shell.state().menu.is_none());
        shell.dispatch(ShellIntent::LongPressCell {
            surface: Surface::Grid,
            key: CellKey::App(app_id("mail")),
        });
        assert!(!shell.state().app_menu_open);
        shell.dispatch(ShellIntent::OpenMenu);
        assert!(shell.state().menu.is_none());
    }

    #[test]
    fn while_search_is_open_its_results_take_presses_or_the_grid_below_an_empty_field() {
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        let press = |shell: &mut HomeShell, surface: Surface| {
            shell.dispatch(ShellIntent::Release(ms(0)));
            come_to_rest(shell, 0);
            shell.dispatch(ShellIntent::Press {
                surface,
                item: 0,
                at: ms(2_000),
            });
            shell.state().press.item.is_some()
        };

        assert!(press(&mut shell, Surface::Grid));
        shell.dispatch(ShellIntent::Apps(AppsIntent::Search("ma".into())));
        assert!(!press(&mut shell, Surface::Grid));
        assert!(press(&mut shell, Surface::Search));
        assert!(!press(&mut shell, Surface::Folder));
    }

    #[test]
    fn back_and_home_close_search_and_the_field_springs_away() {
        // SRCH-04, HOME-07, HOME-08
        let ends: [fn(&mut HomeShell); 4] = [
            |shell| shell.dispatch(ShellIntent::Back),
            |shell| shell.dispatch(ShellIntent::Home),
            |shell| shell.dispatch(ShellIntent::Swipe(Direction::Next)),
            |shell| shell.dispatch(ShellIntent::OpenSettings),
        ];
        for end in ends {
            let fakes = Fakes::default();
            let mut shell = ready_to_search(&fakes);
            shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
            shell.dispatch(ShellIntent::Apps(AppsIntent::Search("ma".into())));
            let t = field_rests(&mut shell, 0);
            assert_eq!(shell.state().search.reveal_px, 160.0);

            end(&mut shell);

            assert_eq!(query(&shell), None);
            assert!(!shell.state().search.open && shell.state().search.animating);
            field_rests(&mut shell, t);
            assert_eq!(shell.state().search.reveal_px, 0.0);
        }
    }

    #[test]
    fn home_closes_search_first_then_scrolls_to_the_top() {
        // HOME-07, SRCH-04: a screen reader can open search anywhere in the grid, and closing it
        // leaves the grid where it was.
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::Scrolled(600.0)));
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        assert!(shell.state().search.open);

        shell.dispatch(ShellIntent::Home);
        assert_eq!(query(&shell), None);
        assert_eq!(shell.apps().state().scroll_px, 600.0);
        assert_eq!(shell.state().current, HomeView::Applications);

        shell.dispatch(ShellIntent::Home);
        assert_eq!(shell.apps().state().scroll_px, 0.0);
    }

    #[test]
    fn pushing_the_field_away_with_nothing_typed_closes_search() {
        // SRCH-04: "scrolling down past the field".
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        let t = pull_field(&mut shell, 400.0, 400.0 + 16.0 + 200.0, 0);
        let t = field_rests(&mut shell, t);
        assert_eq!(query(&shell).as_deref(), Some(""));

        let t = pull_field(&mut shell, 900.0, 900.0 - 16.0 - 200.0, t + 100);

        assert_eq!(query(&shell), None);
        field_rests(&mut shell, t);
        assert_eq!(shell.state().search.reveal_px, 0.0);
    }

    #[test]
    fn with_a_query_typed_the_results_scroll_and_the_field_stays() {
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        shell.dispatch(ShellIntent::Apps(AppsIntent::Search("ma".into())));
        let t = field_rests(&mut shell, 0);

        drag_down(&mut shell, 900.0, 500.0, t);

        assert!(!shell.state().search.pulling);
        assert_eq!(shell.state().search.reveal_px, 160.0);
        assert_eq!(query(&shell).as_deref(), Some("ma"));
    }

    #[test]
    fn launching_from_search_springs_the_field_away() {
        // SRCH-03
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        shell.dispatch(ShellIntent::Apps(AppsIntent::Search("ma".into())));

        shell.dispatch(ShellIntent::Apps(AppsIntent::SubmitSearch { from: None }));

        assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
        assert!(!shell.state().search.open);
    }

    #[test]
    fn a_new_layout_keeps_search_open_and_ends_a_pull() {
        // COMP-03
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        shell.dispatch(ShellIntent::Apps(AppsIntent::Search("ma".into())));
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 1600.0,
            density: 2.0,
            touch_slop_px: 16.0,
            rtl: false,
        }));
        assert_eq!(query(&shell).as_deref(), Some("ma"));

        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        drag_down(&mut shell, 400.0, 600.0, 0);
        assert!(shell.state().search.pulling);
        shell.dispatch(ShellIntent::Layout(Layout {
            width_px: 1600.0,
            density: 2.0,
            touch_slop_px: 16.0,
            rtl: false,
        }));
        assert!(!shell.state().search.pulling);
        assert!(!shell.state().search.open);
    }

    #[test]
    fn a_failing_applications_never_leaves_search_open() {
        // ARCH-09
        let fakes = Fakes::default();
        let mut platform = fakes.platform();
        let broken = FakeApps::default();
        *broken.apps.lock().unwrap() = vec![app("maps", "Maps"), app("mail", "Mail")];
        platform.apps = Arc::new(BrokenLauncher(broken));
        let mut shell = HomeShell::compose(platform);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        shell.dispatch(ShellIntent::Apps(AppsIntent::Search("ma".into())));
        assert!(shell.state().search.open);
        assert!(!shell.state().menu_offered());

        shell.dispatch(ShellIntent::Apps(AppsIntent::SubmitSearch { from: None }));

        assert!(matches!(shell.apps().health(), Health::Failed(_)));
        assert!(!shell.state().search.open);
        assert!(shell.state().menu_offered());
    }

    #[test]
    fn a_screen_reader_opens_search_only_where_it_is_offered() {
        // A11Y-05
        let fakes = Fakes::default();
        let mut shell = ready_to_search(&fakes);
        assert!(shell.state().search_offered());

        go(&mut shell, Direction::Next);
        assert!(!shell.state().search_offered());
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        assert_eq!(query(&shell), None);

        go(&mut shell, Direction::Previous);
        shell.dispatch(ShellIntent::OpenSettings);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        assert_eq!(query(&shell), None);

        shell.dispatch(ShellIntent::Back);
        shell.dispatch(ShellIntent::Apps(AppsIntent::OpenSearch));
        assert_eq!(query(&shell).as_deref(), Some(""));
        assert!(!shell.state().search_offered());
    }
}
