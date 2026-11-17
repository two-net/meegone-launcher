# ADR 0023: Recent's cards

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #22

## Context

Issue #22 fills Recent, the N9's "Open applications" view adapted to Android (C3):
- The view is titled "Recent" and never claims that apps are running or were closed (REC-01).
- Without usage access, a card says in one plain sentence what Recent reads and why, with a button to the system's usage-access screen. No placeholder cards show (REC-02, ONB-02).
- With access, up to 18 launchable apps used in the last 24 hours show, newest first, filling rows from the top left. MeeGone and hidden apps are left out (REC-03).
- Each card is a 12 dp rounded rectangle with roughly the screen's proportions, filled with the app's accent, with the icon large and centred, the label below and, *(Should)*, the time since last use (REC-04).
- A tap brings the app to the front, resuming it if the system kept it, growing from the card (REC-06, ANIM-08).
- The list refreshes when Recent becomes visible and when MeeGone returns, never by polling (REC-09, PWR-02). With nothing used, it reads "No recent apps" (REC-10).
- Usage history stays on the device, in memory (PRIV-03). The filtering is pure and tested without a device (MAINT-01).
- Revoking access while Recent shows brings the explainer back without a crash.

What was there already:
- `UsageAdapter` (`access`, `used_since`, `open_access_settings`), its fake, and `RecentUse { app: AppId, last_used_ms }`. On Android the adapter was a stub that always answered `Access::Missing`.
- `RecentHolder`, which kept hidden apps, apps removed from Recent and apps outside the catalogue out, newest first, at most 18. It refreshed at start, on a swipe to Recent, on `Returned`, on `AppsChanged` and when the hidden apps changed (ADR 0007, ADR 0017).
- `Tokens::app_accent`, documented as the Recent card's fill. `RecentView` drew nothing; the host showed the view's name.

The constraints:
- **Adapters never call back** (ADR 0003). Access granted or revoked reaches MeeGone as a `ShellIntent`.
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, and text travels through natives read by revision, as Events' rows do (ADR 0020).
- **Usage events name the activity resumed**, which is rarely an app's launcher activity, and a third-party app sees only its own profile's usage.

## Decision

1. **The model says what the system reports.**
   - `RecentUse` becomes `{ profile, package, activity, last_used_ms }`: the activity last resumed isn't an `AppId`, which names a launcher activity.
   - `fakes::used(package, at_ms)` builds one in tests.
2. **Recent finds the launchable app, in pure code.**
   - `cards(uses, apps, settings, now_ms)` holds the filtering, and `refresh` calls it. A use shows as the catalogue's app with its profile and package: the one whose activity was last resumed, otherwise the package's first. A use with no launchable app, such as a service, shows nothing.
   - Hidden apps and apps removed from Recent are left out by that resolved app, so they stay out whichever activity was used. An app shows once, as of its newest use.
   - The order is newest first, then the label's collation key, so equal times never flicker. At most 18 show.
   - `RecentCard.age` is `Age::between(last_used, now)`: under a minute is `JustNow`, then whole minutes, hours and days. A time the clock hasn't reached is `JustNow`. It is computed at refresh, and Recent refreshes whenever it shows.
   - `used_since` failing with `AccessDenied`, as when access goes between the two calls, shows the explainer with no problem.
3. **A tap launches from the card.** `RecentIntent::Launch { app, from }` asks `AppsAdapter::launch`, as Applications does. `LauncherApps.startMainActivity` brings an existing task to the front, and its window grows from the card's rect. A failure becomes Recent's `problem`.
4. **The shell follows access as it changes.**
   - `ShellIntent::UsageAccessChanged` refreshes Recent. The refreshes ADR 0007 and ADR 0017 already send stay: at start, a swipe to Recent, `Returned`, `AppsChanged` and new hidden apps.
   - Turning the screen off and on goes through `onStop` and `onResume`, so it arrives as `Returned`. Nothing polls.
   - `HomeShell::recent_ready()` says whether Recent works, as for the other features.
5. **On Android, usage events and the app op.**
   - The manifest declares `PACKAGE_USAGE_STATS`, which only lists MeeGone on the usage-access screen; the user grants it there.
   - `Bridge.usageAccess()` reads MeeGone's `GET_USAGE_STATS` app op, and, while it is `MODE_DEFAULT`, the permission.
   - `Bridge.recentUses(since)` reads `UsageStatsManager.queryEvents` once. For each package it keeps the class last resumed, and the latest `ACTIVITY_RESUMED` or `ACTIVITY_PAUSED` time, so an app used for an hour reads as used when the user left it. MeeGone itself is left out. Each record has the package, the activity, this profile's serial number and the time. `usage_records.rs` parses them like the notifications, and `AndroidUsage` answers `AccessDenied` without access.
   - `Bridge.openUsageAccess()` opens `ACTION_USAGE_ACCESS_SETTINGS` at MeeGone's own page, then the screen itself, and otherwise returns "Usage access can't open right now".
   - `Bridge.watchUsageAccess()`, called as the home starts, watches MeeGone's own app op with `AppOpsManager.startWatchingMode`. A change reaches the Rust app once per burst, through the main thread, as `nativeUsageAccessChanged`. The system reports it; nothing polls (PWR-02).
