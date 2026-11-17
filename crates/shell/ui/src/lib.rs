//! Home shell UI (SRS 4.1). It puts the feature views together, connects each to its state
//! holder through the shell and places content one feature shows inside another (ADR 0002).
//! Only the shell depends on other features (ARCH-03).
//!
//! It also tells the host what to draw for the home views in each frame, as a [`HomeFrame`]:
//! where the carousel's views are, how the view indicator looks, how the content comes back
//! from an app, the home menu, the Applications grid, the pressed icon, the grid's cells as
//! apps come, go and move, a cell's long-press menu, the dragged icon, edit mode, the open
//! folder, search, Settings' Hidden apps list, and the Events header and list (ADR 0006 to
//! ADR 0008, ADR 0010, ADR 0012 to ADR 0017, ADR 0019, ADR 0020).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

pub use meegone_apps_ui::{
    AppMenuFrame, AppMenuItem, AppsFrame, Bounds, Cell, CellKey, FolderId, GridLayout, Surface,
    SurfaceCellsFrame,
};
use meegone_apps_ui::{AppsView, HiddenAppsSection};
use meegone_core_ui::{Dispatch, Latest, Rgb, Slot, View, tokens};
use meegone_events_ui::EventsView;
pub use meegone_events_ui::{EventsListMode, EventsRow};
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

/// What shows below the Events header (EVT-03, EVT-04, EVT-11, ADR 0020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventsListFrame {
    pub mode: EventsListMode,
    /// Changes whenever the mode or the rows do: the host then reads the rows again with
    /// [`EventsView::rows`].
    pub revision: u32,
}

/// The dragged icon, as the host draws it above everything (APPS-11, APPS-12, ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiftFrame {
    /// Where its cell is: the grid, or the open folder.
    pub surface: Surface,
    /// Its cell's place there now, which the host leaves as a gap; `None` once it has gone
    /// into a folder.
    pub cell: Option<usize>,
    /// Where its icon's centre is on screen, in whole pixels from the home views' top left.
    pub x: i32,
    pub y: i32,
    /// Its scale in ten-thousandths of the icon's size: 11 000 while lifted (ANIM-13), springing
    /// to edit mode's 9 200 as it settles, or shrinking into a folder's preview (ANIM-14).
    pub scale: u16,
    /// Its opacity, from 0 to 255: it fades into a folder whose preview is full.
    pub alpha: u8,
    /// The finger has lifted, and it is settling into its cell (ANIM-13).
    pub settling: bool,
    /// The finger has lifted over a folder or an app, and it is going into the folder
    /// (ANIM-14).
    pub merging: bool,
}

/// The icon a dragged app is held over, growing and pulsing (ANIM-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverFrame {
    /// Its place in the grid's cells.
    pub cell: usize,
    /// Its icon's scale in ten-thousandths.
    pub scale: u16,
}

/// A folder taking in a dropped app (ANIM-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeFrame {
    /// The folder's place in the grid's cells.
    pub cell: usize,
    /// The preview slot the app goes into, left empty until it lands, if the preview has room.
    pub slot: Option<usize>,
    /// The folder was just made: its first app shrinks into slot 0 from full size as well.
    pub formed: bool,
    /// How far it has come, from 0 to 10 000.
    pub progress: u16,
}

/// The open folder's panel (FOLD-04, ANIM-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderFrame {
    /// The folder it shows, while it is open or closing.
    pub folder: FolderId,
    /// It is open, rather than closing: it takes touches.
    pub open: bool,
    /// The folder's place in the grid's cells, which it grows from and shrinks back into, or
    /// `None` if it has none.
    pub cell: Option<usize>,
    /// How far it has grown from the folder's icon, from 0 to 10 000.
    pub progress: u16,
}

/// The search field, and whether its results show (SRCH-01 to SRCH-05, ANIM-16). The results'
/// cells are in [`AppsFrame::results`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchFrame {
    /// Search is open, or springing open: the field takes the keyboard (SRCH-01). When it turns
    /// false the keyboard goes and the field empties.
    pub open: bool,
    /// How much of the field's band shows, in whole pixels: 0 hidden, the band's height open,
    /// and a little more while a pull stretches it (ANIM-16). The grid below an empty field
    /// moves down with it.
    pub reveal_px: i32,
    /// A finger is pulling the field, so the content below gets none of the gesture.
    pub pulling: bool,
    /// Something is typed: the results show instead of the grid (SRCH-02).
    pub results_shown: bool,
    /// Nothing matches what is typed: the host says so (SRCH-05)…
    pub nothing_found: bool,
    /// …and offers "Search the web for '…'" if this is set.
    pub web_offered: bool,
    /// Applications offers search, so screen readers can open it with an action (A11Y-05).
    pub offered: bool,
}

