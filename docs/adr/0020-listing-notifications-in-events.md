# ADR 0020: Listing notifications in Events

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #19

## Context

Issue #19 fills Events below its header with what is new, like the N9's events view:
- Without notification access, a card says in one plain sentence what Events reads and why, with a button to the system's notification-access screen. The header and the image card still work (EVT-03, ONB-02).
- With access, active notifications are listed under a "NOTIFICATIONS" section header, grouped by app, newest group first. Each group shows the app's icon, accent marker (EVT-09, Should), name and count, and its newest notification's title and text in at most two lines (EVT-04).
- Ongoing notifications and MeeGone's own are left out (EVT-05).
- The list updates live: new groups slide in at the top, and dismissed ones collapse, over 200 ms (EVT-10, ANIM-20). With nothing to show, it reads "No new events" (EVT-11).
- What notifications say stays on the device and in memory only, never stored, logged or transmitted (EVT-13, PRIV-03).
- Revoking access while Events shows brings the card back without a crash (REL-01, AC-21).

What was there already:
- `NotificationsAdapter`, its fake, the `Notification` model (whose `Debug` leaves out the title and text), `EventsState { access, groups, problem }` with a first grouping, `EventsIntent::OpenAccessSettings` and `ShellIntent::NotificationsChanged`. On Android the adapter was a stub that always answered `Access::Missing`.
- `motion::NOTIFICATIONS_CHANGE` (200 ms) and `Tokens::app_accent`, which keeps an accent at 3:1 against the background (THEME-07).
- `FakeStorage::stored_text()`, which `tests/storage.rs` already uses to check that nothing private is stored.

The constraints:
- **Adapters never call back** (ADR 0003). A notification posted, or access granted, reaches MeeGone as a `ShellIntent`.
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, and text travels through a native that returns `String[]`, read by revision (ADR 0017, ADR 0019).
- **Features meet only in the shell** (ADR 0002). Events can't use Applications' state, and reads each app's label and accent through `AppsAdapter`, as Recent does.
- **Android 11's package visibility.** MeeGone sees the apps it declares in `<queries>`: those with a launcher entry. Some apps that post notifications have none, such as System UI.

## Decision

1. **The model says whose a notification is.**
   - `Notification` gains `profile`, so a personal and a work copy of an app are two groups (APPS-20); `app_label`, the system's name for the app that posted it; and `summary`, for the summary an app posts over a bundle.
   - `NotificationsAdapter::active` returns other apps' notifications, never MeeGone's own (EVT-05).
2. **Events groups, names and orders.**
   - `EventsHolder` takes `AppsAdapter` and keeps the catalogue it last read. A group takes its label, its launcher activity (for its icon) and its accent from the catalogue's app with the same profile and package. An app outside the catalogue keeps the system's name, or its package if the system has none, and gets a neutral marker.
   - Groups are by profile and package. Ongoing notifications are left out. A summary counts only while nothing else of its app shows, so a bundle of four messages reads 4, not 5.
   - Groups are ordered by their newest notification, then by package and profile, so the order never flickers.
   - `Refresh` reads the header and the notifications, at start and on a swipe to Events. New `RefreshNotifications` reads only the notifications. New `AppsChanged` forgets the catalogue, and reads it again at once if any group shows, so a new app or a late accent shows. The catalogue is read only when a notification needs it.
   - A catalogue that can't be read leaves the system's names, with no problem: Events never depends on Applications (REL-01). Missing access empties the groups, with no problem.
   - `groups` is an `Arc<[NotificationGroup]>`, replaced only when the groups change, so the shell and the view tell a change by pointer.
3. **A row keeps its place while its newest notification does.**
   - `GroupKey` is a group's profile, package, and newest notification's key and post time. `rows_kept(before, now)` says where each row was, by key, or `None` for a row that comes in.
   - A newer notification, or an app posting again with the same key, changes the key: the group's old row goes, and a new one comes in at the top (ANIM-20). An older notification only changes the count. Kept rows keep their order, as their newest notification, which orders them, is the same.
