# ADR 0025: Recent's close mode and Clear all

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #24

## Context

Issue #24 lets the user tidy Recent without pretending to close apps. On the N9, a long-press showed a ✕ on each card and "Close all".
- A long-press anywhere on the grid enters close mode: a ✕ on every card and **Clear all**. Tapping empty space, Back or Home leaves it (REC-07).
- ✕ and Clear all remove cards from Recent only. No app is stopped, and a removed app comes back when it is next used. The confirmation reads "Removed from Recent" (REC-08, AC-14).
- The only usage-related data stored is the list of apps removed from Recent, with the time of removal (PRIV-03).
- **Motion.** Entering, the cards shrink to 92 % and the ✕ badges pop in with a short stagger, reversed on exit (ANIM-12). A removed card shrinks and fades and the others close the gap. Clear all collapses the cards in a quick stagger (ANIM-18).
- **Accessibility.** The ✕ is named "Remove Mail from Recent" (A11Y-01). Recent can be cleared without gestures (A11Y-05, AC-19). Removals are announced (A11Y-08, Should).
- A light tick on entering (ANIM-24, Should).

What was there already:
- `RecentState.close_mode`, `RecentIntent::EnterCloseMode` and `Remove(AppId)`. `Remove` saved the removal time in `Settings.removed_from_recent`, which `cards` already used to leave the app out until its next use (ADR 0023).
- `on_back`, which left close mode.
- The shell's long-press in Recent, which entered close mode and ticked (ADR 0008).
- `motion::CARD_REMOVE`, `CARDS_CLOSE_GAP` and `CLEAR_ALL_MAX`.
- Nothing drew close mode.

The constraints:
- **A long-press on a card never reached Rust.** `HomeViews` sends `nativeLongPress` only on empty space (ADR 0008), and cards are clickable (ADR 0023). Lifting the finger opened the app.
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, and cards travel by revision.
- **Recent's reflow** (ADR 0024) moves the card views themselves, so another scale on the same views would undo it.
- **The removed-apps list was never pruned**, so it grew with every removal.

## Decision

1. **Readings of the SRS.**
   - **ANIM-18's durations follow the issue.** A removed card shrinks and fades over 180 ms (`CARD_REMOVE`), and the others close the gap in the same time. Clear all collapses row by row: each row over 180 ms, the last starting 220 ms after the first, so it is over within 400 ms (`CLEAR_ALL_MAX`). `CARDS_CLOSE_GAP` is renamed `CLEAR_ALL_STAGGER`.
   - **The confirmation** is a short message, "Removed from Recent", after a ✕ or Clear all. Nothing says an app was closed (REC-01).
   - **A tap on a card outside its ✕ does nothing** in close mode, as a tap on an icon does in edit mode (ADR 0014).
2. **Recent owns close mode and removals.**
   - `EnterCloseMode` counts only while Recent shows cards. `ExitCloseMode` serves a tap on empty space.
   - `ClearAll` removes every card shown in one settings write. `Remove` and `ClearAll` share one path:
     - A failed save keeps the cards and becomes the `problem` (ARCH-09).
     - Close mode ends with the last card.
     - `removals`, a counter, changes, so the host confirms.
   - In close mode, `Launch` does nothing. A refresh that leaves no card, or no access, ends close mode.
   - **Pruning (PRIV-03).** Each removal write forgets removals older than 72 hours (`REMOVALS_KEPT_MS`). No window reaches further back (REC-11), so those removals hide nothing. The file format doesn't change.
