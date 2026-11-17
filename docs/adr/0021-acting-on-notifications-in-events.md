# ADR 0021: Acting on notifications in Events

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #20

## Context

Issue #20 lets the user act on the notifications Events lists (ADR 0020), as on the N9, where notifications stay until opened or deleted:
- Tapping a notification, or a group holding one, opens its content. MeeGone removes one meant to disappear when opened. Tapping a group of several expands it (EVT-06).
- The "Notifications" section header carries **Clear**, which removes every clearable notification. Ongoing ones stay (EVT-07, AC-12).
- A long-press on a notification or a group offers **Dismiss** and the app's **Notification settings** (EVT-08).
- Screen readers reach the long-press menu through an action (A11Y-05).
- Expanded groups show up to two action buttons per notification (EVT-14, Could).

There is no swipe-to-dismiss: horizontal drags belong to the carousel (SRS 4.3).

What was there already:
- `NotificationsAdapter::open`, `dismiss` and `open_app_settings`, each a stub answering "Not available yet" on Android.
- Rows that weren't clickable, so a long-press on one opened the home menu (ADR 0020, decision 6).
- The app menu's split (ADR 0013): the feature owns the menu, and the shell owns the gesture, decides whether it counts and treats the menu as covering the views. The host has one menu card.
- `Bridge.startFromHome()`, which lets an intent another app made start its screen while MeeGone shows (ADR 0019).

The constraints:
- **Adapters never call back** (ADR 0003). A dismissed notification goes when the listener reports it, as `NotificationsChanged`.
- **Rust decides, Java draws** (ADR 0006). Taps and long-presses name a row by the list's revision and the row's place, as cells do (ADR 0010).
- **The system's shade removes an auto-cancel notification itself** when the user taps it. A listener only sends the content intent. Nothing removes the notification unless MeeGone does.
- **Apps bundle notifications under a summary**, which Events leaves out while the bundle shows (ADR 0020). Removing the bundle can leave the summary behind.

## Decision

1. **Events owns what a tap and the menu do; the shell owns the gesture.**
   - `EventsState` gains `expanded: Option<GroupId>` and `menu: Option<NotificationMenu>`. A `GroupId` is a group's profile and package, which stay the same while its row comes and goes.
   - A `RowTarget` is what a row stands for: a group, or one notification of the expanded group.
   - New intents: `Tap { row, from }`, `Collapse`, `OpenMenu(target)`, `CloseMenu`, `ChooseMenuItem(item)` and `Clear`. Each user action sets `problem` from its own outcome, so the host never shows an old message again.
   - `on_back` closes the menu, then collapses the group. Home and Back therefore need no new code.
   - A refresh collapses a group left with fewer than two notifications. It closes the menu once its group or notification is gone, and offers Dismiss only while something can still be cleared.
2. **A tap (EVT-06).**
   - A group of one opens its notification. A group of several expands, or collapses if it is expanded. A notification of the expanded group opens.
   - Only one group is expanded at a time. Leaving Events collapses it, as leaving Applications closes a folder.
   - Opening sends the content intent through `NotificationsAdapter::open(key, from)`, from the tapped row (ANIM-08). The adapter never removes the notification.
   - Events then dismisses it if it is `auto_cancel` and `clearable`. "MeeGone removes it" is state logic, tested with fakes.
   - A row that no longer shows does nothing.
3. **Dismissing (EVT-07, EVT-08).**
   - `NotificationsAdapter::dismiss(keys)` takes every key at once, so Clear is one request: the listener's `cancelNotifications`.
   - Only clearable notifications that Events shows are sent. Ongoing ones were never shown, so Clear leaves them, even a paused player the system's shade could clear (AC-12).
   - `NotificationGroup` keeps the keys of the clearable summaries it leaves out (`hidden_summaries`). When MeeGone removes everything a group shows, by Dismiss, Clear or opening an auto-cancel notification, those summaries go too. A summary is never left showing alone.
   - Nothing is removed from the list in advance. Rows go when the system reports them gone, so a refused dismissal never flickers.