6. **The host draws Recent.**
   - The Recent page is a `ScrollView` holding `RecentList`. A "Recent" section header, drawn like Events' "Notifications", titles it in every mode (REC-01).
   - **Without access:** `AccessCard`, Events' notification-access card made shared, with "MeeGone reads which apps you used recently only to show them here, and that never leaves this phone." and Allow.
   - **With nothing used:** "No recent apps", as "No new events" is drawn.
   - **The cards:**
     - Each row holds the frame's columns, 3 by default, with 12 dp gaps between cards and 16 dp margins, filled from the start edge. A right-to-left layout mirrors them (L10N-02).
     - The plate is a 12 dp rounded rectangle in the card's fill. Its height over its width is the window's, kept within 0.6 to 2.2.
     - The icon sits in the middle, at 56 dp or half the card's width, whichever is smaller.
     - Below the plate, 4 dp down, the label is at 13 sp in the primary colour, on one line. Under it, the age is at 12 sp in the secondary colour: "Just now", "5 min ago", "3 h ago" or "2 d ago".
     - An unavailable app is greyed and at half opacity, as in the grid (APPS-09).
   - **A card is one clickable item.** Screen readers hear "Mail, work, 5 minutes ago", with "Open" as its action. A tap passes the plate's rect on the screen. Cards aren't long-clickable, so a long-press on one does nothing until close mode is drawn (#24); one between cards enters close mode (HOME-10).
   - **Icons are drawn as the grid draws them.** `AppIcons` gives every view the same `Drawable`, and `AppCell` sets its bounds and filter as it draws. The card's icon view therefore sets its own bounds and filter at each draw; an `ImageView` drew the icon where a grid cell had last put it.
7. **The host contract.**
   - `RecentView::show_cards(state, tokens)` returns a revision that changes only when the mode or the cards do. `mode()`, `cards()` and `columns()` read what it made. `card_tapped(revision, index, from)` ignores a stale revision, as Events' rows do.
   - `ShownCard` holds the app, label, work and available flags, the fill (`app_accent`) and the age.
   - `HomeFrame.recent` is `None` while Recent isn't `Ready` (ARCH-09).
   - The frame grows to 97 slots:
     - 94: what Recent shows: 0 nothing, 1 the card, 2 "No recent apps", 3 the cards;
     - 95: its revision;
     - 96: its columns.
   - New natives:
     - `HomeViews.nativeRecentCards()`: 8 fields per card;
     - `nativeRecentCardTapped(int, int, int, int, int, int)`;
     - `nativeAllowUsage()`;
     - `HomeActivity.nativeUsageAccessChanged()`.
8. **Nothing keeps usage history** (PRIV-03). No `StoredFile` holds it, and Java logs only the kind of an exception.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 1 | `RecentUse` is `{ profile, package, activity, last_used_ms }`. `UsageAdapter` is built on Android. |
| 0006, decision 2 | The frame has 97 slots. Recent scrolls, below its title. |
| 0007, decision 3 | `UsageAccessChanged` also refreshes Recent. |
| 0020, decision 6 | Events' notification-access card is `AccessCard`, which Recent shares. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| REC-03: launchable apps used in the last 24 hours | A third-party app reads only its own profile's usage, so work-profile apps never show. One card shows per package, even for a package with two launcher entries. |
| REC-04: the time since last use | It is as of the last refresh, which happens whenever Recent shows. It doesn't tick while Recent stays on screen. |
| REC-05: 3×3 per screen on a typical phone (#23) | With the window's proportions, a tall phone shows about two and a half rows of three. #23 decides how cards keep three rows on screen as it adds pinch and large screens. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - Resolving a use to its launchable app, by package and activity, and only in its own profile.
    - One card per app, hidden and removed apps by the resolved app, 18 at most, a stable order, and the age at every boundary.
    - Access revoked between the two calls, failed reads, and a launch and its failure.
    - The view's modes, revision, fill in each theme, stale taps and columns.
    - `UsageAccessChanged`, and a failing Recent leaving Applications working.
    - The issue's acceptance criteria, in `tests/acceptance.rs`:
      - AC-03 through a card's tap;
      - revoking access while Recent shows;
      - Allow, "No recent apps", the 18 newest, the fill and the refresh when Recent shows.
  - Checked on a Galaxy S22 (One UI 8, Android 16), with MeeGone as the default home app:
    - Without access, Recent shows its title and the card. Allow opens One UI's usage-access page for MeeGone.
    - With access, Back shows the cards: accent fills, centred icons, labels and ages, newest top-left.
    - A card's app opens and the system's launch-from-bounds options are used. Home brings Recent back. An app opened elsewhere is top-left after Home (AC-03).
    - `appops set … GET_USAGE_STATS ignore` while Recent shows brings the card back in the same process, with no crash, and `allow` brings the cards back.
    - Icons stay centred after Applications has drawn the same apps.
- **Costs:**
  - Usage events are read on the main thread, at the start of a swipe to Recent and on return. Their cost isn't measured yet; the performance budgets come with #40.
  - Opening an app from Recent doesn't clear its new-app dot in Applications (APPS-07). Features don't share state, so that needs a rule in the shell.
  - The frame slots and the card's fields must agree between Java and Rust. Nothing checks that at build time.
  - From Android 13, an APK installed from outside a store may need "Allow restricted settings" before usage access can be granted. The card doesn't say so.
  - Not checked on a device yet: TalkBack, 200 % font size, right to left, dark mode (#32), a tablet and a foldable. The device matrix (#43) covers them.
