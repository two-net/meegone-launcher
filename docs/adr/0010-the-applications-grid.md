# ADR 0010: The Applications grid

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #9

## Context

Issue #9 draws Applications. It needs:
- Every launchable app from every profile, in one vertically scrolling grid with no pages and no dock (APPS-01 to APPS-03).
- Four columns on phones, and cells of 96 to 128 dp on screens 600 dp wide or more (APPS-02).
- Labels on one line, in the theme's colour, that follow the system font size (APPS-16).
- Unavailable apps dimmed, with a tap deferring to the system's explanation (APPS-09).
- A tap that launches with the platform's launch-from-bounds transition (APPS-10, ANIM-08) and leaves MeeGone within 50 ms (PERF-06).
- Icons that shrink when pressed (ANIM-10).
- A grid that stretches at its ends (ANIM-23).
- Names that say "work" (A11Y-01), and work-profile apps with the system's badge (APPS-20).

The constraints:

- **Android's views draw, and the Rust app decides what they show** (ADR 0006). Java pulls a frame of `int` slots. Nothing checks at build time that the two sides agree.
- **Adapters never call back** (ADR 0003). The system's changes reach MeeGone as `ShellIntent`s.
- **Motion is state** (ADR 0006), and only the platform's entry points know the time.
- **The JNI binding is hand-written** and binds only what MeeGone uses (ADR 0005).
- **A grid of 300 apps must show quickly** (PERF-04). Icons are bitmaps that only the views use.
- **There are no Java tests.** Only a device shows what is drawn.

## Decision

1. **The app catalogue is real.**
   - `AppsAdapter` on Android is `android/apps.rs`. `Bridge.launchableApps()` and `Bridge.profiles()` return flat `String[]` records of a fixed number of fields.
   - The records are parsed apart from JNI, in `catalogue.rs`, so the parsing is tested without a device. Its fields and flags are mirrored in `Bridge.java`.
   - A record that doesn't parse is left out. A `null` array means the system couldn't answer: it is an error, so the saved layout keeps its apps.
   - Profiles come from `LauncherApps.getProfiles()`. Each is named by its serial number, which survives restarts and is what the layout saves.
     - A work profile is a managed profile: from Android 15 the profile's user type says so, and before that it is any profile other than MeeGone's own.
     - Private space is invisible to MeeGone, which requests no permissions (SEC-03).
   - An app is available unless its profile is paused or the app is suspended. `App` gains `work`.
   - MeeGone leaves itself out of its grid.
   - The manifest declares a `<queries>` entry for the MAIN/LAUNCHER intent. From Android 11, other apps are hidden without it. It is not a permission.
   - The accent colour is neutral until #10, and the install time is 0 until #11.
2. **A launch grows from the icon.**
   - `AppsAdapter::launch` takes `from: Option<Rect>`, the icon's rect on the screen. `Bridge` passes it to `LauncherApps.startMainActivity` as the source bounds, with `ActivityOptions.makeScaleUpAnimation`.
   - An unavailable app is launched too, as Android's own launcher does. The system then shows why it can't open, and offers to unpause a work profile.
   - If the system refuses outright, `Bridge` returns a short message from the string resources. It becomes Applications' `problem`, and the grid shows it as a toast.
   - The launch happens on the tap. No motion of MeeGone's delays it.
3. **The grid follows the system's changes.**
   - `Bridge` registers once per process. It listens through a `LauncherApps.Callback` and a receiver for the managed-profile broadcasts.
   - A burst of changes is coalesced into one call to `HomeActivity.nativeAppsChanged()`. That sends `ShellIntent::AppsChanged`, which refreshes Applications and Recent.
   - Installs and removals therefore show at once. Their motion, and the rest of APPS-06 and APPS-08, stay with #11.
4. **The grid's state.**
   - `AppsState.items` is an `Arc<[GridItem]>`, so a snapshot stays cheap on every scroll event. The view can also tell when the cells change.
   - The host reports the grid's width and row height with `AppsIntent::Layout`. The columns are `max(4, ceil(width_dp / 128))`, and `rows()` uses them.
     - A phone's grid is under 512 dp, so it gets four columns.
     - Any grid of 384 dp or more, which every screen 600 dp wide leaves after its safe area and margins, gets cells of 96 to 128 dp.
   - When the columns or the row height change, `scroll_px` moves to the row of the same first visible item. This settles ADR 0009's open cost, in Rust and tested.
