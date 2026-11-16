# ADR 0006: Drawing with Android views, and the carousel of home views

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #5

## Context

Issue #5 builds the N9 home gesture: one horizontal drag moves between Events, Applications and Recent, the content follows the finger and the row wraps around (HOME-02 to HOME-04). A spring settles it (ANIM-02, ANIM-07), touching it catches it (ANIM-03), a view indicator fades in and out (HOME-09, ANIM-11), right-to-left layouts mirror it (L10N-02), and screen-reader users switch views without gestures (A11Y-05, A11Y-08).

It is the first issue that has to draw, and ADR 0002 left the UI toolkit open. The constraints:

- **The app's crates use only the standard library** (DEPENDENCIES.md). A Rust renderer would need third-party crates for drawing and text, and would have to build accessibility, right-to-left layout and text input itself.
- **The UI keeps no state of its own** (ARCH-05, ARCH-06). A row that moves with the finger has a position and a velocity, so they are state.
- **Core logic is tested without a device** (MAINT-01), and coverage counts the data and state crates only (MAINT-02).
- **Only platform adapters read the clock** (ARCH-04).
- **Gesture-driven motion lags the finger by at most one frame** (ANIM-01) and fewer than 1 % of frames miss their deadline (PERF-03).

## Decision

1. **Android's own views draw MeeGone.**
   - `HomeViews`, a plain-Java `FrameLayout`, holds the three home views and the view indicator. Java owns the view tree; the Rust app decides what it shows.
   - Accessibility, right-to-left layout and text come from Android's views.
   - Strings are Android resources, so they can be translated (L10N-01).
2. **Java pulls each frame from Rust.**
   - `HomeViews` passes every touch (`nativePointer`), every display frame from `Choreographer` (`nativeFrame`), its layout (`nativeLayout`) and screen-reader actions (`nativeShow`).
   - After each, it reads what to draw with `nativeRead(int[])` and applies it: each view's `translationX`, the indicator's opacity and colours, and the background. It asks for another frame only while the frame says something moves.
   - The slots of that `int[]` are listed in `frame_slots` (`src/android.rs`) and mirrored in `HomeViews.java`. Rust never calls Java to draw.
   - The JNI binding gains `SetIntArrayRegion`.
3. **Motion is state, in the shared core.**
   - The motion specs move from `meegone-core-ui` to `meegone-core-state::motion`, which `meegone-core-ui` re-exports.
   - `motion` adds the spring's exact solution (`SpringMotion`), which is the same at any frame rate, a cap on the release speed that keeps any overshoot within 3 % (`Spring::limit_velocity`), and a `VelocityTracker`.
4. **The carousel lives in the shell's state.**
   - `meegone-shell-state` gains `ShellIntent::Layout`, `Pointer` and `Frame`, and publishes a `CarouselState` inside `ShellState`. `ShellView::frame` turns it into the `HomeFrame` the host draws.
   - Every time is passed in on the host's monotonic clock: `MotionEvent` and `Choreographer` times share the `System.nanoTime()` base. Nothing in the app reads the clock for motion.
   - The rules:
     - A drag starts anywhere once the finger passes the platform's touch slop sideways rather than up or down. Nothing depends on the screen's edges, which belong to the system Back gesture (HOME-13).
     - One drag moves one view at most. With wrap-around off, the row stretches a little past the ends and returns.
     - On release, a fling (500 dp/s or faster) or a drag past 35 % of the width settles on the next view; otherwise the row settles on the nearest view. As on Android, lifting adds no speed, so a finger that stopped first doesn't fling.
     - With wrap-around on, the views sit on a ring. A move that starts before the last one has settled, such as two quick screen-reader actions, goes the shorter way round, so no view jumps and the row never moves more than one and a half views.
     - A touch on the moving row catches it. Home, opening Settings and a new layout end a drag.
     - The system animation speed is read at start and on every `Resumed`. At 0, views change at once.
5. **The touch contract.**
   - Every touch reaches the carousel first, in `HomeViews.dispatchTouchEvent`. The content below keeps a gesture until the carousel takes it.
   - An element that uses horizontal drags itself, such as a dragged icon (#13) or Recent's pinch (#23), keeps them by calling `requestDisallowInterceptTouchEvent(true)` before the carousel takes them.
6. **Screen readers.**
   - The home views are a scrollable node with the scroll and page actions and two named actions, "Next view" and "Previous view" (A11Y-05). An action past an end, with wrap-around off, reports that nothing moved.
   - A status text of one transparent pixel says "Applications, 2 of 3". It is a polite live region, so changes are announced (A11Y-08); `announceForAccessibility` is deprecated. The indicator can't carry it: it is fully transparent while the views rest, and Android hides views without opacity from screen readers.
   - A view out of sight is hidden, from screen readers too.
7. **The home views are opaque.**
   - The window no longer shows the wallpaper, and its background matches the device's dark setting until the first frame (HOME-11).
   - System-bar icons follow the background the Rust app draws: dark icons on a light background, light ones on a dark background.
   - Until each view's content arrives (#9, #19, #22), it shows its name.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0002, decision 5 | The toolkit is chosen: Android's views, with Rust deciding what they show. `View`, `Slot` and `Dispatch` stay toolkit-agnostic. |

## Departures from the SRS (MAINT-05)

None.

## Consequences

- **Benefits:**
  - The gesture, the spring, wrap-around, right-to-left mirroring, the indicator's timing and reduced motion are all tested without a device, in `meegone-shell-state` and `meegone-core-state`.
  - Touches move the views in the same frame: Android delivers input at the start of a frame, and `translationX` changes no layout.
  - Accessibility and right-to-left layout come from the platform, not from code of our own.
  - No new library: the Java host stays on the Android SDK.
- **Costs:**
  - Each feature's content will need its own views in Java and its own pull from Rust, like `nativeRead`.
  - The frame's slots, like `Bridge`'s methods, must agree between Java and Rust, and nothing checks that at build time.
  - The frame budget (PERF-03) can only be measured on a device, and the reference device is still to be chosen (#48).
