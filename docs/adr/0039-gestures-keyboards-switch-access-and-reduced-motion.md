# ADR 0039: Gestures' alternatives, keyboards, switch access and reduced motion

**Status:** Accepted · **Date:** 2026-11-21 · **Issue:** #38

## Context

Issue #38 (Must) asks that every function works without gestures and without animation:
- **A11Y-05:** every gesture has an alternative:
  - switching views through screen-reader actions;
  - moving icons with "Move to…" actions;
  - changing Recent's density from a menu;
  - opening long-press menus through screen-reader actions.
- **A11Y-06:** focus order is logical, and everything works with switch access and a hardware keyboard: arrow keys, Tab and Enter.
- **A11Y-07:** reduced motion is honoured. With animations removed, transitions are instant or fades of 100 ms or less, and no function is lost.
- **Acceptance:** with a screen reader, then with only a hardware keyboard, then with switch access, the user can switch views, launch an app, move an icon, make a folder, clear Recent and open Settings (AC-19). AC-18 asks the same of every flow with animations removed.

Each feature had already given screen readers an alternative to its gesture (ADR 0006, 0013 to 0016, 0024, 0025). The shared motion system made every motion of MeeGone's instant at speed 0 (ADR 0034). Three audits of the code, then a phone, found these gaps:
- **What screen readers could reach:**
  - Six actions were on the home views' container: Next view, Previous view, Home menu, Search apps, Bigger or More cards, and Edit Recent.
  - Screen readers never focus a container that holds other elements, so TalkBack's actions menu couldn't offer them, and the tour had only ever called them directly. AC-19's "open Settings" and "clear Recent" went through them.
  - Recent's cards had no long-click, though a long-press on them enters close mode.
  - "Next view" changed the view under Settings, setup and the image screens.
- **Without a drag:** moving icons and making folders were screen-reader actions only. Someone who can't drag but doesn't use a screen reader had no way.
- **A keyboard:**
  - Only search's Enter was handled. Nothing reached the next view, a long-press menu, the home menu, edit mode's moves, the ✕ or Recent's density.
  - Nothing kept the focus in what was on top. Tab reached cells under Settings, setup and the menus, and Enter launched them.
  - Focus never went back to what a menu was for.
  - Some stops did nothing: containers clickable only to keep touches (ADR 0038), and two rows focusable only to group their text for screen readers.
  - There was no focus indicator in MeeGone's colours, which ADR 0038 left to #38. Android's default takes the device theme's colours and is too faint to see.
  - A focused cell could stop under the system bars: the grid scrolls under them, and Android's scroll to a focused child ignores padding.
  - Android spends the first Esc clearing the focus, and its fallback from Esc to Back isn't on every keyboard.
- **Reduced motion:**
  - Android's own scrolls ran 250 ms whatever the speed: a screen reader's page, a keyboard's arrows and pages, and the scroll to what took focus.
  - Scroll bars faded over 250 ms, and Android 10 and 11's over-scroll glow ignores the speed.
  - MeeGone's dialogs follow the window animation scale, not the animator scale.
  - AC-18's tests left out a folder made by a drop, an app dragged out of a folder, an app removed, notifications cleared and dismissed, coming home to Recent, and search closing.

## Decision

1. **Each view has an element of its own, which carries the view's actions** (A11Y-05).
   - **What it is:** `ViewNode` is the first child of each page. It covers the page, under its content, and draws nothing. Touches pass through it, so the home views' own long-press stays the gesture's (ADR 0008).
   - **Who reaches it:** screen readers focus it, which they never do a container, and come to it first in each view, or by touching empty space. A keyboard's Tab reaches it first in each view.
   - **Its name** is the view's: "Applications, 2 of 3".
   - **Its press,** a click or a long-click, is the view's long-press: the home menu in Events and Applications, Edit Recent in Recent.
   - **Its actions:** Next view, Previous view, Search apps and Bigger or More cards, each while the Rust app offers it.
   - **The container** keeps only the scroll and page actions, for screen readers' scroll gestures and Switch Access's scroll.
   - **A switch from the element** focuses the new view's element. The element reads the view's name, so the status line doesn't say it too.
   - **Recent's cards** offer "Edit Recent" as their long-click, as a long-press on them does.
