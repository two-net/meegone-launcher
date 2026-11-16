# ADR 0014: Edit mode

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #13

## Context

Issue #13 draws edit mode in Applications and makes it work:
- A ✕ on every app the user can uninstall. Tapping it opens the system's confirmation; MeeGone never uninstalls anything itself (APPS-12, APPS-13).
- Icons that can be dragged while the others move aside, with auto-scroll near the top or bottom. Done, Home or Back end it (APPS-12).
- The new order saved, so it survives restarts (APPS-18).
- The motion of entering (ANIM-12) and of dragging and dropping (ANIM-13), a tick on entering (ANIM-24, Should).
- Screen readers: the ✕ named "Uninstall Mail", "Move to…" actions, and "Edit mode" announced (A11Y-01, A11Y-05, A11Y-08).
- "Sort A–Z", with a confirmation and a 5-second undo (APPS-14, Could).

The constraints:
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, and per-cell data travels only with the cells' revision (ADR 0010).
- **The drag already starts** (ADR 0013): a long-press opens the app's menu, and moving on picks the icon up and enters edit mode. Edit mode was only a flag, and a dropped icon went back to its cell.
- **Saving is atomic and on the main thread** (ADR 0004). A save for every cell the icon passes would cost frames (PERF-03).
- **Some layout entries show no cell**, such as the apps of a locked profile (ADR 0012).
- **A failed store is never called again** (ARCH-09).
- **Rust didn't know where the grid's cells are** on screen, or how tall the grid is.

## Decision

1. **Applications owns the arrangement.**
   - `Arrange { app, to }` shows the app's own cell at place `to` while the others move aside. Nothing is saved yet. A refresh keeps it, since a place is where the finger is; it ends if the app leaves.
   - `Drop` keeps what is shown, and saves once:
     - `follow_order` fills the slots of the entries that show a cell again, in the order shown.
     - Entries with no cell keep their slots.
   - `CancelArrange` shows the saved order again. Leaving edit mode, any way, does too.
   - `Move { app, to }` is both at once. It serves screen readers, and a drop before the wait below is over.
   - `RequestUninstall` serves the ✕, only for `App::can_uninstall`. `ExitEditMode` serves Done.
   - In edit mode, `Launch` does nothing.
   - Nothing moves while the saved layout couldn't be read, since nothing could be saved (REL-02).
   - `show()` is the only place the cells are made, so a launch's "opened" mark and an arrangement can't disagree.
2. **The shell owns the drag** (`lift.rs`).
   - Phases: armed, lifted, settling.
   - **Taking a place.** The place under the icon's centre becomes its own once the centre has stayed there for 150 ms (`REORDER_DWELL`). A quick drag across the grid shuffles nothing. A place stays the target while the centre is within 15 % of a cell past its edges.
   - **Auto-scroll.**
     - The zones are the 64 dp at the top and bottom of the visible grid. The grid scrolls up to 640 dp a second at the very edge (`AUTO_SCROLL_ZONE_DP`, `AUTO_SCROLL_MAX_DP`).
     - An icon picked up inside a zone scrolls only once it has been out of it.
     - At the end of the grid it stops, and stops asking for frames.
   - **Under the finger.** The icon is drawn from its cell's *current* place, which moves with each `Arrange`, plus how far the grid has scrolled.
   - **Dropping.**
     - Lifting the finger sends `Move` to the place under it.
     - Then the icon settles into its cell on the `DROP` spring, damping 0.8 and stiffness 900: about 220 ms, overshooting by about 1.5 %. It lands at edit mode's 92 %.
     - Picking it up springs its scale to 110 % the same way.
   - **Any other end** puts the grid back: Back, Home, a new layout, Settings or another view. One rule in the shell's dispatch epilogue sends `CancelArrange` when an icon stops being lifted.
   - **Apps coming and going** leave a dragged icon lifted, unless its own app left.
3. **The grid's geometry is in Rust.**
   - `GridLayout` gains the room above and below the cells (the safe area) and the grid's height.
   - `AppsGrid` reports them whenever its width, row height, padding or height change.
   - `cell_left`, `cell_top`, `place_at` and `max_scroll` copy `AppsGrid`'s integer layout: `width * column / columns`, mirrored right to left.
   - The shell takes right to left from its own layout.
4. **One rule for the grid's motion.**
   - When the cells come, go *or move*, the shell starts the grid's motion and lets go of a pressed icon. This is ANIM-21's 200 ms, decelerating. The rule is in the dispatch epilogue, so an install, a removal, an arrangement and a screen reader's move are handled alike.
   - `AppsGrid` slides each cell from where it is *drawn* when the cells change, not from its old place, so a change that comes while another plays never makes a cell jump.
   - Each cell's previous place is in the cells the host last read, so two revisions between reads still move the right views.
5. **Edit mode's motion** (`mode.rs`).
   - **Entering.** The cells shrink from 1 to 0.92 over 150 ms, decelerating. Each visible row's badges pop in over 150 ms, about 2.3 % past full size and back. The first visible row starts at once, and the last 120 ms later at most.
   - **Leaving** plays it backwards. A change of mind turns back from where it is.
   - With animations removed, it is instant (ANIM-05).
   - **Health.** Edit mode counts only while Applications is `Ready` (ARCH-09). A failed Applications therefore leaves no badges, no Done and no drag on screen.
   - **The tick.** Entering from a menu ticks. Picking an icon up doesn't, since its long-press just did.
   - **Leaving Applications** ends edit mode, so Done is never left out of sight.
   - The home menu leaves out Edit applications in edit mode, as the app menu does, and no icon shrinks under the finger.
