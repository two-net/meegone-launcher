# ADR 0041: Performance budgets and benchmarks

**Status:** Accepted · **Date:** 2026-11-23 · **Issue:** #40

## Context

Issue #40 (Must, two parts Should) asks that "fluid" be measured, and stay so. SRS 6.1 sets eight budgets on the reference device: a mid-range phone at most four years older than v1, with 4 GB of RAM and a 60–120 Hz display.
- **PERF-01:** after a Home, the home is fully drawn and responsive within 300 ms, 95th percentile, while its process runs.
- **PERF-02:** from a cold start, Applications shows with its icons within 1.5 s, 95th percentile.
- **PERF-03:** during swipes, scrolling and every motion, at most 1 % of frames miss their deadline, and none takes over 700 ms.
- **PERF-04:** with 300 apps, the saved layout shows within 500 ms of a cold start. Icons may fill in, but placeholders keep the layout from shifting.
- **PERF-05 (Should):** search's results update within 50 ms of each keystroke, with 300 apps.
- **PERF-06:** a launch request leaves MeeGone within 50 ms of the tap.
- **PERF-07:** at most 150 MB in steady state with 300 apps, and caches shrink under memory pressure.
- **PERF-08 (Should):** stored data, image cache included, at most 50 MB; the install at most 15 MB.
- **Also:** a repeatable benchmark for each budget, with results recorded per release, and AC-20: 50 Homes from an app within 300 ms at the 95th percentile, and 30 s of swiping with at most 1 % of frames late.

Some of it was in place:
- **Storage:** every write stays within `MAX_STORED_BYTES`, 50 MB (ADR 0004), and the image cache keeps crops, never downloads (ADR 0028).
- **Launches:** a tap launches before anything is saved or moves (ADR 0010, ADR 0034).
- **Icons:** they load off the main thread, behind placeholders of their size (ADR 0010).
- **Deferred:** ADRs 0006, 0010, 0016 and 0034 left every measurement to this issue.

