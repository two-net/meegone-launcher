# ADR 0034: The shared motion system

**Status:** Accepted · **Date:** 2026-11-19 · **Issue:** #33

## Context

Issue #33 (Must) asks for fluid, finger-following motion, defined once in the shared core and used by every feature:
- **Following the finger (ANIM-01):** at most one frame behind, while dragging views or icons, pinching, or pulling down for search.
- **Gesture endings (ANIM-02)** carry on at the finger's speed and settle on springs, overshooting by 3 % at most, never easing linearly.
- **Interruptible (ANIM-03):** touching a moving element catches it where it is, and no animation blocks input.
- **Frames (ANIM-04, PERF-03):** at the display's refresh rate, with at most 1 % missing their deadline and none over 700 ms.
- **The system animation speed (ANIM-05, A11Y-07):** durations scale with it. With animations removed, transitions are instant or fades of 100 ms or less, and no function is lost.
- **Lengths (ANIM-06):** 150 to 350 ms for transitions no gesture drives, and nothing decorative over 400 ms. App launches never wait.
- **Over-scroll (ANIM-23):** lists stretch at their ends as the platform's do.
- **Named specs:** springs, durations and easing live in the shared core, and every feature uses them.
- **Haptics (ANIM-24, Should):** a light tick on a long-press, on entering edit or close mode, and when a folder is made, following the system setting.
- **Acceptance:**
  - AC-18: with animations removed, every flow is instant or a short fade, and nothing is missing.
  - A touch on any moving element stops it where it is, and it follows the finger.

What was there already:
- **Every moment of the motion catalogue** (ANIM-07 to ANIM-22), built feature by feature from #5 to #32. Each one ran in `crates/shell/state` on the springs and durations in `meegone-core-state::motion`, took the animation speed, and had tests with animations removed.
- **Following the finger.** `HomeViews` hands each touch to Rust and applies the frame in the same call, so nothing waited for the next vsync, and nothing smoothed the finger.
- **The springs' physics.** `SpringMotion` is exact at any frame time, and `limit_velocity` keeps every release within 3 %.

