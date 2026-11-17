//! What the whole app stores, composed with fake platform adapters: data survives a restart,
//! damaged or failing storage never stops the app, nothing from notifications or app usage is
//! written, and the system's backup keeps only the layout and the settings (REL-01, REL-03,
//! MAINT-04, PRIV-03, EVT-13, APPS-21).

mod backup;

use std::collections::BTreeSet;

use meegone_core_data::fakes::{Fakes, app, app_id};
use meegone_core_data::{
    Access, DataError, Notification, ProfileId, RecentUse, StoredFile, ThemeChoice,
};
use meegone_core_state::{Health, Theme};
use meegone_launcher::{AppMenuItem, CellKey, Direction, Launcher, ShellIntent};

fn grid(launcher: &Launcher) -> Vec<String> {
    let state = launcher.shell().apps().state();
    state
        .items
        .iter()
        .map(|item| item.label().to_owned())
        .collect()
}

fn stored_files(fakes: &Fakes) -> BTreeSet<StoredFile> {
    fakes
        .storage
        .files
        .lock()
        .unwrap()
        .keys()
        .copied()
        .collect()
}

#[test]
fn settings_and_the_layout_survive_a_restart() {
    // REL-03, APPS-18
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![app("phone", "Phone"), app("mail", "Mail")];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().settings().theme_chosen(ThemeChoice::Dark);
    launcher.frame();
    drop(launcher);

    // An app installed while MeeGone wasn't running goes after the saved layout.
    fakes.apps.apps.lock().unwrap().push(app("atlas", "Atlas"));
    let restarted = Launcher::start(fakes.platform());

    assert_eq!(restarted.shell().theme().state().theme, Theme::Dark);
    assert_eq!(grid(&restarted), ["Mail", "Phone", "Atlas"]);
    assert_eq!(
        stored_files(&fakes),
        BTreeSet::from([StoredFile::Layout, StoredFile::Settings])
    );
}

#[test]
fn hidden_apps_stay_hidden_after_a_restart() {
    // APPS-19: they are kept in the settings store, and nowhere else.
    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![app("phone", "Phone"), app("mail", "Mail")];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::OpenCellMenu(CellKey::App(app_id("mail"))));
    let revision = launcher
        .frame()
        .apps
        .menu
        .expect("Mail's menu is open")
        .revision;
    let items = launcher.view().apps().menu_items().to_vec();
    let hide = items.iter().position(|item| *item == AppMenuItem::Hide);
    assert!(
        launcher
            .view()
            .apps()
            .menu_item_chosen(revision, hide.expect("the menu offers Hide"))
    );
    launcher.frame();
    drop(launcher);

    let restarted = Launcher::start(fakes.platform());

    assert_eq!(grid(&restarted), ["Phone"]);
    let hidden = &restarted.shell().apps().state().hidden_apps;
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].label, "Mail");
    assert!(
        fakes
            .storage
            .stored_text()
            .contains("hidden 0 mail mail.Main")
    );
}

#[test]
fn damaged_files_load_as_defaults_without_a_crash() {
    // MAINT-04
    let fakes = Fakes::default();
    fakes.apps.apps.lock().unwrap().push(app("mail", "Mail"));
    for file in StoredFile::ALL {
        let damaged = b"meegone-\xff damaged".to_vec();
        fakes.storage.files.lock().unwrap().insert(file, damaged);
    }

    let launcher = Launcher::start(fakes.platform());

    let shell = launcher.shell();
    assert_eq!(*shell.theme().health(), Health::Ready);
    assert_eq!(*shell.apps().health(), Health::Ready);
    assert_eq!(*shell.recent().health(), Health::Ready);
    assert_eq!(*shell.lockimage().health(), Health::Ready);
    assert_eq!(*shell.settings().health(), Health::Ready);
    assert_eq!(shell.theme().state().theme, Theme::Light);
    assert_eq!(shell.lockimage().state().current, None);
    assert_eq!(grid(&launcher), ["Mail"]);
}

#[test]
fn failing_storage_never_stops_the_app() {
    // REL-01, REL-03
    let fakes = Fakes::default();
    fakes.apps.apps.lock().unwrap().push(app("mail", "Mail"));
    let full = DataError::Failed("Not enough storage space".into());
    *fakes.storage.error.lock().unwrap() = Some(full);
    let mut launcher = Launcher::start(fakes.platform());

    launcher.view().apps().icon_tapped(app_id("mail"), None);
    launcher.frame();
    assert_eq!(*fakes.apps.launched.lock().unwrap(), [app_id("mail")]);
    assert_eq!(grid(&launcher), ["Mail"]);

    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().settings().theme_chosen(ThemeChoice::Dark);
    launcher.frame();
    let settings = launcher.shell().settings().state();
    assert_eq!(
        settings.problem.as_deref(),
        Some("Not enough storage space")
    );
    assert_eq!(settings.theme, ThemeChoice::System);
    assert_eq!(launcher.shell().theme().state().theme, Theme::Light);
}

