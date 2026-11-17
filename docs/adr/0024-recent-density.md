# ADR 0024: Recent's density and its pinch

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #23

## Context

Issue #23 lets the user choose between more cards and bigger ones in Recent, as the N9 did:
- Three columns by default, 3×3 on one screen of a typical phone. Pinching in switches to two columns, 2×2, and pinching out switches back. The choice persists (REC-05, AC-13).
- On large screens both layouts add columns, so cards keep a similar size (REC-05).
- The cards scale with the fingers, at most one frame behind. On release they reflow to two or three columns in about 300 ms (ANIM-17, ANIM-01).
- The density can also change without the gesture (A11Y-05).
- The default, 3×3 or 2×2, is still open, to be settled in user testing.

What was there already:
- `Settings.recent_density` (`ThreeColumns` by default, or `TwoColumns`), saved in the settings file since version 1 and counted among SET-04's options.
- `RecentState.density`, read at every refresh, and `RecentView::columns()`, which turned it into 3 or 2 for the frame's slot 96 (ADR 0023).
- `motion::RECENT_PINCH`, 300 ms.

The constraints:
- **Only the first finger reached Rust** (`nativePointer`, ADR 0006). Every touch reaches the carousel first, and the content below keeps a gesture until something takes it.
- **Rust decides, Java draws** (ADR 0006). Motion is state, and the host pulls a frame of `int`s.
- **A plate had the window's proportions** (ADR 0023). On a 20:9 phone that showed about two and a half rows of three, which ADR 0023 left to this issue.
- **Recent has no home menu.** A long-press there enters close mode (HOME-10, REC-07), and SET-02 lists only the time window under Settings → Recent.

## Decision

1. **Readings of the SRS.**
   - **Direction.** The cards scale with the fingers (ANIM-17), so spreading the fingers grows them, to two columns, and bringing them together shrinks them, to three. REC-05's "pinching in" is zooming in.
   - **The default.** It stays three columns, `RecentDensity::default()`, one line to change after user testing.
2. **Recent owns the density and its columns.**
   - `RecentIntent::SetDensity` saves `recent_density` through the settings store. Recent becomes that field's single writer (ARCH-06). A failed save keeps the density and becomes Recent's `problem` (ARCH-09). The file format doesn't change.
   - `RecentIntent::Layout { width_dp }` keeps the cards area's width, which the host reports.
   - `columns_for(density, width_dp)` is pure, as APPS-02's is:
     - More cards: three, unless three cards would be wider than 160 dp; then as few columns as keep every card at most that wide. Any phone's cards area is under 504 dp, so a phone always has three, and a tablet's cards stay between about 117 and 160 dp wide.
     - Bigger cards: two columns for every three of more cards, rounded, and at least two. They are about half as wide again, and always fewer.
     - A width not reported yet gives a phone's columns.
   - `phone_columns(density)`, 3 or 2, is also how many rows one screen holds: 3×3 or 2×2.
3. **The shell owns the pinch** (`pinch.rs`).
   - **The host passes both fingers.** `ShellIntent::Pinch(PinchEvent)` carries the two fingers' span and the point between them, alongside the first finger's `Pointer` events: `Start` when the second finger goes down, then `Move`, then `End` when one of the two lifts, or `Cancel`.
   - **Where a pinch may start.** A pinch starts only on Recent's cards with nothing covering them, while the views are at rest and no long-press holds the finger. A failed Recent never offers it (ARCH-09).
   - **The pinch takes the gesture.**
     - The carousel lets go of the first finger and stays locked until the next gesture starts, so neither finger, nor the one left after the other lifts, swipes the views.
     - No long-press counts.
     - `nativePointer` and `nativePinch` report it as taken, so the content below gets a cancel and never scrolls.
   - **The scale follows the fingers.** It is the span over the starting span, published with each event (ANIM-01). The other density's size is the ratio of the two column counts: 1.5 from three columns to two on a phone, ⅔ back. Past either size it follows a third as much, at most a quarter of the way further, measured on a log scale.
   - **Release.** Past 35 % of the way, on the same log scale, the shell switches the density. Short of it, or cancelled, the cards go back. Home, a menu, Settings, a new layout or Recent losing its cards ends a pinch without switching.
   - **Reflow.** After any pinch, and whenever the density changes, including from a screen reader, the cards reflow on the shared `Timed` clock: 300 ms, decelerating, instant with animations removed (ANIM-05). A new pinch finishes a reflow at once.