2. **In edit mode, a cell's menu offers what a drag does** (A11Y-05).
   - **The items:** in Edit applications' place come Move to start, Move earlier, Move later and Move to end. An app in the grid also gets Make folder with ‹next app› or Move into ‹next folder›. An app in the open folder also gets Move out of folder.
   - **What decides them:** Applications offers each only where it moves something (`AppsHolder::moves`), and saves the move as a drop does. They are the same as the screen-reader actions on the cell, which stay.
   - **Who uses them:** a long-press in edit mode opens the menu, as it always did, so a finger that can't drag uses them too. So do a keyboard and a switch.
   - **After a move:** the new place is announced, "Mail, place 3 of 40", and the focus stays on the app. The cell's view moved with it (ADR 0012).
3. **The shell refuses what no finger could reach.**
   - A tap on a cell under a menu or a screen, or on the grid under an open folder or search's results, does nothing.
   - "Next view" and "Previous view" do nothing under setup, Settings, the image details or the past week.
   - They still close a menu, a folder or search open on the view, as the views changing does.
4. **A keyboard reaches everything a gesture does** (A11Y-06).
   - **Android's own keys:** Tab, the arrows and Enter reach every element and act on it.
   - **The other keys:** `HomeViews` maps them to the actions screen readers use, after the focused element has had its chance, so the Rust app decides what each does, as it does for them:

     | Key | Does |
     |---|---|
     | ← or → with nothing further that way in the view | The next or the previous view, mirrored right to left, wrapping as set |
     | ↓ on a view's element | Its first element |
     | Menu, Shift+F10 | The focused element's long-press: an app's, a folder's or a notification's menu, Edit Recent on a card. Elsewhere, the view's. |
     | Delete | The ✕: uninstall in edit mode, remove from Recent in close mode |
     | Ctrl+←/→/↑/↓, Ctrl+Home/End | In edit mode, the focused cell a place or a row earlier or later, or to the start or end |
     | A letter, on Applications | Opens search with it. Ctrl+F opens it empty. |
     | Ctrl+= and Ctrl+-, on Recent | Bigger and more cards |
     | Esc | Back |

   - **Esc:** the home views take it before Android's input stages, which would only clear the focus with it. The activity takes it while nothing has focus.
   - **The list:** `HomeActivity.onProvideKeyboardShortcuts` puts these keys in Android's list of shortcuts, Meta+/.
5. **Only what is on top takes the focus, as only it takes touches** (A11Y-06).
   - **Which layer:** the topmost of the views, the image details, Settings, the past week, setup and a menu takes it. Under an open folder, the grid, Done and Applications' element don't.
   - **What is covered:** what lies under the topmost blocks its descendants' focus, and screen readers skip it. This was the pages' rule (ADR 0038), and now covers every layer.
   - **When one opens,** its first stop in Tab's order takes the focus once it is laid out. A scroll view not laid out yet would take it itself.
   - **When one closes,** the focus goes back to the cell or row it was for, as screen readers' does (ADR 0038). Otherwise it goes to the view's element.
   - **Stops that did nothing:**
     - `A11y.notAButton` also makes its container unfocusable.
     - Settings' text rows and Access's statuses group their text with `setScreenReaderFocusable`.
     - A grid is never a stop itself, only its cells, so search's results with nothing found are none.
