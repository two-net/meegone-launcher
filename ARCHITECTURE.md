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
| **core** | Models, platform adapter traits and fakes, storage and the saved-file format, the settings store | `StateHolder`, `Store`, theme resolution | Colour tokens, motion specs, `View`, `Slot`, `Dispatch` | Nothing persistent |
| **apps** | Layout store | Grid, edit mode, folders | `AppsView` | Layout store |
| **events** | – | Notifications grouped by app | `EventsView` | Nothing (memory only) |
| **recent** | – | Filtering, ordering, close mode | `RecentView` | Removed-apps list (in the settings store) |
| **lockimage** | Image cache, image-feed interface | On/off, availability, current image | `ImageCard`, `ImageSettingsSection` | Image cache |
| **settings** | – | Settings, access status, the home-app banner (ONB-04) | `SettingsView`, `HomeBanner` | The settings screens |
| **shell** | – | `HomeShell`: carousel, Home and Back, composes every feature | `ShellView` | Nothing persistent |

The root package `meegone-launcher` is the app. `Launcher::start(platform)` hands the platform adapters to the shell. On Android it is also a native library, which the Java host in `android/` loads ([ADR 0005](docs/adr/0005-android-host-and-home-role.md)). `tools/arch-check` is a development tool that is never part of the app.

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
AppsView.icon_tapped(mail)                       ui     sends AppsIntent::Launch(mail)
  └▶ ShellView queues ShellIntent::Apps(..)      ui
      └▶ HomeShell.dispatch                      state  routes it to the Applications store
          └▶ Store<AppsHolder>.dispatch          state  runs the holder, contains failures
              └▶ AppsHolder.handle               state  asks the data layer for the side effect
                  └▶ AppsAdapter.launch(mail)    data   the only code that talks to Android
          ◀── new Arc<AppsState> snapshot, published to subscribers
```

- **State holders** implement `StateHolder`: `state()`, `handle(intent)` and `on_back()`. They take their adapters and stores as `Arc<dyn …>` traits, so tests drive them with the fakes in `meegone-core-data` (feature `fakes`), without any UI (MAINT-01).
- **Constructors do no I/O.** Each feature loads on its first `Refresh` intent, which runs inside its store.
- **Home and Back.** The shell sends them to the current view's `on_back`, which closes that feature's topmost open element (HOME-07, HOME-08).

## Platform adapters (ARCH-04)

Everything MeeGone asks of Android goes through eight narrow traits in `meegone-core-data`, handed to the shell once as `Platform`: the seven in `adapters.rs`, and `StorageAdapter` (`storage/`) for MeeGone's own files. Each has a fake in `meegone_core_data::fakes`, so the core logic of MAINT-01 runs and is tested without a device. The decisions behind them are in [ADR 0003](docs/adr/0003-platform-adapters.md) and, for storage, [ADR 0004](docs/adr/0004-persistent-storage.md).

On Android, `meegone_core_data::android` builds the `Platform` ([ADR 0005](docs/adr/0005-android-host-and-home-role.md)). Its adapters ask a Java `Bridge` object through a minimal, hand-written JNI binding. Today only the home-app role, the clock and storage are real; every other adapter answers with its "If unavailable" value below until its issue builds it.

| Capability (SRS 8.1) | Adapter | Methods | If unavailable |
|---|---|---|---|
| Home-app role | `SystemConfigAdapter` | `is_default_home`, `request_default_home` | Banner (ONB-04) |
| App catalogue: apps, profiles, shortcuts | `AppsAdapter` | `launchable_apps`, `profiles`, `launch`, `shortcuts`, `launch_shortcut` | `shortcuts` is `Unsupported`: menus show none |
| App catalogue: install and removal events | – | `ShellIntent::AppsChanged` | – |
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
| Device settings and clock | `SystemConfigAdapter` | `is_dark`, `animation_scale`, `now_ms`, `utc_offset_ms`, `locale` | – |
| MeeGone's own files (ADR 0004) | `StorageAdapter` | `read`, `write`, `remove` | A failed save is the feature's `problem`; an unreadable file loads as defaults |

- **Adapters never call back.** They answer questions and carry out requests. A change the system reports reaches MeeGone as a `ShellIntent` (`Resumed`, `SystemThemeChanged`, `AppsChanged`, `NotificationsChanged`) that the platform's entry point passes to `Launcher::dispatch`. The Android activity sends `Resumed` on every `onResume`, including after the home-app role dialog.
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

The skeleton is from issue #1; the Android host and the home-app role from #4. The following arrive with later requirements:

- **Android adapters.** The home-app role, the clock and storage are built (ADR 0005). The app catalogue, notifications, usage, wallpaper, network, scheduling and the remaining device settings are stubs until their issues.
- **The daily image.** The Bing implementation of `ImageFeed` (#26) and the updater (SRS 7.5) come with the WALL requirements.
- **The UI toolkit** is still to be chosen (ADR 0002). Views compile and send intents, but nothing is drawn yet.
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