6. **Drawing.**
   - **Scale.** The whole cell takes edit mode's scale. A lifted or settling cell takes the lift's scale instead, and ignores the grid's motion, so it stays under the finger.
   - **The ✕.**
     - A 24 dp disc in the surface colour, with a faint ring and a cross in the text colour.
     - It sits over the icon's top-start corner, inside the cell, mirrored right to left.
     - A tap within the cell's top-start 48 dp square taps it (A11Y-02). Any other tap does nothing.
   - **Done.**
     - A flat pill in the accent colour, with the background colour's text, tested at 4.5:1 in both themes.
     - It sits at the bottom centre of the Applications page, inside the safe area, over the grid's empty last row, and fades with edit mode.
   - **Scrolling.** The grid scrolls smoothly to the top only for Home, never while an icon is lifted, so auto-scroll reaching the top isn't fought.
7. **Screen readers.**
   - In edit mode, each app's cell offers:
     - "Uninstall ‹label›", if it has a ✕;
     - Move to start, Move earlier, Move later and Move to end.
   - The live region that names the view says "Edit mode" on entering. Leaving puts the view's name back, so the next entry is read again.
   - A move is read as "Mail, place 3 of 40".
8. **The host contract.**
   - The frame grows to 52 slots:
     - 36: edit mode on or off;
     - 37: every cell's scale;
     - 38: how far into edit mode, for Done;
     - 39: the grid's first visible row;
     - 40–51: the ✕ scale of each visible row from it. Rows further down take the last.
   - Cells' flags gain 16: the user can uninstall it.
   - `nativeAppsLayout` also takes the room above and below the cells and the grid's height.
   - New native methods, each naming a cell by revision and place as a tap does:
     - `nativeAppBadgeTapped`;
     - `nativeAppMoved`, which returns the new place;
     - `nativeEditDone`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 52 slots. |
| 0010, decision 5 | Cells' flags gain 16, uninstallable. `nativeAppsLayout` also reports the room above and below the cells and the grid's height. |
| 0010, decision 6 | The grid scrolls smoothly to the top only for Home. In edit mode a tap launches nothing and an icon doesn't shrink. |
| 0012, decision 7 | The grid's motion also starts when cells move, from one rule in the shell, and no longer from `apps_changed`. Each cell slides from where it is drawn, and says where it was in the cells the host last read. A dragged icon goes back only if its own app leaves. |
| 0013, decision 3 | Lifting the finger drops the icon in the place under it, where it settles. Its scale springs up when it is picked up. Any other end puts the grid back. |
| 0013, Departures | Entering edit mode ticks, except by picking an icon up. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| APPS-14: "Sort A–Z", with a confirmation and a 5-second undo (Could) | Deferred to a later issue. `follow_order` can serve it. |
| ANIM-13: spring, about 220 ms | The dropped icon settles on a spring of about 220 ms. The neighbours slide on ANIM-21's 200 ms decelerating motion, restarted from where each cell is drawn whenever the icon takes a new place. |
| A11Y-01: the ✕ named "Uninstall Mail" | The ✕ isn't a node of its own: it is the cell's "Uninstall Mail" action. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - The arrangement and its one save, around entries that show no cell.
    - The geometry, against `AppsGrid`'s integer layout.
    - The target, its wait and hysteresis, auto-scroll and its limits, and the settle.
    - The one motion rule, edit mode's motion and its ticks, and a failed Applications.
    - AC-07 and AC-09, Done, Home and Back, and a screen reader's move, in `tests/acceptance.rs`.
  - Checked on a Galaxy S22 (One UI, Android 16), with MeeGone as the default home app:
    - Entering from an app's menu: the cells shrink. Only apps the user installed have a ✕; Camera, Calendar, Gmail and the other system apps have none. Done shows at the bottom.
    - Dragging Gmail from the sixth row to the first cell: it lifts with its shadow, the others move aside, and the drop settles. After a force-stop, Gmail is still first.
    - Holding an icon at the bottom edge scrolls the grid to its end.
    - The ✕ on Teams opens the system's confirmation. Cancelling it leaves Teams in place, and edit mode on.
    - Back makes the badges go from the bottom row up while the cells grow back. Done ends edit mode too.
    - As the default home app, the app menu lists shortcuts, which ADR 0013 couldn't check.
- **Costs:**
  - Not checked on a device yet: TalkBack's actions and announcements, right to left, animations removed, and the tick. The device matrix (#43) covers them.
  - The frame slots, the flags and the native methods must agree between Java and Rust. Nothing checks that at build time.
  - A save that fails after a drop isn't shown: Applications' problem reaches the host only after a tap or a menu choice. The next refresh saves again (ADR 0012).
  - While an icon is lifted and waiting for a place, or scrolling, the host draws frames.
  - Folders (#14) must tell holding an icon over another, to make a folder (FOLD-01), from taking its place. The wait before a place is taken leaves room for that.
