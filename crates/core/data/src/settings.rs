//! The settings store (SRS 7.4).
//!
//! Several features keep their options here, so the store lives in core while the Settings
//! feature owns the screens (ADR 0002). It is the single source of truth for every setting
//! (ARCH-06).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::model::{AppId, DataError};
use crate::storage::format::{Fields, Line};
use crate::storage::{
    Decode, Encode, Format, Migration, Reader, StorageAdapter, StoredFile, Writer, load, save,
};

/// Version of the saved settings format (MAINT-04). Changing the format means a new version, a
/// migration in [`SETTINGS_MIGRATIONS`] and a fixture (ARCHITECTURE.md, "Persistence").
pub const SETTINGS_VERSION: u32 = 3;

/// Brings settings saved by older versions up to date, one version at a time.
const SETTINGS_MIGRATIONS: [Migration; SETTINGS_VERSION as usize - 1] =
    [add_home_banner, name_recent_window];

/// Version 2 adds when the "make MeeGone the default" banner was last dismissed (ONB-04). Older
/// settings never dismissed it.
fn add_home_banner(body: &str) -> Option<String> {
    let mut added = Writer::default();
    added.line("home_banner_dismissed", None::<u64>);
    Some(format!("{body}{}", added.into_body()))
}

/// Version 3 saves Recent's window as one of REC-11's three choices, by name, instead of a
/// number of hours. A window that isn't one of them becomes the shortest choice that still shows
/// everything it did, or three days if none does (ADR 0026).
fn name_recent_window(body: &str) -> Option<String> {
    let mut migrated = String::with_capacity(body.len());
    for line in body.lines() {
        match line.strip_prefix("recent_window_hours ") {
            Some(hours) => {
                let window = RecentWindow::covering(hours.parse().ok()?);
                let mut named = Writer::default();
                named.line("recent_window", window);
                migrated.push_str(&named.into_body());
            }
            None => {
                migrated.push_str(line);
                migrated.push('\n');
            }
        }
    }
    Some(migrated)
}

/// The user's theme choice (THEME-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

/// What Home shows when the user comes back from an app (HOME-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeTarget {
    #[default]
    LastView,
    Applications,
}

/// Columns in Recent (REC-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecentDensity {
    #[default]
    ThreeColumns,
    TwoColumns,
}

/// How far back Recent looks (REC-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecentWindow {
    TwelveHours,
    #[default]
    TwentyFourHours,
    ThreeDays,
}

impl RecentWindow {
    /// Every choice, shortest first, as Settings lists them.
    pub const ALL: [RecentWindow; 3] = [Self::TwelveHours, Self::TwentyFourHours, Self::ThreeDays];

    /// The window's length in hours.
    pub const fn hours(self) -> u32 {
        match self {
            Self::TwelveHours => 12,
            Self::TwentyFourHours => 24,
            Self::ThreeDays => 72,
        }
    }

    /// The choice that is exactly `hours` long, if there is one.
    pub fn of_hours(hours: u32) -> Option<RecentWindow> {
        Self::ALL.into_iter().find(|window| window.hours() == hours)
    }

    /// The shortest choice at least `hours` long, or the longest.
    fn covering(hours: u32) -> RecentWindow {
        Self::ALL
            .into_iter()
            .find(|window| window.hours() >= hours)
            .unwrap_or(Self::ThreeDays)
    }
}

/// Lock-screen image options (SET-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageOptions {
    /// Off until the user confirms it in setup or Settings (WALL-02).
    pub enabled: bool,
    /// Feed region code; `None` means Automatic (WALL-05).
    pub region: Option<String>,
    pub use_mobile_data: bool,
    pub show_card: bool,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            region: None,
            use_mobile_data: false,
            show_card: true,
        }
    }
}

/// Every setting (SRS 7.4). Of app usage, only the apps removed from Recent are kept, with the
/// time of removal (PRIV-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub home_target: HomeTarget,
    pub wrap_around: bool,
    pub recent_density: RecentDensity,
    pub recent_window: RecentWindow,
    pub hidden_apps: BTreeSet<AppId>,
    pub image: ImageOptions,
    /// Apps the user removed from Recent, with the time of removal (PRIV-03).
    pub removed_from_recent: BTreeMap<AppId, u64>,
    /// When the user last dismissed or answered the "make MeeGone the default" banner, so it
    /// shows at most once a day (ONB-04). Not a user-adjustable option (SET-04).
    pub home_banner_dismissed_ms: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            home_target: HomeTarget::default(),
            wrap_around: true,
            recent_density: RecentDensity::default(),
            recent_window: RecentWindow::default(),
            hidden_apps: BTreeSet::new(),
            image: ImageOptions::default(),
            removed_from_recent: BTreeMap::new(),
            home_banner_dismissed_ms: None,
        }
    }
}

