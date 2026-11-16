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
| **core** | Models, platform adapter traits and fakes, storage and the saved-file format, the settings store | `StateHolder`, `Store`, theme resolution, motion specs and spring physics | Colour tokens, `View`, `Slot`, `Dispatch` | Nothing persistent |
| **apps** | Layout store | Grid, its columns and scroll position, edit mode, folders | `AppsView` | Layout store |
| **events** | – | Notifications grouped by app | `EventsView` | Nothing (memory only) |
| **recent** | – | Filtering, ordering, close mode | `RecentView` | Removed-apps list (in the settings store) |
| **lockimage** | Image cache, image-feed interface | On/off, availability, current image | `ImageCard`, `ImageSettingsSection` | Image cache |
| **settings** | – | Settings, access status, the home-app banner (ONB-04) | `SettingsView`, `HomeBanner` | The settings screens |
| **shell** | – | `HomeShell`: the carousel of views, the home menu, Home and Back, coming back to the home, the icon press, composes every feature | `ShellView`, `HomeFrame` | Nothing persistent |

The root package `meegone-launcher` is the app. `Launcher::start(platform)` hands the platform adapters to the shell. On Android it is also a native library, which the Java host in `android/` loads ([ADR 0005](docs/adr/0005-android-host-and-home-role.md)) and asks what to draw ([ADR 0006](docs/adr/0006-android-views-and-the-carousel.md)). `tools/arch-check` is a development tool that is never part of the app.

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
| Fakes (SEC-06) | A crate's `fakes` feature may be turned on only in `[dev-dependencies]`, so fakes never reach the app. |

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
- **Home and Back** ([ADR 0007](docs/adr/0007-home-back-and-returning.md)). They work the same in every navigation mode; only the return motion depends on it (COMP-02, ADR 0009).
  - Back closes Settings, then the home menu, then goes to the current view's `on_back`, which closes that feature's topmost open element, or does nothing (HOME-08).
  - Home on the home closes Settings, or the home menu and everything open in the current view, then moves to Applications, then scrolls it to the top (HOME-07).
  - Coming back from an app (`Returned`) shows the view the user left. With "Home button opens: Always Applications", Home from an app shows Applications with nothing open (HOME-06).
  - After the system ends the process, the activity's saved state brings back the view and the Applications scroll position (`Restore`, REL-05).
- **The home menu** ([ADR 0008](docs/adr/0008-the-home-menu.md)). A long-press on empty space opens it in Events and Applications, and enters close mode in Recent (HOME-10, REC-07). It lives in `ShellState.menu`, and its items route to Settings and to Applications' edit mode.

## Drawing (ADR 0006)

Android's own views draw MeeGone, and the Rust app decides what they show. The decisions are in [ADR 0006](docs/adr/0006-android-views-and-the-carousel.md).

```
HomeViews (Java)                                       Rust
  dispatchTouchEvent ── nativePointer   ──▶ ShellIntent::Pointer   ─┐
  long-press timer   ── nativeLongPress ──▶ ShellIntent::LongPress ─┤
  Choreographer      ── nativeFrame     ──▶ ShellIntent::Frame     ─┼▶ HomeShell: carousel
  onSizeChanged      ── nativeLayout    ──▶ ShellIntent::Layout    ─┘   (gesture, spring,
                                                                         indicator), home menu
                                                                         and return-home motion
  translationX, scale, alpha, menu ◀── nativeRead(int[]) ◀── ShellView::frame ◀── ShellState
```

