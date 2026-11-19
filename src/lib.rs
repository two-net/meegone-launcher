//! MeeGone Launcher, the one installable app (ARCH-01).
//!
//! This crate is the entry point. It receives the platform adapters and hands them to the Home
//! shell, the only place features are put together (ARCH-03). On Android, the home activity
//! calls the entry points in `android.rs` (ADR 0005). See ARCHITECTURE.md for the crate map and
//! rules.

#[cfg(target_os = "android")]
mod android;

pub use meegone_core_data::{Platform, Rect};
pub use meegone_shell_state::{
    CrossFadeState, Direction, EventsChangeState, FOLDER_COLUMNS, GridChangeState, HomeMenu,
    HomeShell, HomeView, Layout, LiftState, MenuItem, PREVIEW_APPS, PinchEvent, PinchKind,
    PinchState, PointerEvent, PointerKind, PressState, SavedHome, SettingsGroup, Shared,
    ShellIntent,
};
use meegone_shell_ui::ShellView;
pub use meegone_shell_ui::{
    AccessItem, AccessStatus, Age, AppMenuFrame, AppMenuItem, AppsFrame, BADGE_ROWS, Bounds,
    CardContent, Cell, CellKey, CheckStatus, CloseFrame, DailyImage, DetailsContent, EditFrame,
    EventsListFrame, EventsListMode, EventsMenuFrame, EventsRow, FolderFrame, FolderId,
    GridChangeFrame, GridLayout, HomeFrame, HoverFrame, LiftFrame, MenuFrame, MergeFrame,
    NotificationMenuItem, PastAction, PastContent, PastImageContent, Preview, RECENT_ROWS,
    RecentFrame, RecentMode, RecentWindow, RenameFrame, RowTarget, SearchFrame, SetupFrame,
    SetupImageContent, SetupStep, ShownCard, Surface, SurfaceCellsFrame, TodayEntry, UpdateStatus,
};

pub struct Launcher {
    shell: HomeShell,
    view: ShellView,
}

impl Launcher {
    pub fn start(platform: Platform) -> Self {
        Self::start_shared(&Shared::new(platform))
    }

    /// Starts the home with the stores the daily image's job shares, so each sees the other's
    /// writes (ADR 0029).
    pub fn start_shared(shared: &Shared) -> Self {
        let mut shell = HomeShell::compose_shared(shared);
        let view = ShellView::new(&mut shell);
        Self { shell, view }
    }

    /// An event from the system, such as Home, Back or a theme change.
    pub fn dispatch(&mut self, intent: ShellIntent) {
        self.shell.dispatch(intent);
    }

    /// One UI turn: applies the intents the views sent, then draws. Returns what the host
    /// draws for the home views (ADR 0006).
    pub fn frame(&mut self) -> HomeFrame {
        for intent in self.view.take_intents() {
            self.shell.dispatch(intent);
        }
        self.view.render(&self.shell);
        self.view.frame(&self.shell)
    }

    pub fn shell(&self) -> &HomeShell {
        &self.shell
    }

    pub fn view(&self) -> &ShellView {
        &self.view
    }
}
