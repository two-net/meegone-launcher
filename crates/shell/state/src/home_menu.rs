//! The home menu (HOME-10): a long-press on empty space in Events or Applications opens it, with
//! Launcher settings, Lock-screen image and, in Applications, Edit applications. Recent has none:
//! there a long-press enters close mode (REC-07).

use crate::HomeView;

/// One item of the home menu, in the order it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    /// Opens Settings (SET-01).
    LauncherSettings,
    /// Opens the image details screen while it has an image to show (WALL-15), and otherwise
    /// Settings at its Lock-screen image group, so it works before the first image (SET-02,
    /// WALL-02, WALL-10).
    LockScreenImage,
    /// Enters edit mode. Applications only.
    EditApplications,
}

impl MenuItem {
    /// Every item, in the order it shows.
    pub const ALL: [MenuItem; 3] = [
        MenuItem::LauncherSettings,
        MenuItem::LockScreenImage,
        MenuItem::EditApplications,
    ];

    /// The item's place in [`MenuItem::ALL`], from 0.
    pub fn index(self) -> usize {
        match self {
            MenuItem::LauncherSettings => 0,
            MenuItem::LockScreenImage => 1,
            MenuItem::EditApplications => 2,
        }
    }

    /// The item at `index` in [`MenuItem::ALL`], if there is one.
    pub fn from_index(index: usize) -> Option<MenuItem> {
        MenuItem::ALL.get(index).copied()
    }
}

/// The home menu, open over Events or Applications.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HomeMenu {
    /// What it offers, in order.
    pub items: &'static [MenuItem],
    /// Where the finger was held, in pixels from the home views' top left, so the menu opens
    /// there. `None` when a screen reader opened it (A11Y-05) or the layout changed since
    /// (COMP-03), and the host centres it.
    pub anchor: Option<(f64, f64)>,
}

impl HomeMenu {
    /// The items `view`'s menu offers, or `None` for Recent, which has no menu (HOME-10). While
    /// edit mode is on, Applications' leaves out Edit applications, as an app's menu does.
    pub fn items_for(view: HomeView, editing: bool) -> Option<&'static [MenuItem]> {
        const WITHOUT_EDIT: [MenuItem; 2] = [MenuItem::LauncherSettings, MenuItem::LockScreenImage];
        match view {
            HomeView::Applications if !editing => Some(&MenuItem::ALL),
            HomeView::Applications | HomeView::Events => Some(&WITHOUT_EDIT),
            HomeView::Recent => None,
        }
    }

    /// Whether the menu offers `item`.
    pub fn offers(&self, item: MenuItem) -> bool {
        self.items.contains(&item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_applications_is_offered_only_in_applications() {
        // HOME-10
        let without_edit = Some(&[MenuItem::LauncherSettings, MenuItem::LockScreenImage][..]);
        assert_eq!(
            HomeMenu::items_for(HomeView::Applications, false),
            Some(&MenuItem::ALL[..])
        );
        assert_eq!(HomeMenu::items_for(HomeView::Events, false), without_edit);
        assert_eq!(HomeMenu::items_for(HomeView::Recent, false), None);
        // In edit mode, Edit applications has nothing to do (APPS-12).
        assert_eq!(
            HomeMenu::items_for(HomeView::Applications, true),
            without_edit
        );
    }

    #[test]
    fn items_are_found_by_their_place() {
        for item in MenuItem::ALL {
            assert_eq!(MenuItem::from_index(item.index()), Some(item));
        }
        assert_eq!(MenuItem::from_index(3), None);
    }
}
