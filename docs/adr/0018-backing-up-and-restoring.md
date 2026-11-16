# ADR 0018: Backing up and restoring the layout and settings

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #17

## Context

Issue #17 brings the home screen along to a new device, with no account of MeeGone's own (APPS-21, Should):
- The layout and settings take part in the system's standard backup and restore.
- After a restore, apps that aren't installed are skipped, and installed apps the backup doesn't hold join the end.
- Notification content and usage history are never in a backup, since they are never stored (PRIV-03).

What was there already:
- The manifest had `allowBackup="false"`. For an app targeting Android 12 or later, that doesn't stop a transfer between devices, so a cable or Wi-Fi transfer copied every file MeeGone keeps, the image cache included.
- Three stores, each one file in `getFilesDir()`, each read once per process (ADR 0004).
- `reconcile` drops a saved app that isn't installed at once, unless its profile can't list its apps yet (ADR 0012).

The constraints:
- **The other apps come after MeeGone.**
  - The system restores MeeGone's files as it installs MeeGone, before MeeGone first runs.
  - The Play Store then installs the other apps one by one, in no fixed order, over minutes or hours, some only once there is Wi-Fi.
  - Taking the catalogue of the first start as final would drop most of the backup. Each app would then come back at the end with a dot.
- **Profile ids are the system's serial numbers** (`UserManager.getSerialNumberForUser`). The personal profile's is normally 0 on every device. A work profile's differs from one device to another.
- **MeeGone requests no permissions** (SEC-03). The system shows a launcher pending installs (`LauncherApps.getAllPackageInstallerSessions`) only while it is the default home app, which a restored MeeGone may not be yet.
- **Exactly three stores** (SRS 7.1), standard-library crates, and OS access only through adapters (ARCH-04).

## Decision

1. **The system backs up two files.**
   - The manifest sets `allowBackup="true"`. It gives the rules in two forms:
     - `fullBackupContent="@xml/backup_rules"` for Android 10 and 11;
     - `dataExtractionRules="@xml/data_extraction_rules"` from Android 12, with `<cloud-backup>` and `<device-transfer>`.
   - Every rule includes `layout` and `settings` in the `file` domain. Listing them leaves everything else out:
     - the image cache, which belongs to the device that downloaded its images;
     - any `.tmp` file a crash left behind.
   - **No backup agent.** Auto Backup copies whole files, which atomic saves keep consistent (REL-03), and restores them at install, before MeeGone runs.
   - **No requirement for client-side encryption.**
     - Android already encrypts a backup end to end when the device has a screen lock.
     - Requiring it would leave a device without one with no backup at all.
   - `tests/backup/mod.rs` reads both rule files at compile time. It checks that every section names exactly `StoredFile::Layout` and `StoredFile::Settings`. The whole-app tests back up and restore through it, so they copy exactly what a device would.
2. **A restore is recognised by MeeGone's install time.**
   - `SystemConfigAdapter::installed_ms` is this copy's `PackageInfo.firstInstallTime`, or 0 if the system can't say. A reinstall or a new device gives a new time. `Bridge.installedMs` answers it.
   - Every layout Applications saves records that time. A layout whose time is another copy's came from a backup.
   - That layout is saved at once with this copy's time, so it is restored only once.
   - A layout saved before format 3 has no time, so it is this copy's. It is rewritten at the first refresh after the update, so no backup holds a layout without one.
3. **Layout format 3.**
   - Two records follow the `new` records:
     - `installed <ms>`;
     - `restore <until_ms> <apps>`.
   - Each stands alone, so recovery keeps it (REL-02).
   - The migration from version 2 changes nothing. `layout-v3.txt` joins the fixtures.