- **Java pulls.** After every touch, frame or layout change, `HomeViews` reads the frame and applies it. It asks `Choreographer` for another frame only while the frame says something moves. Rust never calls Java to draw.
- **Motion is state.** A moving element's position and velocity live in the state layer, and the motion specs, easing and spring physics in `meegone-core-state::motion`. Time comes in with each event, on the host's monotonic clock, so tests pass explicit times. The return-home motion (ANIM-09) scales and fades the pages, never the background. The icon press (ANIM-10) scales only the pressed icon, and never delays a launch.
- **Touches.** Every touch reaches the carousel first. The content below keeps a gesture until the carousel takes it: sideways, past the platform's touch slop, from anywhere in the view (HOME-03, HOME-13). An element that uses horizontal drags itself calls `requestDisallowInterceptTouchEvent(true)` first.
- **Edge to edge** ([ADR 0009](docs/adr/0009-edge-to-edge-screens-and-navigation-modes.md)). On every Android version the solid background fills the window behind the system bars, which stay visible (HOME-11). Content stays inside a safe area clear of the bars, cut-outs and rounded corners: each page and Settings get it as padding, so a view's content needs no inset code of its own. Phones stay in portrait; screens 600 dp or more across their smallest width turn freely (HOME-12).
- **Size changes keep state** (COMP-03). Rotating, folding and resizing send a new `Layout`. It ends a drag but keeps the view, Settings, the home menu (now centred) and everything open in the views. Other configuration changes recreate the activity, and the shared launcher keeps everything.
- **Applications** ([ADR 0010](docs/adr/0010-the-applications-grid.md)). `AppsGrid`, a `ScrollView`, draws the frame's Applications slots: the columns and rows (the last one empty, HOME-14), the scroll position and the pressed icon. It reads the cells with `nativeAppsCells` only when their revision changes, and names a tapped cell by revision and index. It reports its width and row height, and the Rust app picks the columns (APPS-02) and keeps the same first visible item when they change. Icons stay in Java: `AppIcons` loads them off the main thread.
- **Long-press** (ADR 0008). The host times a finger held still and, on empty space, sends `LongPress`. Empty space is where no clickable or long-clickable element is under the finger, so an element with its own long-press only needs to be clickable. The shell takes it only while the finger is within the touch slop. The home menu and Settings cover the home views: while either is open, the carousel gets no new touches.
- **Text** is in Android string resources (L10N-01). **Accessibility** comes from Android's views: the home views offer scroll, page and named actions, including "Home menu" (A11Y-05).

## Platform adapters (ARCH-04)

Everything MeeGone asks of Android goes through eight narrow traits in `meegone-core-data`, handed to the shell once as `Platform`: the seven in `adapters.rs`, and `StorageAdapter` (`storage/`) for MeeGone's own files. Each has a fake in `meegone_core_data::fakes`, so the core logic of MAINT-01 runs and is tested without a device. The decisions behind them are in [ADR 0003](docs/adr/0003-platform-adapters.md) and, for storage, [ADR 0004](docs/adr/0004-persistent-storage.md).

On Android, `meegone_core_data::android` builds the `Platform` ([ADR 0005](docs/adr/0005-android-host-and-home-role.md)). Its adapters ask a Java `Bridge` object through a minimal, hand-written JNI binding. Today the home-app role, whether the system animates the return home, the haptic tick, the clock, storage and the app catalogue ([ADR 0010](docs/adr/0010-the-applications-grid.md)) are real; every other adapter answers with its "If unavailable" value below until its issue builds it.

| Capability (SRS 8.1) | Adapter | Methods | If unavailable |
|---|---|---|---|
| Home-app role | `SystemConfigAdapter` | `is_default_home`, `request_default_home` | Banner (ONB-04) |
| App catalogue: apps, profiles, shortcuts | `AppsAdapter` | `launchable_apps`, `profiles`, `launch`, `shortcuts`, `launch_shortcut` | `shortcuts` is `Unsupported`: menus show none |
| App catalogue: install, removal, suspension and profile events | `Bridge`'s `LauncherApps` callback and profile broadcasts | `ShellIntent::AppsChanged` | – |
| Notification access | `NotificationsAdapter` | `access`, `active`, `open`, `dismiss`, `open_app_settings`; `ShellIntent::NotificationsChanged` | `access` is `Missing`: explainer card |
| Usage access | `UsageAdapter` | `access`, `used_since` | `access` is `Missing`: explainer card |
| Setting the wallpaper | `WallpaperAdapter` | `lock_screen_support`, `lock_screen_size`, `set_lock_screen`, `reset_lock_screen` | `lock_screen_support` is `Unsupported`: feature unavailable |
| Network and network state | `NetworkAdapter` | `state`, `get` | `Offline`: retry later |
| Uninstall requests | `AppsAdapter` | `request_uninstall`, and `App::can_uninstall` | No ✕ |
| Background scheduling | `SchedulerAdapter` | `schedule_image_job`, `cancel_image_job` | `Unsupported`: check at the next start |
| Next alarm | `SystemConfigAdapter` | `next_alarm_ms`, `open_next_alarm` | `None`: alarm row hidden |
| Vibration | `SystemConfigAdapter` | `haptic_tick` | Does nothing |
| System settings screens | The adapter that owns the subject | `open_access_settings` (notifications, usage), `open_app_info`, `open_wallpaper_settings`, `request_default_home` | `Unsupported`: written instructions |
| Web browser | `SystemConfigAdapter` | `has_browser`, `open_in_browser`, `web_search` | `has_browser` is false: link hidden |
| Device settings and clock | `SystemConfigAdapter` | `is_dark`, `animation_scale`, `animates_return_home`, `now_ms`, `utc_offset_ms`, `locale` | `animates_return_home` is true: no return-home motion |
| MeeGone's own files (ADR 0004) | `StorageAdapter` | `read`, `write`, `remove` | A failed save is the feature's `problem`; an unreadable file loads as defaults |

