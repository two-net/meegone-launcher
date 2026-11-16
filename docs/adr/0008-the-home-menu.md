# ADR 0008: The home menu and long-press on empty space

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #7

## Context

Issue #7 adds the N9's long-press on empty space:
- In Events or Applications it opens the home menu: Launcher settings, Lock-screen image and, in Applications only, Edit applications (HOME-10, SET-01).
- In Recent it enters close mode instead (REC-07).
- Back and Home close the menu (HOME-07, HOME-08).
- A screen-reader action opens it (A11Y-05).
- A light haptic tick plays on the long-press (ANIM-24).
- Applications always leaves an empty row below its last row, so the menu can always be reached (HOME-14).

The constraints:

- **The Rust app decides and Java reports** (ADR 0006). The carousel already sees every touch, with the platform's touch slop, so it knows whether a finger is still a press.
- **A finger held still sends no events.** Something has to wake the app when the long-press time has passed. Android's time follows the accessibility "touch and hold delay".
- **Content with its own long-press is coming:** icons (APPS-11, #12), notifications (EVT-08, #20) and Recent's cards (#24). A long-press on them must never open the home menu.
- **MeeGone asks for no permissions** (SEC-03), and vibrating directly needs `VIBRATE`.
- **Menus follow MeeGone's theme** (THEME-03), which can differ from the device's.
- **Settings (#34), the grid (#9) and the image details screen (#30) aren't drawn yet.**

## Decision

1. **The host times the long-press; Rust decides whether it counts.**
   - `HomeViews` posts a callback after `ViewConfiguration.getLongPressTimeout()` when the driving finger goes down. Lifting it, a second finger, or the carousel or a child taking the gesture cancels the callback.
   - When the callback fires, Java checks what is under the finger. The press is on **empty space** only if no clickable or long-clickable element of the current page is there. An element with its own long-press therefore always wins, without any timing race.
   - Java then calls `nativeLongPress`, which sends `ShellIntent::LongPress`. The shell takes the long-press only while the carousel's gesture is still a press: down, within the touch slop, not dragging or catching the row. A slow scroll that stops therefore never opens the menu.
   - Once it is taken, the gesture is `Held`: `CarouselState.held` stays true until the finger lifts. `nativePointer` reports it like a drag, so the content below gets a cancel and a lift taps nothing.
2. **What a long-press does is the shell's call.**
   - Events and Applications: the home menu opens where the finger went down.
   - Recent: close mode.
   - Either way, `SystemConfigAdapter::haptic_tick` plays a tick (ANIM-24). A long-press the shell doesn't take plays none.
3. **The home menu is shell state, drawn in the home views' own tree.**
   - `ShellState.menu` holds a `HomeMenu`: its items for the view (`HomeMenu::items_for`) and the anchor, or `None` when a screen reader opened it.
   - `HomeViews` draws it as a card in the `surface` colour, from the frame. It does not use Android's `PopupMenu`, which is a separate window: that window would take Back itself, bypassing HOME-08, and would follow the device's theme rather than MeeGone's.
   - The card opens at the finger, towards the end of the line, above the finger when there is no room below, and inside the view below the status bar.
4. **The menu covers the home views, and closes on:**
   - Back: Settings first, then the menu, then the view's topmost element.
   - Home: Settings first; otherwise the menu and everything open in the view close together, and Home goes no further.
   - Choosing an item.
   - A touch outside it, which goes nowhere: the host sends `CloseMenu` and swallows the gesture.
   - A change of view or of layout, opening Settings, and Home from an app with "Always Applications" (ADR 0007).

   While it is open, the carousel gets no `Down` or `Move`, so nothing behind it moves. `Up` and `Cancel` still arrive, so a held gesture ends. Like other open elements, it isn't kept when the system ends the process (REL-05).
5. **The items.**
   - Launcher settings opens Settings.
   - Lock-screen image opens Settings at its Lock-screen image group (`SettingsGroup::LockScreenImage`, SET-02), so it works before any image has been downloaded (WALL-02, WALL-10).
   - Edit applications enters edit mode.
   - An item the menu doesn't offer is ignored.
   - Until #34 draws Settings, Settings covers the home views with its name (and the group's), as the home views show theirs. Without it, a chosen item would leave an invisible screen that swallows touches.
6. **Screen readers** (A11Y-05).
   - While the current view offers the menu and nothing covers it, the home views add a "Home menu" action beside "Next view" and "Previous view". It sends `ShellIntent::OpenMenu`, and the menu opens centred.
   - The menu has a pane title. The pages behind it, or behind Settings, are hidden from screen readers. Focus moves to the first item.
7. **The haptic tick needs no permission.**
   - `Bridge.hapticTick` calls `performHapticFeedback(LONG_PRESS)` on the resumed activity's window.
   - A view's haptics follow the system's touch-feedback setting and need no `VIBRATE` permission. Without a window, nothing plays.
8. **HOME-14 is a rule the grid follows.** `AppsState::rows(columns)` is every item's row plus one empty row. The grid (#9) lays out that many rows, and its empty cells are empty space by decision 1.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 18 slots: the menu's items, whether the view offers it, its anchor, the surface colour and Settings follow the content's opacity. `HomeViews` gains `nativeLongPress`, `nativeOpenMenu` and `nativeMenu`. |
| 0006, decision 5 | An element with its own long-press is clickable or long-clickable. A long-press on it never opens the home menu. |
| 0007, decision 3 | Home closes the home menu together with everything else open in the view. |

## Departures from the SRS (MAINT-05)

None. ANIM-24's ticks on entering edit mode and on creating a folder come with those features (#13, #14).

## Consequences

- **Benefits:**
  - Which long-press counts, what it does and when the menu closes are tested without a device, in `meegone-shell-state` and `tests/acceptance.rs`.
  - Later content needs no code of its own to keep its long-press: being clickable is enough.
  - Back, Home and the theme work on the menu exactly as on everything else on the home.
- **Costs:**
  - The empty-space check walks the current page's views on each long-press. That is cheap for one page, but it relies on content being clickable where it handles touches.
  - The frame's slots and the new native methods must agree between Java and Rust, and nothing checks that at build time.
  - The menu appears at once. The motion catalogue has no menu motion; one can join ANIM later.
