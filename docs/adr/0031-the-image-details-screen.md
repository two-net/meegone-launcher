# ADR 0031: The image details screen and the update status

**Status:** Accepted · **Date:** 2026-11-19 · **Issue:** #30

## Context

Issue #30 gives the daily image one screen of its own: today's image, its credit and its status, and what can be done with it.
- **The screen (WALL-15).** It shows the image, the title, the attribution, the date and the region, with More info, Update now and Settings.
- **More info (WALL-15, SEC-02).** It opens the source's info link in the external browser, only over HTTPS, and only when there is a link.
- **The way in.** The screen opens from the Events image card and from the home menu's "Lock-screen image".
- **Plain text (LEGAL-03, SEC-02).** The attribution is shown as the source provides it, and every text detail as plain text.
- **The status (WALL-16, Should).** Settings shows when the image last updated and, in plain words, the last problem, such as "No connection. Will try again later."
- **Acceptance.**
  - With a cached image that has an info link, More info opens it in the browser. With no link, More info is hidden.
  - After an attempt that failed for lack of connection, Settings shows "No connection. Will try again later."

What was there already:
- **The screen's place.** `ShellState.image_details_open`, opened by the card's `LockImageIntent::OpenDetails` and closed by Back and Home (ADR 0022, decision 3). The host covered the home views with the screen's name only.
- **The home menu.** "Lock-screen image" opened Settings at its group, so it worked before any image existed (ADR 0008, decision 5).
- **The data.**
  - `ImageRecord` keeps the date, the region, the title, the attribution and the info link (WALL-09).
  - `applied_at_ms` records when MeeGone set the image on the lock screen (ADR 0030).
  - The check log keeps the last problem in plain words, and `CheckStatus` says whether the checks are retrying or have failed for 3 days (ADR 0029).
  - `BingFeed` keeps an info link only while it is plain `https://` (ADR 0027).
- **The browser.** `SystemConfigAdapter` had `has_browser` and `open_in_browser`. On Android, `open_in_browser` answered "Not available yet" (ADR 0030).
- **Dates and times.** `format_time` writes a moment in the device's language, time zone and 12- or 24-hour setting, in four styles (ADR 0019).

## Decision

1. **The menu's "Lock-screen image" opens the details once there is an image.**
   - While the details screen has something to show, the item opens it. That is when the daily image is on and has a cached image (`LockImageState::has_details`, `HomeShell::image_details_offered`).
   - Otherwise it opens Settings at its Lock-screen image group, as before. Before the first image, the user can still turn the feature on (WALL-02, WALL-10).
2. **The screen is offered only while it has something to show.**
   - It opens only while the lock-screen image works (ARCH-09) and `has_details`. Opening it reads the image and the checks again, so the status is current.
   - `keep_image_details_in_step`, in the shell's dispatch epilogue, closes it once nothing is left to show. Examples: the daily image turned off in Settings over it, or the lock-screen image failing. Back then goes home, never to an empty screen.
   - **Settings on the screen** sends `LockImageIntent::OpenSettings`. The shell intercepts it, as it does `OpenDetails`, and opens Settings at the Lock-screen image group over the screen. Back closes Settings first, so it returns to the details (HOME-08).
3. **More info opens only what the screen offers.**
   - `LockImageIntent::OpenInfoLink` opens the current image's link only when all of these hold:
     - the daily image is on;
     - the image has a link;
     - the link starts with `https://`;
     - a browser is installed (`LockImageState::has_browser`, SRS 8.1).
   - A link the browser can't open becomes `problem`, which the host shows briefly.
   - The screen offers More info (`DetailsContent::more_info`) under the same conditions, so it is hidden without a link or a browser.
   - **On Android,** `AndroidSystem::open_in_browser` refuses anything but `https://` before asking the bridge.
   - **`Bridge.openInBrowser`** checks the link again: scheme `https` and a host. It then sends `ACTION_VIEW` with `CATEGORY_BROWSABLE`.
     - Where a default browser is set, the link goes to that package. An app that claims the site's links, such as a search app for `bing.com`, therefore doesn't take it, and the link opens in the external browser (SEC-02). Without a default, the system chooses.
     - It never logs the link. If nothing opens it, it answers "No browser can open this link".
   - The manifest already lets MeeGone see browsers (ADR 0016).