4. **The long-press menu (EVT-08).**
   - Its items are Dismiss, offered only if something the menu is for can be cleared, then Notification settings.
   - Dismiss on a group takes all of the group's clearable notifications. On a notification of the expanded group, it takes just that one.
   - Notification settings opens the app's own page, in its profile: `open_app_settings(profile, package)` (APPS-20).
5. **Expanded notifications are rows.**
   - `EventsState::rows()` lists each group's row and, below the expanded group's, one row per notification. `EventsView` and the shell both use it.
   - `RowKey` is `Group(GroupKey)` or `Notification { group: GroupKey, key }`. A notification's row includes its group's key, so it comes and goes with its group's row, and the rows that stay keep their order. `rows_kept` therefore still holds.
   - Expanding and collapsing run the 200 ms list motion that ANIM-20 already uses (`keep_events_in_step` compares row keys). Rows slide in or collapse, within ANIM-06's 150–350 ms.
   - An expanded group's row shows only the app's name and count. Each notification's row shows its own title and text.
6. **The shell.**
   - `ShellIntent::LongPressNotification(target)` counts only on Events, while the gesture is still a press and nothing covers the views. The menu opens, then the carousel becomes `Held`, the press finishes and the phone ticks (ANIM-24). Lifting the finger therefore opens nothing.
   - `ShellIntent::OpenNotificationMenu(target)` is the screen-reader path, with no tick (A11Y-05).
   - Events' menu covers the views like the other menus. It counts only while Events is `Ready`, so a failed Events never leaves the home covered (ARCH-09).
   - `CloseMenu`, Settings and a change of view close it. `ShellState.events_menu_open` says whether it is open, and `menu_offered` is false while it is.
7. **On Android.**
   - `Bridge.openNotification(key, rect)` looks the notification up with `getActiveNotifications(keys)`.
     - It sends the `contentIntent` with the row's scale-up (ANIM-08), through `startFromHome`, which now takes options.
     - With no content intent, or one its app cancelled, it starts the app's first launcher activity.
     - Otherwise it says "This notification can't open right now".
     - A notification already gone opens nothing.
   - `Bridge.dismissNotifications(keys)` calls the listener's `cancelNotifications`.
   - `Bridge.openNotificationSettings(package, profile)` sends `ACTION_APP_NOTIFICATION_SETTINGS` with `EXTRA_APP_PACKAGE`.
     - For an app in another profile, it adds the `app_uid` extra, which the Settings app reads to choose that profile's copy.
     - If that screen doesn't open, it opens the app's App info through `LauncherApps`.
   - Records gain flag 8, `FLAG_AUTO_CANCEL`, as `Notification::auto_cancel`.
   - Every method catches what Android throws, and logs only its kind (EVT-13).
8. **The host draws it.**
   - Rows are clickable, with a ripple, and at least 48 dp tall (A11Y-02).
     - They aren't long-clickable. `HomeViews` times the long-press and sends `nativeEventsRowLongPressed`, as for cells, so Android starts no second timer.
     - Rows that are going take no touches.
   - A notification's row keeps the icon's room, so its text lines up with the app's name, and keeps the accent marker. Its title and text are at most two lines.
   - **Clear** is a text button in the accent colour at the end of the section header, at least 48 dp. Screen readers read it as "Clear all notifications". It shows only while something shown can be cleared.
   - Accessibility:
     - Each row's click is labelled "Open", or "Expand" or "Collapse" for a group of several, which also offers `ACTION_EXPAND`/`ACTION_COLLAPSE` and an expanded or collapsed state (Android 11 and later).
     - Its long-click is labelled "Open menu" (A11Y-05).
     - "No new events" is a polite live region, so a Clear is announced (A11Y-08).
   - The menu card shows the notification menu at its row, below it, or above it when there's no room. Its pane title names the app ("Shell menu").