/// Reads and writes [`Settings`]. Changes take effect at once (SET-03).
pub trait SettingsStore: Send + Sync {
    /// The current settings, or the defaults if none could be read (MAINT-04).
    fn get(&self) -> Settings;
    /// Applies `change` and saves the result atomically (REL-03), returning the new settings.
    fn update(&self, change: &mut dyn FnMut(&mut Settings)) -> Result<Settings, DataError>;
}

/// Keeps the settings in [`StoredFile::Settings`].
pub struct PersistentSettingsStore {
    storage: Arc<dyn StorageAdapter>,
    /// The settings as last read or saved. Read on first use, because constructors do no I/O.
    kept: Mutex<Option<Settings>>,
}

impl PersistentSettingsStore {
    pub fn new(storage: Arc<dyn StorageAdapter>) -> Self {
        Self {
            storage,
            kept: Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Settings>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The kept settings, read from storage the first time.
    fn current<'a>(&self, kept: &'a mut Option<Settings>) -> &'a Settings {
        kept.get_or_insert_with(|| load(&*self.storage, StoredFile::Settings))
    }
}

impl SettingsStore for PersistentSettingsStore {
    fn get(&self) -> Settings {
        self.current(&mut self.lock()).clone()
    }

    fn update(&self, change: &mut dyn FnMut(&mut Settings)) -> Result<Settings, DataError> {
        let mut kept = self.lock();
        // Change a copy, so a change that panics part-way, or a save that fails, leaves the
        // settings as they were.
        let mut next = self.current(&mut kept).clone();
        change(&mut next);
        save(&*self.storage, StoredFile::Settings, &next)?;
        *kept = Some(next.clone());
        Ok(next)
    }
}

/// One record per setting, in this order, with one `hidden` record per hidden app and one
/// `removed` record per app removed from Recent.
impl Format for Settings {
    const KIND: &'static str = "settings";
    const VERSION: u32 = SETTINGS_VERSION;
    const MIGRATIONS: &'static [Migration] = &SETTINGS_MIGRATIONS;

    fn write(&self, out: &mut Writer) {
        out.line("theme", self.theme)
            .line("home_target", self.home_target)
            .line("wrap_around", self.wrap_around)
            .line("recent_density", self.recent_density)
            .line("recent_window", self.recent_window);
        for app in &self.hidden_apps {
            out.line("hidden", app);
        }
        out.line("image_enabled", self.image.enabled)
            .line("image_region", &self.image.region)
            .line("image_use_mobile_data", self.image.use_mobile_data)
            .line("image_show_card", self.image.show_card);
        for removal in &self.removed_from_recent {
            out.line("removed", removal);
        }
        out.line("home_banner_dismissed", self.home_banner_dismissed_ms);
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            theme: input.line("theme")?,
            home_target: input.line("home_target")?,
            wrap_around: input.line("wrap_around")?,
            recent_density: input.line("recent_density")?,
            recent_window: input.line("recent_window")?,
            hidden_apps: input.lines("hidden")?.into_iter().collect(),
            image: ImageOptions {
                enabled: input.line("image_enabled")?,
                region: input.line("image_region")?,
                use_mobile_data: input.line("image_use_mobile_data")?,
                show_card: input.line("image_show_card")?,
            },
            removed_from_recent: input.lines("removed")?.into_iter().collect(),
            home_banner_dismissed_ms: input.line("home_banner_dismissed")?,
        })
    }
}