4. **The words are written by the platform; the lock-screen image decides which.**
   - `LockImageState` gains:
     - `date`: the image's date in the device's language, such as "15 November";
     - `updated`: when MeeGone set it on the lock screen, such as "19 November, 07:00", or `None` before the first;
     - `has_browser`.
   - `refresh` writes them, and `ClockChanged` now refreshes the lock-screen image as well as the Events header, so a new time zone or 12- or 24-hour setting takes effect (L10N-03).
   - **The date** is the feed's `yyyymmdd`. `checks::day_of` turns it into a day, a pure civil-date conversion that rejects anything that isn't a date. `local_noon_ms` gives noon on that day in the local zone, which `format_time` writes as `DayMonth`. Noon keeps the day right however far the zone is from UTC (UTC−11 to UTC+14 are tested).
   - **A fifth style,** `TimeStyle::DayMonthTime`, writes the update time. On Android it is ICU's `MMMMdHm` or `MMMMdhm` skeleton, so the language orders the fields and joins them in its own words, such as "19 November at 7:00 am". The fake writes "19 November, 7:00 AM".
   - **The region** stays a BCP 47 tag in Rust. The host names it with `Locale.getDisplayName`, such as "English (United States)", as #34's Region picker will.
5. **The status line.**
   - `ImageSettingsSection::update_status()` is `UpdateStatus { updated, checks }` while the daily image is on, and `None` otherwise. `status_revision()` changes only when it does.
   - The host words it from string resources (L10N-01):
     - "Last updated ‹time›", or "Not updated yet" before the first image;
     - while the checks retry, "‹problem›. Will try again later.";
     - after 3 days of failures, "‹problem›." under "Daily image unavailable" with Try again (SRS 7.6).
   - The problem is the check log's own words, such as "No connection" (`DataError::Offline`).
   - Settings shows the line below the daily image's switch. The details screen shows the same line.
6. **The screen's content** is `ImageDetailsScreen` in the lock-screen image's UI crate, built like `ImageCard`:
   - `revision()` and `content()`: a `DetailsContent` with:
     - the title and the attribution, as the source gives them (LEGAL-03);
     - the date and the region's tag;
     - the image's file and key;
     - `more_info`.
   - `more_info()` and `open_settings()`, which ask only while the screen offers them.
   - The private `Told<T>` now keeps the card's, the screen's and the status line's last content and revision alike.
   - The shell keeps the screen for the host, as it does the card (ADR 0022). `HomeFrame` gains `image_details`, its revision while it is open, and `image_status`, the status line's revision while the daily image is on.
7. **On Android.**
   - **`ImageDetails.java`** replaces the screen's name. It is a `ScrollView` in the surface colour, inside the safe area, and contains:
     - the image, full width and centre-cropped, at most 4:3 of the width and 55 % of the screen tall. `ImageCard`'s decoder decodes it on its background thread, only as large as it shows. Screen readers skip it, as the text says what it shows.
     - the title, 24 sp, a heading;
     - the attribution and "‹date› · ‹region›", 15 sp, in the secondary colour;
     - the status line;
     - More info, Update now and Settings as full-width rows, at least 56 dp tall, in the accent colour (A11Y-02). Update now reads "Updating…" as frame slot 131 says.
   - Every text is set as plain text, never as HTML, and links are never made clickable (SEC-02). It scrolls, so at 200 % text it wraps and nothing overlaps (A11Y-04).
   - **`ImageSettings.java`** draws the status line below the switch: "Last updated …" in the secondary colour, and the problem in the text colour.
   - **The frame grows to 137 slots:**
     - 135: the details screen's revision, or −1 while it is closed;
     - 136: the status line's revision, or −1 while the daily image is off.
   - **New natives:**
     - `nativeImageDetails()`: seven fields, the title, the attribution, the date, the region's tag, the file, the key and "1" while More info is offered;
     - `nativeImageStatus()`: three fields, the update time, the problem and how the checks are going ("retrying", "unavailable" or empty);
     - `nativeImageMoreInfo()`: the link opens, and returns a message or `null`;
     - `nativeImageDetailsSettings()`.
   - Update now reuses `nativeImageUpdateNow`.
   - `Bridge` gains `openInBrowser(String)` and style 4 in `formatTime`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0008, decision 5 | Lock-screen image opens the image details screen while it has an image to show, and Settings at its Lock-screen image group otherwise. |