#[test]
fn notification_content_and_usage_history_are_never_stored() {
    // PRIV-03, EVT-13: of app usage, only the removal from Recent and its time are stored.
    const NOW_MS: u64 = 1_763_299_999_999;
    const USED_MS: u64 = 1_763_299_123_456;
    const POSTED_MS: u64 = 1_763_298_765_432;
    let fakes = Fakes::default();
    *fakes.system.now_ms.lock().unwrap() = NOW_MS;
    *fakes.apps.apps.lock().unwrap() = vec![app("chat", "Chat"), app("maps", "Maps")];
    *fakes.notifications.access.lock().unwrap() = Access::Granted;
    *fakes.notifications.active.lock().unwrap() = vec![Notification {
        key: "notification-key-1".into(),
        profile: ProfileId(0),
        package: "chat".into(),
        app_label: "Chat".into(),
        title: "Dinner with Robin".into(),
        text: "Table booked for eight".into(),
        posted_at_ms: POSTED_MS,
        clearable: true,
        ongoing: false,
        summary: false,
    }];
    *fakes.usage.access.lock().unwrap() = Access::Granted;
    *fakes.usage.uses.lock().unwrap() = vec![
        RecentUse {
            app: app_id("chat"),
            last_used_ms: USED_MS,
        },
        RecentUse {
            app: app_id("maps"),
            last_used_ms: USED_MS + 1,
        },
    ];

    let mut launcher = Launcher::start(fakes.platform());
    for _ in 0..3 {
        launcher.dispatch(ShellIntent::Swipe(Direction::Next));
        launcher.frame();
    }
    launcher.dispatch(ShellIntent::NotificationsChanged);
    assert_eq!(launcher.shell().events().state().groups.len(), 1);
    // The host draws the group's row, with what its notification says (EVT-04).
    launcher.frame();
    assert_eq!(
        launcher.view().events().rows()[0].title,
        "Dinner with Robin"
    );
    assert_eq!(launcher.shell().recent().state().cards.len(), 2);
    launcher.view().recent().remove_tapped(app_id("chat"));
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().settings().wrap_around_toggled(false);
    launcher.frame();

    let stored = fakes.storage.stored_text();
    assert!(
        stored.contains(&format!("removed 0 chat chat.Main {NOW_MS}")),
        "{stored}"
    );
    // So no backup holds them either (APPS-21).
    let backed_up: String = backup::back_up(&fakes.storage)
        .values()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect();
    assert!(
        backed_up.contains("removed 0 chat chat.Main"),
        "{backed_up}"
    );
    for private in [
        "notification-key-1".to_owned(),
        "Dinner".to_owned(),
        "Robin".to_owned(),
        "Table booked".to_owned(),
        POSTED_MS.to_string(),
        USED_MS.to_string(),
        (USED_MS + 1).to_string(),
    ] {
        assert!(
            !stored.contains(&private),
            "{private:?} was stored:\n{stored}"
        );
        assert!(
            !backed_up.contains(&private),
            "{private:?} was backed up:\n{backed_up}"
        );
    }
}

#[test]
fn the_backup_keeps_the_layout_and_the_settings_and_nothing_else() {
    // APPS-21: every rule, for every Android version and for transfers between devices, keeps
    // the same two files. The image cache stays on the device that downloaded its images.
    let only_those = BTreeSet::from([StoredFile::Layout, StoredFile::Settings]);
    assert_eq!(backup::backed_up_files(), only_those);

    let fakes = Fakes::default();
    *fakes.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::OpenSettings);
    launcher.view().settings().theme_chosen(ThemeChoice::Dark);
    launcher.frame();
    drop(launcher);
    fakes
        .storage
        .files
        .lock()
        .unwrap()
        .insert(StoredFile::ImageDetails, b"meegone-images 1\n".to_vec());

    let backup = backup::back_up(&fakes.storage);

    assert_eq!(backup.keys().copied().collect::<BTreeSet<_>>(), only_those);
    // Restored on another device, they are all MeeGone needs.
    let other = Fakes::default();
    *other.apps.apps.lock().unwrap() = vec![app("mail", "Mail")];
    *other.system.installed_ms.lock().unwrap() = 1;
    backup::restore(&other.storage, &backup);
    let restored = Launcher::start(other.platform());
    assert_eq!(restored.shell().theme().state().theme, Theme::Dark);
    assert_eq!(grid(&restored), ["Mail"]);
}