4. **The shell runs the motion.**
   - `NotificationsChanged` sends `RefreshNotifications`. `Resumed` sends it too, as access may have changed in system settings (AC-11). `AppsChanged` also sends Events `AppsChanged`.
   - `keep_events_in_step` starts `EventsChange` when `rows_came_or_went`. It runs `NOTIFICATIONS_CHANGE`, 200 ms, decelerating, on `Timed`, the clock factored out of the grid's change motion (ANIM-21): it starts at the first frame drawn, takes a late frame in steps of at most 34 ms, and is instant with animations removed (ANIM-05). `ShellState.events_change` is its progress.
5. **On Android, a listener service.**
   - `NotificationListener`, a `NotificationListenerService`, is exported, as only the system can bind it (`BIND_NOTIFICATION_LISTENER_SERVICE`, SEC-04). It keeps itself in a static field while connected, and tells `Bridge` when a notification is posted or removed, and when it is connected or disconnected, as when access is granted or revoked.
   - `Bridge.notificationsChanged()` reports a burst once, through the main thread, as `watchApps` does, but only after `HomeActivity.onCreate` calls `watchNotifications()`. The system may start MeeGone's process just to bind the listener, before any home exists. The home then reads the notifications as it starts.
   - `Bridge.notificationAccess()` asks `NotificationManager.isNotificationListenerAccessGranted`. If access is granted but the listener isn't connected, it calls `requestRebind`.
   - `Bridge.activeNotifications()` returns one record per notification, parsed in `notification_records.rs` like the catalogue. While the listener isn't connected yet it returns none, and its connection reports itself. A record has: key, package, profile serial number, the app's name, title, text, post time, and flags.
     - Title and text come from `EXTRA_TITLE` or `EXTRA_TITLE_BIG`, and `EXTRA_TEXT` or `EXTRA_BIG_TEXT`.
     - Ongoing means `FLAG_ONGOING_EVENT`, `FLAG_FOREGROUND_SERVICE`, `CATEGORY_TRANSPORT`, `CATEGORY_NAVIGATION`, or a media session (`EXTRA_MEDIA_SESSION`), so a paused player is still left out.
     - The app's name comes from `LauncherApps.getApplicationInfo`. For an app MeeGone can't see, it comes from the app info every notification carries (`android.appInfo`, which `Notification.Builder.recoverBuilder` reads), and otherwise it is empty.
   - `Bridge.openNotificationAccess()` opens MeeGone's own page of the access screen (`ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS`, Android 11 and later), then the screen itself. If neither opens, it returns "Notification access can't open right now".
   - `AppIcons.get` with an empty activity loads an app's own icon, or the system's default icon for an app MeeGone can't see, in MeeGone's shape.
6. **The host draws the list below the header.**
   - The Events page is a `ScrollView` holding `EventsHeader` and then `EventsList`, which shows one of three things.
   - **Without access:** a flat card, 12 dp corners, in the surface colour. It has one 15 sp sentence, "MeeGone reads your notifications only to list them here, and they never leave this phone.", and an **Allow** text button in the accent colour, at least 48 dp tall.
   - **With access:** a "Notifications" section header, drawn in capitals at 13 sp, letter-spaced, in the secondary colour, a heading for screen readers, with a hairline in the divider colour to the right edge. Below it, either "No new events" at 15 sp in the secondary colour, or the groups.
   - **A group's row:** a 4 dp marker in the accent, the 40 dp icon, the name in medium weight with the count, and "title: text" in at most two lines at 15 sp. Screen readers read a row as one item, "Messages, 3 notifications: Anna: See you at eight", with ", work" after a work app's name.
   - Rows aren't clickable until #20. A long-press on one therefore still opens the home menu (ADR 0008).
   - **Motion:** each row says where it was in the rows last read, so a staying row keeps its view, and a row that went stays where it was. As the frame's progress goes from 0 to 10 000, a new row grows to its height and fades in, showing its bottom first, so it slides down. A row that went collapses from below and fades, and is dropped at the end. No `ValueAnimator` runs.