/// The folder whose new name the host asks the user for (FOLD-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenameFrame {
    pub folder: FolderId,
    /// Its place in the grid's cells, whose name the dialog starts with.
    pub cell: Option<usize>,
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
    /// The item of the current view under the finger, by its place in the view's cells or the
    /// open folder's (ANIM-10).
    pub pressed: Option<usize>,
    pub pressed_surface: Surface,
    /// The pressed item's icon scale in ten-thousandths: 10 000 at rest, 9 200 held (ANIM-10).
    pub press_scale: u16,
    /// The grid's cells as apps come and go (ANIM-21).
    pub grid_change: GridChangeFrame,
    /// The dragged icon, while one is lifted, settles into its cell or goes into a folder
    /// (APPS-11, APPS-12, ANIM-14).
    pub lifted: Option<LiftFrame>,
    /// The icon a dragged app is held over (ANIM-14).
    pub hover: Option<HoverFrame>,
    /// A folder taking in a dropped app (ANIM-14).
    pub merge: Option<MergeFrame>,
    /// The open folder's panel, while it is open or closing (FOLD-04, ANIM-15).
    pub folder: Option<FolderFrame>,
    /// The folder being renamed (FOLD-03).
    pub renaming: Option<RenameFrame>,
    /// Edit mode and its motion (APPS-12, ANIM-12).
    pub edit: EditFrame,
    /// The search field and its results (SRCH-01, ANIM-16).
    pub search: SearchFrame,
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
    /// Settings → Applications → Hidden apps (APPS-19): the list's revision, which changes
    /// whenever its apps do, or `None` while Applications isn't working, so Settings doesn't
    /// offer it (ARCH-09).
    pub hidden_apps: Option<u32>,
    /// The Events header (EVT-01, EVT-02): its revision, which changes whenever its text does,
    /// or `None` while Events isn't working, so it isn't drawn (ARCH-09).
    pub events_header: Option<u32>,
    /// What shows below the Events header, or `None` while Events isn't working, so nothing
    /// does (ARCH-09).
    pub events_list: Option<EventsListFrame>,
    /// How far the Events list has come in showing rows that came and went, from 0 to 10 000
    /// (ANIM-20). Each row says where it was, so the host knows which came and which went.
    pub events_change: u16,
    /// The home views' solid background (HOME-11, THEME-04).
    pub background: Rgb,
    /// Menus and Settings (THEME-03).
    pub surface: Rgb,
    /// Text, and the indicator's highlighted segment.
    pub foreground: Rgb,
    /// The indicator's other segments, and secondary text such as section headers.
    pub dim: Rgb,
    /// Hairlines, such as the one beside a section header (SRS 5.3).
    pub divider: Rgb,
    /// The dot on a newly installed app (APPS-07).
    pub accent: Rgb,
    /// The opacity of the black scrim behind an open folder, 0 to 255 (FOLD-04).
    pub scrim: u8,
}

pub struct ShellView {
    outbox: Outbox,
    apps: AppsView,
    events: EventsView,
    recent: RecentView,
    settings: SettingsView,
    hidden_apps: HiddenAppsSection,
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

        // Applications shows its Hidden apps section in Settings (APPS-19). It reads the latest
        // Applications state, which the shell keeps current, and comes before the lock-screen
        // image's, in SET-02's order.
        let apps_state = Latest::new(shell.apps().state());
        shell.subscribe_apps({
            let apps_state = apps_state.clone();
            move |state| apps_state.set(Arc::clone(state))
        });
        let hidden_apps = HiddenAppsSection::new(apps_state, dispatch(&outbox, ShellIntent::Apps));

