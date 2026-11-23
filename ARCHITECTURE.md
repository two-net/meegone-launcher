# Architecture

MeeGone Launcher is one Cargo workspace that builds one app (ARCH-01). This page describes the shape every feature follows (SRS §7) and the rules CI enforces. The decisions behind it are in [docs/adr](docs/adr).

## Layers

Code is split into three layers, and dependencies only go one way, each layer using the one directly below it (ARCH-02):

```
UI  ──▶  state  ──▶  data  ──▶  Android OS, Bing feed
```

| Layer | Holds | Never |
|---|---|---|
| **UI** | Views that render an immutable state snapshot and send intents. | Calls the data layer or keeps state of its own. |
| **state** | One state holder per feature. It applies intents, publishes a new snapshot, and asks the data layer for side effects. | Knows about the UI. |
| **data** | Platform adapters (ARCH-04), stores and models. Every side effect happens here: launching apps, requesting uninstalls, downloads, setting the wallpaper. | Knows about state holders or the UI. |

State crates re-export the data models their state contains (for example `App` and `AppId` from `meegone-core-state`), so a UI crate never needs a data crate.

## Crate map

Each crate is `crates/<area>/<layer>`, with the package name `meegone-<area>-<layer>`. A layer with nothing in it has no crate.

| Area | data | state | ui | Owns |
|---|---|---|---|---|
| **core** | Models, the problems the user can be told of (`Problem`), platform adapter traits and fakes, storage and the saved-file format, the settings store, app accents and colour contrast | `StateHolder`, `Store`, theme resolution, the motion system: every moment's named spring or tween with its easing, the system animation speed, and spring physics (ADR 0034) | Colour tokens, each app's accent per theme, `View`, `Slot`, `Dispatch` | Nothing persistent |
| **apps** | Layout store | Grid, its columns, scroll position and geometry, an app's long-press menu, edit mode and its arrangement, folders, search and its ranking, hidden apps, keeping the layout in step with the installed apps, the apps a restore waits for | `AppsView`, `HiddenAppsSection` | Layout store; hidden apps (in the settings store) |
| **events** | – | The header with the date and the next alarm, notifications grouped by app, the expanded group, a notification's long-press menu, what opening, Dismiss and Clear remove | `EventsView` | Nothing (memory only) |
| **recent** | – | Which launchable app each use is, filtering by the time window, ordering, how long ago, opening an app from its card, the density and its columns, close mode, removing cards and forgetting old removals | `RecentView`, `RecentSettingsSection` | Time window, density and removed-apps list (in the settings store) |
| **lockimage** | Image cache (the current and previous images, their details, the markers of the wallpapers they were set as, the pin, and the check log), the image feed: its interface, `BingFeed` and the region, and the download: the size to crop to, picking a version, checking the image and cropping it; in memory only, the past week with its checked previews, today's image for setup, and the queue of what the user asked of the job's thread (`ImageMemory`) | On/off, availability, current image, its date and when it last updated in the device's language, opening its info link, whether Events shows its card, the feed region, "Use mobile data", the daily check: when it is due, its retries and back-off, and the updater that runs it and sets the wallpaper; "Update now", the pause when someone else changes the lock screen, and the reset; browsing the past week and pinning one of its images; setup's lock-screen image step: today's image, the step's switch and what Continue applies | `ImageCard`, `ImageSettingsSection`, `ImageDetailsScreen`, `PastImagesScreen`, `ImageSetupStep` | Image cache; image options (in the settings store) |
| **settings** | – | Settings' own options (theme, Home target, wrap-around), the status of each access and the written instructions where its system screen can't open, Reset launcher's own part, first-run setup: its steps, what skips them and whether this install finished it (ADR 0036), the home-app banner (ONB-04) | `SettingsView`, `AccessStatus`, `SetupView`, `SetupFrame`, `HomeBanner` | The Settings screen (ADR 0035), first-run setup (ADR 0036) |
| **shell** | – | `HomeShell`: the carousel of views, the home menu, Settings over the home views and Reset launcher across the features, which long-press counts, Home and Back, coming back to the home, the icon press, the dragged icon across the grid and the open folder, the grid's motion as apps come, go and move, edit mode's motion, a folder's opening and closing, the search field's pull, the Events list's motion as notifications come and go and groups expand, which long-press on an Events row counts, the image details screen (opened only while it has an image, closed once it hasn't), the past week's images over it or over Settings, and the image card's cross-fade, Recent following the hidden apps and usage access, Recent's pinch and its cards' reflow, which long-press counts in Recent, close mode's motion and cards leaving Recent, the whole screen's cross-fade to a new theme, ending every motion when animations are removed, first-run setup over the home views with the lock-screen image's step and Applications at its top when it ends, when the home-app banner shows, composes every feature; `Shared`, the stores the home shares with the daily image's job | `ShellView`, `HomeFrame` | Nothing persistent |

The root package `meegone-launcher` is the app. `Launcher::start(platform)` hands the platform adapters to the shell; `Launcher::start_shared` does it with the `Shared` stores the daily image's job uses too ([ADR 0029](docs/adr/0029-scheduling-the-daily-image-check.md)). On Android it is also a native library, which the Java host in `android/` loads ([ADR 0005](docs/adr/0005-android-host-and-home-role.md)) and asks what to draw ([ADR 0006](docs/adr/0006-android-views-and-the-carousel.md)). `tools/arch-check` is a development tool that is never part of the app.

## Dependency rules

`tools/arch-check` reads the workspace's declared dependencies with `cargo metadata`, scans the app's sources, and fails CI if any rule is broken. The layer and feature rules are the same for normal, dev and build dependencies. Cargo also refuses to compile a `use` of any crate a manifest doesn't declare, so the declared graph is the real one.

| Rule | Allowed |
|---|---|
| Layers (ARCH-02) | A crate may depend on its own layer or the one directly below: ui → state, state → data. Upward dependencies (such as data → ui) and skipped layers (ui → data) are rejected. |
| Features (ARCH-03) | A crate may depend on crates of its own area, on `core`, or, if it is a `shell` crate, on any feature. `core` depends only on `core`. |
| App crate | `meegone-launcher` may depend only on `shell` and `core` crates. Nothing depends on it. |
| Naming | Every member is `meegone-<area>-<layer>` with a known area, the root package, or a tool in `tools/`. Nothing in the app depends on a tool. |
| Third-party crates (ARCH-08) | Each one must be listed with its reason in [DEPENDENCIES.md](DEPENDENCIES.md). A listed crate that nothing uses is also reported. |
| OS access (ARCH-04) | No file in `crates/` or `src/` may use `std::{env, fs, net, os, process}`, read the clock (`Instant`, `SystemTime`) or contain `unsafe`, which foreign code such as JNI needs. Only the files listed in `OS_ACCESS_ALLOWED` in arch-check may: `FileStorage` (`storage/files.rs`), the JNI binding and the Android `SystemConfigAdapter` (`android/jni.rs`, `android/system.rs` in core data), and the app's Android entry points (`src/android.rs`). Comments and strings are ignored. The scan catches accidents, not determined workarounds. |
| Fakes (SEC-06) | A crate's `fakes` feature, and `meegone-core-state`'s `faults`, its bugs made to order, may be turned on only in `[dev-dependencies]`, so neither reaches the app. The app's own `faults` feature is turned on only by the debug build (ADR 0043). |

It also checks the legal rules ([ADR 0037](docs/adr/0037-branding-assets-and-the-store-listing.md)):
- The app's label and the store listing's title are "MeeGone Launcher" (LEGAL-01).
- No text resource and no listing file names Nokia, Microsoft, MeeGo, Harmattan or the N9, except in the non-affiliation statement (LEGAL-01, LEGAL-05).
- Every file under `android/app/src/*/res` and `assets`, apart from `values*` and `xml`, is listed with its source and licence in [ASSETS.md](ASSETS.md) (LEGAL-02).
- Every licence text is shown in About (LEGAL-06).
- The listing in `fastlane/metadata/android/` carries About's privacy notice and the LEGAL-05 statement word for word (PRIV-04).

And it checks two accessibility rules on the Java host ([ADR 0038](docs/adr/0038-named-reachable-legible-and-scalable.md)):
- Every colour drawn comes from the theme's tokens: a colour literal outside `COLOUR_ALLOWED`, each entry with its reason, fails (A11Y-03, SRS 5.3). An entry that no longer matches anything fails too.
- Every text size is in sp, so it follows the system font size (A11Y-04).

And the localisation rules ([ADR 0040](docs/adr/0040-localisation-and-right-to-left-layouts.md)):
- **The Java code that ships writes no text for the user** (L10N-01):
  - No literal with a letter goes into a text sink, such as `setText` or `new AccessibilityAction`.
  - No other literal reads as words, outside logs, thread names, exceptions and `TEXT_ALLOWED`.
  - No number goes into a sink in ASCII digits whatever the language (L10N-03).
- **No layout or manifest attribute holds text** instead of `@string/…`.
- **The Rust app writes no words:** outside tests, fakes, panics, assertions and `PROSE_ALLOWED`, no literal reads as prose.
- **Every `Problem` is worded:** its code, its constant and case in `Problems.java`, and its `problem_<code>` string match.
- **Every string resource can be translated:** each follows a comment for translators, numbers its arguments when it has several, and only `app_name` is untranslatable.
- **The allow-lists stay true:** an entry of `TEXT_ALLOWED` or `PROSE_ALLOWED` that matches nothing fails.

And the security and privacy rules ([ADR 0042](docs/adr/0042-security-and-privacy.md)):
- **The manifest asks only for SRS 8.1's access:** exactly the permissions in `PERMISSIONS_ALLOWED`, each with its row of SRS 8.1 (SEC-03).
- **Other apps reach only the home entry, the settings entry and the notification listener:** exactly the components, guards and intent filters in `EXPORTED_ALLOWED`. Every component says whether it is exported (SEC-04).
- **No role or reach that SEC-03 and SEC-05 rule out:** no accessibility service or device administrator, and nothing over the lock screen.
- **HTTPS with the system's certificates only:** the app names its network security config, which refuses cleartext and has no `debug-overrides` or `domain-config`. The app isn't debuggable, test-only or profileable from the shell (SEC-01, SEC-06).
- **The Java code that ships:**
  - It uses none of `FORBIDDEN`: web views and links in text, the lock screen's flags, device identifiers, stack traces (SEC-02, SEC-05, PRIV-01, PRIV-03).
  - It connects only from the files in `NETWORK_ALLOWED`, `Bridge.java` (PRIV-02).
  - No log passes an exception (PRIV-03).
- **The release APK as built:** after the Gradle build, CI reads it with `aapt2` and `dexdump`. It fails on the manifest rules above, and on any class only the debug build's source sets define (SEC-06).
- **The allow-lists stay true** here too.

And the reliability rules ([ADR 0043](docs/adr/0043-failure-containment-and-crash-free-sessions.md)), so the platform's own stability reports see every crash, and nothing else as one (REL-07):
- **Nothing hides a crash:** the shipped Java never sets an uncaught-exception handler, never kills a process or halts the VM, and ends its own process only in `EXIT_ALLOWED`, which is Restart MeeGone.
- **No library or SDK, such as a crash reporter:** the Gradle build applies only `PLUGINS_ALLOWED`'s Android plugin and declares no dependency, and the release APK as built holds only classes in MeeGone's own package.
- **No bugs made to order in a release:** the Rust app's `faults` feature is turned on only for the debug build, and the release APK's native library has no fault hook.
- **Panics unwind:** `meegone-core-state` doesn't compile with `panic = "abort"`, however it is set, as containing a failed feature needs unwinding.
- **The allow-lists stay true** here too.

And the compatibility rules ([ADR 0044](docs/adr/0044-the-device-and-navigation-mode-matrix.md)), so MeeGone keeps running on Android 10 (COMP-01):
- **One oldest version:** SRS.md's API level, the APK's `minSdk` and the platform cargo-ndk builds the Rust library for agree.
- **`NewApi` is fatal:** lint's vital checks, which the release build runs, fail on a call newer than Android 10 without its version check, and nothing in the Gradle build turns that off.
- **Nothing hidden without a reason:** the shipped Java hides a method's calls from `NewApi` only where `API_HIDING_ALLOWED` says why every caller checks the version.
- **The allow-lists stay true** here too.

