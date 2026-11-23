//! MeeGone Launcher, the one installable app (ARCH-01).
//!
//! This crate is the entry point. It receives the platform adapters and hands them to the Home
//! shell, the only place features are put together (ARCH-03). On Android, the home activity
//! calls the entry points in `android.rs` (ADR 0005). See ARCHITECTURE.md for the crate map and
//! rules.

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "android", test))]
mod panics;

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
    EventsListFrame, EventsListMode, EventsMenuFrame, EventsRow, Failures, Feature, FolderFrame,
    FolderId, GridChangeFrame, GridLayout, HomeFrame, HoverFrame, LiftFrame, MenuFrame, MergeFrame,
    NotificationMenuItem, PastAction, PastContent, PastImageContent, Preview, RECENT_ROWS,
    RecentFrame, RecentMode, RecentWindow, RenameFrame, RowTarget, SearchFrame, SetupFrame,
    SetupImageContent, SetupStep, ShownCard, StoppedCards, Surface, SurfaceCellsFrame, TodayEntry,
    UpdateStatus,
};

pub struct Launcher {
    shell: HomeShell,
    view: ShellView,
    /// The features drawn as failed in the last frame (ARCH-09, ADR 0043).
    drawn: Failures,
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
        Self {
            shell,
            view,
            drawn: Failures::default(),
        }
    }

    /// An event from the system, such as Home, Back or a theme change.
    pub fn dispatch(&mut self, intent: ShellIntent) {
        self.shell.dispatch(intent);
    }

    /// One UI turn: applies the intents the views sent, then draws. Returns what the host
    /// draws for the home views (ADR 0006).
    ///
    /// A feature that fails while it is drawn is drawn as failed in this same frame: what it
    /// covered, such as the image details, closes first (ARCH-09, ADR 0043).
    pub fn frame(&mut self) -> HomeFrame {
        for intent in self.view.take_intents() {
            self.shell.dispatch(intent);
        }
        // At most once more for each feature, as each failure is for good.
        loop {
            let failed = self.shell.failures();
            for feature in failed.since(self.drawn) {
                self.shell.dispatch(ShellIntent::Failed(feature));
            }
            self.drawn = failed;
            self.view.render(&self.shell);
            let frame = self.view.frame(&self.shell);
            if !self.needs_redraw() {
                return frame;
            }
        }
    }

    /// Whether a feature failed since the last frame was drawn, such as in one of the host's
    /// readers, so the host should draw again (ADR 0043).
    pub fn needs_redraw(&self) -> bool {
        self.shell.failures() != self.drawn
    }

    pub fn shell(&self) -> &HomeShell {
        &self.shell
    }

    pub fn view(&self) -> &ShellView {
        &self.view
    }
}