9. **The host contract.**
   - The frame grows to **89 slots**:
     - 86: 1 while Clear is offered.
     - 87: the notification menu's items as bits, 1 Dismiss and 2 Notification settings; 0 while it is closed.
     - 88: the row it opens at, or -1 to centre it.
   - Row fields keep their count of 10. Their flags gain 2 (a notification's row) and 4 (an expanded group's row).
   - New natives on `HomeViews`: `nativeEventsRowTapped(revision, index, rect)`, `nativeEventsRowLongPressed(revision, index)`, `nativeEventsMenuRequested(revision, index)`, `nativeEventsMenuChosen(item)` and `nativeClearNotifications()`.
   - Each one that acts returns Events' `problem` as a message to show briefly.
   - Rows that changed since the host drew them do nothing.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 1 | `NotificationsAdapter::open` takes the row it grows from and never removes the notification. `dismiss` takes several keys. `open_app_settings` takes the app's profile. `Notification` gains `auto_cancel`. Opening, dismissing and an app's notification settings are built on Android. |
| 0006, decision 2 | The frame has 89 slots. |
| 0008, decision 1 | A long-press on an Events row reaches Rust too, as `LongPressNotification`. |
| 0013, decision 5 | The one menu card also shows a notification's menu, at its row. |
| 0020, decisions 3 and 6 | Rows are tapped and long-pressed. Rows are keyed by `RowKey`, which includes the expanded group's notifications. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| EVT-14 (Could): expanded groups show up to two action buttons | Not built. A later issue adds them, leaving out actions that need typed input, such as inline reply. |
| EVT-06: "opens the notification's content" | A notification without content, or whose app cancelled its intent, opens its app. With neither, a short message says it can't open. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - Taps, expanding one group at a time, and collapsing when a group shrinks, goes, or the user leaves Events.
    - Auto-cancel, Dismiss of a group or of one notification, hidden summaries, Clear in one request, and menu items that follow what is clearable. Failures become the problem.
    - The shell's long-press, the screen-reader path and the tick. The menu covering the views, Back and Home. A failing Events never leaving the home covered.
    - The row keys and their motion, stale taps, the menu's row, and redaction.
    - The issue's acceptance criteria, in `tests/acceptance.rs`: AC-12, a group of three expanding with one item opening, EVT-08's menu, and A11Y-05.
  - Checked on a Galaxy S22 (One UI, Android 16), with MeeGone as the default home app and notification access granted:
    - Clear shows in the section header.
    - Six notifications posted from the shell show as "Shell, 6". A tap expands the group, and its notifications slide in below it, lined up with the name. A second tap collapses it.
    - A notification with no content intent, from an app with no launcher entry, says "This notification can't open right now".
    - A long-press on a notification opens Dismiss and Notification settings below its row. Dismiss removes it from the system, and the group shows 5, still expanded. On the group's row, Notification settings opens Shell's page in Settings.
    - Back closes the menu, then collapses the group. Home collapses the group and stays on Events.
    - Dismiss on the group removes every shell notification. The system's auto-group summary, which Events left out, is gone too, and the row leaves the list.
    - Neither MeeGone's log nor its files hold any test notification's text, and nothing crashed.
- **Costs:**
  - A listener's cancel sends the app's delete intent, as a swipe in the shade does, even for a notification removed because it was opened. A tap in the system's shade sends none.
  - Opening a work app's notification settings relies on the Settings app reading `app_uid`, which isn't public API. App info is the fallback.
  - The frame slots, the row flags and the menu's item codes must agree between Java and Rust. Nothing checks that at build time.
  - Not checked on a device yet:
    - Clear, because it would also have removed the device owner's own notifications.
    - A real content intent with auto-cancel, and its launch motion.
    - A work profile, TalkBack's actions, 200 % font size, right to left, a tablet and dark mode.
    - The device matrix (#43) covers them.
