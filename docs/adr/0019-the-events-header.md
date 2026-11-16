# ADR 0019: The Events header

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #18

## Context

Issue #18 opens Events with a calm, large date, like the N9's date bar:
- A header shows the weekday and date in large, light type: 40 sp light on phones, scaling with the system font size (EVT-01, SRS 5.2).
- Dates and times follow the device's language and its 12- or 24-hour setting (L10N-03).
- The header works without notification access (EVT-03).
- When an alarm is set, it shows the next alarm's time, and a tap opens the app that set it. Without one, the alarm row is hidden (EVT-02, Should; SRS 8.1).
- The header updates when the date changes at midnight or the time zone changes.

What was there already:
- `SystemConfigAdapter` had `next_alarm_ms` and `open_next_alarm`, and its fake had `next_alarm_ms` and `alarm_opened`. On Android both were stubs waiting for this issue.
- `utc_offset_ms` and `now_ms` were real. `locale()` is a stub until #39.
- Events had a state holder for notifications, but its page showed only its name.

The constraints:
- **The app's crates use only the standard library** (DEPENDENCIES.md). It has no calendar, no time zones and no locale data. ADR 0016 met the same limit for search: the platform folds, and Rust ranks.
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, and text travels through a native that returns `String[]`, read by revision (ADR 0017).
- **Adapters never call back** (ADR 0003). A change the system reports reaches MeeGone as a `ShellIntent`.
- **No periodic wakes** (PWR-02). Only the daily image is scheduled (ARCH-07), so the header can't run a timer of its own.

## Decision

1. **The platform writes; Events decides.**
   - `SystemConfigAdapter::format_time(at_ms, style)` writes a moment in the device's language, time zone and 12- or 24-hour setting. `TimeStyle` has four styles: `Weekday` ("Tuesday"), `DayMonth` ("17 November" or "November 17"), `Time` ("07:00" or "7:00 AM") and `WeekdayTime` ("Wed 07:00"). It returns an empty string if the platform can't write the moment.
   - On Android, `Bridge.formatTime` asks ICU's `DateFormat.getInstanceForSkeleton` with the default locale:
     - skeletons `EEEE`, `MMMMd`, `Hm`/`hm` and `EHm`/`Ehm`, picked by `DateFormat.is24HourFormat`;
     - the language's own patterns, so the fields come in its order;
     - the default time zone, set on each call, so a new zone counts at once;
     - standalone capitalisation, as the language does at the start of a line.
   - The fake writes English with the day before the month, as the SRS's layout mock does, in `utc_offset_ms`'s zone. Its new `hour_24` field picks the time's form.
2. **Events owns the header.**
   - `EventsState.header` is an `EventsHeader`: `weekday`, `date` and `alarm: Option<String>`. `EventsHolder` takes `SystemConfigAdapter`.
   - The header reads `now_ms` once and writes the weekday and the date from it.
   - The alarm shows only its time when it rings within 24 hours; otherwise it shows its short weekday too. A time alone therefore always means the next time the clock shows it, so "07:00" is never ambiguous.
   - `Refresh` reads the header and the notifications. Missing notification access now empties only the notifications, and the header stays (EVT-03).
   - `RefreshHeader` reads only the header. `OpenAlarm` opens the alarm's app; it does nothing while no alarm shows, and a failure becomes `problem`.
   - `Store` publishes only on a change, so the snapshot changes exactly when the text does.
3. **The system tells MeeGone when the clock changes.**
   - New `ShellIntent::ClockChanged`: the date, the time, the time zone, the 12- or 24-hour setting or the next alarm changed. The shell sends `RefreshHeader`.
   - `Resumed` sends `RefreshHeader` too. While MeeGone was away the day may have changed, and a new language recreates the activity.
   - Swiping to Events already sends `Refresh`, which now includes the header.
   - `Bridge.watchClock()`, called from `onCreate` beside `watchApps()`, registers once per process for:
     - `ACTION_DATE_CHANGED`: midnight;
     - `ACTION_TIME_CHANGED`: the clock set, or the 12- or 24-hour setting changed;
     - `ACTION_TIMEZONE_CHANGED`;
     - `AlarmManager.ACTION_NEXT_ALARM_CLOCK_CHANGED`.
   - A burst of them is reported once, through the main thread, as `watchApps` does. `HomeActivity.clockChanged` calls `nativeClockChanged()` and redraws a resumed home.
   - No timer runs, and nothing wakes the device.