7. **The host contract.**
   - `EventsView::show_list(state, tokens)` keeps the mode and rows it last told the host, made again only when the groups, the access or the theme change. It returns a revision that changes only when they differ. `rows()` and `mode()` read them. `EventsRow` holds the place it came from, the app, the marker's colour (`app_accent`, or the secondary text colour), the count, the title and the text. Its `Debug` leaves out the title and text.
   - `HomeFrame` gains `events_list: Option<EventsListFrame>`, which is `None` while Events isn't `Ready` (ARCH-09), `events_change` and `divider`.
   - The frame grows to 86 slots:
     - 82: what shows: 0 nothing, 1 the card, 2 "No new events", 3 the groups;
     - 83: the list's revision;
     - 84: the change's progress, 0 to 10 000;
     - 85: the divider colour.
   - New natives:
     - `HomeViews.nativeEventsList()`: 10 fields per row;
     - `HomeViews.nativeAllowNotifications()`: returns Events' problem, or `null`;
     - `HomeActivity.nativeNotificationsChanged()`.
8. **Nothing keeps or prints what notifications say** (EVT-13).
   - No `StoredFile` holds them, and `SavedHome` holds only the view and a scroll position.
   - Errors carry plain words, never content. `Notification`, `NotificationGroup` and `EventsRow` print `<redacted>` for the title and text.
   - Java logs only the kind of an exception, and every `Bridge` method catches what Android throws, so `ExceptionDescribe` never prints a notification.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 1 | `Notification` gains `profile`, `app_label` and `summary`. `NotificationsAdapter` is built on Android. Events takes `AppsAdapter`, as Recent does (ADR 0002). |
| 0006, decision 2 | The frame has 86 slots. Events scrolls, with its list below its header. |
| 0010 | `AppIcons` also loads an app's own icon, for an app with no launcher entry. |
| 0019, decision 3 | `Resumed` also reads the notifications, as access may have changed in system settings. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| EVT-04: each group shows the app icon and accent colour (APPS-17) | An app with no icon in the grid, such as Android System, shows the system's icon for it and a neutral marker in the secondary text colour: no accent is derived for it. |
| EVT-04: "count" | A summary over a bundle counts only while none of the bundle shows, so the count is what the user would read. |
| ANIM-20: dismissed groups collapse | A group whose newest notification changes collapses where it was and slides in again at the top, so a row never jumps. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - Grouping by app and profile, labels, accents and the system's names, summaries, ongoing notifications, and a stable order.
    - Reading the catalogue only when needed, and again after apps change. A failed catalogue read, a failed notification read, and revoked access.
    - Which rows stay, come and go, and the 200 ms motion at normal, double and no animation speed, with a late frame.
    - The view's revision, the accent per theme, and a failing Events leaving Applications working.
    - The issue's acceptance criteria, in `tests/acceptance.rs`: the card and the list after access is granted (AC-11), revoking access (AC-21), media playback left out, and no title or text on disk or in debug output.
  - Checked on a Galaxy S22 (One UI 8, Android 16), with MeeGone as the default home app:
    - Without access, Events shows the card below the header. Allow opens One UI's notification-access screen at MeeGone.
    - Access granted there, Back shows the groups within a second, each with its icon, accent marker, name, count and two lines.
    - A notification the shell posts slides in at the top as "Shell", with the system's icon and a neutral marker. A second one makes its count 2, and removing both takes the group away.
    - Revoking access while Events shows brings the card back, in the same process, and Applications works.
    - Neither MeeGone's log nor its files hold any of the test notifications' text.
- **Costs:**
  - From Android 13, an APK installed from outside a store needs "Allow restricted settings" in MeeGone's App info before the system lets the user grant notification access. The card doesn't say so.
  - An app MeeGone can't see shows the system's default icon. Its name relies on the app info the notification carries, which the platform puts there but doesn't document as an extra.
  - The frame slots and the row's fields must agree between Java and Rust. Nothing checks that at build time.
  - Rows still going when another change comes are dropped at once, and rows still coming show in full, as the motion starts again.
  - Not checked on a device yet: the motion frame by frame, media playback, a work profile, granting access with the switch itself (the check used `cmd notification allow_listener`), 200 % font size, TalkBack, right to left, a tablet and dark mode (#32). The device matrix (#43) covers them.