        let mut settings = SettingsView::new(dispatch(&outbox, ShellIntent::Settings));
        settings.add_section(Box::new(hidden_apps.clone()));
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
            hidden_apps,
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
        let apps_state = shell.apps().state();
        let mut apps = self.apps.frame(&apps_state);
        // A failed Applications can't close its menu, so the home is never left covered by it
        // (ARCH-09).
        if !state.app_menu_open {
            apps.menu = None;
        }
        let in_grid = |key: &CellKey| self.apps.place_of(Surface::Grid, key);
        let lift = &state.lift;
        let lifted = lift.key.as_ref().map(|key| LiftFrame {
            surface: lift.surface,
            cell: self.apps.place_of(lift.surface, key),
            x: lift.x.round() as i32,
            y: lift.y.round() as i32,
            scale: scale(lift.scale),
            alpha: (lift.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            settling: lift.settling,
            merging: lift.merging,
        });
        let hover = state.hover.key.as_ref().and_then(|key| {
            Some(HoverFrame {
                cell: in_grid(key)?,
                scale: scale(state.hover.scale),
            })
        });
        let merge = state.merge.folder.and_then(|folder| {
            Some(MergeFrame {
                cell: in_grid(&CellKey::Folder(folder))?,
                slot: state.merge.slot,
                formed: state.merge.formed,
                progress: ten_thousandths(state.merge.progress),
            })
        });
        let folder = state.folder.folder.map(|folder| FolderFrame {
            folder,
            open: state.folder.open && state.folder_open,
            cell: in_grid(&CellKey::Folder(folder)),
            progress: ten_thousandths(state.folder.progress),
        });
        let renaming = state.renaming.map(|folder| RenameFrame {
            folder,
            cell: in_grid(&CellKey::Folder(folder)),
        });
        let mode = &state.mode;
        // A failed Applications never shows search (ARCH-09): the shell's field closes with it.
        let open = state.search.open;
        let search = SearchFrame {
            open,
            reveal_px: state.search.reveal_px.round() as i32,
            pulling: state.search.pulling,
            results_shown: open && apps.has_query,
            nothing_found: open && apps.nothing_found,
            web_offered: open && apps.nothing_found && apps.web_search_offered,
            offered: state.search_offered(),
        };
        HomeFrame {
            pages: HomeView::ALL.map(|view| state.page_offset_px(view).round() as i32),
            indicator_alpha: (state.carousel.indicator.clamp(0.0, 1.0) * 255.0).round() as u8,
            indicator_selected: state.nearest_view(),
            current: state.current,
            content_scale: (state.return_home.scale.clamp(0.0, 1.0) * 10_000.0).round() as u16,
            content_alpha: (state.return_home.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            apps,
            pressed: state.press.item,
            pressed_surface: state.press.surface,
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
            hover,
            merge,
            folder,
            renaming,
            edit: EditFrame {
                on: mode.on,
                scale: scale(mode.scale),
                progress: ten_thousandths(mode.progress),
                first_row: u16::try_from(mode.first_row).unwrap_or(u16::MAX),
                badges: mode.badges.map(scale),
            },
            search,
            animating: state.carousel.animating
                || state.return_home.animating
                || state.press.animating
                || state.grid_change.animating
                || state.lift.animating
                || mode.animating
                || state.folder.animating
                || state.search.animating
                || state.events_change.animating,
            menu: state.menu.map(|menu| MenuFrame {
                items: menu.items,
                anchor: menu
                    .anchor
                    .map(|(x, y)| (x.round() as i32, y.round() as i32)),
            }),
            menu_offered: state.menu_offered(),
            settings_open: state.settings_open,
            settings_group: state.settings_group,
            hidden_apps: shell.apps_ready().then(|| self.hidden_apps.revision()),
            events_header: shell
                .events_ready()
                .then(|| self.events.show_header(&shell.events().state())),
            events_list: shell.events_ready().then(|| {
                let revision = self.events.show_list(&shell.events().state(), tokens);
                EventsListFrame {
                    mode: self.events.mode(),
                    revision,
                }
            }),
            events_change: ten_thousandths(state.events_change.progress),
            background: tokens.background,
            surface: tokens.surface,
            foreground: tokens.text_primary,
            dim: tokens.text_secondary,
            divider: tokens.divider,
            accent: tokens.accent,
            scrim: (u16::from(tokens.scrim_percent.min(100)) * 255 / 100) as u8,
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

    /// Settings → Applications → Hidden apps (APPS-19). Offered only while
    /// [`HomeFrame::hidden_apps`] is.
    pub fn hidden_apps(&self) -> &HiddenAppsSection {
        &self.hidden_apps
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