6. **A focus ring in MeeGone's colours** (A11Y-06).
   - **What it draws:** `FocusRing` draws a 2 dp ring in the accent, 2 dp outside the focused element, with a 1 dp halo in the background colour inside it.
   - **When:** only out of touch mode, as Android draws focus.
   - **Where:** over the home views and over every dialog.
     - It follows the element through its transforms and scrolls.
     - It stays inside the nearest scrolling column, or card clipped to its outline, that holds the element, such as the menu's. So it never crosses onto what is behind a card.
     - Otherwise it stays inside the window, so a view's element is ringed just inside the screen.
   - **Android's highlight** is turned off on each element that takes the focus.
   - **No wash under focus:** `SettingsRows.pressed`, every ripple's colour, draws nothing for focus alone. A press's wash lasts only while a finger is on it, so ADR 0038 exempts it from contrast, but a keyboard's focus can rest anywhere for as long as it likes. Pressing still washes.
   - **Contrast:** `PAIRINGS` holds the ring to 3:1 against the background and the surface, in both themes. The accent already has 4.5:1 there, as text.
7. **One scroll view for every column** (A11Y-06, A11Y-07).
   - **Which:** `Scroll` is the grid's, Events', Recent's, Settings', the menu card's, the image screens', setup's and the licences'.
   - **Focus:** it keeps a focused element clear of its padding.
   - **With animations removed** (`ValueAnimator.areAnimatorsEnabled()`):
     - a screen reader's page, a keyboard's arrows and pages, and the scroll to what took focus are instant;
     - its scroll bars go at once;
     - below Android 12 it has no over-scroll glow.
   - **Dialogs:** `Dialogs.show` opens them without the window's animation then.
8. **CI checks what it can without a device.**
   - **The edit-mode items** in `meegone-apps-state`: offered where they move something, only in edit mode, and each one moving, grouping or moving out, and saving.
   - **The shell's refusals.**
   - **The acceptance scenarios** (`tests/acceptance.rs`):
     - **AC-19 with no gesture at all,** first at normal speed, then with animations removed and every step over as it happens: the views, a launch, edit mode from an app's menu, a move, a folder made, joined and left from the menu, Recent cleared through Edit Recent and Clear all, and Settings from Events' menu.
     - **AC-18's missing flows:** a folder made by a drop, an app dragged out of a folder, an app removed, notifications dismissed and cleared, coming home to Recent, and search closing.
9. **The device audit checks reach** (`A11yAudit`, ADR 0038).
   - **The rules,** checked on every screen in both themes:
     - `ACTION_REACH`: every action of MeeGone's own is on an element screen readers can focus.
     - Tab's stops are walked in Android's own order without moving the focus:
       - `FOCUS_REACH`: every element on top that takes a tap or a long-press is a stop. A list's rows count as reached when the list is a stop, as it moves among them itself.
       - `FOCUS_TRAP`: every stop is in what is on top, and seen by screen readers.
       - `FOCUS_EMPTY`: every stop does something: a tap, a long-press, typing, scrolling, or an action of its own, such as a card's "Remove from Recent", which Delete does.
       - `FOCUS_ORDER`: stops in one container go in reading order.
     - `FOCUS_RING`: last in each theme, the tour leaves touch mode as a keyboard does and focuses an app, then the home menu. The ring is held to 3:1 against what is around it, at the middle of each side.
   - **The tour** goes through each view's element, as a screen reader does.
   - **Text cut short under an open pane:** it is no longer checked as read whole, since screen readers skip all of it until the pane closes.
   - **`tools/a11y-audit.sh`** adds a third pass with all three animation scales at 0, as "Remove animations" sets them, and puts them back. Every screen must still be reached and pass.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 6 | The container offers only scroll and page actions. Each view's element offers Next view, Previous view and the view's own actions (decision 1). |
