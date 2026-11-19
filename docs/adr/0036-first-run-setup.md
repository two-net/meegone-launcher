# ADR 0036: First-run setup

**Status:** Accepted · **Date:** 2026-11-20 · **Issue:** #35

## Context

Issue #35 (Must) asks for a working home screen within a minute of installing:
- **At most four screens (ONB-01):**
  1. Welcome.
  2. Make MeeGone the default home app, with the system's dialog.
  3. The daily lock-screen image: a preview of today's image when online, the switch (on), Region (Automatic) and Continue.
  4. Optional access: Notifications for Events and Usage for Recent, each with Allow and Not now.
- **One plain sentence before each access request (ONB-02).** Declining never blocks launching apps.
- **Under 60 seconds** for a user who accepts every default (ONB-03).
- **No theme question (ONB-05):** System applies, and Settings changes it.
- **Nothing on the lock screen before Continue (WALL-02).**
- **Applications afterwards, in alphabetical order (HOME-05, APPS-04).**
- **Acceptance:**
  - AC-01: a fresh install on Wi-Fi, accepting every default, ends with MeeGone as the default home app, the System theme, Applications showing, and today's Bing image on the lock screen within 60 s.
  - Not now on both access requests still completes setup, and apps launch normally.

What was there already:
- **In Settings:**
  - `SettingsIntent::RequestDefaultHome`, `OpenNotificationAccess` and `OpenUsageAccess`, with each access's status and the written instructions where a system screen can't open (ADR 0035).
  - The ONB-04 banner's state, `SettingsState.home_banner`, and its `HomeBanner` slot. The host never drew it, so a debug-only adb extra stood in for its button (ADR 0005, decision 10).
- **In the lock-screen image:**
  - `LockImageIntent::SetEnabled`, which ADR 0030, decision 7, kept for setup's Continue.
  - The daily image off by default, and nothing checked, fetched or set while it is off.
- **In Applications:** the first grid. With no saved layout, the default phone, messaging, browser and camera apps come first where the system names them, then every app alphabetically (APPS-04, APPS-05, ADR 0012).
- **In the shell:** Applications as the view after a start (HOME-05).

What was missing:
- **No record of setup.** Nothing said whether it had run, and no setup state, screen or intent existed.
- **No preview while the image is off.** Browsing is offered only while the daily image is on (ADR 0032).
- **Turning the image on didn't check at once.** It only schedules the job, and JobScheduler gives no deadline, so 60 s wasn't assured.

## Decision

1. **Setup belongs to Settings and setup (SRS 7.3).**
   - `SettingsState.setup` holds the step showing: Welcome, the default home app, the lock-screen image or access.
   - It also holds the accesses put off with Not now.
   - It lives in memory only. Leaving MeeGone part-way, or the process ending, starts setup again at Welcome. Nothing was applied by then.
   - **Steps already done are skipped:**
     - The default home app, while MeeGone is already it, as when it was picked at the Home key.
     - Access, while both are already allowed.
   - **Each answer moves setup on:**
     - Continue on Welcome and on the image step.
     - At the default-home step, `Resumed` after the system's dialog, once MeeGone holds the role.
     - At the access step, once each item is allowed or put off. Setup then ends; there is no Done button.
   - **Back** returns to the step before, skipping the default-home step while MeeGone holds the role. It does nothing on Welcome.
   - **Home** leaves setup as it is.
   - **Reset launcher leaves setup alone.** Setup is not a setting.
2. **Once per install.** The settings file records which install finished setup.
   - **The record.** `Settings.setup_install_ms` (`SETTINGS_VERSION` 4) holds that install's time (`SystemConfigAdapter::installed_ms`). 0 stands for any install. Setup is done when the record is 0 or this install's time (`setup_done`).
   - **Before version 4.** The migration writes 0, so an install used before setup existed never sees it.
   - **After a restore.** Settings restored from another device's backup carry that install's time, so setup runs again on the new device (ADR 0018). That device has no role and no access yet. The restored options stay as they were, the theme too.
   - **A save that fails** still ends setup for the session, and its problem shows. Setup then runs again at the next start.
   - **Not an option.** SET-04's count stays 10.