Nothing measured anything, and the reference device is still to be chosen (#48). A Galaxy S22 (SM-S901E, Android 16, 1080 × 2340 at 120 Hz, three-button navigation) stood in. A first look found:
- **Frames:** the installed debug build had 3.3 % of its frames late, and one frame of about 1.15 s.
- **Memory:** 133 MB with about 50 apps. With 300 apps it was 250 to 346 MB:
  - every one of the 300 icons was kept twice, as a bitmap and as a GPU texture;
  - nothing answered `onTrimMemory`.

## Decision

1. **The budgets are measured on a device, by `tools/perf-bench.sh`.**
   - **What it measures is the benchmark build** (`assembleBenchmark`):
     - It is the release build's code, signed with the debug key, not debuggable and profileable from the shell. A debuggable build runs the Java host much slower and would mislead.
     - It installs beside MeeGone as `io.github.twonet.meegone.benchmark`, so a run never touches the user's layout or settings.
     - It takes the debug build's source set, so it has the debug hooks. Release builds keep the no-op `DebugHooks` (SEC-06).
   - **The probe.** `PerfProbe`, in the debug source set, logs marks under `MeeGonePerf`. The extra `debug.PERF` turns it on; a cold start's Home carries no extras, so the setting is kept in the no-backup folder. Logcat's monotonic clock times the system's lines and the marks alike, so no clocks need lining up.
     - **`home-ready`:** after a Home, the first frame with every icon in sight, once the main thread is idle.
     - **`layout-shown` and `icons-shown`:** after a cold start, the first frame with the saved layout's cells and the first with all their icons. `shift` if any cell in sight moved between the two.
     - **`search <ms>`:** from a change of the field's text to the frame with its results committed.
     - **`launch <ms>`:** from the finger lifting, the touch's own event time, to `startMainActivity` returning in `Bridge.launch`.
     - **`trim`, `stored` and `first-cell`:** the icons held after a trim, the bytes in the files folder, and where to tap.
   - **300 apps.** `debug.APPS` pads the catalogue to that many with copies of the installed apps, "‹label› ‹k›", whose activity is `‹activity›#k`. `DebugHooks.realActivity` names the real activity wherever Android is asked, so copies have real icons and accents and launch the real app.
   - **The run:**
     - It makes the benchmark build the default home and gives it notification and usage access, so Home, Events and Recent do their real work.
     - The probe answers first-run setup as the accessibility tour does, with the daily image off: nothing is downloaded, and the lock screen stays as it was.
     - Afterwards it puts back the home app (MeeGone, if a run was cut short), the accesses and the screen timeout.

   | Budget | Measure | Samples | Passes at |
   |---|---|---|---|
   | PERF-01 | `input keyevent KEYCODE_HOME` from Settings: the system's `START … HOME` line to `home-ready` | 50 | 95th percentile ≤ 300 ms |
   | PERF-02 | Force-stopped behind Settings, then Home: the system's "Fully drawn" | 20 | 95th percentile ≤ 1.5 s |
   | PERF-04 | The same starts: `START` to `layout-shown`, and no `shift` | 20 | 95th percentile ≤ 500 ms |
   | PERF-03 | `dumpsys gfxinfo` over 30 s each of swiping, of scrolling Applications, and of the home menu, search and Recent's close mode by key | 3 runs | ≤ 1 % past the deadline, none over 700 ms |
   | PERF-05 | 50 keystrokes in search: `search` | 50 | 95th percentile ≤ 50 ms |
   | PERF-06 | `input tap` on the first cell, then Home: `launch` | 20 | 95th percentile ≤ 50 ms |
   | PERF-07 | Total PSS at rest after all of the above, the median of three readings. Then `am send-trim-memory … RUNNING_CRITICAL` | 3 readings | ≤ 150 MB, and the icons held drop |
   | PERF-08 | `stored`, and the installed APK's size | 1 | ≤ 50 MB, ≤ 15 MB |

   - **Why these measures:**
     - **PERF-01:** the system logs the start of the home when Home is pressed, whatever the navigation mode, so it covers the system's part and MeeGone's.
     - **PERF-02:** "Fully drawn" is Android's own measure of a launch. `HomeViews` reports it, in every build, once a frame shows Applications with every icon in sight (`reportFullyDrawn`).
     - **PERF-03:** "Frame deadline missed" is Android's count against each frame's own deadline, so it holds at any refresh rate. Below Android 12 the count is "Janky frames". The histogram's last bucket with a frame is the longest.
     - **PERF-05:** "each keystroke" is held at the 95th percentile, as the other latencies are, and the longest is recorded.
     - **A cold start after a force stop** finds the storage cache warm, which a reboot doesn't. So the measure flatters a reboot somewhat.
     - **`BACKGROUND`** can't be sent to a home process, which Android keeps in the foreground, so the trim is checked at `RUNNING_CRITICAL`.
2. **CI holds the Rust app's share** (`benches/budgets.rs`, `cargo bench --bench budgets`).
   - It drives the whole `Launcher` with the fakes and 300 apps:
     - Home from an app;
     - a cold start on a saved layout, to its cells;
     - each frame of a swipe and its spring;
     - a keystroke to its result cells;
     - a tap to the launch request;
     - the bytes stored.
   - Each median must stay within a tenth of its device budget. The fakes cost nothing, so only MeeGone's own work is timed, and only a much slower algorithm fails, not a busy runner. The table goes on the run's summary page.
   - The APK job builds the benchmark build too, and fails if the release APK is over 15 MB.
   - Arch-check's clock rule covers `crates/` and `src/`, so `benches/` may read `Instant`.
3. **Results are recorded per release.** `--record` writes the table, with the commit, device, Android version, refresh rate and app count, to `docs/benchmarks/<version>-<model>.md`. `RELEASING.md` asks for a passing run on the reference device, and blocks a release on #48.
4. **Memory** (PERF-07).
   - **Icons are hardware bitmaps.** `AppIcons` keeps each shaped icon once, in graphics memory, and immutable.
     - A software bitmap is kept twice, in the heap and as a texture.
     - A mutable one is uploaded for every frame whose views hold it, in sight or not. That alone made 300 textures.
     - Nothing may draw an icon on a software canvas. `ThemeFade`, and the debug build's audit, record a `Picture` for the GPU to draw instead.
   - **Only cells near sight hold icons.** A grid cell more than half a screen out of sight draws nothing and lets its icon go. It draws in full, and asks again, as it comes near, by its laid-out place or the place it is moving from.
   - **`AppIcons` holds the 96 icons asked for last.** An older one is held weakly, and comes back without loading while a view still draws it.
   - **On `onTrimMemory`:**
     - From `RUNNING_LOW` while the home shows (Android 10 to 13), and from `BACKGROUND` on every version, every icon out of sight goes: the grids' cells out of sight let theirs go, and `AppIcons` holds every icon only weakly.
     - At `UI_HIDDEN` nothing goes, so Home still finds the home drawn (PERF-01).
   - **The image details screen** lets its decoded image, up to 6 MB, go when it closes, as the past week and setup already did.
5. **Frames** (PERF-03).
   - **Recent keeps its catalogue.** It read every app, collation keys and folding included, each time it showed: about 75 ms with 300 apps, on the touch that lands a swipe. It now reads them once, and again only on `RecentIntent::AppsChanged`, which the shell sends where it sent `Refresh` for `AppsChanged`. Events already did so (ADR 0020).
   - **A view that comes into sight is read again once nothing moves.**
     - Recent's uses, and Events' header, notifications and image card, took 13 to 28 ms, on the frame where the spring began. `show` now only notes the view.
     - The host's idle handler sends `ShellIntent::Idle` (`nativeIdle`) once the frame says nothing moves. The shell then reads the view again, unless a finger holds the row.
     - A change with no motion, such as with animations removed, reads it at that idle.
   - **Far cells draw nothing.** A grid's first draw, such as search's results, lays out only the labels near sight.
6. **The probe's hooks cost nothing in release builds.** Each is a call to an empty static method of the release `DebugHooks`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | Once nothing moves, the host's idle handler sends `ShellIntent::Idle` (decision 5). Frames are measured against PERF-03 by `tools/perf-bench.sh` (decision 1). |
| 0010, decision 7 | Icons are hardware bitmaps, held only for cells near sight, and at most 96 at once (decision 4). PERF-02, PERF-04 and PERF-06 are measured (decision 1). |
| 0016 | PERF-05 is measured (decision 1). |
| 0019, decision 3 | A swipe to Events reads the header again once the views are at rest (decision 5). |
| 0022, decision 2 | So does the image card. |
| 0023, decision 4 | Recent reads its uses again once the views are at rest, and its catalogue only when the apps change (decision 5). |
| 0033, decision 5 | The picture of the old screen is recorded and drawn by the GPU (decision 4). |
| 0034 | Frames are measured against PERF-03 (decision 1). |
| 0038, decision 9 | The audit records what it checks as drawn, for the GPU to draw (decision 4). |

## Consequences

- **On the Galaxy S22, with 300 apps.** The recorded run is `docs/benchmarks/0.1.0-SM-S901E.md`.

  | Budget | Before | After, recorded |
  |---|---|---|
  | PERF-01, Home | 142 ms | 61 ms |
  | PERF-02, cold start | 567 ms | 776 ms |
  | PERF-03, swiping | 3.03 %, 121 ms | 1.21 %, 24 ms |
  | PERF-03, scrolling | 0.58 %, 27 ms | 0.48 %, 34 ms |
  | PERF-03, by key | 6.22 %, 129 ms | 6.24 %, 32 ms |
  | PERF-04, saved layout | 363 ms | 410 ms |
  | PERF-05, search | 46 ms | 29 ms, longest 52 ms |
  | PERF-06, launch | 88 ms | 43 ms |
  | PERF-07, memory | 250–346 MB | 125 MB |
  | PERF-08 | 0.04 MB, 1.8 MB | the same |

  - **"Before"** is the benchmark build's first full measure, which already held icons only near sight. Memory's is from before any of decision 4.
  - **The 88 ms** was most likely the first launch after search's keyboard: single launches took 19 to 32 ms.
  - **Runs vary:** a run before the recorded one had Home at 126 ms, a cold start at 613 ms, swiping at 1.05 % and by key at 4.62 %.
  - **Cold starts took longer** than before: each icon is now copied into graphics memory, and the device was warm from the runs before. They stay well within 1.5 s, and the layout within 500 ms.
- **Not met on the S22 yet,** at 120 Hz, where each frame has 8.3 ms:
  - **Swiping** is just over the 1 % line. Its late frames start a swipe:
    - the touch's own work and the page coming into sight being recorded, 2 to 5 ms;
    - then about 7 ms of GPU.

    Keeping the neighbouring pages drawable out of sight, rather than invisible, would spare the recording. Screen readers and Tab rely on their invisibility (ADR 0038, ADR 0039), so it needs its own decision.
  - **By key, opening search** brings up the keyboard. Its late frames wait on the GPU and on the system's buffers while the keyboard's window slides in, not on MeeGone's main thread. The menus alone missed none.
  - **One earlier run** had a single 250 ms frame among its swipes, which didn't come back. It is under 700 ms, so it counts as one late frame.

  These are for the reference device to settle (#48). A mid-range phone at 60 or 90 Hz has more time per frame, and less GPU.
- **Benefits:**
  - Every budget has a repeatable measure. CI catches a much slower algorithm in the Rust app, and the install size.
  - The memory fix holds on any device: icons are kept once, and only near sight.
- **Costs:**
  - **Scrolling fast** can show placeholders for a moment, where every icon used to be held. Icons load in the order asked, a screen ahead.
  - **Events and Recent** update just after the swipe that shows them has settled, not as it starts.
  - **Debug builds** read a small settings file at each start.
  - **The benchmark build** is another APK to build in CI.
  - **On the reference device,** the device run takes about 15 minutes and needs a person.
- **Not checked yet:**
  - the reference device (#48), a 60 Hz screen, and Android 10 to 13's `RUNNING_LOW` trims;
  - a reboot's cold start, and memory while the home is in the background;
  - `tools/a11y-audit.sh` since the cells and the audit's capture changed.

  The device matrix (#43) covers other devices.