| 0019, decision 1 | `TimeStyle` gains `DayMonthTime`, style 4, for when the daily image last updated. |
| 0022, decisions 3 and 7 | The details screen is drawn whole. It opens only while it has something to show, and closes once it hasn't. The frame gains slots 135 and 136. |
| 0030, amendment to 0005 | Opening links is built: `open_in_browser` opens `https://` links in the default browser. |

## Departures from the SRS (MAINT-05)

None.

## Consequences

- **Benefits:**
  - **Tested without a device:**
    - **The date:**
      - `day_of` with valid dates, a leap day, the turn of the year and malformed dates;
      - noon on a day in zones from UTC−11 to UTC+14;
      - the date and update time written in the device's zone, or not at all before the first image.
    - **More info:**
      - opening only an `https://` link of an image that has one, with a browser and the daily image on;
      - a browser's refusal as the problem.
    - **The screen:**
      - its content, its revision, and when it is offered;
      - More info hidden without a link or a browser;
      - Settings only while it is offered.
    - **The status line:** while it retries and before the first image, nothing while the image is off, and a revision that changes only when it does.
    - **The shell:**
      - the menu item before and after the first image;
      - the screen offered only with an image;
      - Settings over the screen, and Back back to it;
      - the screen closing when the image is turned off;
      - `ClockChanged` writing the update time again.
    - **The acceptance scenarios** (`tests/acceptance.rs`):
      - both of the issue's;
      - the menu's way in;
      - AC-15, which now checks the screen's title, attribution, date, region, link and update time.
  - **Checked on a Galaxy S22 (One UI, Android 16, Bangkok, Wi-Fi)** with the debug build. The lock screen showed the home wallpaper (lock id −1) and the daily image was off.
    - **Before any image:** Lock-screen image in the home menu opened Settings at its group. Turning the image on showed "Not updated yet".
    - **No image that day:** "Update now" ran, but none of the feed's 12 regions offered that day's image as a wallpaper (en-AU, which the feed doesn't list, did). Nothing was cached or set, and the status stayed "Not updated yet".
    - **A seeded image:** to see the screen, the app's own cache was given the previous day's en-US image (barn owls, 18 November, offered as a wallpaper), with its real title, attribution and info link, set at "19 November, 07:00".
      - The card showed it. A tap opened the screen with the image across the top, the title, the attribution, "18 November · English (United States)", "Last updated 19 November at 7:00 am", More info, Update now and Settings.
      - More info opened the image's `https://www.bing.com/search?…` page in Microsoft Edge, the default browser. Back returned to the screen.
      - Settings opened over it at Lock-screen image, with the same status. Back returned to the screen, then to Events.
      - Lock-screen image in the home menu now opened the screen.
    - **A seeded failure:** with a recent "No connection" failure in the check log, the screen and Settings both read "No connection. Will try again later."
    - **Turned off** in Settings over the screen, the cache emptied, and Back went to Events.
    - **At 200 % text,** the screen wrapped every line and scrolled to its last row without overlap.
    - **Afterwards:** the daily image was off, the cache empty, the text size 100 %, and the lock screen still showed the home wallpaper. "Update now" was never run with an image cached, so MeeGone set no wallpaper.
    - **Not checked on the device:**
      - the Dark theme, which comes with #32;
      - an attempt that fails offline, as turning the network off would also end adb over Wi-Fi. The seeded failure and the tests cover it.
- **Costs:**
  - **Words in two places.** The problem's words come from Rust; the sentence around them comes from string resources. Translating them waits for #39.
  - **The date has no year.** The image is from the last few days, as the Events header's date has none either.
  - **The default browser is looked up on each tap.** Where none is set, the system's chooser may also offer apps that claim the site's links.
  - **Kept in step by hand:**
    - `nativeImageDetails`, `nativeImageStatus`, `nativeImageMoreInfo` and `nativeImageDetailsSettings` between `src/android.rs` and `HomeViews.java`, and their fields with `ImageDetails.java` and `ImageSettings.java`;
    - slots 135 and 136;
    - `openInBrowser` and style 4 between `Bridge.java` and the Rust adapter.