- **Adapters never call back.** They answer questions and carry out requests. A change the system reports reaches MeeGone as a `ShellIntent` (`Home`, `Back`, `Resumed`, `Returned`, `Restore`, `SystemThemeChanged`, `AppsChanged`, `NotificationsChanged`) that the platform's entry point passes to `Launcher::dispatch`. The Android activity sends `Resumed` on every `onResume`, including after the home-app role dialog, and `Returned` after it when an app or a system screen had covered the home (ADR 0007). `Bridge` sends `AppsChanged` once for each burst of app and profile changes (ADR 0010).
- **Expected failures are values.** Adapters return a `DataError` (`AccessDenied`, `Unsupported`, `Offline`, `Invalid` or `Failed`), and each feature turns it into its own state (ARCH-09).
- **The image feed is the lock-screen image's own.** `ImageFeed` in `meegone-lockimage-data` returns format-free `FeedImage`s. Its one implementation, built on `NetworkAdapter`, is the only code that knows the Bing feed's format (SRS 8.2). Its fake is `FakeFeed`.
- **Fakes.** Enable the `fakes` feature of `meegone-core-data` (and of `meegone-lockimage-data` for `FakeFeed`) in `[dev-dependencies]`. Every fake has public fields that set what the "system" reports and record what was asked of it. Its `error` field makes every call that can fail fail, and other fields reproduce each "if unavailable" case.

## Failure containment (ARCH-09)

Expected problems are part of each feature's own state. Examples are missing access (`Access::Missing`), a failed adapter call (`problem`) and an unsupported device (`ImageStatus::Unavailable`).

Bugs are contained by `Store`. It runs every intent inside `catch_unwind`. If a holder panics, the store marks that feature `Health::Failed` and never calls it again. The shell and every other feature keep working. This requires `panic = "unwind"`, which is Cargo's default; never set `panic = "abort"` in a profile.

## Sources of truth (ARCH-06)

| Entity | Single source of truth | Written by |
|---|---|---|
| App | The system, through `AppsAdapter` (memory only) | The system |
| Layout item, folder | `LayoutStore` (`meegone-apps-data`), saved in `layout` | Applications |
| Settings | `SettingsStore` (`meegone-core-data`), saved in `settings` | Each field has one writer: Settings (theme, Home target, wrap-around, when the home-app banner was dismissed), Applications (hidden apps), Lock-screen image (image options), Recent (apps removed from Recent) |
| Image record | `ImageCache` (`meegone-lockimage-data`), saved in `images/details` | Lock-screen image |
| Notification group | The system, through `NotificationsAdapter` (memory only) | The system |
| Recent entry | The system, through `UsageAdapter` (memory only) | The system |

A view holds only the snapshot it renders. Content shown inside another feature holds only a `Latest<State>`, which the shell keeps current.

## Persistence (REL-03, MAINT-04)

MeeGone keeps three persistent stores, each in its own file under one private folder. The decisions are in [ADR 0004](docs/adr/0004-persistent-storage.md).

| Store | Type | File | Format version |
|---|---|---|---|
| Layout | `PersistentLayoutStore` (`meegone-apps-data`) | `layout` | `LAYOUT_VERSION` |
| Settings | `PersistentSettingsStore` (`meegone-core-data`) | `settings` | `SETTINGS_VERSION` |
| Image cache | `PersistentImageCache` (`meegone-lockimage-data`) | `images/details` | `IMAGE_CACHE_VERSION` |

- **Where.**
  - `StoredFile` names every file, so nothing else can be stored. Notification content and usage history have no file (PRIV-03, EVT-13).
  - The host gives `FileStorage` a folder in the app's private internal storage (`Context.getFilesDir()`). Android deletes that folder on uninstall and on "Clear storage" (PRIV-05).
- **Atomic writes.** `FileStorage` writes `<file>.tmp`, flushes it, renames it over the file and flushes the folder. A crash at any point leaves the old file or the new one (REL-03). Its tests stop writes part-way to check this.
- **Format.**
  - Text: a `meegone-<kind> <version>` header, one `key field …` record per line, and an `end <checksum>` trailer.
  - A file that is cut short, damaged, of another kind or from a newer version loads as the defaults instead of crashing (MAINT-04).