Three audits found what was missing:
- **The speed wasn't real on a device.**
  - `AndroidSystem::animation_scale` answered 1 (#33).
  - The view indicator's 600 ms stay was scaled, so with animations removed it went the moment the views settled.
  - The icon's spring back and the folder pulse ignored the speed.
  - A motion already playing kept the speed it started with.
  - Only the carousel cleaned the value up, and core's `scaled` panicked on ∞.
- **Easing was each feature's.**
  - Eight modules paired a core duration with an easing of their own.
  - The indicator faded on a curve of its own, and the folder pulsed on another.
  - Rest thresholds, the merge's end scale and the late-frame step were defined twice.
  - Java kept its own copy of the scale a card leaving Recent shrinks to.
- **Things a touch didn't catch, or that blocked input:**
  - The carousel caught any touch while its spring was still above rest, about 0.5 s after a flick. A tap in the last invisible sliver of that was swallowed, and so were a scroll, a long-press, the search pull and a pinch.
  - A row locked under an open folder or search could still be caught.
  - A long-press in a folder still opening was dropped.
  - ✕ badges took no tap until their row had popped in, up to 120 ms after edit or close mode began.
  - A touch on an icon settling after a drop landed it in its cell at once.
  - A pinch during Recent's reflow snapped the cards to their places first.
- **The frame was built twice on every vsync,** once by `nativeFrame` and again by the `nativeRead` that always follows it.
- **A screen reader's "Edit Recent"** entered close mode without the tick that edit mode's entry plays.

## Decision

1. **Every moment of the catalogue is a named spec in core** (`motion.rs`).
   - **`Tween`** is a length with its **`Easing`**: `Linear`, `Decelerate`, `Pop` or `Pulse`. `eased(played)` gives where it is, `length_at(speed)` how long it lasts, and `repeating` restarts it every length, for the pulse.
   - **Springs** stay `SETTLE`, `ICON_RELEASE` and `DROP`.
   - **Each timed moment is a `Tween`**, keeping its values and curves:
     - `RETURN_HOME`, `ICON_PRESS`;
     - `INDICATOR_IN` and `INDICATOR_OUT`, linear, as they were;
     - `MODE_CHANGE`, `MODE_BADGE` and `MODE_STAGGER`;
     - `FOLDER_HOVER_GROW`, `FOLDER_PULSE`, `FOLDER_MERGE` and `FOLDER`;
     - `RECENT_REFLOW`, which replaces `RECENT_PINCH`, and `CARD_REMOVE`;
     - `THEME_CROSSFADE`, `NOTIFICATIONS_CHANGE`, `APP_INSTALL_REMOVE` and `IMAGE_CROSSFADE`;
     - `APP_LAUNCH`, which the system's launch-from-bounds runs with its own timing.
   - **`CATALOGUE`** lists every moment with its id and what cues it:
     - `Touch` while the finger acts;
     - `Release`, a gesture ending;
     - `Transition`, anything else.

     One test holds them all to ANIM-02 and ANIM-06. Transitions last 150 to 350 ms. Nothing lasts over 400 ms, apart from the pulse, which repeats while a finger holds it. No gesture ending is linear, and every spring overshoots by 3 % at most.
   - **What was duplicated moves into core:**
     - the scale's rest thresholds;
     - `MERGE_AWAY_SCALE`, and `CARD_REMOVE_SCALE`;
     - `MAX_FRAME_STEP`;
     - `stagger`, which edit mode's badges and Clear all's rows share.
   - **The shell's motions only use the specs.** `Timed` takes a `Tween` and eases with its curve. `CrossFade` and `PinchMotion::reflow` take one too, and no shell module has an easing of its own.
   - **Recent's frame carries the leaving card's end scale** (slot 141), so `RecentList` keeps no copy.
2. **The system animation speed is one value, read on Android and followed live.**
   - **`motion::speed`** cleans it up, as the carousel did: anything that isn't a number counts as 1, below 0.01 counts as 0, and the most is 10. `scaled` saturates instead of panicking.
   - **`motion_time`** runs a spring's clock at the speed, for the carousel, search, the lift and now the icon's spring back. **`frame_step`** is a tween's step, after `MAX_FRAME_STEP`.
   - **`Bridge.animationScale()`** reads `Settings.Global.ANIMATOR_DURATION_SCALE`, which Developer options' "Animator duration scale" and Accessibility's "Remove animations" both set. It answers in thousandths, so the existing `call_int` carries it, and answers 1000 if it can't read the setting.
   - **`Bridge.watchAnimationScale()`** registers a `ContentObserver` on that setting once per process, as the other `watch` methods do. Its change reaches `HomeActivity.animationScaleChanged`, then `nativeAnimationScaleChanged`, then the new `ShellIntent::AnimationScaleChanged`, and the home redraws.
     - So a change made over the home, such as from the accessibility shortcut, takes effect at once. `Resumed` still reads the speed, as before.
     - The native call does nothing before MeeGone starts, so a callback never throws.
   - **Removing animations ends whatever moves.** When the speed becomes 0, `finish_motions` puts every motion where it was going. A finger keeps what it holds: the dragged row, the pulled field, a lifted icon and a pinch. Each motion gained a `finish` for it, and `Press::land` keeps a held icon at 92 %.
   - **Waits are not motion, so they never scale:**
     - the 150 ms before a dragged icon takes a place (`REORDER_DWELL`);
     - the 400 ms before a folder is armed (`FOLDER_HOVER`), and the icon's grow that marks it;
     - the 600 ms the view indicator stays (`INDICATOR_HOLD`).

     With animations removed, the indicator therefore still shows where the views are, then goes at once. The folder pulse's period and the icon's spring back now follow the speed.
3. **A touch catches what is visibly moving, and never waits for the rest** (ANIM-03).
   - **The carousel** catches the row only while it is further than a third of the touch slop from rest (`CATCH_SLOP_FRACTION`), as Android's paged launchers do.
     - Closer, `Carousel::touch_down` lands it at once, before anything else decides what the touch is. The touch is then the content's: a tap, a scroll, a long-press, the search pull or a pinch.
     - A locked row is never caught. It lands, and the touch is the open folder's or search's.
   - **A settling dropped icon** is caught by a finger inside its drawn bounds. It goes back to lifted, held where the finger caught it, its scale heading to 110 % again, and the shell picks it up as after a long-press.
     - The carousel holds the rest of the gesture, so neither the row nor the content gets it.
     - A drop then saves as the first did.
     - A finger anywhere else still lands it, and a merging icon, at once.
   - **A pinch during the reflow** freezes it where the cards are drawn (`Timed::freeze`). The pinch scales them from there, and its end reflows them on from where they are drawn. `RecentList` already combined the two, as the reflow moves each card and the pinch scales the grid.
   - **A long-press in a folder still opening** opens the panel all the way at once (`FolderMotion::finish`), so its cells are where the host laid them out, and counts.
   - **✕ badges take a tap as soon as the mode is on,** whether or not their row has popped in.
   - **Transitions no finger moves** keep finishing at once, or carrying on, under a touch. None blocks input:
     - the return home and a merge finish;
     - the cross-fades, the list changes and Clear all carry on.
4. **Ticks.** Entering close mode ticks by any route, as entering edit mode does, unless the long-press that entered it just ticked. So "Edit Recent" from a screen reader now ticks. Every other tick was already there, through `performHapticFeedback(LONG_PRESS)`, which follows the system's touch-feedback setting.
5. **Frames.**
   - `nativeFrame` only advances the motion. The `nativeRead` that `apply()` always calls next builds the frame, so a vsync builds it once.
   - The shell stops comparing Recent's cards on every dispatch: while Recent's store publishes no new state, they haven't changed.
   - Measuring frames against PERF-03 on the reference device belongs to #40.
6. **Over-scroll stays the platform's** (ANIM-23).
   - Every scrolling view keeps Android's edge effect: the stretch from Android 12, and before that the glow `Themed` colours.
   - At the top of Applications, the search pull is the elastic end instead (ANIM-16), with its own resistance.
   - Lists that fit their screen don't stretch (`OVER_SCROLL_IF_CONTENT_SCROLLS`), as the platform's don't.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 142 slots. |
| 0006, decision 4 | A touch catches the moving row only while it is further than a third of the touch slop from rest; closer, the row lands and the touch is the content's. A locked row is never caught. The speed is also read on `AnimationScaleChanged`, and its going to 0 ends every motion. |
| 0014, decision 5 | A ✕ takes a tap as soon as edit mode is on. A finger on a settling icon picks it up again. |
| 0015, decision 6 | A long-press in a panel still opening opens it all the way at once and counts. |
| 0024, decision 3 | A new pinch catches the reflow where it is drawn instead of finishing it. |
| 0025, decision 3 | Screen readers enter close mode with the tick, as edit mode's entry has. A ✕ takes a tap as soon as close mode is on. |
| 0033, Costs | Lifted: Android's animation speed is read, so the theme's fade is instant with animations removed on a device. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| ANIM-05: durations scale with the system animation speed | Home's scroll back to the top of Applications (HOME-07) is the platform's `smoothScrollTo`, which follows only whether animations are on. It is instant with them removed, and a touch catches it, as any scroll does. |
| ANIM-05: durations scale with the system animation speed | The 600 ms the view indicator stays, the 150 ms before a dragged icon takes a place and the 400 ms before a folder is armed are waits, not motion, so they don't scale. Removing animations would otherwise lose them. |
| ANIM-08: about 300 ms | The system runs the launch-from-bounds transition with its own timing and transition scale. `APP_LAUNCH` names the target. |
| ANIM-03, a second change while one plays | Not a touch, so left as it was: a second card removed within 180 ms, Events' and the grid's rows still leaving, the hovered icon snapping back as a dragged app leaves it, and a newer thumbnail fading in over the one shown in full each start again from where they are drawn, without the earlier one's tail. |

## Consequences

- **Benefits:**
  - **One place for every motion value,** and a test that holds the catalogue to ANIM-02 and ANIM-06.
  - **Removing animations works on a device,** even while the home shows, and loses nothing.
  - **Tested without a device:**
    - Core: every easing's ends and shape, the pulse's repeat, a tween's length at each speed, `speed`'s clean-up, `motion_time`, `stagger`, and a spring run to its end.
    - The shell:
      - the last sliver of a settle going to the content, a visible settle caught, and a locked row never caught;
      - a settling icon caught and dragged on, with the row still;
      - the reflow frozen under a pinch and reflowing on from there;
      - a long-press in a folder still opening;
      - the spring back and the pulse at half speed;
      - the indicator staying 600 ms with animations removed;
      - animations removed mid-motion ending the folder's and the theme's motion;
      - "Edit Recent" ticking.
    - **The acceptance scenarios** (`tests/acceptance.rs`):
      - AC-18 across the flows: the views by finger and screen reader, the return home, the theme, edit mode, a drag and drop, a folder, search, an app installed, a notification, and Recent's pinch;
      - `anim05`: a new speed taken on at once, removing animations mid-swipe, and a folder at half speed;
      - `anim03`: the row and a settling icon caught, and a tap in the last sliver of a settle launching the app.
  - **Built:** `cargo clippy` for `aarch64-linux-android`, and `assembleDebug` and `assembleRelease`.
- **Costs:**
  - **The indicator's stay asks for frames** for 600 ms with animations removed, as it always has at normal speed. Nothing moves meanwhile.
  - **A setting read per process:** one `ContentObserver`, never unregistered, like the other `watch` methods.
  - **Kept in step by hand:**
    - `animationScale` between `Bridge.java` and `android/system.rs`;
    - `nativeAnimationScaleChanged` with `HomeActivity.java`;
    - slot 141 between `frame_slots` and `HomeViews.RECENT_LEAVE_SCALE`.
  - **Not checked on a device yet:** removing animations and the accessibility shortcut over the home, the catches by a real finger, the badges' early taps, TalkBack's tick, and frame timing against PERF-03. The device matrix (#43) and the benchmarks (#40) cover these.
