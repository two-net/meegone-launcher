//! MeeGone Launcher, the one installable app (ARCH-01).
//!
//! This crate is the entry point. It receives the platform adapters and hands them to the Home
//! shell, the only place features are put together (ARCH-03). On Android, the home activity
//! calls the entry points in `android.rs` (ADR 0005). See ARCHITECTURE.md for the crate map and
//! rules.

#[cfg(target_os = "android")]
mod android;

pub use meegone_core_data::Platform;
pub use meegone_shell_state::{
    Direction, HomeShell, HomeView, Layout, PointerEvent, PointerKind, SavedHome, ShellIntent,
};
pub use meegone_shell_ui::HomeFrame;
use meegone_shell_ui::ShellView;

pub struct Launcher {
    shell: HomeShell,
    view: ShellView,
}

impl Launcher {
    pub fn start(platform: Platform) -> Self {
        let mut shell = HomeShell::compose(platform);
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