/// Saves each choice as its name.
macro_rules! named {
    ($t:ty { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl Encode for $t {
            fn encode(&self, line: &mut Line<'_>) {
                line.token(match self {
                    $(Self::$variant => $name,)+
                });
            }
        }

        impl Decode for $t {
            fn decode(fields: &mut Fields<'_>) -> Option<Self> {
                match fields.token()? {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

named!(ThemeChoice {
    System => "system",
    Light => "light",
    Dark => "dark",
});
named!(HomeTarget {
    LastView => "last_view",
    Applications => "applications",
});
named!(RecentDensity {
    ThreeColumns => "three_columns",
    TwoColumns => "two_columns",
});
named!(RecentWindow {
    TwelveHours => "twelve_hours",
    TwentyFourHours => "twenty_four_hours",
    ThreeDays => "three_days",
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::{FakeStorage, app_id};
    use crate::storage::format::seal;
    use crate::storage::{decode, encode};

    #[test]
    fn there_are_at_most_twelve_user_adjustable_options() {
        // SET-04, SRS 7.1. Every field is named, with no `..`, so a new one doesn't compile
        // until it is counted here as an option or set aside as not one.
        let Settings {
            theme,
            home_target,
            wrap_around,
            // Recent's columns, chosen by pinching (REC-05).
            recent_density,
            // Settings → Recent → Time window (REC-11).
            recent_window,
            // Settings → Applications → Hidden apps (APPS-19).
            hidden_apps,
            image:
                ImageOptions {
                    enabled,
                    region,
                    use_mobile_data,
                    show_card,
                },
            // What the user removed from Recent, not an option.
            removed_from_recent: _,
            // When the banner was last answered, not an option (ADR 0005).
            home_banner_dismissed_ms: _,
        } = Settings::default();
        let options: &[&dyn std::fmt::Debug] = &[
            &theme,
            &home_target,
            &wrap_around,
            &recent_density,
            &recent_window,
            &hidden_apps,
            &enabled,
            &region,
            &use_mobile_data,
            &show_card,
        ];

        assert!(options.len() <= 12, "SET-04 allows 12 at most");
    }

    /// A saved file, and the value it must load as.
    type Fixture = (&'static str, fn() -> Settings);

    /// Every saved version, oldest first, with the settings it must load as (MAINT-04). Never
    /// change or remove a fixture; add one for each new version.
    const FIXTURES: [Fixture; SETTINGS_VERSION as usize] = [
        // 48 hours, which no choice is, becomes three days.
        (include_str!("../fixtures/settings-v1.txt"), || Settings {
            home_banner_dismissed_ms: None,
            ..sample()
        }),
        (include_str!("../fixtures/settings-v2.txt"), sample),
        (include_str!("../fixtures/settings-v3.txt"), sample),
    ];

    /// Settings with no default left in place.
    fn sample() -> Settings {
        Settings {
            theme: ThemeChoice::Dark,
            home_target: HomeTarget::Applications,
            wrap_around: false,
            recent_density: RecentDensity::TwoColumns,
            recent_window: RecentWindow::ThreeDays,
            hidden_apps: BTreeSet::from([app_id("games"), app_id("weather")]),
            image: ImageOptions {
                enabled: true,
                region: Some("en-GB".into()),
                use_mobile_data: true,
                show_card: false,
            },
            removed_from_recent: BTreeMap::from([
                (app_id("chat"), 1_763_251_200_000),
                (app_id("maps"), 1_763_254_800_000),
            ]),
            home_banner_dismissed_ms: Some(1_763_290_000_000),
        }
    }

    fn store() -> (Arc<FakeStorage>, PersistentSettingsStore) {
        let storage = Arc::new(FakeStorage::default());
        (storage.clone(), PersistentSettingsStore::new(storage))
    }

    fn change_all(settings: &mut Settings) {
        *settings = sample();
    }

    /// The body of a saved file, without its header and trailer.
    fn body(bytes: &[u8]) -> String {
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        lines[1..lines.len() - 1]
            .iter()
            .map(|line| format!("{line}\n"))
            .collect()
    }

    #[test]
    fn defaults_follow_the_srs() {
        let settings = Settings::default();

        assert_eq!(settings.theme, ThemeChoice::System); // THEME-01
        assert!(settings.wrap_around); // HOME-04
        assert!(!settings.image.enabled); // WALL-02
        assert_eq!(settings.recent_window, RecentWindow::TwentyFourHours); // REC-03, REC-11
    }

    #[test]
    fn saved_settings_are_there_after_a_restart() {
        let (storage, store) = store();

        let saved = store.update(&mut change_all).unwrap();

        assert_eq!(saved, sample());
        assert_eq!(*storage.writes.lock().unwrap(), [StoredFile::Settings]);
        assert_eq!(PersistentSettingsStore::new(storage).get(), sample());
    }

    #[test]
    fn settings_are_read_once_and_only_when_needed() {
        let (storage, store) = store();
        storage
            .files
            .lock()
            .unwrap()
            .insert(StoredFile::Settings, encode(&sample()));

        assert_eq!(store.get(), sample());
        storage.files.lock().unwrap().clear();
        assert_eq!(store.get(), sample());
    }

    #[test]
    fn a_change_that_panics_leaves_the_settings_unchanged() {
        let (storage, store) = store();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = store.update(&mut |settings| {
                settings.wrap_around = false;
                panic!("change failed part-way");
            });
        }));

        assert!(result.is_err());
        assert!(store.get().wrap_around);
        assert!(storage.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_save_leaves_the_previous_settings() {
        // REL-03
        let (storage, store) = store();
        store
            .update(&mut |settings| settings.theme = ThemeChoice::Dark)
            .unwrap();
        let failure = DataError::Failed("Not enough storage space".into());
        *storage.error.lock().unwrap() = Some(failure.clone());

        let result = store.update(&mut |settings| settings.theme = ThemeChoice::Light);

        assert_eq!(result, Err(failure));
        assert_eq!(store.get().theme, ThemeChoice::Dark);
        *storage.error.lock().unwrap() = None;
        assert_eq!(
            PersistentSettingsStore::new(storage).get().theme,
            ThemeChoice::Dark
        );
    }

    #[test]
    fn damaged_or_newer_settings_load_as_the_defaults() {
        // MAINT-04
        let saved = encode(&sample());
        let newer = seal("settings", SETTINGS_VERSION + 1, &body(&saved));
        let mut damaged = saved.clone();
        damaged[30] ^= 1;
        let cut_short = saved[..saved.len() / 2].to_vec();

        for bytes in [
            newer,
            damaged,
            cut_short,
            b"theme dark".to_vec(),
            Vec::new(),
        ] {
            let (storage, store) = store();
            storage
                .files
                .lock()
                .unwrap()
                .insert(StoredFile::Settings, bytes);

            assert_eq!(store.get(), Settings::default());
            store.update(&mut change_all).unwrap();
            assert_eq!(PersistentSettingsStore::new(storage).get(), sample());
        }
    }

    #[test]
    fn every_saved_version_still_loads() {
        // MAINT-04: one fixture per version, each brought up to today's settings.
        for (version, (fixture, expected)) in (1..).zip(FIXTURES) {
            assert_eq!(
                decode::<Settings>(fixture.as_bytes()),
                Some(expected()),
                "settings-v{version}.txt"
            );
        }
    }

    #[test]
    fn the_latest_fixture_is_what_is_saved_today() {
        let (fixture, expected) = FIXTURES[FIXTURES.len() - 1];

        assert_eq!(String::from_utf8(encode(&expected())).unwrap(), fixture);
    }

    #[test]
    fn every_choice_is_saved_by_name() {
        for theme in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
            for home_target in [HomeTarget::LastView, HomeTarget::Applications] {
                for recent_density in [RecentDensity::ThreeColumns, RecentDensity::TwoColumns] {
                    for recent_window in RecentWindow::ALL {
                        let settings = Settings {
                            theme,
                            home_target,
                            recent_density,
                            recent_window,
                            ..Settings::default()
                        };
                        assert_eq!(decode(&encode(&settings)), Some(settings));
                    }
                }
            }
        }

        let unknown = body(&encode(&Settings::default())).replace("theme system", "theme sepia");
        assert_eq!(
            decode::<Settings>(&seal("settings", SETTINGS_VERSION, &unknown)),
            None
        );
    }

    #[test]
    fn each_window_is_its_hours() {
        // REC-11
        let hours = RecentWindow::ALL.map(RecentWindow::hours);

        assert_eq!(hours, [12, 24, 72]);
        for window in RecentWindow::ALL {
            assert_eq!(RecentWindow::of_hours(window.hours()), Some(window));
        }
        assert_eq!(RecentWindow::of_hours(48), None);
    }

    #[test]
    fn an_older_window_becomes_the_shortest_choice_that_shows_as_much() {
        // MAINT-04, ADR 0026: the window saved in hours before version 3.
        let saved_at_v2 = |hours: &str| {
            let v3 = body(&encode(&Settings::default()));
            let v2 = v3.replace(
                "recent_window twenty_four_hours\n",
                &format!("recent_window_hours {hours}\n"),
            );
            seal("settings", 2, &v2)
        };

        for (hours, window) in [
            ("0", RecentWindow::TwelveHours),
            ("12", RecentWindow::TwelveHours),
            ("13", RecentWindow::TwentyFourHours),
            ("24", RecentWindow::TwentyFourHours),
            ("25", RecentWindow::ThreeDays),
            ("72", RecentWindow::ThreeDays),
            ("1000", RecentWindow::ThreeDays),
        ] {
            let settings = decode::<Settings>(&saved_at_v2(hours));
            assert_eq!(
                settings.map(|s| s.recent_window),
                Some(window),
                "{hours} hours"
            );
        }
        for not_hours in ["day", "-1", ""] {
            assert_eq!(decode::<Settings>(&saved_at_v2(not_hours)), None);
        }
    }
}
