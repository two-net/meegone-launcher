# ADR 0035: The Settings screen

**Status:** Accepted · **Date:** 2026-11-20 · **Issue:** #34

## Context

Issue #34 (Must, with one Should) asks for a short, honest Settings screen: few options, applied instantly.
- **Reachable (SET-01)** from the home menu (HOME-10) and from the system's home-app settings entry, where the platform has one.
- **Only these groups (SET-02):**
  - Appearance: Theme.
  - Home: Home button opens, Wrap-around.
  - Applications: Hidden apps.
  - Recent: Time window.
  - Lock-screen image: Daily image, Region, Use mobile data, Show card in Events, Update now, Reset lock screen to home wallpaper, the last-update status.
  - Access: the default home app, notification access and usage access, each with its status and a shortcut, or written instructions where there is no shortcut (SRS 8.1).
  - About.
- **Instant (SET-03):** there is no Save button.
- **At most 12 options (SET-04, SRS 7.1).**
- **About (SET-05)** shows:
  - the version;
  - the open-source licences of every third-party component (LEGAL-06);
  - the privacy notice (PRIV-04);
  - "MeeGone Launcher is an independent project and is not affiliated with or endorsed by Nokia or Microsoft." (LEGAL-05).
- **Reset launcher (SET-06, Should)** restores the default layout and settings after confirmation.
- **Acceptance:** any change shows on returning home without a restart, and the build has 12 options or fewer.

What was there already:
- **Settings covered the home views** as one scrolling column, but drew only what earlier issues needed:
  - Theme (ADR 0033);
  - Hidden apps (ADR 0017);
  - Time window (ADR 0026);
  - the lock-screen image's items (ADR 0022, ADR 0029 to ADR 0032), without Region.

  Opened at the Lock-screen image group, it only showed the group's name below its own.
- **In the Rust app,** most of the rest:
  - `SettingsIntent::{SetHomeTarget, SetWrapAround, OpenNotificationAccess, OpenUsageAccess, RequestDefaultHome}`;
  - the access status in `SettingsState`;
  - `ImageSettingsSection::{regions, region, automatic_region, region_chosen}` (ADR 0027);
  - `ShellIntent::OpenSettings`, which only tests sent.

  The frame had no slots for any of them, and no natives reached them.
- **Not there at all:**
  - nothing answered the system's settings entry;
  - an access shortcut that failed became a problem message like any other, not instructions;
  - nothing could restore defaults: each feature saved only its own changes.
- **SET-04's test** already counted 10 options (ADR 0017), and every one of them was in place.

## Decision

1. **One screen, its groups in SET-02's order.**
   - Settings stays one `ScrollView` over the home views, not a list of sub-screens: ten options and three accesses fit on a few screens.
   - The groups are Appearance, Home, Applications, Recent, Lock-screen image, Access and About. Each shows only while the feature behind it works (ARCH-09).
   - Opened at the Lock-screen image group (HOME-10, WALL-15), it scrolls that group's heading to the top once laid out, and gives it screen readers' focus. The group's name below Settings' own goes.
   - Every group is built from `SettingsRows`: a heading, switches, actions, notes, a message with a button, and a title with its value below. `ImageSettings`' private builders moved there, so the groups look and read alike. `ChoiceSettings` can go without a heading of its own, inside a group that has one.
2. **Each group belongs to the feature whose fields it changes (ADR 0002).**
   - **Settings' own:** Appearance, Home (`HomeSettings`: "Home button opens" as a `ChoiceSettings` of the last view or always Applications, and Wrap-around) and Access.
   - **The shell puts in the rest,** as before: Applications' Hidden apps, Recent's time window and the lock-screen image's section.
   - **About is the host's own text,** in its resources, so it is translated with them (L10N-01). The version is the installed package's `versionName`.
3. **Region (WALL-05)** is a row in the lock-screen image's section: "Region", and below it the choice, such as "Automatic (English (United Kingdom))".
   - A tap opens a single-choice dialog of Automatic and the feed's regions, each named with `Locale.getDisplayName`. Thirteen radio buttons inline would have made the group longer than the rest of Settings.
   - `nativeRegions` gives Automatic's tag, then the feed's, and is read each time Settings opens, as the device's language may have changed. `ImageSettingsSection::region_choice()` gives the place of the chosen region in that list.
   - The section's items follow SET-02's order: the switch with its status and messages, Region, Use mobile data, Show card in Events, then the actions.