What a check can't decide, such as the image source's terms (LEGAL-04) and the store graphics, is in [RELEASING.md](RELEASING.md). What only a device can show, every screen as it is drawn, is checked by the debug build's accessibility audit (`tools/a11y-audit.sh`, ADR 0038). Its tour also runs in an expanded and a right-to-left pseudo-locale (ADR 0040). Where the release build's traffic goes over a day is checked by `tools/net-audit.sh` (ADR 0042). That a failed part leaves the rest working, and Restart MeeGone isn't a crash, is checked by `tools/fault-check.sh` (ADR 0043). That MeeGone works on each device and in each navigation mode of the release's matrix is checked by `tools/compat-check.sh`, with its results in `docs/devices/` (ADR 0044).

## How a feature works

The example below follows a tap on the Mail icon (ARCH-05):

```
AppsView.cell_tapped(revision, 4, icon)          ui     sends AppsIntent::Launch { app: mail, from: icon }
  └▶ ShellView queues ShellIntent::Apps(..)      ui
      └▶ HomeShell.dispatch                      state  routes it to the Applications store
          └▶ Store<AppsHolder>.dispatch          state  runs the holder, contains failures
              └▶ AppsHolder.handle               state  asks the data layer for the side effect
                  └▶ AppsAdapter.launch(mail, icon)  data  the only code that talks to Android
          ◀── new Arc<AppsState> snapshot, published to subscribers
```

- **State holders** implement `StateHolder`: `state()`, `handle(intent)` and `on_back()`. They take their adapters and stores as `Arc<dyn …>` traits, so tests drive them with the fakes in `meegone-core-data` (feature `fakes`), without any UI (MAINT-01).
- **Constructors do no I/O.** Each feature loads on its first `Refresh` intent, which runs inside its store.
- **Home and Back** ([ADR 0007](docs/adr/0007-home-back-and-returning.md)). They work the same in every navigation mode; only the return motion depends on it (COMP-02, ADR 0009). MeeGone takes the mode for gestures when a side edge is Back's or the bottom is a handle with nothing to tap (ADR 0044).
  - Back closes Settings, then the home menu, then goes to the current view's `on_back`, which closes that feature's topmost open element, or does nothing (HOME-08). Settings opened from the system's settings entry closes to the home views too.
  - Home on the home closes Settings, or the home menu and everything open in the current view, then moves to Applications, then scrolls it to the top (HOME-07).
  - Coming back from an app (`Returned`) shows the view the user left. With "Home button opens: Always Applications", Home from an app shows Applications with nothing open (HOME-06).
  - After the system ends the process, the activity's saved state brings back the view and the Applications scroll position (`Restore`, REL-05).
- **The home menu** ([ADR 0008](docs/adr/0008-the-home-menu.md)). A long-press on empty space opens it in Events and Applications. In Recent, a long-press anywhere on the cards enters close mode instead (HOME-10, REC-07). It lives in `ShellState.menu`, and its items route to Settings, to the image details screen once there is an image (ADR 0031), and to Applications' edit mode.
- **Settings** ([ADR 0035](docs/adr/0035-the-settings-screen.md)). One screen over the home views, its groups in SET-02's order: Appearance, Home, Applications, Recent, Lock-screen image, Access and About. It opens from the home menu, at the Lock-screen image group from that item or the details screen, and from the system's settings entry for MeeGone (`ShellIntent::OpenSettings`, SET-01). Every change is saved and applied at once (SET-03).
  - Each group belongs to the feature whose fields it changes (ADR 0002): Settings draws Appearance, Home and Access, and the shell puts in Applications', Recent's and the lock-screen image's sections. About is the host's own text. A group shows only while its feature works (ARCH-09).
  - An Access shortcut that finds no system screen leaves its written instructions in `SettingsState.instructions`, shown below that item until Settings opens again or MeeGone comes back (SRS 8.1).
  - **Reset launcher** (SET-06), once the host has asked, goes to the shell, which has each feature restore what it owns: Applications builds a first start's layout and shows hidden apps again (`AppsIntent::Reset`), Recent restores its density and window and forgets its removals (`RecentIntent::Reset`), the lock-screen image restores its options, which turns the daily image off and leaves the lock screen as it is (`LockImageIntent::Reset`), and Settings restores its own (`SettingsIntent::ResetLauncher`). Settings closes, and Applications shows at its top in the default theme. It is offered only while all four work, so a reset is never partial.
