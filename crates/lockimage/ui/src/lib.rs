//! Lock-screen image UI (SRS 4.5).
//!
//! This feature shows its content inside other features' views: the card in Events (EVT-12)
//! and a section in Settings (SET-02). Both are [`Slot`]s that the shell puts in place, so
//! neither Events nor Settings depends on this feature (ARCH-03, ADR 0002).

use meegone_core_ui::{Dispatch, Latest, Slot, Tokens};
use meegone_lockimage_state::{ImageStatus, LockImageIntent, LockImageState};

/// The image-of-the-day card shown in Events (EVT-12).
pub struct ImageCard {
    latest: Latest<LockImageState>,
}

impl ImageCard {
    pub fn new(latest: Latest<LockImageState>) -> Self {
        Self { latest }
    }

    /// Shown while the daily image is on, there is an image and the card isn't hidden (EVT-12).
    pub fn is_visible(&self) -> bool {
        let state = self.latest.get();
        state.status == ImageStatus::On && state.show_card && state.current.is_some()
    }
}

impl Slot for ImageCard {
    fn render(&mut self, _tokens: &Tokens) {
        // Drawing the thumbnail, title and attribution when `is_visible` arrives with #21
        // (ADR 0006).
    }
}

/// The "Lock-screen image" section of Settings (SET-02).
pub struct ImageSettingsSection {
    latest: Latest<LockImageState>,
    dispatch: Dispatch<LockImageIntent>,
}

impl ImageSettingsSection {
    pub fn new(latest: Latest<LockImageState>, dispatch: Dispatch<LockImageIntent>) -> Self {
        Self { latest, dispatch }
    }

    /// The switch is disabled when the device can't set the lock-screen wallpaper (WALL-13).
    pub fn is_switch_enabled(&self) -> bool {
        !matches!(self.latest.get().status, ImageStatus::Unavailable(_))
    }

    /// WALL-02, WALL-12
    pub fn daily_image_toggled(&self, on: bool) {
        self.dispatch.send(LockImageIntent::SetEnabled(on));
    }
}

impl Slot for ImageSettingsSection {
    fn render(&mut self, _tokens: &Tokens) {
        // Drawing arrives with the Settings screen, #34 (ADR 0006).
    }
}
