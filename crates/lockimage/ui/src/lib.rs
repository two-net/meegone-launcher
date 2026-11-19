//! Lock-screen image UI (SRS 4.5).
//!
//! This feature shows its content inside other features' views: the card in Events (EVT-12)
//! and a section in Settings (SET-02). Both are [`Slot`]s that the shell puts in place, so
//! neither Events nor Settings depends on this feature (ARCH-03, ADR 0002).

use std::cell::RefCell;
use std::rc::Rc;

use meegone_core_ui::{Dispatch, Latest, Slot, Tokens};
use meegone_lockimage_state::{ImageStatus, LockImageIntent, LockImageState};

pub use meegone_lockimage_state::CheckStatus;

#[cfg(test)]
mod tests;

/// What the image-of-the-day card shows (EVT-12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardContent {
    pub title: String,
    /// Shown as the source provides it (LEGAL-03).
    pub attribution: String,
    /// The file that holds the image, relative to the storage root, which the host decodes the
    /// thumbnail from (ADR 0022).
    pub image_path: &'static str,
    /// Tells images apart: it changes only when the image does, so the host decodes the
    /// thumbnail again, and cross-fades to it, only then (ANIM-22).
    pub image_key: String,
}

/// The image-of-the-day card shown in Events, below the notifications (EVT-12; ADR 0022).
///
/// The host draws it as it draws Events' rows (ADR 0020): [`ImageCard::revision`] changes
/// whenever what the card shows does, and the host then reads [`ImageCard::content`]. A tap on
/// a card that changed meanwhile does nothing.
///
/// Clones share what the host was last told, so the shell can keep one for the host while
/// Events holds another.
#[derive(Clone)]
pub struct ImageCard {
    latest: Latest<LockImageState>,
    dispatch: Dispatch<LockImageIntent>,
    told: Rc<RefCell<Told>>,
}

/// The card the host was last told about, and its revision.
#[derive(Default)]
struct Told {
    content: Option<CardContent>,
    revision: u32,
}

impl ImageCard {
    pub fn new(latest: Latest<LockImageState>, dispatch: Dispatch<LockImageIntent>) -> Self {
        Self {
            latest,
            dispatch,
            told: Rc::default(),
        }
    }

    /// Shown while the daily image is on, there is an image and the card isn't hidden (EVT-12).
    pub fn is_visible(&self) -> bool {
        Self::shown(&self.latest.get()).is_some()
    }

    /// The card's revision, which changes whenever what it shows does, so the host reads
    /// [`ImageCard::content`] again. While the card is hidden it keeps what it last showed.
    pub fn revision(&self) -> u32 {
        let mut told = self.told.borrow_mut();
        if let Some(content) = Self::shown(&self.latest.get()) {
            if told.content.as_ref() != Some(&content) {
                told.content = Some(content);
                told.revision = told.revision.wrapping_add(1);
            }
        }
        told.revision
    }

    /// What the card shows, as of the last [`ImageCard::revision`].
    pub fn content(&self) -> Option<CardContent> {
        self.told.borrow().content.clone()
    }

    /// The card at `revision` was tapped: the image details screen opens (EVT-12, WALL-15).
    /// Returns whether that was asked; a card that changed or was hidden meanwhile asks
    /// nothing.
    pub fn tapped(&self, revision: u32) -> bool {
        let current = {
            let told = self.told.borrow();
            told.revision == revision && told.content.is_some()
        };
        if !current || !self.is_visible() {
            return false;
        }
        self.dispatch.send(LockImageIntent::OpenDetails);
        true
    }

    fn shown(state: &LockImageState) -> Option<CardContent> {
        if state.status != ImageStatus::On || !state.show_card {
            return None;
        }
        let record = state.current.as_ref()?;
        let file = record.image?;
        Some(CardContent {
            title: record.title.clone(),
            attribution: record.attribution.clone(),
            image_path: file.path(),
            image_key: format!("{} {} {}", record.date, record.region, file.path()),
        })
    }
}

impl Slot for ImageCard {
    fn render(&mut self, _tokens: &Tokens) {
        // The host draws the card from `revision` and `content` (ADR 0006, ADR 0022).
    }
}

/// The "Daily lock-screen image" switch (SET-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyImage {
    Off,
    On,
    /// The device can't set the lock-screen wallpaper: the switch is disabled, with the reason
    /// (WALL-13).
    Unavailable,
}