4. **The next alarm on Android.**
   - `Bridge.nextAlarmMs()` asks `AlarmManager.getNextAlarmClock()`, which needs no permission. -1, or a call that fails, hides the row.
   - `Bridge.openNextAlarm()` sends the alarm's show intent, which the app that set it provides.
     - From Android 14, the sender must let such an intent start its screen: `setPendingIntentBackgroundActivityStartMode` with `ALLOW_IF_VISIBLE` on Android 16, and `ALLOWED` on 14 and 15. Otherwise the system may block it, because the clock app is in the background.
     - Without a show intent, it opens the default clock app's alarms (`AlarmClock.ACTION_SHOW_ALARMS`).
     - If neither opens, it returns "The alarm can't open right now", which shows briefly.
5. **The host draws the header at the top of Events.**
   - `EventsHeader` replaces Events' name. It sits at the top and start of the page, inside the safe area, with 16 dp side margins and 8 dp above.
   - The weekday and the date are two lines in the text colour, 40 sp (so they scale with the system font size), `Typeface.create(DEFAULT, 300)`. At large sizes they wrap, hyphenating a long word, rather than overlap or be cut short (A11Y-04). Screen readers read the two lines as one heading.
   - The alarm row, "Alarm ‹time›", is 15 sp in the secondary colour. It is clickable, with a 48 dp minimum height and a ripple (A11Y-02), and screen readers hear "Open alarm" as its action.
   - Being clickable, a long-press on the alarm row isn't empty space, while one on the date still opens the home menu (ADR 0008).
   - Below the header, the notifications come with #19.
6. **The host contract.**
   - `EventsView::show_header(state)` keeps the header it last told the host, and returns a revision that changes only when the header does. `ShellView::frame` calls it every frame, whichever view shows, so the header is current as Events slides in.
   - `HomeFrame::events_header` is that revision, or `None` while Events isn't `Ready` (`HomeShell::events_ready`, ARCH-09).
   - The frame grows to 82 slots:
     - 80: 1 while Events works;
     - 81: the header's revision.
   - New natives:
     - `HomeViews.nativeEventsHeader()`: three fields, the weekday, the date and the alarm, which is empty to hide its row;
     - `HomeViews.nativeOpenAlarm()`: returns Events' problem, or `null`;
     - `HomeActivity.nativeClockChanged()`.
   - `Bridge` gains `formatTime(long, int)`, `nextAlarmMs()` and `openNextAlarm()`. Style numbers 0 to 3 follow `TimeStyle`'s order (`style_code` in `android/system.rs`).

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 1 | `SystemConfigAdapter` gains `format_time`, with `TimeStyle` in the models. The platform writes dates and times, as it folds text for search (ADR 0016). |
| 0003, decision 2 | `ShellIntent::ClockChanged` joins the system's changes. |
| 0006, decision 2 | The frame has 82 slots. Events draws its header instead of its name. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| EVT-01: 40 sp light "on phones" (SRS 5.2) | Tablets and foldables use 40 sp too. SRS 5.2 gives no larger size. |
| EVT-02: tapping the alarm opens the app that set it | When the alarm offers no screen of its own, the default clock app's alarms open instead. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - The header with and without notification access.
    - 12- and 24-hour times, an alarm within a day and further away, and no alarm.
    - Midnight, a new time zone, `ClockChanged` and `Resumed`, and the revision changing only with the text.
    - Opening the alarm, a tap after it was turned off, and a failure.
    - The issue's acceptance criteria, in `tests/acceptance.rs`.
  - Checked on a Galaxy S22 (One UI 8, Android 16), with MeeGone as the default home app and no notification access: Events shows "Tuesday" and "17 November" in large, light type at its top, and no alarm row while no alarm is set.
- **Costs:**
  - The text is only as good as the platform's patterns. The fake's English is no check of other languages; #39 covers them.
  - The frame slots, the header's fields and the style numbers must agree between Java and Rust. Nothing checks that at build time.
  - The view indicator, shown while swiping, overlaps the top of the date, as it does the search field.
  - Not checked on a device yet: the alarm row and opening it, 24-hour time, a new time zone, midnight, 200 % font size, TalkBack, right to left, a tablet and dark mode (#32). The device matrix (#43) covers them.