4. **A restore waits a week for its apps.**
   - At a restore (`Start::Restored`), some saved apps become the apps the restore waits for, until now plus `RESTORE_WAIT_MS` (7 days). They are those of a listed profile that aren't installed and that no update renamed.
   - **A waiting app keeps its place,** in its folder too, out of sight, as a locked profile's apps do.
     - It has no cell, search result or menu.
     - Edit mode keeps its slot.
     - A folder whose other apps wait stays a folder, as with hidden apps (ADR 0017).
   - **Installed during the wait, it shows in its place,** without a dot. Installed under another launcher activity, it takes the place by the rename rule (APPS-08).
   - **When the wait ends** (`end_restore`, at the next refresh):
     - apps still missing leave;
     - a folder left with one app turns back into it (FOLD-06);
     - installed later, an app is new at the end (APPS-06).
   - **Apps the backup doesn't hold** join the end at the restore, alphabetically and without a dot: they were on the device before the backup arrived. Later installs join as usual, with a dot.
   - **A hidden app stays hidden** while the restore waits for it (`follow_hidden`).
   - **A damaged restored layout** is recovered as any other (REL-02), with no wait.
   - **Why a week.**
     - The system usually installs everything within hours.
     - A week also covers apps that wait for Wi-Fi, and apps the user installs again by hand, such as those from other stores.
     - A waiting app costs nothing: it is never seen.
5. **Settings are restored whole.**
   - That is the theme, the Home target, wrap-around, hidden apps, Recent's options and removed apps, the daily image's options, and when the home-app banner was dismissed.
   - The removed apps are the one usage-related item PRIV-03 lets MeeGone store. Usage history and notification content have no file, so no backup can hold them.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0004, decisions 2 and 3 | The system backs up the layout and settings files, not the image cache. |
| 0004, decision 6 | The layout format is version 3, adding `installed` and `restore` records. |
| 0012, decision 4 | A restore's apps that aren't installed keep their places for 7 days. Apps joining at a restore come alphabetically, unmarked. |
| 0017, decision 3 | `follow_hidden` keeps a hidden app while a restore waits for it. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| APPS-06, APPS-07: a new install joins the end with a dot | Within 7 days of a restore, a backed-up app takes its old place, without a dot. |
| APPS-21 with APPS-20: work-profile apps | The old device's work apps are skipped: a profile's id is the system's serial number, which differs on another device. The new work profile's apps join the end, and a work app hidden on the old device shows. |
| PRIV-05: uninstalling removes everything MeeGone stored | MeeGone's own files still go. The system's copy of the backup lives in the user's Google account, under the system's rules, and outlives an uninstall. The user sees and deletes it in the system's backup settings. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - Every section of both rule files names exactly the layout and settings files, and a backup with an image cache holds only those two.
    - A restore: the waiting apps, profiles the new device lacks, apps already installed, apps arriving in folders, renamed activities, the end of the wait, hidden apps, and properties over 2,000 generated restores.
    - The holder saves a restore with this copy's time, recognises it only once, and ends the wait at its time.
    - Format 3: every fixture loads, and recovery keeps the new records.
    - The issue's acceptance criterion and a late app, in `tests/acceptance.rs`, and the PRIV-03 check of the backed-up bytes, in `tests/storage.rs`.
  - A transfer between devices no longer copies the image cache.
  - Checked on a Galaxy S22 (Android 16), with MeeGone as the default home app, through the system's local backup transport:
    - The update rewrote the user's 70-cell version 2 layout as version 3, with MeeGone's install time and every cell unchanged.
    - `bmgr backupnow` backed MeeGone up while it was the home app.
    - Uninstalling MeeGone and installing it again restored exactly `files/layout` and `files/settings`, byte for byte, before MeeGone first ran. No image cache or temporary file came back.
    - One app was disabled to stand in for one not installed yet, keeping its data. MeeGone's first start recognised the restore and saved it with the new install time. It kept that app's place out of sight, waiting until 7 days later.
    - Enabling the app put it back in its old place, without a dot, and ended the restore. The grid showed the old order.
    - The restored files' modification time was in 1970, so a file's time couldn't have told a restore.
- **Costs:**
  - Not checked on a device yet: a transfer between devices, a restore from the cloud with the Play Store installing apps, Android 10 and 11's `backup_rules.xml`, and a work profile. The device matrix (#43) covers them.
  - Every layout carries this copy's install time, and a version 2 layout is rewritten once after the update.
  - The wait is a guess. An app that arrives after it lands at the end, with a dot.
  - A restore isn't recognised if the system gives 0 for MeeGone's install time on both devices. Its missing apps then leave at the first start, as before.
  - The system decides when to back up, about once a day while the device is idle, charging and on Wi-Fi. It keeps up to 25 MB per app; MeeGone's two files take a few kB.
  - Android restores no backup from a newer version of MeeGone, so a newer format never arrives by restore. If one did, MAINT-04 would still load it as defaults.
  - The privacy notice (PRIV-04) has to say that the layout and settings join the system's backup.