3. **The lock-screen image step is that feature's own (ADR 0002).**
   - **Placement.** `ImageSetupStep` is a `Slot` of the lock-screen image's UI, which the shell puts into Settings' `SetupView`, as its Settings section is put in.
   - **Opening it.** When setup reaches the step, the shell sends `LockImageIntent::OpenSetup`. This makes a draft whose switch is on and saves nothing (WALL-02). `SetupSwitched` changes only the draft.
   - **Today's image (ONB-01).** `OpenSetup` queues `ImageTask::PreviewToday` for the job's thread. `ImageUpdater::preview_today` then:
     - makes one request for today's image in the region in use, and fetches its preview only if the image is offered as a wallpaper (LEGAL-04);
     - asks nothing while offline, so the step then shows no image;
     - keeps the result in memory (`ImageMemory.today`) for that region and day;
     - never touches the cache, the check log, the job or the lock screen, and isn't one of the day's checks.
   - **The step shows** today's preview, its title and its attribution in full (LEGAL-03), the switch, and Region as Settings words it. A region chosen there is saved as in Settings, and the step reads today's image again for it.
   - **Continue.** At the image step, the shell sends `ConfirmSetup` to the lock-screen image before Settings moves on.
     - With the switch on, the image is turned on as Settings' switch does, scheduling the job. Then `ImageTask::Check` runs the daily check at once on the job's thread, within its rules (WALL-03, WALL-04, PWR-04). On Wi-Fi the image is on the lock screen within seconds (ONB-03). The job, sharing the thread and the updater's lock, finds the check not due.
     - With the switch off, nothing changes. An image restored as on is turned off.
     - Either way, today's preview is dropped.
   - **Settings' switch is unchanged.** It only schedules, as ADR 0030 says.
   - **Where it can't run:**
     - Where the device can't set the wallpaper (WALL-13), the switch is disabled with Settings' reason, nothing is read, and Continue applies nothing.
     - While the lock-screen image isn't working (ARCH-09), the step offers Continue alone.
4. **Access (ONB-02).**
   - **Each item** has its one sentence, Allow and Not now:
     - "MeeGone reads your notifications only to list them in Events, and they never leave this phone."
     - "MeeGone reads which apps you used recently only to show them in Recent, and that never leaves this phone."
   - **Allow** is Settings → Access's shortcut, so a device without the system screen shows the written instructions below the item (SRS 8.1).
   - **Once answered,** an item shows its status, or "Not now. You can allow it later in Settings."
   - **The default-home step's Not now** counts as today's answer to the banner, as a request does (ADR 0005). The banner doesn't greet the user the moment setup ends.
5. **The end of setup.**
   - The shell closes everything and shows Applications at its top (HOME-05). `close_all` is the part of Reset launcher that does this.
   - The grid is the first one Applications built at the first start (APPS-04, APPS-05).
   - The theme is System, untouched (ONB-05).
6. **Setup covers the home views.**
   - **No input reaches the home.** While it shows, swipes, long-presses, menus, search and the details and past-week screens are refused.
   - **The system's settings entry** brings MeeGone forward with setup still showing: Settings never opens over it, where its options could conflict with the draft.
   - **ARCH-09.** A failed Settings ends setup, and the home shows, never covered for good.
7. **The banner is drawn (ONB-04).**
   - **What it is.** `HomeBannerView` is a flat card at the bottom of the home views: "MeeGone isn't your home screen yet.", with Dismiss and Make default.
   - **When it shows.** `HomeShell::banner_shown` shows it while Settings says so and nothing covers the views: no setup, Settings, screen or menu, no open folder or search, and no edit or close mode.
   - **Touches.** A long-press on it opens no home menu.
   - **The debug hook goes,** as ADR 0005 planned: the `REQUEST_HOME` extra and `nativeRequestDefaultHome`. `DebugHooks` keeps its legacy-icons extra.
8. **The host contract.**
   - **`HomeFrame` gains:**
     - `setup: Option<SetupFrame>`;
     - `setup_image_switch: Option<DailyImage>`;
     - `setup_image: Option<u32>`;
     - `home_banner: bool`.
   - **The frame grows to 155 slots:**

     | Slot | Holds |
     |---|---|
     | 150 | The step: 0 Welcome, 1 the default home app, 2 the lock-screen image, 3 access, or -1 |
     | 151 | The put-off bits: 1 notifications, 2 usage |
     | 152 | The image step's switch: 0 off, 1 on, 2 unavailable, or -1 for Continue alone |
     | 153 | The image step's revision |
     | 154 | The banner |

     The accesses' status and instructions stay in slots 145 to 147.
   - **New natives on `HomeViews`:**
     - `nativeSetupContinue()`;
     - `nativeSetupNotNow(int)`;
     - `nativeSetupImageSwitch(boolean)`;
     - `String[] nativeSetupImage()`;
     - `byte[] nativeSetupImagePreview(int)`;
     - `nativeHomeBannerMakeDefault()` and `nativeHomeBannerDismissed()`.
   - **Reused natives:**
     - Make default and Allow use Settings → Access's.
     - Region uses `nativeRegions` and `nativeRegion`, read when the image step shows.
   - **`SetupScreens`** draws the steps above every other screen, below the menu. It uses the home views' background and `SettingsRows`' rows.
     - Each step is a pane with its own title, so screen readers announce it.
     - The preview is decoded on `ImageCard`'s thread at the size shown.
   - **Tests.** `Fakes::after_setup()` gives a device where setup is done, so the tests of everything else start on the home.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0005, decisions 6 and 10 | The banner is drawn, over the home views only, and the debug `REQUEST_HOME` extra is gone. |