- **First-run setup** ([ADR 0036](docs/adr/0036-first-run-setup.md)). Until this install has finished it, setup covers the home views: Welcome, the default home app, the daily lock-screen image and access, at most four screens (ONB-01). It never asks about the theme (ONB-05).
  - Settings owns the flow in `SettingsState.setup`, in memory: a step already done is skipped (MeeGone is the default, both accesses are allowed), the default-home step moves on at the `Resumed` after the system's dialog, and setup ends once each access is allowed or put off with Not now (ONB-02). Back returns to the step before; Home leaves setup.
  - Settings records the install that finished it, `Settings.setup_install_ms`; 0, which older settings migrate to, is any install. Settings restored from another install's backup run setup again (ADR 0018).
  - The lock-screen image step is that feature's own `ImageSetupStep` (ADR 0002). `OpenSetup` starts its switch on and reads today's image and preview on the job's thread while the daily image may still be off (`ImageTask::PreviewToday`), saving and setting nothing (WALL-02). Continue reaches the shell, which sends `ConfirmSetup` first: with the switch on, the image is turned on and the daily check runs at once within its rules (`ImageTask::Check`), so on Wi-Fi the lock screen has today's image within seconds (ONB-03).
  - The shell refuses swipes, menus, search, Settings (even from the system's entry) and the image screens while setup shows. When it ends, everything closes and Applications shows at its top (HOME-05). A failed Settings ends it, so the home is never covered for good (ARCH-09).
  - The banner that offers to make MeeGone the default (ONB-04) shows only on the home views with nothing over them (`HomeShell::banner_shown`). Setup's Not now on the default-home step counts as that day's answer.
- **An app's menu** ([ADR 0013](docs/adr/0013-the-app-long-press-menu.md)). A long-press on an app's icon opens it: App info, Uninstall when the app allows it, Hide, Edit applications and, while MeeGone is the default home app, the app's shortcuts (APPS-11, APPS-13, APPS-19). A folder's menu has Rename folder and Edit applications (ADR 0015).
  - It lives in `AppsState.menu`, and each item's side effect goes through `AppsAdapter`. Back closes it before a folder or edit mode.
  - The shell decides whether the long-press counts and treats the menu as covering the views, but only while Applications works (ARCH-09).
  - Dragging on picks the icon up (`lift.rs`): edit mode starts, and the icon follows the finger until it lifts.
- **Edit mode** ([ADR 0014](docs/adr/0014-edit-mode.md)). The app menu, the home menu or a drag enters it; Done, Home, Back or leaving Applications ends it (APPS-12).
  - Applications owns the arrangement. A drag edits a draft layout (`Arrange`, `Combine`, `TakeOut`, `ArrangeInFolder`) without saving, and the drop tidies the draft and saves it once (`Drop`). Entries that show no cell keep their slots. The pure edits are in `folders.rs` (ADR 0015).
  - The shell owns the drag: the place or icon under the dragged icon, the waits before a place is taken or a merge is armed, auto-scroll near the edges, leaving the open folder, and the drop's spring or merge. It works places out from each surface's geometry, which its `AppsGrid` reports.
  - A drag that ends any way but a drop puts the grid back: one rule in the shell's dispatch epilogue.
  - The ✕ asks `AppsAdapter::request_uninstall`; a tap on an icon launches nothing. Edit mode counts only while Applications works (ARCH-09).
- **Folders** ([ADR 0015](docs/adr/0015-folders.md)). An app dropped on another, after a 400 ms hover, makes a folder named "Folder" until the user renames it; one dropped on a folder joins it (FOLD-01, FOLD-03). A folder left with one app turns back into it when the drop is saved (FOLD-06).
  - Applications keeps the open folder and its cells as a second surface, `Surface::Folder`, beside the grid. Cells, presses, menus and the drag name their surface and a `CellKey`.
  - A tap opens a folder. Back, Home, a tap outside, a new view or Settings closes it. While it is open it locks the carousel, and only its panel takes long-presses and presses (FOLD-04).
  - The shell runs the panel's motion (`folder.rs`), the hover and the merge, and ticks when a folder is made (ANIM-14, ANIM-15, ANIM-24). The open folder, its rename and the app menu count only while Applications works (ARCH-09).
- **Search** ([ADR 0016](docs/adr/0016-app-search.md)). Pulling down at the top of Applications reveals the search field; pulled past its band, it springs open and the keyboard rises (SRCH-01, ANIM-16).
  - The platform folds each label into `App::search_key`, and each query through `AppsAdapter::search_key`, so case and accents don't count, and "lodz" finds "Łódź" (L10N-04, ADR 0040). The pure `search::rank` puts matches at a word's start first, then those inside a word, each in the locale's order (SRCH-02).
  - Applications owns the query and its results, a third surface, `Surface::Search`. The action key launches the first result, or searches the web when nothing matches and a browser is installed (SRCH-03, SRCH-05).
  - The shell owns the pull (`search.rs`). While search is open it locks the carousel and offers no menus. A screen reader opens search with "Search apps" on Applications' own element, and a keyboard by typing a letter or with Ctrl+F (A11Y-05, A11Y-06, ADR 0039).
  - Back, Home, a launch, another view, Settings, or pushing the empty field away closes it. Search never moves the grid, so it is where it was (SRCH-04). Search counts only while Applications works (ARCH-09).
- **Hidden apps** ([ADR 0017](docs/adr/0017-hiding-apps.md)). Hide in an app's menu leaves it out of the grid, search and Recent; Settings → Applications → Hidden apps unhides it (APPS-19).
  - Applications writes `hidden_apps` in the settings store. Reconcile still sees every installed app, so a hidden app keeps its place and folder; the cells and the drag's edits see only the apps that aren't hidden. Unhiding changes nothing else, so the app comes back where it was.
  - A hidden app is forgotten when it is gone, by the rule that keeps its place (`follow_hidden`), so reinstalling it shows it.
  - Applications provides `HiddenAppsSection`, which the shell puts into Settings. It is offered only while Applications works (ARCH-09).
  - When the hidden apps change, the shell refreshes Recent at once, in `dispatch`.
- **Backup and restore** ([ADR 0018](docs/adr/0018-backing-up-and-restoring.md)). The system's backup and device-to-device transfer keep the layout and settings files, and nothing else (APPS-21).
  - Every layout Applications saves records when this copy of MeeGone was installed (`SystemConfigAdapter::installed_ms`). A layout another copy saved came from a backup, and is saved at once as this copy's.
  - At a restore, its apps that aren't installed yet keep their places, out of sight, for 7 days. Each one installed in that time takes its place, without a dot. When the wait ends, those still missing leave.
  - Apps the backup doesn't hold join the end, alphabetically and unmarked. Settings come back whole.
- **The Events header** ([ADR 0019](docs/adr/0019-the-events-header.md)). Events opens with today's weekday and date and, while an alarm is set, the next alarm; a tap on the alarm opens the app that set it (EVT-01, EVT-02).
  - The platform writes each date and time in the device's language and 12- or 24-hour setting (`SystemConfigAdapter::format_time`, L10N-03); Events decides what shows. An alarm within 24 hours shows its time, a later one its weekday too.
  - The header never needs notification access (EVT-03). `ClockChanged`, `Resumed` and a swipe to Events, once the views are at rest (ADR 0041), read it again.
- **Events' notifications** ([ADR 0020](docs/adr/0020-listing-notifications-in-events.md)). With notification access, Events lists the active notifications grouped by app, newest group first: each app's icon, accent, name and count, and its newest notification's title and text. Without access, a card says what Events reads and why, with Allow (EVT-03, EVT-04).
  - Events reads each app's label, icon and accent through `AppsAdapter`, as Recent does, and names an app outside the catalogue as the system does. Ongoing notifications, MeeGone's own, and a summary while its bundle shows are left out (EVT-05).
  - `NotificationsChanged` and `Resumed` read the notifications again, and `AppsChanged` the catalogue. A row keeps its place while its newest notification stays the same (`rows_kept`). The shell runs the list's 200 ms motion when rows come or go (ANIM-20).
  - Nothing stores, logs or prints what a notification says (EVT-13).
- **Acting on notifications** ([ADR 0021](docs/adr/0021-acting-on-notifications-in-events.md)). A tap on a group of one opens its notification; a group of several expands, one at a time, and lists its notifications as rows of their own, which open when tapped (EVT-06). Clear removes every clearable notification shown, in one request (EVT-07). A long-press on a row offers Dismiss and the app's Notification settings (EVT-08).
  - Events decides: it dismisses a notification meant to disappear when opened, and with the last of what a group shows, the summaries hidden behind it. Rows go when the system reports them gone, never in advance.
  - The expanded group and the menu live in `EventsState`. `on_back` closes the menu, then collapses the group; leaving Events collapses it.
  - The shell decides whether a long-press on a row counts, ticks, and treats Events' menu as covering the views, but only while Events works (ARCH-09). A screen reader opens the menu with "Open menu", and a keyboard with Menu or Shift+F10 (A11Y-05, A11Y-06).
- **The image-of-the-day card** ([ADR 0022](docs/adr/0022-the-image-of-the-day-card.md)). While the daily image is on and has an image, a card below the notifications shows its thumbnail, title and attribution. It works offline and without notification access. "Show card in Events" hides it, and a tap opens the image details screen (EVT-12, LEGAL-03).
  - The lock-screen image owns the card and the setting; the shell places the card in Events (ADR 0002). `Resumed` and swiping to Events, once the views are at rest, read the image again.
  - The image cache keeps two images, `images/a` and `images/b`. Each record names its file, and `ImageCache::put` picks the file and writes it crash-safely (WALL-09).
  - The details screen covers the home views as Settings does, and Back and Home close it. It opens only while the lock-screen image works (ARCH-09) and has an image to show.
  - When the host has decoded a new thumbnail over the old one, it sends `NewImageShown`, and the shell fades it in over 300 ms (ANIM-22).
- **The image details screen** ([ADR 0031](docs/adr/0031-the-image-details-screen.md)). It shows the image, its title, its attribution as the source gives it, its date and its region, with More info, Update now and Settings (WALL-15, LEGAL-03). The card opens it, and so does the home menu's Lock-screen image once there is an image; before that, the item opens Settings at its group.
  - It is offered only while the daily image is on and has an image, and closes once it hasn't, such as when the image is turned off in Settings over it. Settings opens over it at Lock-screen image, and Back returns to it.
  - More info opens the image's `https://` link in the default browser, or, without one, in a browser the user picks, never in another app that claims the site's links (ADR 0042). It is offered only while the daily image is on, the image has a link and a browser is installed; otherwise it is hidden (SEC-02, SRS 8.1).
  - The platform writes the date (at local noon, so the zone never moves it) and when the image last updated (`TimeStyle::DayMonthTime`). `ClockChanged` writes them again (L10N-03).
  - **The status line** (WALL-16), in Settings and on the screen: "Last updated ‹time›" and, while the checks retry, "‹problem›. Will try again later." Rust gives the parts (`UpdateStatus`), the problem as its code; the host words them from string resources (ADR 0040).
- **Recent's cards** ([ADR 0023](docs/adr/0023-recent-cards.md)). With usage access, Recent shows up to 18 apps used in its time window, 24 hours by default, newest top-left, each as a card in its accent with its icon, label and how long ago it was used. Without access, a card says what Recent reads and why, with Allow. With nothing used, it reads "No recent apps" (REC-01 to REC-04, REC-10).
  - The system reports the activity last resumed, so the pure `cards` finds the launchable app with the same profile and package, and leaves out hidden apps and apps removed from Recent by that app. MeeGone itself is never one.
  - A tap opens the app through `AppsAdapter::launch`, growing from the card (REC-06, ANIM-08).
  - Recent reads the uses again when it shows, once the views are at rest (ADR 0041), on `Returned`, on `AppsChanged`, when the hidden apps change, and on `UsageAccessChanged`, so revoking access while it shows brings the card back. Nothing polls (REC-09, PWR-02). It keeps the catalogue it read, until the apps change (`RecentIntent::AppsChanged`).
  - Usage history stays in memory (PRIV-03).
- **Recent's density** ([ADR 0024](docs/adr/0024-recent-density.md)). Recent shows more cards, three columns, or bigger ones, two, and saves the choice (REC-05). A large screen adds columns to both: more cards are at most 160 dp wide, and bigger cards take two columns for every three. The pure `columns_for` picks them from the width the host reports.
  - Spreading two fingers on the cards switches to bigger cards, and bringing them together to more. The shell owns the pinch (`pinch.rs`). It starts only on Recent's cards at rest with nothing covering them, and it takes the gesture, so neither finger swipes the views.
  - The cards scale with the fingers (ANIM-01). Released past 35 % of the way to the other size, the density switches. After every pinch, and whenever the density changes, the cards reflow for 300 ms (ANIM-17).
  - Screen readers, Switch Access and Voice Access switch with "Bigger cards" or "More cards" on Recent's own element (A11Y-05, ADR 0039).
- **Recent's close mode** ([ADR 0025](docs/adr/0025-recent-close-mode.md)). A long-press anywhere on Recent's cards enters it: a ✕ on every card and Clear all (REC-07).
  - ✕ and Clear all remove cards from Recent only, in one settings write, and no app is stopped. An app comes back when it is next used (REC-08, AC-14).
  - Each removal write forgets removals older than 72 hours, which can no longer hide anything (PRIV-03).
  - A tap on empty space, Back, Home or another view leaves close mode, as does removing the last card. A tap on a card opens nothing.
  - The long-press counts, and ticks, only while close mode can start. Close mode offers no pinch (ANIM-24).
  - The shell runs edit mode's motion for it, in a second `Mode` (ANIM-12). When cards leave Recent while it shows, `recent_change.rs` shrinks and fades them row by row, and the others close the gap through the pinch's reflow: 180 ms for one card, and at most 400 ms for Clear all (ANIM-18).
  - Screen readers enter it with "Edit Recent", which ticks as a long-press does (ADR 0034): the press of Recent's own element, and each card's long-click. They remove a card with "Remove ‹app› from Recent" (A11Y-01, A11Y-05). A keyboard enters it with Menu and removes the focused card with Delete (A11Y-06, ADR 0039).
- **Recent's time window** ([ADR 0026](docs/adr/0026-recent-time-window.md)). Settings → Recent → Time window chooses how far back Recent looks: 12 hours, 24 hours or 3 days (REC-11).
  - `RecentWindow` is one of the three, saved by name in the settings file since version 3. The migration turns an older number of hours into the shortest choice that shows as much.
  - Recent writes it (`RecentIntent::SetWindow`) and reads its cards again as soon as it is saved, so the change takes effect at once (SET-03). A failed save keeps the window and the cards.
  - Recent provides `RecentSettingsSection`, which the shell puts into Settings after Hidden apps. It is offered only while Recent works (ARCH-09).
- **The image feed** ([ADR 0027](docs/adr/0027-the-image-feed-client.md)). `BingFeed`, MeeGone's one client of its one network service, is the only code that knows the feed's format (SRS 8.2).
  - `today(region)` makes exactly one request for today's image and never retries; the updater keeps to the call budget (REL-04). The response is read strictly, at most 64 KiB, by a small JSON reader (`json.rs`). Anything unexpected is `Invalid(FORMAT_CHANGED)`, never a panic (SEC-02).
  - It gives the date, the headline as the title, the whole credit line as the attribution (LEGAL-03), an `https://` info link if there is one, whether the image is offered as a wallpaper, and three sizes, each built on `https://www.bing.com` from the image's checked name (PRIV-02).
  - The region is Automatic unless the user chose one the feed lists. Automatic matches the device's language and country, then its language, then falls back to en-US (`region.rs`, WALL-05). The lock-screen image saves the choice (`LockImageIntent::SetRegion`), chosen in Settings → Lock-screen image → Region (ADR 0035).
  - On Android, `Bridge.httpsGet` makes one HTTPS request with the platform's certificate checks and follows no redirect. `https::fetch` follows at most 3, only to `https://` on the first URL's origin, so a response can't send a request to another host (SEC-01, PRIV-02, ADR 0042). Requests carry a fixed user agent, no cookies and no cache. The network security config refuses cleartext. `get` blocks, so it runs only on a JVM thread other than the main one.
- **The daily image's download** ([ADR 0028](docs/adr/0028-downloading-and-cropping-the-daily-image.md)). `Downloader::download` turns one feed image into the lock-screen image. It caches the result only when every step succeeds, so any failure keeps the current image and the wallpaper set from it (WALL-08).
  - The pure steps are tested without a device:
    - `pick_version` takes the smallest portrait version that covers the lock screen, or else the largest (WALL-06).
    - `check_image` reads the JPEG header strictly (`jpeg.rs`) and requires 720 to 8192 px on each side, and at most 16 million pixels for a progressive image, whose decoder holds the whole image (WALL-07, SEC-02, ADR 0042).
    - `plan_crop` keeps the middle in the lock screen's proportions and never scales up. It decodes at a power-of-two fraction, so the decoded picture is less than twice the result.
  - Downloads stop at 20 MB. With less free storage than that, nothing is requested ("Not enough storage space"). Every failure is plain words for the status line (WALL-16).
  - The platform decodes and crops (`WallpaperAdapter::crop_image`). On Android, `Bridge.cropImage` uses `BitmapRegionDecoder` and writes a new JPEG without metadata. What comes back is checked again, and the cache keeps the crop, never the download (PERF-08).
  - It blocks, so it runs only on a JVM thread other than the main one. The daily check calls it.
- **The daily image check** ([ADR 0029](docs/adr/0029-scheduling-the-daily-image-check.md)). MeeGone's one scheduled job (ARCH-07). `ImageUpdater::check` runs when the system starts the job, at each start of MeeGone, which covers devices without background scheduling (SRS 8.1), and right after setup's Continue turns the image on (ADR 0036).
  - The pure `checks.rs` decides when a check is due, from the check log the image cache keeps:
    - once a day after local midnight;
    - while the feed still offers the current image, up to three more times that day, 2 hours apart (WALL-03);
    - after a failure, after 15 minutes, doubling (REL-04);
    - never more than 6 attempts a day.
  - A due check waits, without counting an attempt, while battery saver is on or the battery is low, and on a metered network while "Use mobile data" is off or Data Saver is on (WALL-04, PWR-04). The job comes back no earlier than 15 minutes later.
  - Each attempt is saved before its request. The feed is asked for today's image in the region in use. The same image, by the feed's id, or one not offered as a wallpaper, isn't downloaded; any other goes to `Downloader::download`. Then the job is scheduled for the next check.
  - If the daily image was turned off during the check, nothing of it stays (WALL-12). The home and the job share one settings store and one image cache, `Shared`, so each sees the other's writes at once.
  - After 3 days of failures, Settings says "Daily image unavailable", with Try again, which forgets the failures within the day's budget (SRS 7.6). `ShellIntent::ImageChecked` reads the image and the checks again after a check.
- **The lock-screen wallpaper** ([ADR 0030](docs/adr/0030-setting-the-lock-screen-wallpaper.md)). The updater sets the daily image as the lock screen's wallpaper only, in SRS 7.5's order: cached, set, then recorded with its time and the system's marker for it (WALL-01, SEC-05).
  - Only once every step has succeeded, and never while the daily image is off: with no image, nothing changes (WALL-02, WALL-08, WALL-10). A set the system refuses is the attempt's failure; the retry finds the image cached and sets it without downloading it again.
  - Right after setting, the lock screen's marker is read back. Another one means the device didn't keep the image (`Marker::NotKept`), and Settings says it may not appear (SRS C5).
  - Later, another marker means someone else changed the lock screen: daily updates pause, and Settings asks to resume (WALL-14). Nothing is stored for it; where the platform can't tell wallpapers apart, it never pauses.
  - "Update now" and Resume are `ImageUpdater::update_now`, on the job's thread through `SchedulerAdapter::run_image_tasks` (ADR 0032). It puts the current image back first, then asks the feed at once over any connection, past the day's budget, though it counts (WALL-11).
  - Once the daily image is off, Settings offers "Reset lock screen to home wallpaper" and the system's wallpaper settings (WALL-12). Where wallpapers can't be set, the switch is disabled with the reason (WALL-13).
  - The crop is `lock_screen_target`'s: a phone's screen upright, or a square on a screen that turns or folds.
- **The past week and pinning** ([ADR 0032](docs/adr/0032-browsing-the-past-week-and-pinning-an-image.md)). "Past week" on the details screen, or "Choose from the past week" in Settings, opens the past week's images over it: today's and the six before, each with its preview, title, attribution and date, and Pin (WALL-17).
  - `BingFeed::past_week` asks for the seven in one request. Each preview, the feed's 800 × 480, is checked as a JPEG at most 256 KiB and 1 920 px a side (SEC-02), and fetched only for images offered as a wallpaper (LEGAL-04). They are kept in memory only, in `PastImages`, for the rest of the local day; nothing new is stored (PERF-08, WALL-09).
  - Pinning chooses the image from the cache when it is the current or previous one, and otherwise downloads it as the daily image is: 20 MB, 720 px, the crop (WALL-06 to WALL-08). The pin is saved on the record in the same write as the image (`IMAGE_CACHE_VERSION` 5). It is set, and the job is cancelled.
  - While an image is pinned, the daily check asks nothing and no job waits (`Checked::Pinned`). Unpin is "Update now", as Resume is: it lets go of the pin, and brings today's image back, from the cache when it is the previous one.
  - What the user asks of the job's thread ("Update now", browsing, pinning) waits in `ImageTasks`, which the job's thread runs one task at a time. "Updating…" and "Pinning…" come from that queue, never from whichever check reported back.
- **Themes** ([ADR 0033](docs/adr/0033-themes-and-the-theme-cross-fade.md)). Settings → Appearance → Theme offers System, the default, Light and Dark. System follows the device's dark theme while MeeGone runs, by the user or on its schedule (THEME-01, THEME-02).
  - Settings writes the choice, and core's `ThemeHolder` resolves it with `SystemConfigAdapter::is_dark` (ADR 0002). Every colour drawn comes from the theme's tokens (SRS 5.3), and each app's accent from `Tokens::app_accent` (THEME-07).
  - The shell refreshes the theme on `SystemThemeChanged`, on `Resumed`, which catches a change while another app showed, and after every Settings intent.
  - When the theme drawn changes, the whole screen cross-fades to it in 250 ms (`CrossFade`, which the image card's fade shares), and nothing else changes: the view, the scroll positions, an open folder or search, edit and close mode, and Settings stay as they are (THEME-06, ANIM-19). A refresh that resolves to the same theme fades nothing.
- **Moving without a gesture** ([ADR 0039](docs/adr/0039-gestures-keyboards-switch-access-and-reduced-motion.md)). In edit mode, a cell's menu offers what a drag does in place of Edit applications: Move to start, Move earlier, Move later and Move to end, Make folder with ‹next app› or Move into ‹next folder›, and, in an open folder, Move out of folder (A11Y-05). Applications offers each only where it moves something, and saves it as a drop does (`AppMenuItem`, `AppsHolder::moves`).
  - The shell refuses a tap on a cell no finger could reach: under a menu or a screen, or on a surface an open folder or search covers. It refuses "Next view" and "Previous view" under setup, Settings, the image details and the past week (A11Y-06).
- **Motion** ([ADR 0034](docs/adr/0034-the-shared-motion-system.md)). Every moment of the motion catalogue is a named spec in `meegone-core-state::motion`: a `Spring`, or a `Tween` of a length and an `Easing`. Every motion in the shell uses them and has no easing of its own. `CATALOGUE` lists them, and one test holds them to ANIM-02 and ANIM-06.
  - **The system animation speed** (ANIM-05, A11Y-07) is `SystemConfigAdapter::animation_scale`, cleaned up by `motion::speed`. Tweens last `length_at` it, and springs run on `motion_time`.
    - It is read at start, on `Resumed`, and on `AnimationScaleChanged`, which Android sends as soon as the setting changes, even while the home shows.
    - When it becomes 0, `finish_motions` puts whatever moves where it was going. A finger keeps what it holds.
    - The host's own motion follows it too (ADR 0039). With animations removed, `Scroll` scrolls at once, its scroll bars go at once, and below Android 12 it has no glow. `Dialogs.show` opens dialogs without the window's animation.
    - Waits never scale: the 150 ms before a dragged icon takes a place, the 400 ms before a folder is armed, and the 600 ms the view indicator stays. So nothing is lost with animations removed.
  - **A touch catches what visibly moves** (ANIM-03): the row, while further than a third of the touch slop from rest; the search field; a settling dropped icon, which it picks up again; and Recent's reflow, which a pinch freezes where it is drawn.
    - Anything closer to rest, or locked, lands at once, and the touch is the content's.
    - A long-press in a folder still opening opens it all the way at once.
    - ✕ badges take taps as soon as their mode is on.
    - Other transitions finish at once or carry on under a touch; none blocks input.
  - **Ticks** (ANIM-24) come with every long-press the shell takes, entering edit or close mode by any route unless a long-press just ticked, and a folder made.

## Drawing (ADR 0006)

Android's own views draw MeeGone, and the Rust app decides what they show. The decisions are in [ADR 0006](docs/adr/0006-android-views-and-the-carousel.md) and the ADRs after it.

```
HomeViews (Java)                                            Rust
  dispatchTouchEvent ── nativePointer        ──▶ ShellIntent::Pointer      ─┐
  long-press timer   ── nativeLongPress      ──▶ ShellIntent::LongPress    ─┤
    (on a cell)      ── nativeAppLongPressed ──▶ ShellIntent::LongPressCell ┤
  Choreographer      ── nativeFrame          ──▶ ShellIntent::Frame        ─┼▶ HomeShell: carousel
  onSizeChanged      ── nativeLayout         ──▶ ShellIntent::Layout       ─┘   (gesture, spring,
                                                                         indicator), menus, the
                                                                         dragged icon, the open
                                                                         folder and return-home
                                                                         motion
  translationX, scale, alpha, menu ◀── nativeRead(int[]) ◀── ShellView::frame ◀── ShellState
```

- **Java pulls.** After every touch, frame or layout change, `HomeViews` reads the frame and applies it. It asks `Choreographer` for another frame only while the frame says something moves. Once nothing moves, the main thread's idle handler calls `nativeIdle` (`ShellIntent::Idle`), and the view that came into sight is read again (ADR 0041). Rust never calls Java to draw.
- **Motion is state.** A moving element's position and velocity live in the state layer, and the named motion specs, easing and spring physics in `meegone-core-state::motion` (ADR 0034). Time comes in with each event, on the host's monotonic clock, so tests pass explicit times. The return-home motion (ANIM-09) scales and fades the pages, never the background. The icon press (ANIM-10) scales only the pressed icon, and never delays a launch.
- **One frame per vsync.** A touch is applied in the same call that sends it, so the drawn position is never a frame behind (ANIM-01). `nativeFrame` only advances the motion, and the `nativeRead` that `apply()` calls next builds the frame once (ANIM-04).
- **Touches.** Every touch reaches the carousel first. The content below keeps a gesture until the carousel takes it: sideways, past the platform's touch slop, from anywhere in the view (HOME-03, HOME-13). The search field's pull takes a vertical drag at the top of Applications the same way (ADR 0016). An element that uses horizontal drags itself calls `requestDisallowInterceptTouchEvent(true)` first.
- **Edge to edge** ([ADR 0009](docs/adr/0009-edge-to-edge-screens-and-navigation-modes.md)). On every Android version the solid background fills the window behind the system bars, which stay visible (HOME-11). Content stays inside a safe area clear of the bars, cut-outs and rounded corners: each page and Settings get it as padding, so a view's content needs no inset code of its own. Phones stay in portrait; screens 600 dp or more across their smallest width turn freely (HOME-12).
- **Size changes keep state** (COMP-03). Rotating, folding and resizing send a new `Layout`. It ends a drag but keeps the view, Settings, the home menu (now centred) and everything open in the views. The device's dark theme turning on or off is handled in place too (ADR 0033). Other configuration changes recreate the activity, and the shared launcher keeps everything.
- **Applications** ([ADR 0010](docs/adr/0010-the-applications-grid.md)). `AppsGrid`, a `Scroll` (a `ScrollView`), draws the frame's Applications slots: the columns and rows (the last one empty, HOME-14), the scroll position and the pressed icon. It reads the cells with `nativeAppsCells` only when their revision changes, and names a tapped cell by revision and index. It reports its width and row height, and the Rust app picks the columns (APPS-02) and keeps the same first visible item when they change. It also reports the room above and below its cells, its height, where it is on screen and where its icons are, so the Rust app knows which place or icon is under a dragged icon (ADR 0014, ADR 0015). Icons stay in Java: `AppIcons` loads them off the main thread, once for both grids, as hardware bitmaps. Only the cells in sight, or within half a screen of it, hold theirs and draw; the others draw nothing until they come near. So 300 apps never mean 300 icons in memory, nor 300 cells drawn (PERF-03, PERF-07, ADR 0041).
- **Apps coming, going and moving** ([ADR 0012](docs/adr/0012-keeping-the-layout-in-step.md), [ADR 0014](docs/adr/0014-edit-mode.md)). Each cell says where it was in the cells the host last read, so `AppsGrid` moves the same view, icon and all, to its new place. When cells come, go or move, the shell's grid-change motion runs for 200 ms: new cells grow and fade in, cells that left shrink away where they were, and the rest slide to their new places from where they are drawn (ANIM-21). A revision that only refreshes cells, such as a new accent, keeps a change playing. A new app has a dot in the accent colour before its label until it is opened from MeeGone (APPS-07).
- **Edit mode** ([ADR 0014](docs/adr/0014-edit-mode.md)). The cells shrink to 92 % and the ✕ badges pop in row by row, from frame slots per visible row (ANIM-12). A dragged icon lifts to 110 % with a shadow, stays under the finger as cells move and the grid scrolls, and settles into its cell on a spring (ANIM-13). `DragShadow` draws it above the pages, so it can leave an open folder, and its cell stays as a gap (ADR 0015). Done is a flat pill at the bottom of Applications. Screen readers get "Uninstall ‹app›" and "Move to…" actions on each cell, and hear "Edit mode" (A11Y-01, A11Y-05, A11Y-08). The cell's menu offers the same moves to everyone, and a keyboard moves the focused cell with Ctrl and the arrows, Home or End, and uninstalls it with Delete (A11Y-06, ADR 0039).
- **Folders** ([ADR 0015](docs/adr/0015-folders.md)). `AppCell` draws a folder as a plate in the icons' shape with a 2×2 preview of its first apps, and the icon held over grows and pulses (FOLD-02, ANIM-14). `FolderPanel`, inside the Applications page, draws the scrim and a panel four cells wide, with the folder's name over its own `AppsGrid`, growing from the folder's icon (ANIM-15). A tap on the name opens a rename dialog (FOLD-03). Screen readers hear "Folder: ‹name›, N apps", and make and empty folders with actions (A11Y-01, A11Y-05).
- **The Events header** ([ADR 0019](docs/adr/0019-the-events-header.md)). `EventsHeader`, at the top of the Events page, draws the weekday and the date in 40 sp light type that scales with the system font size and wraps rather than overlap, and the alarm row below them (EVT-01, A11Y-04). It reads its text with `nativeEventsHeader` only when its revision changes, and the alarm row calls `nativeOpenAlarm`.
- **The Events list** ([ADR 0020](docs/adr/0020-listing-notifications-in-events.md), [ADR 0021](docs/adr/0021-acting-on-notifications-in-events.md)). Below the header, in one `ScrollView`, `EventsList` draws the notification-access card, "No new events", or the "Notifications" section and its rows, as the frame says: each group, and below the expanded group's row, its notifications. It reads the rows with `nativeEventsList` only when their revision changes. Each row says where it was, so new rows slide in and rows that went collapse as the frame's progress says (ANIM-20). Allow calls `nativeAllowNotifications`.
  - Rows are clickable and name themselves by revision and place: a tap calls `nativeEventsRowTapped` with the row's rect (ANIM-08), and `HomeViews`' long-press timer calls `nativeEventsRowLongPressed`. Screen readers get "Open", "Expand" or "Collapse", and "Open menu" (A11Y-05).
  - Clear, at the end of the section header, shows while the frame offers it and calls `nativeClearNotifications`. The menu card shows a notification's menu at its row, and reports a choice with `nativeEventsMenuChosen`.
- **The image-of-the-day card** ([ADR 0022](docs/adr/0022-the-image-of-the-day-card.md)). Below `EventsList`, in the same `ScrollView`, `ImageCard` draws an "Image of the day" section header and a flat card in the surface colour: the thumbnail at 16:9 (at most 240 dp tall), the title and the attribution in full. It reads them with `nativeImageCard` only when their revision changes. It decodes the middle of the cached image, from the app's files folder, on a background thread, and fades a new one in as the frame's progress says (ANIM-22). A tap calls `nativeImageCardTapped`. `SectionHeader` draws Events' section headers.
- **The image details screen** ([ADR 0031](docs/adr/0031-the-image-details-screen.md)). `ImageDetails`, a `ScrollView` over the home views in the surface colour, draws the image full width (at most 4:3 of it, and 55 % of the screen), decoded on `ImageCard`'s thread, then the title, the attribution, "‹date› · ‹region›" (the region named by `Regions`, ADR 0040), the status line, and the rows More info, Unpin while pinned, Update now, Past week and Settings. It reads them with `nativeImageDetails` and `nativeImageStatus` only when slots 135 and 136 change. Every text is plain text (SEC-02). The rows call `nativeImageMoreInfo`, `nativeUnpinImage`, `nativeImageUpdateNow`, `nativeOpenPastImages` and `nativeImageDetailsSettings`.
- **The past week's images** ([ADR 0032](docs/adr/0032-browsing-the-past-week-and-pinning-an-image.md)). `PastImages`, a `ScrollView` above Settings and the details screen in the surface colour, draws a card for each image: the preview at 5:3 (at most 240 dp tall), the title, the attribution, the date with "On the lock screen" for the current image, and a row with Pin, Unpin, "Pinning…" or "Not offered as a wallpaper". It reads them with `nativePastImages` only when slot 138 changes; slot 137 shows it. Each preview's bytes come from `nativePastImagePreview` on the main thread and are decoded on `ImageCard`'s thread at the size shown, kept by image id until the screen hides. Pin and Unpin call `nativePinImage` and `nativeUnpinImage`, and are named for screen readers ("Pin ‹title›"); Try again, after a problem, calls `nativePastImagesTryAgain`.
- **Recent** ([ADR 0023](docs/adr/0023-recent-cards.md)). The Recent page is a `ScrollView` holding `RecentList`: a "Recent" section header, then the usage-access card, "No recent apps", or the cards, as the frame says. It reads the cards with `nativeRecentCards` only when their revision changes.
  - The cards fill rows of the frame's columns from the start edge. Each is a 12 dp rounded plate in the app's accent, with the window's proportions within 0.6 to 2.2 but no taller than lets the frame's rows fit on one screen below the title, and the icon in the middle. The label and the age are below the plate.
  - `RecentList` reports its cards' width with `nativeRecentLayout`. `HomeViews` passes the first two fingers to `nativePinch`. While they pinch, the grid scales about where they met. When the frame's reflow starts, each card moves from where it was drawn to its place (ADR 0024). The home views offer "Bigger cards" or "More cards" to screen readers while the frame says so.
  - A tap calls `nativeRecentCardTapped` with the plate's rect, and Allow calls `nativeAllowUsage`. Screen readers hear "Mail, 5 minutes ago", with "Open" as the action.
  - Its icons, like the grid's, set their bounds and filter at each draw, as `AppIcons` shares one drawable per app. `AccessCard` draws Events' and Recent's access cards.
  - **Close mode** ([ADR 0025](docs/adr/0025-recent-close-mode.md)):
    - A long-press on a card reaches `nativeLongPress`, as one on empty space does. In close mode, a tap where nothing takes taps calls `nativeRecentEmptyTapped`.
    - Each card's body, inside the view the reflow moves, takes close mode's 92 %. `Badge`, shared with edit mode's cells, draws the ✕ on the plate's top-start corner, popping in row by row. A tap in the card's top-start 48 dp square calls `nativeRecentBadgeTapped`.
    - Clear all, at the end of the section header, fades in with close mode and calls `nativeRecentClearAll`.
    - Each card says where it was in the cards read before. A card no card keeps shrinks and fades where it was, as the frame says for its row, while the others reflow into the gap. "No recent apps" shows once it has gone.
    - When the frame's removals change, "Removed from Recent" shows briefly. With touch exploration on, the status line says it instead, e.g. "Mail removed from Recent" (A11Y-08). The home views offer "Edit Recent" while the frame says so, and the status says "Editing Recent" on entering.
- **Settings** ([ADR 0035](docs/adr/0035-the-settings-screen.md)) covers the home views with its name and one `ScrollView` of its groups, in SET-02's order. Each group is built from `SettingsRows`, so they look alike, and is offered only while its slot says so. Opened at the Lock-screen image group (slot 17 is 2), it scrolls to that group's heading, which takes screen readers' focus. Its dialogs, the region choice, the licences and Reset launcher's question, are laid out by the device's light or dark dialog theme and drawn by `Dialogs` in MeeGone's colours, with 48 dp buttons (ADR 0038), and close with Settings.
  - Appearance → Theme, a `ChoiceSettings` with a radio button for System, Light and Dark, which calls `nativeTheme`; the frame's slot 140 says which is chosen ([ADR 0033](docs/adr/0033-themes-and-the-theme-cross-fade.md)).
  - `HomeSettings`: Home → "Home button opens", a `ChoiceSettings` for the last view or always Applications (`nativeHomeTarget`, slot 142), and Wrap-around (`nativeWrapAround`, slot 143).
  - `HiddenAppsList`: Applications → Hidden apps, each app with its icon and Unhide, read by revision like the cells ([ADR 0017](docs/adr/0017-hiding-apps.md)).
  - Recent → Time window, a `ChoiceSettings` with a radio button for each choice, which calls `nativeRecentWindow` with its hours; the frame's slot 127 says which is chosen ([ADR 0026](docs/adr/0026-recent-time-window.md)).
  - `ImageSettings`: Lock-screen image ([ADR 0030](docs/adr/0030-setting-the-lock-screen-wallpaper.md)):
    - "Daily lock-screen image" (`nativeDailyImage`), disabled with `nativeImageUnavailableReason` while slot 130 says so;
    - below it, the status line from `nativeImageStatus` while slot 136 says so: "Last updated …" and the last problem ([ADR 0031](docs/adr/0031-the-image-details-screen.md));
    - while slots 132 and 133 say so, the pause with Resume, and that the image may not appear;
    - while the status line says so, "Pinned. Daily updates are paused." with Unpin (`nativeUnpinImage`);
    - "Daily image unavailable" with Try again (`nativeImageTryAgain`, [ADR 0029](docs/adr/0029-scheduling-the-daily-image-check.md));
    - Region, named by `Regions` in the device's language, which opens a single-choice dialog of Automatic and then the feed's regions from `nativeRegions` in the language's order (ADR 0040), read each time Settings opens; slot 148 says which is chosen, and `nativeRegion` saves one;
    - "Use mobile data" (`nativeUseMobileData`) and "Show card in Events" (`nativeShowImageCard`, [ADR 0022](docs/adr/0022-the-image-of-the-day-card.md));
    - while the image is on, "Choose from the past week" (`nativeOpenPastImages`, [ADR 0032](docs/adr/0032-browsing-the-past-week-and-pinning-an-image.md));
    - "Update now" (`nativeImageUpdateNow`), "Updating…" as slot 131 says;
    - while slot 134 says so, "Reset lock screen to home wallpaper" (`nativeResetLockScreen`) and "Wallpaper settings" (`nativeOpenWallpaperSettings`).
  - `AccessSettings`: Access → Default home app, Notification access and Usage access, each with its status from slots 144 to 146 and a button that calls `nativeDefaultHome`, `nativeNotificationAccessSettings` or `nativeUsageAccessSettings`. While slot 147 names an item, `nativeAccessInstructions` is shown below it.
  - `AboutSettings`: About → the installed package's version, the privacy notice (PRIV-04), the non-affiliation statement (LEGAL-05), "Open-source licences", which shows the Rust standard library's licences from `res/raw` (LEGAL-06), and, while slot 149 says so, Reset launcher, which asks first and then calls `nativeResetLauncher`.
  - `HomeActivity` answers the system's `APPLICATION_PREFERENCES` entry, such as the gear beside MeeGone in Default apps → Home app, with `nativeOpenSettings` (SET-01).
- **First-run setup** ([ADR 0036](docs/adr/0036-first-run-setup.md)). `SetupScreens`, a `ScrollView` above every other screen and below the menu, in the home views' background, draws the step slot 150 names, each a pane with its own title, built from `SettingsRows`.
  - Welcome with Continue (`nativeSetupContinue`); the default home app with Not now (`nativeSetupNotNow`) and Make default (`nativeDefaultHome`), and the instructions of slot 147.
  - The lock-screen image: today's preview at 5:3 (at most 240 dp tall), decoded on `ImageCard`'s thread from `nativeSetupImagePreview`, its title and attribution from `nativeSetupImage`, read only when slot 153 changes; the switch as slot 152 says (`nativeSetupImageSwitch`); Region, whose dialog is Settings' (`nativeRegions`, read when the step shows, and `nativeRegion`); and Continue. With slot 152 at -1, Continue alone.
  - Access: for each item its sentence (ONB-02), Not now and Allow (`nativeNotificationAccessSettings`, `nativeUsageAccessSettings`), then its status once answered, as slots 145, 146 and 151 say.
  - `HomeBannerView`, a flat card at the bottom of the home views inside the safe area, shows while slot 154 says so, with Dismiss (`nativeHomeBannerDismissed`) and Make default (`nativeHomeBannerMakeDefault`). A long-press on it opens no menu.
- **Search** ([ADR 0016](docs/adr/0016-app-search.md)). `SearchPanel`, inside the Applications page, draws the field in a pill as far as the frame's reveal, and moves the grid down with it. Once something is typed, its own `AppsGrid` shows the results instead of the grid, which keeps its scroll position. Each change of the field's text goes to Rust; Go launches the first result. The window asks for `adjustResize`, and only the results take the keyboard's height, as padding, so the safe area never changes with it.
- **Icons and accents** ([ADR 0011](docs/adr/0011-icon-shape-and-app-accents.md)). Every icon is one rounded square: adaptive icons are masked to it, and legacy ones sit on a white plate of that shape (APPS-15). Each app's accent comes from its icon once per process: `AppAccents` samples the icon on a background thread, and the Rust app's pure `nativeAccent` picks the colour (APPS-17). It reaches Rust in the app catalogue, and a theme draws it with `Tokens::app_accent`, at 3:1 or more against its background (THEME-07).
- **Long-press** (ADR 0008, ADR 0013).
  - The host times a finger held still. On empty space, or on a Recent card, it sends `LongPress` (ADR 0025); on a cell's icon, `LongPressCell`, named by the surface, the cells' revision and the cell's place; on an Events row, `LongPressNotification`, named by the rows' revision and the row's place (ADR 0021).
  - Empty space is where no clickable or long-clickable element is under the finger, so any other element with its own long-press only needs to be clickable.
  - The shell takes a long-press only while the finger is within the touch slop. Once taken, the rest of the gesture is the shell's, so lifting the finger taps nothing.
  - The menus and Settings cover the home views: while any is open, the carousel gets no new touches. The one card shows the home menu at the finger, a cell's menu at its icon, or a notification's menu at its row. An open folder locks the carousel instead, so long-presses in its panel still count.
- **The theme** ([ADR 0033](docs/adr/0033-themes-and-the-theme-cross-fade.md)). `HomeActivity` handles the device's dark theme in place, so no view is made again: when its night bits change, it calls `nativeSystemThemeChanged` and redraws.
  - When a frame brings a new background while slot 139 says the fade has just started, `ThemeFade` records `HomeViews` as a `Picture` first, in the old theme, which the GPU draws into a bitmap, as the icons are hardware bitmaps (ADR 0041). It puts it in the view's overlay, above everything and taking no touches, fades it out as slot 139 says, and lets it go at 10 000.
  - The system bars switch at once to the new background (ADR 0006). An open rename dialog opens again at once in the new theme, with the name as it is being typed.
  - `Themed` gives Android's own widgets MeeGone's colours rather than the device's: a text field's cursor, selection handles and highlight, and every scroll view's edge glow and scrollbar thumb.
- **Text** is in Android string resources (L10N-01, [ADR 0040](docs/adr/0040-localisation-and-right-to-left-layouts.md)).
  - **Problems:** the Rust app never writes words for the user. A problem reaches the host as its `Problem` code, and `Problems.text` words it from `problem_<code>` in the language of the moment: every toast through `HomeViews.toast` or `AppsGrid`'s, the status line, the past week's problem, the image's unavailable reason and the written instructions. A `Bridge` method that fails returns a code too.
  - **Numbers** reach the screen in the locale's digits, through a string's `%d`, a plural, or `NumberFormat`.
  - **Dates and times** are written by ICU. A weekday or a date is capitalised as at the start of a line, and a time as inside a sentence.
  - **Regions** are named by ICU's `LocaleDisplayNames` and listed in the language's order (`Regions`).
  - **Right to left:** the views take the direction they are laid out in, which `HomeViews` sends with each `Layout`. Rust mirrors what it places, and the host lays out start and end. Debug builds have the pseudo-locales en-XA and ar-XB for the audit.
- **Accessibility** comes from Android's views ([ADR 0038](docs/adr/0038-named-reachable-legible-and-scalable.md)).
  - **Gestures' alternatives** (A11Y-05, [ADR 0039](docs/adr/0039-gestures-keyboards-switch-access-and-reduced-motion.md)): each view has its own element, `ViewNode`, first in its page. It is named "Applications, 2 of 3". Its press is the view's long-press, the home menu or Edit Recent. It offers Next view, Previous view, Search apps and Bigger or More cards. Screen readers focus it, which they never do the container, and Switch Access and Voice Access offer its actions. The container keeps only the scroll and page actions. Each app's cell, notification row and Recent card offers its long-press as its long-click action.
  - **Keyboards and switches** (A11Y-06):
    - **Keys:** Tab, the arrows and Enter reach every element, Android's own way. `HomeViews.dispatchKeyEvent` maps the other keys to the same actions screen readers use: Menu or Shift+F10 for a long-press, Delete for the ✕, and ← or → past a view's edge for the next view. In edit mode, Ctrl with the arrows, Home or End moves a cell. Typing on Applications opens search. Esc is Back. Android's list of shortcuts (Meta+/) shows them all.
    - **Panes keep the focus:** only the topmost of the views, the image details, Settings, the past week, setup and a menu takes it, or an open folder, as only it takes touches. When one opens, its first element takes the focus. When one closes, the focus goes back to what it was for.
    - **The focus ring:** `FocusRing` draws it around the focused element, out of touch mode, in the accent with a halo in the background colour, over the home views and every dialog. `PAIRINGS` holds it to 3:1.
    - `Scroll`, every scrolling column's class, keeps a focused element clear of the bars. With animations removed, it scrolls at once (A11Y-07).
  - **Names** (A11Y-01): `A11y.appName` names an app wherever it is named, with "work" for a work-profile app. `A11y.describe` adds a state, after the name on Android 10. `A11y.notAButton` keeps the containers that only keep touches, such as Settings and the menu card, from reading as buttons. The ✕ is a named action on its cell or card.
  - **Targets** (A11Y-02): `A11y.TARGET_DP` is every target's least size. A view that takes taps in places of its own, such as the ✕, makes its target 48 dp on screen at its scale.
  - **Contrast** (A11Y-03): every colour is a token. `PAIRINGS` in `tokens.rs` lists each pairing the host draws, which a test holds to its ratio in both themes. Every press is `SettingsRows.pressed`, which draws nothing for focus alone (ADR 0039). Unavailable apps keep their contrast: a grey icon at half opacity, with the label, dot and Recent plate in the secondary text colour.
  - **Large text** (A11Y-04): `ButtonRow` stacks buttons that don't fit side by side, `SectionHeader`'s title wraps before its button, and the menu card's items scroll.
  - **Announcements** (A11Y-08) go through `HomeViews.announce` and the polite live region, a repeat read again. When a menu, a folder or search closes, screen readers' focus goes back to the cell or row it was for.
  - **The audit:** debug builds go through every screen in both themes and check it as drawn (`A11yTour`, `A11yAudit`). `tools/a11y-audit.sh` starts it at font scale 1.0, at 2.0, and with animations removed. A view that draws its own text or targets reports them through `Audited`. The audit also checks reach (ADR 0039):
    - every action MeeGone adds is on an element screen readers can focus;
    - Tab reaches every element on top that takes a tap or a long-press, stays on what is on top, makes no stop that does nothing, and goes in reading order;
    - the focus ring has 3:1.

## Platform adapters (ARCH-04)

Everything MeeGone asks of Android goes through eight narrow traits in `meegone-core-data`, handed to the shell once as `Platform`: the seven in `adapters.rs`, and `StorageAdapter` (`storage/`) for MeeGone's own files. Each has a fake in `meegone_core_data::fakes`, so the core logic of MAINT-01 runs and is tested without a device. The decisions behind them are in [ADR 0003](docs/adr/0003-platform-adapters.md) and, for storage, [ADR 0004](docs/adr/0004-persistent-storage.md).

On Android, `meegone_core_data::android` builds the `Platform` ([ADR 0005](docs/adr/0005-android-host-and-home-role.md)). Its adapters ask a Java `Bridge` object through a minimal, hand-written JNI binding.
- Real today:
  - The home-app role, whether the system animates the return home, the haptic tick, the clock, MeeGone's install time and storage.
  - The app catalogue ([ADR 0010](docs/adr/0010-the-applications-grid.md)) and the default apps ([ADR 0012](docs/adr/0012-keeping-the-layout-in-step.md)).
  - Shortcuts, App info and uninstall requests ([ADR 0013](docs/adr/0013-the-app-long-press-menu.md)).
  - Folding text for search, whether a browser is installed, and web search ([ADR 0016](docs/adr/0016-app-search.md)).
  - Writing dates and times, and the next alarm ([ADR 0019](docs/adr/0019-the-events-header.md)).
  - Notification access, the active notifications through MeeGone's `NotificationListener`, and the access screen ([ADR 0020](docs/adr/0020-listing-notifications-in-events.md)). Opening and dismissing notifications through the listener, and an app's notification settings ([ADR 0021](docs/adr/0021-acting-on-notifications-in-events.md)).
  - Usage access, the apps used recently through `UsageStatsManager`, and the access screen ([ADR 0023](docs/adr/0023-recent-cards.md)).
  - The network's state, HTTPS requests, and the device's locale ([ADR 0027](docs/adr/0027-the-image-feed-client.md)).
  - The lock screen's size, cropping the daily image, and free storage ([ADR 0028](docs/adr/0028-downloading-and-cropping-the-daily-image.md)).
  - The daily image's job through `JobScheduler` and `ImageJobService`, battery saver and Data Saver ([ADR 0029](docs/adr/0029-scheduling-the-daily-image-check.md)).
  - Setting, reading and resetting the lock screen's wallpaper through `WallpaperManager`, the system's wallpaper settings, whether the device folds, and "Update now" on the job's thread ([ADR 0030](docs/adr/0030-setting-the-lock-screen-wallpaper.md)).
  - Opening `https://` links in the default browser, or only in a browser the user picks ([ADR 0031](docs/adr/0031-the-image-details-screen.md), [ADR 0042](docs/adr/0042-security-and-privacy.md)).
  - Browsing the past week and pinning an image on the job's thread, with the rest of the user's tasks ([ADR 0032](docs/adr/0032-browsing-the-past-week-and-pinning-an-image.md)).
  - The device's dark theme, as a question and as `SystemThemeChanged` ([ADR 0033](docs/adr/0033-themes-and-the-theme-cross-fade.md)).
  - The system animation speed, as a question and as `AnimationScaleChanged` ([ADR 0034](docs/adr/0034-the-shared-motion-system.md)).
- Each answers with its "If unavailable" value below when Android can't.

| Capability (SRS 8.1) | Adapter | Methods | If unavailable |
|---|---|---|---|
| Home-app role | `SystemConfigAdapter` | `is_default_home`, `request_default_home` | Banner (ONB-04) |
| App catalogue: apps, profiles, shortcuts, search keys | `AppsAdapter` | `launchable_apps`, `profiles`, `launch`, `shortcuts`, `launch_shortcut`, `search_key` | `shortcuts` is `Unsupported`: menus show none; `search_key` only lowercases |
| Default phone, messaging, browser and camera apps | `AppsAdapter` | `default_apps` | None: the first grid is alphabetical |
| App catalogue: install, removal, suspension, profile and language events | `Bridge`'s `LauncherApps` callback and its broadcast receiver | `ShellIntent::AppsChanged` | – |
| Notification access | `NotificationsAdapter` | `access`, `active`, `open`, `dismiss`, `open_app_settings`; `ShellIntent::NotificationsChanged` | `access` is `Missing`: explainer card |
| Usage access | `UsageAdapter` | `access`, `used_since`; `ShellIntent::UsageAccessChanged` | `access` is `Missing`: explainer card |
| Setting the wallpaper | `WallpaperAdapter` | `lock_screen_support`, `set_lock_screen`, `reset_lock_screen` | `lock_screen_support` is `Unsupported`: feature unavailable |
| Which wallpaper the lock screen has (ADR 0030) | `WallpaperAdapter` | `lock_screen_marker`, and the marker `set_lock_screen` returns | `None`: never paused (WALL-14) |
| The screen the lock-screen image is cropped for (ADR 0028, ADR 0030) | `WallpaperAdapter` | `screen`, `folds` | An empty size: the download fails without a request |
| Decoding and cropping the lock-screen image (ADR 0028) | `WallpaperAdapter` | `crop_image` | `Invalid`: the image is discarded and the current one kept |
| Network and network state | `NetworkAdapter` | `state`, `get` | `Offline`: retry later |
| Uninstall requests (`REQUEST_DELETE_PACKAGES`, granted at install) | `AppsAdapter` | `request_uninstall`, and `App::can_uninstall` | No Uninstall item, no ✕ |
| Background scheduling | `SchedulerAdapter` | `schedule_image_job`, `cancel_image_job`, and `run_image_tasks` on the job's thread (ADR 0030, ADR 0032) | `Unsupported`: check at the next start; "Update now", browsing, pinning and setup's preview and first check still run |
| Battery saver and a low battery (ADR 0029) | `SystemConfigAdapter` | `saves_battery` | `false`: the job's scheduler still waits while the battery is low |
| Data Saver (ADR 0029) | `NetworkAdapter` | `saves_data` | `true`: automatic downloads wait for an unmetered network |
| Next alarm | `SystemConfigAdapter` | `next_alarm_ms`, `open_next_alarm` | `None`: alarm row hidden |
| Vibration | `SystemConfigAdapter` | `haptic_tick` | Does nothing |
| System settings screens | The adapter that owns the subject | `open_access_settings` (notifications, usage), `open_app_info`, `open_wallpaper_settings`, `request_default_home` | `Unsupported`: written instructions |
| Web browser | `SystemConfigAdapter` | `has_browser`, `open_in_browser`, `web_search` | `has_browser` is false: link and web search hidden |
| Device settings and clock | `SystemConfigAdapter` | `is_dark`, `animation_scale`, `animates_return_home`, `now_ms`, `utc_offset_ms`, `locale`, `format_time`; `ShellIntent::ClockChanged`, `AnimationScaleChanged` | `is_dark` is false: System is Light; `animation_scale` is 1: normal speed; `animates_return_home` is true: no return-home motion; `format_time` is empty: that line of the header is left out |
| MeeGone's install time (ADR 0018) | `SystemConfigAdapter` | `installed_ms` | 0: a restore is recognised only by the other device's time |
| Free storage (ADR 0028) | `SystemConfigAdapter` | `free_storage_bytes` | `None`: the download goes ahead, and a full disk fails the write |
| MeeGone's own files (ADR 0004) | `StorageAdapter` | `read`, `write`, `remove` | A failed save is the feature's `problem`; an unreadable file loads as defaults |

- **Adapters never call back.** They answer questions and carry out requests. A change the system reports reaches MeeGone as a `ShellIntent` (`Home`, `Back`, `Resumed`, `Returned`, `Restore`, `SystemThemeChanged`, `AnimationScaleChanged`, `AppsChanged`, `NotificationsChanged`, `UsageAccessChanged`, `ClockChanged`, `ImageChecked`, `Failed`) that the platform's entry point passes to `Launcher::dispatch`. The Android activity sends `Resumed` on every `onResume`, including after the home-app role dialog, and `Returned` after it when an app or a system screen had covered the home (ADR 0007). It sends `SystemThemeChanged` when the device's dark theme turns on or off (ADR 0033). `Bridge` sends `AnimationScaleChanged` when the system animation speed changes (ADR 0034). `Bridge` sends `AppsChanged` once for each burst of app, profile and language changes (ADR 0010, ADR 0012), `ClockChanged` once for each burst of date, time, time-zone, 12- or 24-hour and next-alarm changes (ADR 0019), `NotificationsChanged` once for each burst of notifications posted or removed and of the listener connecting or disconnecting (ADR 0020), and `UsageAccessChanged` once for each burst of changes to MeeGone's usage access (ADR 0023). `ImageJobService` sends `ImageChecked` after each check that reached the feed (ADR 0029), and after each run of the tasks the user asked for, such as "Update now" (ADR 0030, ADR 0032). `HomeViews` sends `Failed` when it catches a bug in a feature's own views (ADR 0043).
- **Expected failures are values.** Adapters return a `DataError` (`AccessDenied`, `Unsupported`, `Offline`, `Invalid` or `Failed`), and each feature turns it into its own state (ARCH-09). `Unsupported`, `Invalid` and `Failed` carry a `Problem`, never text, and `DataError::problem` is what the user is told (L10N-01, ADR 0040).
- **The image feed is the lock-screen image's own.** `ImageFeed` in `meegone-lockimage-data` returns format-free `FeedImage`s. Its one implementation, `BingFeed`, built on `NetworkAdapter`, is the only code that knows the Bing feed's format (SRS 8.2, ADR 0027). Its fake is `FakeFeed`.
- **Fakes.** Enable the `fakes` feature of `meegone-core-data` (and of `meegone-lockimage-data` for `FakeFeed`) in `[dev-dependencies]`. Every fake has public fields that set what the "system" reports and record what was asked of it. Its `error` field makes every call that can fail fail, and other fields reproduce each "if unavailable" case.
- **Bugs made to order.** Enable the `faults` feature of `meegone-core-state` in `[dev-dependencies]`. `faults::arm("EventsHolder")` makes that feature's next contained step panic, once, as a bug would; `tests/containment.rs` uses it (ADR 0043).

## Failure containment (ARCH-09)

Launching apps works whatever else fails (REL-01). The decisions are in [ADR 0043](docs/adr/0043-failure-containment-and-crash-free-sessions.md).

Expected problems are part of each feature's own state. Examples are missing access (`Access::Missing`), a failed adapter call (`problem`, a `Problem`) and an unsupported device (`ImageStatus::Unavailable`). SRS 7.6's table has a test for each row, each ending with an app opened from Applications (`tests/acceptance.rs`).

Bugs are contained by each feature's `Store`, its fuse:
- **Wherever a feature's code runs.** `Store` runs every intent, and the listeners that publish its snapshot, inside `catch_unwind`. Its `contain` does the same for the feature's code run elsewhere: each feature's part of `ShellView::frame`, the shell's calls to its state, such as Events' `row_keys`, and every reader and action of `src/android.rs` (through `HomeShell::contain`). The closure only computes; the caller changes its own state with the result.
- **A panic stops that feature for good.** Its `Health` turns `Failed`, which keeps nothing of the panic (PRIV-03). Its store never runs it again, and its last snapshot stays. `HomeShell::failures` names every failed `Feature`; `Launcher::frame` dispatches `ShellIntent::Failed` for each new one, so what it covered closes, and draws again in the same frame.
- **The host stops a feature too.** `HomeViews` draws each feature's views inside its own `try`, catching `RuntimeException` and `OutOfMemoryError`. A catch hides that feature's views without running their code, logs only the exception's kind, and calls `nativeFeatureFailed`. The frame's slot 155 lists the failed features, so the host stops drawing one that failed in Rust too.
- **What stays uncontained.** Applications' own views and the shell's own logic: a bug there is a crash, which the platform's reports count (REL-07). A failed Applications' last grid still scrolls, and a tap on an app there still opens it, through the shell (`launch_from_last_grid`).
- **What the user sees.** A card says the part stopped working, with Restart MeeGone, where the frame's slot 156 says: Applications' in the home-app banner's place, Events' and Recent's on their pages, the lock-screen image's in Events where its card was and in Settings, and Settings' at its top. `HomeViews` owns the cards, so none of the failed part's code runs to show them. `Restart` starts the home activity again and ends the process with code 0, which Android records as the app exiting itself, not as a crash.
- **Saved files and the job.** A file whose reading panics is unreadable, so it loads as the defaults or is salvaged (`format::decode`, `format::salvage`), rather than stopping MeeGone at every start. A daily check that panics is a failed attempt, with its back-off, its status and, after 3 days, "Daily image unavailable". A task that panics ends, and the queue goes on.
- **Logs.** The panic hook writes each panic's file and line to logcat, once, never its message (PRIV-03). `HomeViews` logs each feature that stops, by its number.

This requires `panic = "unwind"`, which is Cargo's default: `meegone-core-state` doesn't compile without it. A panic that reaches the Android entry points outside any feature, such as in the shell, becomes a Java exception that names only its file and line, and crashes the app (ADR 0005, ADR 0042).

## Sources of truth (ARCH-06)

| Entity | Single source of truth | Written by |
|---|---|---|
| App | The system, through `AppsAdapter` (memory only) | The system |
| Layout item, folder, new-app mark, a restore's waiting apps | `LayoutStore` (`meegone-apps-data`), saved in `layout` | Applications |
| Settings | `SettingsStore` (`meegone-core-data`), saved in `settings` | Each field has one writer: Settings (theme, Home target, wrap-around, when the home-app banner was dismissed, which install finished setup), Applications (hidden apps), Lock-screen image (image options), Recent (its time window, its density, apps removed from Recent) |
| Image record, its image, its pin and the check log | `ImageCache` (`meegone-lockimage-data`), saved in `images/details`, `images/a` and `images/b` | Lock-screen image: its holder, and its updater on the job's thread, through the same `Shared` cache |
| The past week's images and previews, today's image for setup, and the tasks for the job's thread | `ImageMemory` (`meegone-lockimage-data`, memory only) | Lock-screen image: its holder queues tasks, and its updater, on the job's thread, writes the past week, through the same `Shared` memory |
| Notification group | The system, through `NotificationsAdapter` (memory only) | The system |
| Recent entry | The system, through `UsageAdapter` (memory only) | The system |

A view holds only the snapshot it renders. Content shown inside another feature holds only a `Latest<State>`, which the shell keeps current.

## Persistence (REL-03, MAINT-04)

MeeGone keeps three persistent stores, each in its own file under one private folder. The decisions are in [ADR 0004](docs/adr/0004-persistent-storage.md).

| Store | Type | File | Format version |
|---|---|---|---|
| Layout | `PersistentLayoutStore` (`meegone-apps-data`) | `layout` | `LAYOUT_VERSION` |
| Settings | `PersistentSettingsStore` (`meegone-core-data`) | `settings` | `SETTINGS_VERSION` |
| Image cache | `PersistentImageCache` (`meegone-lockimage-data`) | `images/details`, and the images in `images/a` and `images/b` | `IMAGE_CACHE_VERSION` |

- **Where.**
  - `StoredFile` names every file, so nothing else can be stored. Notification content and usage history have no file (PRIV-03, EVT-13).
  - The host gives `FileStorage` a folder in the app's private internal storage (`Context.getFilesDir()`). Android deletes that folder on uninstall and on "Clear storage" (PRIV-05). The lock screen's wallpaper is the system's, and keeps the last daily image until the user changes it, as the privacy notice says (ADR 0042).
- **Atomic writes.** `FileStorage` writes `<file>.tmp`, flushes it, renames it over the file and flushes the folder. A crash at any point leaves the old file or the new one (REL-03). Its tests stop writes part-way to check this.
- **Format.**
  - Text: a `meegone-<kind> <version>` header, one `key field …` record per line, and an `end <checksum>` trailer.
  - A file that is cut short, damaged, of another kind or from a newer version loads as the defaults instead of crashing (MAINT-04).
  - The layout's records stand alone, so a damaged layout keeps the ones that still read, and the grid is rebuilt around them and saved at once (REL-02, ADR 0012). A layout that can't be read at all, because storage failed, is shown rebuilt but never saved over.
- **Budget.** A write that would take stored data past 50 MB fails and changes nothing (PERF-08).
- **Backup** ([ADR 0018](docs/adr/0018-backing-up-and-restoring.md)). The system's backup and device-to-device transfer keep `layout` and `settings`, as `res/xml/backup_rules.xml` (Android 10 and 11) and `res/xml/data_extraction_rules.xml` (from Android 12) list them, and nothing else (APPS-21). `tests/backup/mod.rs` checks the lists against `StoredFile`. Android restores them as it installs MeeGone, before MeeGone first runs.
- **Failures.** A failed save keeps the previous data. Each feature shows the error as its `problem`.

### Changing a saved format

1. Bump the store's version constant, for example `SETTINGS_VERSION`.
2. Add a `Migration` to the store's migrations array, which turns the previous version's body into the new one. The array's length is `VERSION - 1`, so the build fails until you add it.
3. Change the store's `Format::write` and `Format::read` for the new body.
4. Add `fixtures/<kind>-v<N>.txt`, written by the new code, and its expected value to the store's `FIXTURES`. Never edit or delete an older fixture: the tests load every one of them through every migration step.

## Performance (ADR 0041)

The budgets of SRS 6.1 apply to the reference device (#48). Each has a repeatable benchmark on a device, and the Rust app's share of it is held in CI. The decisions are in [ADR 0041](docs/adr/0041-performance-budgets-and-benchmarks.md).

| Budget | Measured on a device as | The Rust app's share in CI, with 300 apps |
|---|---|---|
| PERF-01, Home | The system's start of the home to `home-ready`: a drawn frame with every icon in sight, and the main thread idle. 50 Homes from Settings, 95th percentile | `Returned` and `Resumed` to the first frame |
| PERF-02, cold start | The system's "Fully drawn", which `HomeViews` reports once Applications shows with its icons (`reportFullyDrawn`) | – |
| PERF-03, frames | `dumpsys gfxinfo`: frames past their deadline and the longest, over 30 s each of swiping, scrolling and motion by key | One frame of a swipe and its spring |
| PERF-04, saved layout | The system's start to `layout-shown`, the first frame with the saved layout's cells, and no cell moved by `icons-shown` | A cold start to its first frame and cells |
| PERF-05, search | A change of the field's text to the frame with its results | A keystroke to its result cells |
| PERF-06, launch | The finger lifting to `Bridge.launch` having asked the system | A tap to the launch request |
| PERF-07, memory | `dumpsys meminfo`'s total PSS at rest, and the icons held after the system asks for memory back | – |
| PERF-08, storage | The app's files folder; the installed APK. CI also checks the release APK | The bytes stored |

- **The device benchmark** is `tools/perf-bench.sh`. It measures the **benchmark build**: the release build's code, not debuggable but profileable, with the debug build's hooks, installed beside MeeGone as `io.github.twonet.meegone.benchmark`. `PerfProbe`, in the debug source set, logs each mark under `MeeGonePerf`. Its marks and the system's lines share logcat's monotonic clock. Release builds have neither (SEC-06).
- **300 apps:** the benchmark build pads the catalogue with copies of the installed apps, which `DebugHooks.realActivity` maps back to the real ones for icons and launches.
- **CI** runs `cargo bench --bench budgets` (`benches/budgets.rs`). Each median must stay within a tenth of its device budget, and the release APK within 15 MB.
- **Memory** (PERF-07):
  - `AppIcons` keeps each icon once, as a hardware bitmap in graphics memory, and holds the 96 asked for last. An older one lives on only while a view draws it.
  - Grid cells more than half a screen out of sight draw nothing and let their icon go. They draw again, and ask again, as they come near.
  - On `onTrimMemory`, from `RUNNING_LOW` while shown or from `BACKGROUND`, every icon out of sight goes. The home keeps everything at `UI_HIDDEN`, so Home finds it drawn (PERF-01).
  - The image details screen lets its image go when it closes, as the past week and setup already did.
- **Frames** (PERF-03):
  - What a view shows is read again when it comes into sight, but only once nothing moves: the host sends `ShellIntent::Idle` from the main thread's idle handler, never while a finger holds the row.
  - Recent keeps its catalogue, until the apps change, rather than reading every app each time it shows.
- **Results** are recorded per release in `docs/benchmarks/`, from the reference device ([RELEASING.md](RELEASING.md)).

## Simplicity budget (SRS 7.1)

| Item | v1 limit |
|---|---|
| Home views | 3 (`HomeView`) |
| User-adjustable settings | 12 at most; SET-02 lists 10, which a test in `settings.rs` counts (SET-04, ADR 0017, ADR 0035) |
| Setup screens | 4 at most: `SetupStep` (ADR 0036) |
| Persistent stores | 3: layout, settings, image cache (`StoredFile`) |
| Scheduled background jobs | 1: the daily image (`SchedulerAdapter`, `ImageJobService`) |
| External network services | 1: the Bing image feed |
| Servers and accounts | 0 |
| Special access requested | 2, both optional: notifications, usage |

## Not built yet

The skeleton is from issue #1; the Android host and the home-app role from #4; the carousel of home views from #5; Home, Back and coming back to the home from #6; the home menu from #7; edge to edge, screen sizes and navigation modes from #8; the Applications grid and the app catalogue from #9; one icon shape and each app's accent from #10; the layout kept in step with the installed apps from #11; the app long-press menu from #12; edit mode from #13; folders from #14; app search from #15; hiding apps from #16; backup and restore from #17; the Events header from #18; Events' notifications from #19; opening, clearing and dismissing them from #20; the image-of-the-day card from #21; Recent's cards from #22; Recent's density and its pinch from #23; Recent's close mode and Clear all from #24; Recent's time window from #25; the image feed, the network adapter and the region from #26; downloading, checking, cropping and caching the daily image from #27; the daily image check, its retries and back-off from #28; the daily image as the lock-screen wallpaper, "Update now", the pause and the reset from #29; the image details screen, opening links and the update status from #30; browsing the past week and pinning an image from #31; System, Light and Dark themes and the theme cross-fade from #32; the shared motion system and the system animation speed from #33; the Settings screen and Reset launcher from #34; first-run setup and the home-app banner from #35; the product's name, the asset inventory, the store listing and their checks from #36; names, targets, contrast, large text, announcements and the accessibility audit from #37; gestures' alternatives, keyboards, switch access, the focus ring and reduced motion's scrolls from #38; problems as codes, the localisation checks, the locale's numbers, dates, regions and search, right-to-left tests and the pseudo-locale audit from #39; the performance benchmarks on a device and in CI, the benchmark build, icons held only near sight, and caches that shrink under memory pressure from #40; the security and privacy checks of the manifest, the Java host and the release APK, redirects kept on the feed's origin, links only to a browser, progressive images bounded, logs without data, and the traffic audit on a device from #41; each feature's failure contained wherever its code runs, the stopped cards and Restart MeeGone, a failed Applications' last grid, saved files and the daily check that panic, the reliability checks and the fault check on a device from #42; the device and navigation-mode matrix, the compatibility checks, and the compatibility check on a device from #43. The following arrive with later requirements:

- **Android adapters.**
  - Built:
    - The home-app role, the clock and storage (ADR 0005), and MeeGone's install time (ADR 0018).
    - The navigation-mode check for the return-home motion (ADR 0007, ADR 0044) and the haptic tick (ADR 0008), which also marks a folder made (ADR 0015).
    - The app catalogue (ADR 0010), with its shortcuts, App info and uninstall requests (ADR 0013), and its search keys (ADR 0016).
    - Whether a browser is installed, and web search (ADR 0016).
    - Dates and times in the device's language and 12- or 24-hour setting, and the next alarm (ADR 0019).
    - Notification access and the active notifications (ADR 0020), opening and dismissing them, and an app's notification settings (ADR 0021).
    - Usage access and the apps used recently (ADR 0023).
    - The network's state, HTTPS requests and the device's locale (ADR 0027).
    - The lock screen's size, cropping the daily image, and free storage (ADR 0028).
    - The daily image's job, battery saver and Data Saver (ADR 0029).
    - Setting, reading and resetting the lock screen's wallpaper, the wallpaper settings, and "Update now" (ADR 0030).
    - Opening links in the browser (ADR 0031).
    - The device's dark theme (ADR 0033).
    - The system animation speed (ADR 0034).
- **The daily image.** The updater checks, caches and sets the image (ADR 0029, ADR 0030), the details screen and the status line show it (ADR 0031), and the past week can be browsed and an image pinned (ADR 0032). Settings offers the feed's Region (ADR 0035).
- **Drawing.** The home views, their carousel, the view indicator, the return-home motion, the home menu, the Applications grid, its edit mode, its folders, search, Settings' Hidden apps, Recent's time window, the daily image's switch, "Update now", the pause, "Show card in Events", "Use mobile data", "Daily image unavailable", the status line and the reset, the pin and "Choose from the past week", the Events header and notifications, the image-of-the-day card, the image details screen, the past week's images, Recent's cards, their pinch and close mode, the theme cross-fade, the whole of Settings, first-run setup and the home-app banner are drawn (ADR 0006 to ADR 0008, ADR 0010, ADR 0014 to ADR 0017, ADR 0019 to ADR 0026, ADR 0029 to ADR 0033, ADR 0035, ADR 0036). Expanded groups' action buttons (EVT-14, Could) come with a later issue.
  - The pages, the carousel, the safe area, the menu, Settings, the Applications columns and Recent's columns adapt (HOME-12, APPS-02, REC-05).
  - Edit mode's "Sort A–Z" (APPS-14, Could) and folders' category names (FOLD-07, Could) come with later issues.
- **The launcher icon.** There isn't one yet, so Android shows its default. An original icon, recorded in `ASSETS.md`, is needed before release (ADR 0037, `RELEASING.md`).

## Adding a crate

1. Create `crates/<area>/<layer>` with the package name `meegone-<area>-<layer>`, inheriting `version`, `edition`, `rust-version` and `publish` from the workspace.
2. Add it to `[workspace.dependencies]` in the root `Cargo.toml`, and depend on other crates only as the rules allow.
3. A new area means a change to `AREAS` in `tools/arch-check` and an ADR, because it changes the simplicity budget.
4. Add every new third-party crate to `DEPENDENCIES.md` with its reason.

## Checks

These are the commands CI runs (`.github/workflows/ci.yml`):

```sh
cargo fmt --all --check
cargo test -p arch-check                       # architecture, legal, accessibility, localisation, security, reliability and compatibility rules
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench --bench budgets                    # the Rust app's share of each performance budget
cargo coverage                                 # core-logic line coverage, fails below 80 %
cargo clippy --workspace --exclude arch-check --all-targets --target aarch64-linux-android -- -D warnings
(cd android && ./gradlew assembleDebug assembleRelease assembleBenchmark)   # the APKs (separate CI job), lint's
                                               # NewApi on the release build (COMP-01), and the release
                                               # APK within 15 MB (PERF-08)
cargo test -p arch-check --test release_apk -- --ignored   # then the release APK's access, classes and native library
```

Building the APK needs JDK 17 or later, the Android SDK with NDK 27.3 (`ANDROID_HOME`, or `sdk.dir` in `android/local.properties`), cargo-ndk and the `aarch64-linux-android` Rust target. The workspace checks above need none of them.

`cargo coverage` is an alias in `.cargo/config.toml` (MAINT-02). It runs every test in the workspace and measures line coverage of the data and state crates, where the core logic lives (MAINT-01). UI crates, test files and fakes are left out. Inline `#[cfg(test)]` modules still count, because stable Rust can't leave them out. It needs a one-time setup:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

The acceptance scenarios of SRS section 9 run in `tests/acceptance.rs`, one test per scenario, on the whole app with fakes (MAINT-03). A feature adds its scenarios there. `tests/containment.rs` makes each feature fail where its code runs, and checks the rest goes on and an app still opens (ADR 0043).

The accessibility audit runs on a device, not in CI (ADR 0038). With the debug build installed (`(cd android && ./gradlew installDebug)`), `tools/a11y-audit.sh` checks every screen in both themes at font scale 1.0 and 2.0, with animations removed, and on Android 13 or later in the pseudo-locales en-XA and ar-XB, and fails on any problem it finds (ADR 0040). `--first-run` clears MeeGone's data first, so setup is checked too; use it only on a test device.

The fault check also runs on a device, with the debug build, whose Rust app alone makes bugs to order (ADR 0043). `tools/fault-check.sh` starts MeeGone with each fault armed, in a feature's Java views or its Rust code, and checks that the part says it stopped, the process lives on with no crash recorded, and an app still opens. Then it taps Restart MeeGone and checks Android records the app exiting itself, not a crash.

The performance budgets are measured on a device too (ADR 0041). With the benchmark build installed (`(cd android && ./gradlew installBenchmark)`), `tools/perf-bench.sh` measures every budget with 300 apps, puts back what it changed, and fails on any miss. `--record` writes the results to `docs/benchmarks/`, and `--quick` takes fewer samples while trying a change.

Where the traffic goes is checked on a device too (ADR 0042). With the release build and PCAPdroid installed, `tools/net-audit.sh --start` captures MeeGone's traffic alone; after a day of use, `--stop` pulls the capture and checks with tshark that every connection went to `www.bing.com` over HTTPS. `--record` writes the result to `docs/privacy/`, and `--check FILE` reads a capture made another way.

The device and navigation-mode matrix is checked on each of its devices before a release (ADR 0044). With the debug build installed and set up, `tools/compat-check.sh` checks in each navigation mode the device offers that MeeGone reads the mode right, that Home and Back behave and the return motion plays only with buttons, and that swipes move the views but never compete with Back. It turns and folds the device with Settings open, checks each profile's apps are called work only in a work profile, and asks whether the lock screen shows the daily image. It switches the mode itself on Pixels and emulators, and asks for it elsewhere. `--record` writes the results to `docs/devices/`, `--mode` runs one mode and records it in a file of its own, and `--lock-screen` gives the person's answer where there is no terminal to ask in. `RELEASING.md` lists the rows each release needs.