4. **Access: status, shortcut, or instructions (SET-02, SRS 8.1).**
   - Each item shows its status, such as "Not allowed. Recent can't show the apps you used.", and a button: Make default or Allow, and Change once granted. Screen readers hear "Usage access: Allow".
   - The buttons send `RequestDefaultHome`, `OpenNotificationAccess` and `OpenUsageAccess`.
   - When the system has no screen to open, the adapter answers `DataError::Unsupported` with written instructions. `SettingsHolder` keeps them in `SettingsState.instructions` with their `AccessItem`, rather than as a passing problem. They show below that item until Settings opens again or MeeGone comes back to the foreground, when the status is read again too.
   - Bridge's instructions are string resources now, the default home app's moved out of the code. The notification- and usage-access ones say where to go, as Events and Recent show them too.
5. **The system's settings entry (SET-01).**
   - `HomeActivity` answers `android.intent.action.APPLICATION_PREFERENCES`. That is the gear beside MeeGone in the system's Default apps → Home app, and its App info's settings link.
   - The intent brings the one home task forward. `nativeOpenSettings` sends `OpenSettings`, and Settings shows at its top, over whatever was open.
   - It is the "settings entry" SEC-04 allows other apps to reach.
   - Back closes it to the home views, as Back always closes Settings. The home task can't hand Back to the system screen it came from.
6. **About (SET-05).**
   - The version.
   - The privacy notice, which says exactly what PRIV-01 to PRIV-03 and PRIV-05 promise, and that the layout and settings join the system's backup (ADR 0018).
   - LEGAL-05's statement, word for word.
   - **"Open-source licences"** opens a dialog (LEGAL-06):
     - MeeGone ships no library but the Rust standard library and the crates it is built with (DEPENDENCIES.md). The dialog names them, under MIT or Apache-2.0, then gives the MIT licence and the Apache License 2.0 in full.
     - Rust's own copyright notice puts `core`'s Unicode tables under the Unicode License v3, so that licence is given too.
     - The texts are `res/raw/licence_*.txt`, copied from the toolchain's `share/doc/rust/licenses`, with the MIT licence's holder filled in as Rust's notice gives it.
     - MeeGone's own licence isn't stated: the repository has none yet.
7. **Reset launcher (SET-06).**
   - **Where:** the last item of About, offered only while Settings, Applications, Recent and the lock-screen image all work. A reset is never partial (ARCH-09).
   - **Asking first:** the host asks "Reset launcher?". The dialog says what happens:
     - Applications goes back to its first order, without folders and with hidden apps shown;
     - every setting goes back to its default;
     - the daily image turns off and the lock screen keeps its picture;
     - the access the user allowed stays.
   - **Each feature restores only what it writes (ADR 0002).** Confirmed, `SettingsIntent::ResetLauncher` reaches the shell, which sends each feature its own reset:
     - **`AppsIntent::Reset`** clears the hidden apps first, so a failed save changes nothing. It leaves edit mode, closes the menu, folder, search and a rename, and scrolls to the top. It then builds the layout as a first start does (`Start::First` with the default apps, nothing marked new) and saves it. If the apps can't be listed just then, the next refresh that can list them builds it.
     - **`RecentIntent::Reset`** restores the density and the window, forgets the removals, and leaves close mode, in one save.
     - **`LockImageIntent::Reset`** restores the image options in one save. Off is applied as the switch applies it: the job is cancelled, and the cache and the past week are cleared. The lock-screen wallpaper stays (WALL-12).
     - **`SettingsIntent::ResetLauncher`** restores the theme, the Home target and wrap-around. When the banner was answered is not a setting, and stays.
   - **Afterwards:**
     - the shell closes Settings and everything open;
     - it refreshes the theme, which cross-fades to the default (THEME-06), and the carousel;
     - it shows Applications at its top.
     - The host shows "Launcher reset", or the first feature's problem.
8. **The host contract.**
   - **`HomeFrame` gains:**
     - `home_target: Option<HomeTarget>` and `wrap_around: Option<bool>`;
     - `access: Option<AccessStatus>`, with each item's status and the item showing instructions;
     - `region: Option<usize>`, 0 for Automatic;
     - `reset_offered: bool`.
   - **The frame grows to 150 slots:**
     - slot 142 is the Home target: 0 the last view, 1 Applications;
     - slot 143 is wrap-around;
     - slots 144 to 146 are the default home app, notification access and usage access;
     - slot 147 is the item showing instructions;
     - slot 148 is the region's place;
     - slot 149 says Reset launcher is offered.

     Each is -1 while its feature isn't working.
   - **New natives:**
     - `HomeActivity.nativeOpenSettings()`;
     - on `HomeViews`, each returning null or a message:
       - `nativeHomeTarget(int)` and `nativeWrapAround(boolean)`;
       - `nativeDefaultHome()`, `nativeNotificationAccessSettings()` and `nativeUsageAccessSettings()`;
       - `nativeRegion(int)`;
       - `nativeResetLauncher()`.
     - Also on `HomeViews`: `String nativeAccessInstructions()` and `String[] nativeRegions()`.
   - **Dialogs over Settings:** the regions, the licences and Reset launcher's question. They use the device's light or dark dialog theme matched to MeeGone's, with MeeGone's accent, as the rename dialog does (ADR 0033), and close with Settings.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 150 slots. |