5. **The host contract.**
   - The frame grows to 24 slots: the cells' revision, the scroll position, the columns and rows, and the pressed cell and its scale.
   - `AppsView` keeps the cells it last handed out, with a revision. When the revision changes, `AppsGrid` reads `nativeAppsCells()`: five fields per cell.
   - A tap names a cell by revision and index. A stale revision is ignored, so a grid that changed under the finger launches nothing.
   - `nativeAppsLayout` answers with the new columns and rows, so the grid lays them out in the same measure pass.
   - The grid reports its scroll position. It follows the frame's position only when it didn't report it itself: after Home, a restart or a new layout.
   - The JNI binding gains:
     - `NewString`, `GetArrayLength`, `NewObjectArray`, `GetObjectArrayElement` and `SetObjectArrayElement`.
     - `int` and object arguments.
     - An owned `Local` that frees each reference right after use.
6. **The Java grid.**
   - `AppsGrid` is a `ScrollView` with the platform's overscroll: stretch, or glow before Android 12.
     - It takes the safe area plus 16 dp margins as its padding, with `clipToPadding` off, so icons scroll under the bars.
     - Vertical scrolling hands the gesture away from the carousel, as ADR 0006 expects.
   - Its cells sit in rows of equal height, and a right-to-left layout mirrors the columns.
   - `AppCell` is one view per cell that draws its icon and label: 300 views lay out quickly.
     - The icon is 56 dp, with the label 4 dp below at 12 sp, single-line and ellipsised with `StaticLayout`.
     - There are 8 dp above the icon and below the label, which leaves 16 dp between rows.
     - Its height comes from font metrics, so it grows with the font size.
   - Cells are clickable, so a long-press on one never opens the home menu. Between cells and in the last, empty row, it does.
   - Screen readers read the label, "Mail, work" for a work app, and "Unavailable" for an unavailable one.
7. **Icons stay in Java.**
   - `AppIcons` loads the system's badged icon on one background thread, in grid order. It draws each one into a bitmap of the icon's size.
   - A placeholder plate of the same size shows until the icon arrives, so nothing shifts (PERF-04).
   - Its own `LauncherApps.Callback` drops a changed package's icons. The old icon stays until the new one arrives.
8. **The icon press is shell motion.**
   - `ShellIntent::Press { item, at }` and `Release(at)` drive `press.rs` in `meegone-shell-state`, beside the carousel and the return-home motion.
   - The icon shrinks to 92 % over 90 ms, decelerating, and springs back with `motion::ICON_RELEASE`, overshooting by about 1.5 %.
   - It follows the system animation speed.
   - A carousel drag, the menu, Settings or another view let it go.
   - The host sends `Press` when Android marks the cell pressed. In a scrolling container that waits for the tap timeout, so a scroll never shrinks icons.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0005, decision 7 | The app catalogue is no longer a stub. Its shortcuts, uninstalling and App info answer `Unsupported` until #12 and #13. |
| 0006, decision 2 | The frame has 24 slots. Applications' cells come separately, by revision, with `nativeAppsCells`. |
| 0006, decision 7 | Applications shows its grid instead of its name. |
| 0009, decision 2 | The Applications page has no padding of its own: its grid takes the safe area as padding and scrolls under the bars. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| APPS-01: "every launchable app" | MeeGone leaves itself out. It is already on screen. |
| APPS-09: apps "on missing storage" are dimmed | Android doesn't list apps whose storage is missing. They leave the grid until it returns. Keeping their place belongs to #11 (APPS-18). Paused profiles and suspended apps are dimmed. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - The column rule and the first visible item across layouts.
    - The press motion.
    - The cells' revision and stale taps.
    - The catalogue's parsing.
    - The issue's acceptance scenarios, in `tests/acceptance.rs`.
  - Unavailable apps dim and undim while MeeGone shows. Checked on a Galaxy S22 with `pm suspend` and `pm unsuspend`. A tap on a suspended app shows Android's own "App isn't available" dialog.
  - Icons never cross JNI, and the grid draws one light view per app.
- **Costs:**
  - The frame slots, the catalogue's records and the cells' fields must agree between Java and Rust. Nothing checks that at build time.
  - The catalogue is read on the main thread: at start, and on every change the system reports. For 300 apps that includes each label.
    - PERF-02 and PERF-04 can only be measured on the reference device (#48), with the benchmarks of #40.
    - Reading it off the main thread would need JNI thread attachment (ADR 0005).
  - On the Galaxy S22 (One UI, Android 16), MeeGone ran as an ordinary app, not the default home. The system played its own open transition instead of growing the window from the icon (ANIM-08), although the icon's bounds reached it.
    - The device matrix (#43) checks it with MeeGone as the default home.
    - From the bridge's launch call to the system's activity start took 14 to 22 ms there (PERF-06's budget is 50 ms from the tap).
  - Icons cost about 110 KB each at 3× density: about 34 MB for 300 apps. #10 redraws them in one shape and may revisit this.
  - Lifting the finger after a long-press on an icon launches the app, as for any Android view with no long-press of its own. The app's long-press menu (#12) takes that gesture.
