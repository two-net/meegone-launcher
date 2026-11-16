# ADR 0013: The app long-press menu

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #12

## Context

Issue #12 adds a long-press on an app's icon (APPS-11):
- It opens a menu with App info, Uninstall (only when the app can be uninstalled) and Edit applications.
- While MeeGone is the default home app, the menu also lists the app's shortcuts (Should).
- Dragging after the long-press picks the icon up and enters edit mode.
- Uninstall opens the system's confirmation; MeeGone never uninstalls anything itself (APPS-13).
- Back and Home close the menu (HOME-07, HOME-08), a screen-reader action opens it (A11Y-05), and the long-press ticks (ANIM-24).

The constraints:

- **Long-presses only reached Rust on empty space** (ADR 0008). App cells are clickable, so lifting the finger after a long-press on one launched the app (ADR 0010, Consequences).
- **The home menu could show only its three fixed items.** They were bits in one frame slot, with their text in Android's resources. Shortcut labels come from the system.
- **Edit mode exists as state, but #13 draws it** and moves icons: neighbours making room, auto-scroll, the drop and its motion (APPS-12, ANIM-13).
- **The system offers shortcuts only to the default home app.** Since Android 9, its uninstall screen refuses apps without `REQUEST_DELETE_PACKAGES`. MeeGone had no permissions at all.
- **A failed feature's store is never called again** (ARCH-09). A menu it owns could never close.

## Decision

1. **Applications owns the menu; the shell owns the gesture.**
   - `AppsState.menu` is an `AppMenu`: the app and its items, in order.
     - App info.
     - Uninstall if `App::can_uninstall`.
     - Edit applications unless edit mode is on.
     - Then the app's first four shortcuts.
   - Every item's side effect goes through `AppsAdapter` inside Applications' store, as launching does.
     - A failure becomes `problem`, which the host shows briefly.
     - An item the menu doesn't offer does nothing.
   - Applications' `on_back` closes the menu first, then the folder, then edit mode. Home and Back therefore need no new code.
   - A refresh closes the menu if its app was removed. Entering edit mode closes it.
   - The shell decides whether a long-press counts, ticks, treats the menu as covering the views, and owns the picked-up icon. This is the same split as the icon press (ADR 0010).
   - An app's menu counts as open only while Applications is `Ready`, so a failed Applications never leaves the home covered.
2. **The host times the long-press on an icon, as on empty space.**
   - When the timer fires, `HomeViews` finds the element under the finger. If it is an app's `AppCell`, `AppsGrid.nativeAppLongPressed(revision, index)` asks Rust, by the cells' revision and the cell's place, as a tap does. A folder or a stale revision takes nothing.
   - The shell takes it only on Applications, while the gesture is still a press and nothing covers the views.
     1. It opens the menu.
     2. Only if the menu opened does the carousel's gesture become `Held`. The cell then gets a cancel, so lifting the finger launches nothing.
     3. It ticks.
   - Folders still count as empty space until #14 draws them.
3. **Dragging on picks the icon up.**
   - `lift.rs` arms the long-press. A move past the touch slop while the menu is still open enters edit mode, which closes the menu. The icon then follows the finger at 110 % (`DRAG_LIFT_SCALE`), raised 8 dp with a shadow in the icons' shape.
   - Lifting the finger puts it back in its cell at once. It also goes back on Back, on a layout change, when apps come or go, when the view changes, and when Settings opens.
   - The carousel stays `Held`, so a sideways drag never moves the views.
   - #13 adds the drop, neighbours making room, auto-scroll and ANIM-13's motion.
   - `AppCell` keeps the lift apart from the press and the grid's motion, and combines them, so none undoes another.
4. **The menu opens at its icon.**
   - It opens below the icon's cell, or above it when there is no room, starting at the cell's start edge.
   - It is placed again after each layout pass, so it follows the icon through folds and rotations (COMP-03).
   - It is centred if the app has no cell of its own.
   - The home menu still opens at the finger.