3. **The shell owns the gesture and the motion.**
   - **Long-press.** In Recent, a long-press counts only while close mode can start: Recent works, shows cards, isn't in close mode, and nothing covers it. Then it enters close mode and ticks (ANIM-24). Otherwise it takes nothing and doesn't tick.
   - **Leaving close mode.** Back and Home leave it through `on_back`, as before. So does showing another view, as leaving Applications ends edit mode.
   - **No pinch in close mode.** `density_offered` is false in close mode, so neither the pinch nor the screen reader's density action is offered.
   - **Entering and leaving** reuse edit mode's motion (`mode.rs`, ANIM-12), in a second `Mode`: the cards shrink to 92 % over 150 ms, and each row's badges pop in, the last at most 120 ms after the first. Recent has at most nine rows (`MAX_ROWS`), and every row counts as visible.
   - **Cards leaving** (`recent_change.rs`):
     - When cards leave while Recent shows with access, the shell starts the leaving motion with their rows in the cards' previous layout. A ✕, Clear all and a refresh are handled alike.
     - Rank `r` of `n` distinct rows starts `CLEAR_ALL_STAGGER × r/(n−1)` in, and goes over `CARD_REMOVE`, decelerating. With animations removed it is instant (ANIM-05).
     - If cards remain, the pinch's reflow (ADR 0024) also starts, now with a length: `CARD_REMOVE` here, `RECENT_PINCH` for a density change. The others close the gap through the host's existing reflow.
     - Cards that leave while Recent is out of sight, such as a hidden app's, just go.
   - **Screen readers** enter close mode without the tick, while `close_offered` says they can.
   - **Health.** A failed Recent never shows close mode or offers it (ARCH-09).
4. **The view says where each card was.** `ShownCard.from` is its place in the cards the host last read, as Events' rows do (ADR 0020). New view methods:

   | Method | Does |
   |---|---|
   | `badge_tapped(revision, index)` | Names the card as a tap does, and replaces `remove_tapped(AppId)` |
   | `clear_all_tapped()` | Clear all |
   | `empty_tapped()` | A tap on empty space |
   | `close_mode_requested()` | The screen-reader path |

5. **The host draws it.**
   - **Long-press.** `HomeViews` sends `nativeLongPress` for a long-press on a Recent card too (REC-07). Once Rust takes it, the rest of the gesture is the shell's, so lifting the finger opens nothing.
   - **Tap on empty space.** A tap on Recent in close mode where nothing takes taps calls `nativeRecentEmptyTapped`: lifted within the touch slop, before the long-press time.
   - **Each card has a body**: the plate, the label and the age.
     - The card view moves and scales as the cards reflow (ADR 0024).
     - The body takes close mode's scale and the leaving shrink and fade, about its centre, so neither undoes the other.
   - **The ✕.**
     - `Badge` draws it, shared with edit mode's cells.
     - Its centre is 16 dp inside the plate's top-start corner, so it stays on the plate, mirrored right to left.
     - A tap within the card's top-start 48 dp square taps it (A11Y-02).
   - **Leaving cards.**
     - A card that no card keeps leaves where it was, takes no touches, and is hidden from screen readers.
     - It shrinks towards 50 % and fades by its row's progress, then goes.
     - "No recent apps" shows once the last one has gone.
   - **Clear all** is a text button in the accent colour at the end of the Recent section header, at least 48 dp, like Events' Clear (ADR 0021). It fades with close mode and takes taps only in it.
   - **The confirmation.**
     - One short message, "Removed from Recent", which replaces one still showing, so quick removals don't queue.
     - With touch exploration on, the status line says it instead: "Mail removed from Recent" when one card went, otherwise "Removed from Recent". The message would be read too.
6. **Screen readers.**
   - The home views offer "Edit Recent" while `close_offered`. It is Recent's counterpart of "Home menu", since a long-press in Recent enters close mode instead of opening the menu.
   - In close mode, each card offers "Remove ‹app› from Recent", and its click isn't offered. Clear all reads "Remove all apps from Recent". Back leaves close mode.
   - The status line says "Editing Recent" on entering, and the view's name on leaving, as edit mode does (ADR 0014).