- **Budget.** A write that would take stored data past 50 MB fails and changes nothing (PERF-08).
- **Failures.** A failed save keeps the previous data. Each feature shows the error as its `problem`.

### Changing a saved format

1. Bump the store's version constant, for example `SETTINGS_VERSION`.
2. Add a `Migration` to the store's migrations array, which turns the previous version's body into the new one. The array's length is `VERSION - 1`, so the build fails until you add it.
3. Change the store's `Format::write` and `Format::read` for the new body.
4. Add `fixtures/<kind>-v<N>.txt`, written by the new code, and its expected value to the store's `FIXTURES`. Never edit or delete an older fixture: the tests load every one of them through every migration step.

## Simplicity budget (SRS 7.1)

| Item | v1 limit |
|---|---|
| Home views | 3 (`HomeView`) |
| User-adjustable settings | 12 at most |
| Setup screens | 4 at most |
| Persistent stores | 3: layout, settings, image cache (`StoredFile`) |
| Scheduled background jobs | 1: the daily image (`SchedulerAdapter`) |
| External network services | 1: the Bing image feed |
| Servers and accounts | 0 |
| Special access requested | 2, both optional: notifications, usage |

## Not built yet

The skeleton is from issue #1; the Android host and the home-app role from #4; the carousel of home views from #5; Home, Back and coming back to the home from #6; the home menu from #7; edge to edge, screen sizes and navigation modes from #8; the Applications grid and the app catalogue from #9. The following arrive with later requirements:

- **Android adapters.** The home-app role, the clock and storage are built (ADR 0005), as are the navigation-mode check for the return-home motion (ADR 0007), the haptic tick (ADR 0008) and the app catalogue (ADR 0010). Shortcuts, uninstalling and App info (#12, #13), notifications, usage, wallpaper, network, scheduling and the remaining device settings are stubs until their issues. The ticks on entering edit mode and on creating a folder (ANIM-24) come with #13 and #14.
- **The daily image.** The Bing implementation of `ImageFeed` (#26) and the updater (SRS 7.5) come with the WALL requirements.
- **Drawing.** The home views, their carousel, the view indicator, the return-home motion, the home menu and the Applications grid are drawn (ADR 0006 to ADR 0008, ADR 0010). Events and Recent show only their names until their content arrives (#19, #22).
  - Recent's cards adapt to large screens when #22 builds them. Today the pages, the carousel, the safe area, the menu, Settings and the Applications columns adapt (HOME-12, APPS-02).
  - Icons are the system's until #10 gives them one shape and derives each app's accent colour. Folders show as a plain cell with their name until #14 draws them.
  - Settings covers the home views with only its name, and its group's, until #34 draws it. "Home button opens", the home-app banner and first-run setup send intents but aren't drawn yet.
  - Edit mode and close mode change state but aren't drawn yet (#13, #24).
- **First-run setup** comes with the ONB requirements. The lock-screen image step is provided as a `Slot`, like its Settings section.

## Adding a crate

1. Create `crates/<area>/<layer>` with the package name `meegone-<area>-<layer>`, inheriting `version`, `edition`, `rust-version` and `publish` from the workspace.
2. Add it to `[workspace.dependencies]` in the root `Cargo.toml`, and depend on other crates only as the rules allow.
3. A new area means a change to `AREAS` in `tools/arch-check` and an ADR, because it changes the simplicity budget.
4. Add every new third-party crate to `DEPENDENCIES.md` with its reason.

## Checks

These are the commands CI runs (`.github/workflows/ci.yml`):

```sh
cargo fmt --all --check
cargo test -p arch-check                       # architecture rules
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo coverage                                 # core-logic line coverage, fails below 80 %
cargo clippy --workspace --exclude arch-check --all-targets --target aarch64-linux-android -- -D warnings
(cd android && ./gradlew assembleDebug assembleRelease)   # the APK (separate CI job)
```

Building the APK needs JDK 17 or later, the Android SDK with NDK 27.3 (`ANDROID_HOME`, or `sdk.dir` in `android/local.properties`), cargo-ndk and the `aarch64-linux-android` Rust target. The workspace checks above need none of them.

`cargo coverage` is an alias in `.cargo/config.toml` (MAINT-02). It runs every test in the workspace and measures line coverage of the data and state crates, where the core logic lives (MAINT-01). UI crates, test files and fakes are left out. Inline `#[cfg(test)]` modules still count, because stable Rust can't leave them out. It needs a one-time setup:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

The acceptance scenarios of SRS section 9 run in `tests/acceptance.rs`, one test per scenario, on the whole app with fakes (MAINT-03). A feature adds its scenarios there.