5. **One menu card, made for the menu that opens.**
   - `HomeViews` builds the card's items when a menu opens or its items change. An app menu's own items are in the issue's order, then a divider, then its shortcuts.
   - The card is now clickable, so a touch on its padding never reaches an icon behind it.
   - Its pane title names the app ("Mail menu"), and focus moves to the first item.
6. **Shortcuts.**
   - `Bridge.shortcuts` returns `null` unless `LauncherApps.hasShortcutHostPermission()`, so the menu shows none (SRS 8.1, "If unavailable").
   - It lists enabled manifest and dynamic shortcuts for the app's activity, with their short labels.
   - `catalogue::shortcuts` orders them as the system's launcher does: manifest first, then by rank.
   - At most four are shown, as text only.
7. **Uninstall requests.**
   - `Bridge.requestUninstall` starts `ACTION_DELETE` for the package, with `EXTRA_USER` for its profile.
   - The manifest declares `REQUEST_DELETE_PACKAGES`. It is granted at install, with no prompt, which SRS 8.1 lists for uninstall requests (SEC-03). The system still asks the user to confirm each one.
   - `can_uninstall` stays "not a system app". When an admin blocks an uninstall, the system's own screen explains it.
8. **App info** is `LauncherApps.startAppDetailsActivity`, which also serves work-profile apps.
9. **Screen readers.**
   - Each app's cell offers the long-click action, labelled "Open menu" (A11Y-05). It sends `nativeAppMenuRequested`, which opens the menu with no tick and nothing to pick up.
   - The view itself stays not long-clickable, so Android starts no second timer.
10. **The host contract.**
    - The frame grows to 36 slots:
      - 30: the app menu's cell, -1 to centre it, or -2 while it is closed.
      - 31: its revision.
      - 32: the lifted cell, or -1.
      - 33–34: the lift's offset in px.
      - 35: the lift's scale.
    - `HomeViews` reads the menu's items with `nativeAppMenu()`, two fields each: a kind and a shortcut's label. It reports a choice with `nativeAppMenuChosen(revision, item)`, so a stale menu does nothing.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0005, decision 7 | Shortcuts, App info and uninstall requests are no longer stubs. MeeGone's first permission is `REQUEST_DELETE_PACKAGES`, granted at install. |
| 0006, decision 2 | The frame has 36 slots. |
| 0008, decision 1 | A long-press on an app's icon reaches Rust too, as `LongPressApp`. Any other clickable element still keeps its own. |
| 0008, decision 3 | The menu card is clickable. It shows the home menu or an app's menu, whose items are made when it opens. |
| 0010, Consequences | Lifting the finger after a long-press on an icon no longer launches the app. |

## Departures from the SRS (MAINT-05)

None.
- ANIM-13's motion and APPS-12's dragging come with #13.
- ANIM-24's tick on entering edit mode comes with #13. Picking an icon up ticks only for its long-press.

## Consequences

- **Benefits:**
  - Tested without a device:
    - Which long-press counts, the menu's items, Back and Home, and the pick-up.
    - The shortcuts' parsing and order.
    - The issue's acceptance criteria, in `tests/acceptance.rs`.
  - Checked on a Galaxy S22 (One UI, Android 16), with MeeGone as an ordinary app:
    - The menu opens at the icon.
    - Camera, a system app, has no Uninstall.
    - App info opens.
    - Uninstall shows the system's confirmation (cancelled).
    - Lifting launches nothing.
    - A drag lifts the icon, with its shadow, and drops it back.
    - Back closes the menu.
    - The home menu still opens on empty space.
- **Costs:**
  - Not checked on a device yet: shortcuts, which need MeeGone as the default home app, and TalkBack. The device matrix (#43) covers both.
  - Shortcuts are read on the main thread when the menu opens. It is one binder call for one app.
  - The frame slots, the menu's fields and the bridge's methods must agree between Java and Rust. Nothing checks that at build time.
  - Until #13, a dropped icon goes back to its cell, and edit mode isn't drawn.