| 0008, decision 6 | Screen readers open the home menu with the press of Events' or Applications' element. A keyboard opens it with Menu (decisions 1 and 4). |
| 0013, decision 9 | A keyboard opens an app's menu with Menu or Shift+F10 (decision 4). |
| 0014, decision 7 | In edit mode, an app's or a folder's menu offers the moves; Ctrl with an arrow, Home or End moves the focused cell; Delete is the ✕ (decisions 2 and 4). |
| 0015, decision 10 | The menu makes and leaves folders too; the open folder keeps the keyboard's focus (decisions 2 and 5). |
| 0016, decision 4 and departures | "Search apps" is on Applications' element. Typing a letter on Applications opens search with it, so the A11Y-06 departure is lifted (decision 4). |
| 0024, decision 4 and departures | "Bigger cards" and "More cards" are on Recent's element; a keyboard has Ctrl+= and Ctrl+- (decisions 1 and 4). |
| 0025, decision 6 | "Edit Recent" is the press of Recent's element and each card's long-click; Delete removes the focused card (decisions 1 and 4). |
| 0034, decision 2 | Android's own scrolls, scroll bars, glow and MeeGone's dialogs follow the speed too (decision 7). With animations removed, the folder hover's grow is motion and is left out: the icon goes to 110 % at once when the 400 ms wait arms it. |
| 0038, decisions 3, 4 and 6 | `notAButton` containers aren't a keyboard's stop. The focus goes back to the opener for a keyboard too. The focus indicator is built (decisions 5 and 6). |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| A11Y-05: Recent's density "from a menu" | As in ADR 0024, the menu is the actions menu of screen readers, Switch Access and Voice Access, now on Recent's element, and a keyboard has Ctrl+= and Ctrl+-. Someone who can't pinch and uses none of these has no control on screen. |
| A11Y-07, ANIM-05: with animations removed, transitions are instant | MeeGone follows the animator duration scale. Launches, system screens and toasts follow the system's transition and window scales. Accessibility's "Remove animations" turns all three off; Developer options' "Animator duration scale" alone leaves those running. |
| HOME-08 and ADR 0016: the first Back hides the on-screen keyboard | Esc is Back at once, so with search open it closes search; a hardware keyboard has no on-screen keyboard to hide. |
| AC-19 and A11Y-06: switch access and TalkBack | CI checks the actions and the Rust app's paths. The device audit checks that the actions are on elements screen readers can focus, and Tab's reach. TalkBack's speech and Switch Access's scanning are checked by hand (`RELEASING.md`). |

## Consequences

- **Benefits:**
  - **AC-19 is tested end to end with no gesture,** at normal speed and with animations removed.
  - **One path for every input.** A screen reader's action, a switch, a key and the edit-mode menu all send the intents a gesture sends, and the Rust app decides each.
  - **Moving without a drag works for everyone,** not only screen readers.
  - **What is on top owns the focus,** for a finger, a screen reader and a keyboard alike.
  - **The audit now checks reach,** which no earlier check did. Its first run on the phone found what reading the code hadn't:
    - search's empty results grid and Recent's cards in close mode as stops (the second wrongly: Delete removes them);
    - the ripple's focus wash under the details' and the past week's accent buttons, at 4.46:1;
    - the ring crossing the home menu's edge onto the icons behind it, at down to 1.02:1.
- **Checked on a device:** a Galaxy S22 on Android 16, with MeeGone as its home app, and key events sent from adb.
  - **The keys:**
    - Tab, then the arrows across views.
    - Menu on a notification, and Esc closing it.
    - Menu on an app, then Edit applications, then Ctrl+→.
    - Make folder with the next app from the menu; the folder opened, then Move out of folder; Ctrl+← back.
    - Typing to search, and Esc closing it.
    - Menu on Recent's element, then Tab to Clear all.
    - The home menu, then Settings.
  - **The ring** shows on each element.
  - **The audit:** 198 screens and 19,794 checks, at font scale 1.0, at 2.0 and with animations removed, both themes each time. Every check passed.
- **Costs:**
  - **Each view has one more stop,** for screen readers and for Tab: its element, at the start of the view.
  - **Kept in step by hand:** the menu's kinds 6 to 12 between `menu_item_fields` in `src/android.rs` and `HomeViews.APP_MENU_ITEMS`.
  - **The keys are host-side,** like the screen-reader actions. CI can't press them; the device audit checks their reach, not each key.
  - **The tour ends out of touch mode,** with the ring showing, until the next touch.
- **Not checked yet:**
  - TalkBack's speech for the view's elements, and touching empty space;
  - Switch Access's scanning;
  - a physical keyboard's Esc fallback, and its Meta+/ list;
  - right to left;
  - a tablet;
  - Android 10's glow.

  The device matrix (#43) covers them.