4. **Screen readers switch without a pinch** (A11Y-05). While Recent shows its cards with nothing covering them, the home views offer "Bigger cards" or "More cards", whichever isn't shown, beside "Home menu" and "Search apps". TalkBack lists them in its actions menu, and Switch Access and Voice Access offer them too. The status line says the new density (A11Y-08). There is no Settings item, so SET-02 is unchanged.
5. **The host draws the cards to fit a screen.**
   - `RecentList` reports its cards area's width with `nativeRecentLayout` whenever it changes, and draws the frame's columns.
   - **A plate's height** is the smaller of two:
     - the window's proportions, as before;
     - the height that lets the frame's rows fit on one screen below the title, with their labels and ages at the current font size.
     It is never squatter than 0.6. A 20:9 phone now shows three whole rows of three, or two of two.
   - **While pinching**, the grid scales about where the fingers met, worked out from the views' layout and scroll. Cards may grow past the margins.
   - **Reflow.** When the frame's reflow counter changes, the host records where each card is drawn, at the scale it was drawn. It then drops the scale and lays the cards out in their columns. Each card starts from its recorded rect and moves to its place by the frame's progress, as the grid's cells do (ANIM-21).
6. **The host contract.**
   - **`RecentFrame` gains:**
     - `rows_per_screen`;
     - `density_offered`;
     - `scale` and `pivot`;
     - `reflow` and `reflow_progress`.
   - **`ShellState` gains** `pinch: PinchState` and `density_offered`.
   - **The frame grows to 104 slots:**
     - 97: the cards' scale in ten-thousandths;
     - 98 and 99: where the fingers met;
     - 100: the reflow counter;
     - 101: the reflow's progress;
     - 102: rows on one screen, 3 or 2;
     - 103: whether screen readers can switch.
   - **New natives:**
     - `HomeViews.nativePinch(int action, float span, float x, float y, long timeNanos)`;
     - `nativeRecentLayout(int widthPx)`;
     - `nativeRecentDensity(int rows)`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 104 slots. The host also passes a second finger, through `nativePinch`. |
| 0023, decision 6 | A plate's height is also limited so the frame's rows fit on one screen. The cards area's width goes to Rust, which picks the columns. |
| 0023, departures | REC-05's 3×3 per screen holds on a typical phone. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| A11Y-05: changing Recent density "from a menu" | The menu is the screen reader's actions menu (and Switch Access's and Voice Access's), not a menu drawn on screen: Recent's long-press is close mode, and SET-02 has no density item. |
| REC-04: cards with roughly the screen's proportions | On tall phones, plates are squatter than the screen, so three rows fit (REC-05). |

## Consequences

- **Benefits:**
  - Tested without a device:
    - The columns on every phone width and up to 2 560 dp, with bigger cards always fewer.
    - The density saved and kept after a restart, and a failed save.
    - The scale following each event, the resistance, the 35 % threshold, cancels, and the reflow's length with animations on and off.
    - That no pinch starts off Recent, under Settings, without cards or while Recent fails.
    - That neither finger swipes the views, and that no long-press counts.
    - The screen-reader path.
  - In `tests/acceptance.rs`:
    - AC-13: pinching in with six apps gives two columns, still there after a restart.
    - Pinching back, a tablet's columns, ANIM-17's scale and reflow, and A11Y-05.
  - Checked on a Galaxy S22 (One UI 8, Android 16): Recent shows three whole rows of three below its title.
- **Costs:**
  - A pinch whose save fails reflows back without a message; only the screen-reader path shows one.
  - The frame slots, the pinch's actions and the card fields must agree between Java and Rust by hand.
  - Not checked on a device yet: the pinch itself and its reflow, since adb can't inject two fingers on a device that isn't rooted, and TalkBack's actions. Nor 200 % font size, right to left, a tablet and a foldable. The device matrix (#43) covers these.
