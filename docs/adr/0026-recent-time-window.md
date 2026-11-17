# ADR 0026: Recent's time window

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #25

## Context

Issue #25 lets the user choose how far back Recent looks:
- Settings → Recent → **Time window**: 12 hours, 24 hours (the default) or 3 days (REC-11, Could).
- The change takes effect at once, with no Save button (SET-03).
- It counts toward the limit of 12 user-adjustable options, and appears only once this feature ships (SET-02, SET-04).
- Acceptance: an app last used 30 hours ago shows in Recent with "3 days" and not with "24 hours".

What was there already:
- `Settings.recent_window_hours: u32`, 24 by default, saved in the settings file since version 1 and counted among SET-04's 10 options (ADR 0017). The fixtures saved 48, which isn't one of REC-11's choices.
- `RecentHolder` read it at every refresh and asked `UsageAdapter::used_since` for the uses since then (ADR 0023).
- Removals are kept for 72 hours, the longest window REC-11 allows (ADR 0025).
- Nothing wrote the field, and nothing offered it.

The constraints:
- **Settings isn't drawn yet** (#34). It shows its name and the sections features add: Applications' Hidden apps (ADR 0017) and the lock-screen image's "Show card in Events" (ADR 0022).
- **Each field has one writer** (ARCH-06), and a feature's options live in the shared settings store while the feature owns them (ADR 0002).
- **Nothing told Recent about a settings change.** `ShellIntent::Settings` refreshes only the theme and the carousel.

## Decision

1. **The window is one of three choices.**
   - `RecentWindow` in `meegone-core-data`: `TwelveHours`, `TwentyFourHours` (the default) or `ThreeDays`, with `hours()`. It is re-exported by `meegone-core-state`, as `RecentDensity` is.
   - `Settings.recent_window` replaces `recent_window_hours`, so no other window can be saved or shown.
2. **The settings file goes to version 3.**
   - The window is saved by name, as every other choice is: `recent_window twelve_hours`, `twenty_four_hours` or `three_days`.
   - The migration from version 2 rewrites `recent_window_hours N` as the shortest choice at least `N` hours long, or three days if none is. Every window saved before still shows at least what it did; 48 hours becomes three days. A line that isn't a number makes the file unreadable, so it loads as the defaults (MAINT-04).
   - `fixtures/settings-v3.txt` is the sample written by the new code. Versions 1 and 2 load as it, with three days.
3. **Recent owns the window.**
   - `RecentIntent::SetWindow` saves `recent_window` through the settings store. Recent becomes that field's single writer (ARCH-06), as it is of the density and the removals.
   - Once saved, Recent reads its cards again at once (SET-03). That differs from the density, which changes only the columns. A swipe to Recent after Settings closes therefore already shows the new cards. Choosing the window already chosen saves nothing.
   - A failed save keeps the window and the cards, and becomes Recent's `problem`, which the host shows (ARCH-09).
   - `RecentState.window` is read at every refresh.
4. **Recent provides its Settings section.**
   - `RecentSettingsSection` (`meegone-recent-ui`) reads the latest Recent state through a `Latest`, kept current by a new `HomeShell::subscribe_recent`. It offers `window()` and `window_chosen()`.
   - The shell puts it in Settings between Applications' and the lock-screen image's groups, in SET-02's order. A failed Recent doesn't offer it (ARCH-09).
5. **The host draws a radio group.** `RecentSettings` draws:
   - a "Recent" group heading;
   - a "Time window" heading, labelling the group;
   - one radio button per choice, each a 56 dp row in the accent colour.
   Screen readers hear "3 days, radio button, not checked" (A11Y-01). A tap checks its button at once. The next frame puts the check back if the choice couldn't be saved.
6. **No new motion.** Cards that a new window brings or takes away come or go as on any refresh (ANIM-18, ADR 0025): Settings covers Recent while they do.
7. **The host contract.**
   - **`HomeFrame` gains** `recent_window: Option<RecentWindow>`.
   - **The frame grows to 128 slots.** Slot 127 holds 12, 24 or 72, or -1 while Recent isn't working.
   - **New native:** `HomeViews.nativeRecentWindow(int hours)`, which returns null or a message. Other hours do nothing.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 128 slots. |
| 0022, decision 6 | "Show card in Events" is drawn below Recent's time window, which comes after Hidden apps. |
| 0023, context | Recent shows apps used in the chosen window, 24 hours unless the user chooses 12 hours or 3 days. |

## Departures from the SRS (MAINT-05)

None.

## Consequences

- **Benefits:**
  - Tested without a device:
    - Each choice's hours, and each choice saved by name.
    - The migration of 0, 12, 13, 24, 25, 72 and 1 000 hours, and of a line that isn't a number. Every older fixture still loads.
    - Recent following the window at once, saving it, keeping it after a restart, and a failed save.
    - The section sending the choice, and the shell refreshing Recent while Settings is open.
  - In `tests/acceptance.rs`: an app used 30 hours ago shows with three days and not with 24 hours, still after a restart (REC-11).
  - SET-04's count is unchanged: the window was already one of the 10.
  - Checked on a Galaxy S22 (One UI 8, Android 16):
    - A real version 2 settings file loaded, and was saved as version 3 at the first choice with nothing else changed.
    - Three days brought in apps used one and two days before. Apps removed from Recent that day stayed out.
    - The choice lasted after the process was stopped.
- **Costs:**
  - A choice that can't be saved isn't checked, but the host draws no message beside it; the toast says why.
  - The window's hours must agree between `RecentWindow::hours` and `RecentSettings.HOURS` by hand.
  - Not checked on a device yet: TalkBack's reading of the radio group, 200 % font size and right to left. The device matrix (#43) covers these.