7. **The host contract.**
   - **`RecentFrame`** gains:
     - `close: CloseFrame`, with `on`, `scale`, `progress` and `badges` per row;
     - `close_offered`;
     - `removals`;
     - `leave`, per row.
   - **`ShellState`** gains `close`, `close_offered` and `recent_change`.
   - **Card fields** grow to 9. The ninth is where the card was, or -1.
   - **The frame grows to 127 slots:**

     | Slot | Holds |
     |---|---|
     | 104 | Close mode on |
     | 105 | Every card's scale |
     | 106 | Clear all's opacity |
     | 107 | "Edit Recent" offered |
     | 108 | Removals |
     | 109–117 | The ✕ scale per row |
     | 118–126 | How far each row's leaving cards have gone |

   - **New natives on `HomeViews`:**

     | Native | Notes |
     |---|---|
     | `nativeRecentBadgeTapped(revision, index)` | Returns Recent's `problem`; only in close mode |
     | `nativeRecentClearAll()` | Returns Recent's `problem`; only in close mode |
     | `nativeRecentEmptyTapped()` | |
     | `nativeRecentEditRequested()` | |

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 127 slots. |
| 0008, decisions 1 and 2 | A long-press on a Recent card reaches Rust too, as `LongPress`. In Recent it counts only while close mode can start. |
| 0014, decision 6 | The ✕ is drawn by `Badge`, which Recent's cards share. |
| 0023, decisions 6 and 7 | A long-press on a card enters close mode. Cards have nine fields; the ninth is where the card was. |
| 0024, decisions 3 and 6 | No pinch, and no density action, in close mode. The reflow takes a length, and also closes the gap a removed card left. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| A11Y-01: the ✕ named "Remove Mail from Recent" | The ✕ isn't a node of its own: it is the card's "Remove Mail from Recent" action, as edit mode's ✕ is (ADR 0014). |
| ANIM-18: "180 ms and 220 ms; Clear all 400 ms at most" | Read as the issue states it: 180 ms for a card and the gap, and Clear all staggered over 220 ms, so it ends within 400 ms. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - Close mode only with cards, its ends, no launch in it, and removals by ✕ and by Clear all in one write. A failed save, pruning, and an app returning once used again.
    - The view's card places across two revisions, and stale ✕ taps.
    - The long-press's gating and tick, close mode's motion and its reverse, and no pinch in it.
    - The leaving motion's timing, stagger, speed and instant form, and that unseen cards just go. A failing Recent.
    - The issue's acceptance criteria, in `tests/acceptance.rs`:
      - AC-14: every card gone, nothing launched, uninstalled or asked about, only the settings written, and an app back once used.
      - REC-07's ways out, REC-08's ✕, ANIM-12 at rest, and ANIM-18's 180 ms and 400 ms.
      - AC-18 with animations removed, and A11Y-05's path.
  - Checked on a Galaxy S22 (One UI 8, Android 16), with MeeGone as the default home app:
    - A long-press on a card, and one between cards, enters close mode. The cards shrink, every card has a ✕ on its plate, and Clear all shows in the header.
    - A ✕ removes its card. A screen recording shows it shrinking and fading as the next card slides into its place, and "Removed from Recent" shows.
    - A tap between rows and Back each leave close mode.
    - Clear all leaves "No recent apps". Clock, opened afterwards, is back on its own.
    - The first build left the long-pressed card's ripple showing, since close mode took its touches before the press ended. Entering close mode now ends the press.
- **Costs:**
  - The frame slots, the card fields and the native methods must agree between Java and Rust by hand.
  - Every row's badges count as visible, so on a phone the visible rows pop in closer together than edit mode's.
  - Clear all at the top of the section scrolls away with the title when Recent is scrolled.
  - The removals list is pruned only when Recent saves a removal, so a removal older than 72 hours stays in the file until the next one.
  - Not checked on a device yet: TalkBack's actions and announcements, right to left, animations removed, a tablet, and dark mode (#32). The device matrix (#43) covers them.