| 0008, decision 5 | Settings is drawn. Opened at a group, it scrolls to the group's heading instead of naming the group below its own. |
| 0017, decision 7 | Hidden apps sits in Applications' place among all of SET-02's groups, after Home. |
| 0022, decision 6 | "Show card in Events" follows Region and "Use mobile data", in SET-02's order. |
| 0026, decision 5 | Time window sits in Recent's place among all of SET-02's groups. |
| 0027, decision 3 | Settings draws the Region picker, through `ImageSettingsSection::region_choice()`. |
| 0033, decision 8 | Theme is the first of all of SET-02's groups. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| SET-01: reachable from the system's home-app settings entry | It is, and Back then closes Settings to the home views rather than returning to the system screen. |
| SET-02: "only these groups" | Reset launcher (SET-06) has no group of its own, so it is the last item of About. |
| LEGAL-06: licences for all third-party components | The Rust standard library and the crates built into it are listed as one component, with their licences in full, as Rust's own notice groups them. |
| PRIV-04: the privacy notice in the store listing | Only in About: there is no store listing yet. |
| THEME-03: dialogs follow the theme | As ADR 0033: the device's light or dark dialog with MeeGone's accent. The region list's radio buttons are the dialog's own. |

## Consequences

- **Benefits:**
  - **Tested without a device:**
    - Settings: its reset leaves every other feature's fields alone; an access shortcut without a system screen leaves instructions under its item, cleared by `Refresh`; any other failure is a problem.
    - Applications: reset gives a first start's grid, nothing hidden or new, after a restart too; a failed save changes nothing; a reset while the apps can't be listed happens at the next refresh.
    - Recent: reset in one save, removed apps back; a failed save keeps everything.
    - The lock-screen image: reset turns the image off with no job or cache, and leaves the lock screen; a failed save says why.
    - The shell: reset restores every feature's defaults, closes Settings onto Applications at its top and fades the theme; it isn't offered, and does nothing, while Recent has failed; `OpenSettings` covers edit mode.
    - **The acceptance scenarios** (`tests/acceptance.rs`):
      - SET-01: from the home menu and the system's entry;
      - SET-02: every group in the frame, with each access's status, the shortcuts, the instructions and their clearing, and the region;
      - SET-03: theme, wrap-around and Home target taking effect on the home at once;
      - SET-06: the layout and every option back to their defaults, after a restart too.
    - SET-04's count is still 10, as no field was added to the settings file.
  - **Built.** `cargo clippy` for `aarch64-linux-android`, and `assembleDebug` and `assembleRelease`.
  - **Checked on a Galaxy S22 (One UI 8, Android 16)** with the debug build:
    - `am start -a android.intent.action.APPLICATION_PREFERENCES -p io.github.twonet.meegone` opened Settings at its top over the home; `cmd package query-activities` resolves it to `HomeActivity`.
    - Every group drew in SET-02's order, with the region as "Automatic (English (United Kingdom))", each access's status, and the version 0.1.0.
    - The region dialog listed Automatic and the feed's twelve regions with Automatic checked; the licences dialog showed the intro and the texts; Reset launcher's question showed. Each was cancelled, so nothing on the phone was reset.
    - The home menu's Lock-screen image, with no image yet, opened Settings scrolled to that group. Back closed Settings onto the home views.
    - Each Access button's accessibility name read like "Notification access: Change".
  - **Not checked on the device:** a confirmed reset, which would have reset the phone's own layout; the system's gear itself, reached here through `am start`; the instructions, as the phone has every screen; TalkBack in full; Android 10 to 15. The device matrix (#43) covers these.
- **Costs:**
  - **Several saves, not one.** A reset is four features' saves, so a failure part way leaves some features reset. Each reports its problem, and the first one shows.
  - **The licence texts** add about 13 KB to the APK. They must be updated if the Rust standard library's licences ever change.
  - **Kept in step by hand:**
    - slots 142 to 149 between `frame_slots` and `HomeViews`, with the Home target's and the Access items' numbers between `home_target_code`, `access_item_code`, `HomeSettings` and `AccessSettings`;
    - the new natives with `HomeActivity.java` and `HomeViews.java`.