| 0006, decision 2 | The frame has 155 slots. |
| 0007 | During setup, Back returns to the step before and Home leaves setup as it is. |
| 0018 | Settings restored from another install run setup again on the new device. |
| 0029, decision 2 | Besides the job and each start, the check runs once more right after setup's Continue, within its rules. |
| 0030, decision 7 | Setup reads today's image and its preview while the daily image is still off; nothing else is fetched, and nothing is set, before Continue. |
| 0032 | Previews also show on setup's image step, kept in `ImageMemory.today`, and two tasks join the queue: `PreviewToday` and `Check`. |
| 0035, decisions 5 and 8 | During setup the system's settings entry shows setup, not Settings. Slots 150 to 154 follow 149. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| SET-01: Settings reachable from the system's settings entry | Not while setup shows; it shows setup instead. |
| ONB-04: the banner at most once a day | Setup's Not now on the default-home step counts as that day's answer, as a declined request does (ADR 0005). |
| SRS 8.2: one request for today's image per check | Setup adds one request for today's image and one for its preview, outside the day's budget, as browsing the past week does. |

## Consequences

- **Benefits:**
  - **Tested without a device:**
    - The settings file: version 4 and its fixture; older versions migrate to "done on any install"; `setup_done` for every case.
    - Settings:
      - a fresh install starts at Welcome;
      - every step in order, done for this install, and not again after a restart;
      - the skips and Back;
      - Not now on both ends setup;
      - the default-home step's Not now answers the banner;
      - setup runs again for another install's settings;
      - a failed save still ends it;
      - Reset launcher keeps it done.
    - The lock-screen image:
      - the step starts on and saves nothing;
      - today's image is read while off, once a day and region, never offline or where the wallpaper can't be set, and without a preview when not offered;
      - Continue turns the image on and checks at once, or with the switch off changes nothing.
      - The step's revision and preview in the UI.
    - The shell:
      - setup covers the home: no swipe, menu, Settings or Home;
      - Back steps back;
      - the image step opens with setup, and Continue confirms it first;
      - the end shows Applications at its top;
      - a failed Settings uncovers the home;
      - a failed lock-screen image leaves Continue alone;
      - the banner only on the uncovered views.
    - **The acceptance scenarios** (`tests/acceptance.rs`):
      - AC-01 automated: the role, the System theme, Applications at its top in alphabetical order, and today's image on the lock screen with no time passing;
      - ONB-01 and ONB-05: the four steps, with the image step's content;
      - ONB-02: Not now on both, then an app launches;
      - WALL-02: setup left before Continue, then a restart: nothing set, cached or scheduled, and only the feed and the preview asked;
      - setup once per install, and again after a restore from another install.
    - `tests/home_role.rs` checks the banner as drawn: never during setup, and setup's answer counts for the day.
  - **Built.** `cargo clippy` for `aarch64-linux-android`, and `assembleDebug` and `assembleRelease`.
  - **Checked on a Galaxy S22 (One UI 8, Android 16)** with the debug build:
    - **Installed over the previous build,** with settings at version 3: no setup, and the home as it was.
    - **After `pm clear`,** setup showed Welcome. MeeGone still held the home role, so Continue went straight to the image step.
    - **The image step:**
      - Today's en-GB image wasn't offered as a wallpaper (`wp` false), so the step showed its title and attribution without a preview (LEGAL-04).
      - Choosing Japanese (Japan) in the Region dialog over setup showed that region's preview, title and attribution.
      - Continue put today's image on the lock screen about 3 s later, on Wi-Fi.
    - **The access step:**
      - With notification and usage access turned off with adb first, it showed both sentences.
      - Allow opened MeeGone's page of the system's notification-access screen; once granted, the item read "Allowed. Events lists your notifications."
      - Not now on usage ended setup on Applications at its top: the default phone, messaging, browser and camera apps first, then A to Z.
      - The settings file was at version 4, with this install's time.
  - **Not checked on the device:**
    - the default-home step and the system's role dialog from setup, as MeeGone held the role;
    - the banner, for the same reason;
    - Back through the steps;
    - TalkBack;
    - Android 10 to 15.

    The device matrix (#43) covers these.
- **Costs:**
  - **A damaged or unreadable settings file loads as the defaults (MAINT-04),** so setup runs again. Finishing it then saves the defaults over the file.
  - **Settings restored with the daily image on** may set today's image at MeeGone's first start, before setup's image step. That step can turn it off.
  - **Granting access in split screen,** without leaving MeeGone, is seen at the next `Resumed`.
  - **Kept in step by hand:**
    - slots 150 to 154 between `frame_slots`, `HomeViews` and `SetupScreens`, with `setup_step_code` and `access_item_code`;
    - the fields of `nativeSetupImage`;
    - the new natives with `HomeViews.java`.