/// The "Lock-screen image" section of Settings (SET-02).
///
/// Clones share their state, so the shell can keep one for the host while Settings holds
/// another.
#[derive(Clone)]
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

    /// Whether the daily image is on, off or can't be used on this device (WALL-02, WALL-13).
    pub fn daily_image(&self) -> DailyImage {
        match self.latest.get().status {
            ImageStatus::Off => DailyImage::Off,
            ImageStatus::On => DailyImage::On,
            ImageStatus::Unavailable(_) => DailyImage::Unavailable,
        }
    }

    /// Why the device can't set the lock-screen wallpaper, while it can't (WALL-13).
    pub fn unavailable_reason(&self) -> Option<String> {
        match &self.latest.get().status {
            ImageStatus::Unavailable(reason) => Some(reason.clone()),
            _ => None,
        }
    }

    /// The switch is the user's confirmation: nothing changes on the lock screen before it is
    /// on (WALL-02, WALL-12).
    pub fn daily_image_toggled(&self, on: bool) {
        self.dispatch.send(LockImageIntent::SetEnabled(on));
    }

    /// "Update now" is offered while the daily image is on (WALL-11).
    pub fn update_now_offered(&self) -> bool {
        self.latest.get().status == ImageStatus::On
    }

    /// Whether "Update now" is on its way: it shows as "Updating…" and takes no tap (WALL-11).
    pub fn updating(&self) -> bool {
        self.latest.get().updating
    }

    /// "Update now", or Resume while paused, was tapped (WALL-11, WALL-14).
    pub fn update_now(&self) {
        self.dispatch.send(LockImageIntent::UpdateNow);
    }

    /// Whether the section says someone else changed the lock screen, so daily updates are
    /// paused, with Resume (WALL-14).
    pub fn paused(&self) -> bool {
        let state = self.latest.get();
        state.status == ImageStatus::On && state.paused
    }

    /// Whether the section says the image may not appear on this device's lock screen (SRS C5,
    /// ADR 0030).
    pub fn may_not_show(&self) -> bool {
        let state = self.latest.get();
        state.status == ImageStatus::On && state.may_not_show
    }

    /// "Reset lock screen to home wallpaper" and the shortcut to the system's wallpaper
    /// settings are offered once the daily image is off (WALL-12).
    pub fn off_actions_offered(&self) -> bool {
        self.latest.get().status == ImageStatus::Off
    }

    /// "Reset lock screen to home wallpaper" was tapped (WALL-12).
    pub fn reset_lock_screen(&self) {
        self.dispatch.send(LockImageIntent::ResetLockScreen);
    }

    /// The shortcut to the system's wallpaper settings was tapped (WALL-12).
    pub fn open_wallpaper_settings(&self) {
        self.dispatch.send(LockImageIntent::OpenWallpaperSettings);
    }

    /// Whether "Show card in Events" is on (EVT-12).
    pub fn show_card(&self) -> bool {
        self.latest.get().show_card
    }

    /// "Show card in Events" was switched (EVT-12, SET-03).
    pub fn show_card_toggled(&self, on: bool) {
        self.dispatch.send(LockImageIntent::SetShowCard(on));
    }

    /// The regions offered besides Automatic, as BCP 47 tags such as `en-GB` (WALL-05).
    pub fn regions(&self) -> Vec<String> {
        self.latest.get().regions.clone()
    }

    /// The region chosen, or `None` for Automatic (WALL-05).
    pub fn region(&self) -> Option<String> {
        self.latest.get().region.clone()
    }

    /// The region Automatic stands for on this device, to show with it (WALL-05).
    pub fn automatic_region(&self) -> String {
        self.latest.get().automatic_region.clone()
    }

    /// A region was chosen, or Automatic with `None` (WALL-05, SET-03).
    pub fn region_chosen(&self, region: Option<String>) {
        self.dispatch.send(LockImageIntent::SetRegion(region));
    }

    /// Whether "Use mobile data" is on (WALL-04).
    pub fn use_mobile_data(&self) -> bool {
        self.latest.get().use_mobile_data
    }

    /// "Use mobile data" was switched (WALL-04, SET-03).
    pub fn use_mobile_data_toggled(&self, on: bool) {
        self.dispatch.send(LockImageIntent::SetUseMobileData(on));
    }

    /// How the daily checks are going: the last problem while they retry, for the status line
    /// #30 draws (WALL-16, SRS 7.6).
    pub fn checks(&self) -> CheckStatus {
        self.latest.get().checks.clone()
    }

    /// Whether the section says "Daily image unavailable", with Try again: the daily image is
    /// on, and its checks have failed for 3 days (SRS 7.6).
    pub fn daily_image_unavailable(&self) -> bool {
        let state = self.latest.get();
        state.status == ImageStatus::On && matches!(state.checks, CheckStatus::Unavailable(_))
    }

    /// Try again was tapped (SRS 7.6).
    pub fn try_again(&self) {
        self.dispatch.send(LockImageIntent::TryAgain);
    }
}

impl Slot for ImageSettingsSection {
    fn render(&mut self, _tokens: &Tokens) {
        // The host draws the daily image's switch, Update now, the pause, "Show card in
        // Events", "Use mobile data", "Daily image unavailable" and, once the image is off, the
        // reset and the wallpaper settings, until the Settings screen, #34, draws the rest,
        // Region included (ADR 0006, ADR 0022, ADR 0027, ADR 0029, ADR 0030).
    }
}
