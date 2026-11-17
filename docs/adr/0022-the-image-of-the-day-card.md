# ADR 0022: The image-of-the-day card

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #21

## Context

Issue #21 brings the daily lock-screen image into Events, where the N9 had its feeds:
- While the daily lock-screen image is on, an "Image of the day" card below the notifications shows a thumbnail, the title and the attribution (EVT-12, Should).
- A tap on it opens the image details screen (EVT-12, WALL-15).
- "Show card in Events", in Settings → Lock-screen image, can hide it (EVT-12, SET-02).
- The attribution is shown as the image source provides it (LEGAL-03).
- When a new image arrives, the thumbnail cross-fades to it in 300 ms (ANIM-22, Should).
- It works offline from the cached image, and without notification access (EVT-03, REL-06).
- Visually, it is a flat card with 12 dp corners in the `surface` colour and no shadow. Photography appears only here, on the image details screen and on the lock screen (SRS 5.2).

What was there already:
- `ImageCard`, a `Slot` that the shell put into Events, with `is_visible()` and an empty `render` (ADR 0002). Nothing asked it anything.
- `ImageOptions.show_card`, saved and on by default, which nothing wrote.
- `IMAGE_CROSSFADE`, 300 ms, which nothing used.
- The image cache, which kept only the current and previous images' details. The images themselves were left for #27 (ADR 0004).
- No image details screen (#30) and no Settings screen (#34).
- On Android, the wallpaper adapter is a stub until #29, so the lock-screen image reports itself unavailable.

The constraints:
- **Features don't depend on each other** (ARCH-03, ADR 0002). The card is the lock-screen image's content inside Events, so the shell places it.
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, text travels through natives read by revision, and motion is state in the Rust app.
- **Only the files `StoredFile` names are stored** (ADR 0004), within the 50 MB budget (PERF-08), and only `layout` and `settings` are backed up (ADR 0018).
- **The app's crates use only the standard library** (DEPENDENCIES.md), so they can't decode images.

## Decision

1. **The image cache keeps the images.**
   - `StoredFile` gains `ImageA` (`images/a`) and `ImageB` (`images/b`). They are part of the image cache, so the number of persistent stores stays at 3. The budget counts them, and backup leaves them out.
   - `ImageRecord.image: Option<ImageFile>` names the file holding the record's image. Version 2 of the image-cache format adds it. Records saved by version 1 had no image.
   - `ImageCache::put(record, image)` picks the file itself: the one the current image doesn't use. Each step is one atomic write, so a crash at any point leaves every record naming the file that holds its image:
     1. If the previous record uses that file, its record lets go of it first.
     2. The image is written.
     3. The details make it current, and the current one previous. The oldest record is gone (WALL-09).
   - `ImageCache::image(record)` reads an image back, for setting the wallpaper in #29. `clear()` removes the details, then both images (WALL-12).
   - #27 now only downloads, validates and crops, and hands the result to `put`.
2. **The lock-screen image owns the card; the shell places it.**
   - The card shows while the daily image is `On`, `show_card` is on, and the current record has an image. A record without one shows no card.
   - `ImageCard` becomes `Clone`, sharing what the host was last told, like `HiddenAppsSection` (ADR 0017). The shell keeps one handle, and Events holds the other.
   - `revision()` changes only when its `CardContent` does: the title, the attribution, the image's path and the image's key (its date, region and file). `content()` returns it.
   - `tapped(revision)` sends `LockImageIntent::OpenDetails`. A card that changed or was hidden meanwhile does nothing.
   - `LockImageIntent::SetShowCard(on)` writes `show_card`; the lock-screen image is that field's one writer. A failed save becomes `problem`. `ImageSettingsSection` gains `show_card()` and `show_card_toggled()`.
   - `Resumed` and swiping to Events refresh the lock-screen image, as they do Events, so an image the daily job cached while MeeGone was away shows.
3. **The image details screen covers the home views, as Settings does.**
   - The shell intercepts `LockImageIntent::OpenDetails`, as it does `AppsIntent::OpenSearch`, and sets `ShellState.image_details_open`. It opens only while the lock-screen image works (ARCH-09). Opening it ends gestures and closes menus, the folder and search.
   - It covers the views: no touches, no home menu, no search.
   - Back and Home close Settings first, then it (HOME-07, HOME-08). Home from an app with "Always Applications" closes it (HOME-06).
   - Until #30 draws it, the host shows only its name, "Image of the day", in the surface colour.
4. **The host decodes; the shell times the fade.**
   - The Java host reads the image straight from the cache's file, `getFilesDir()` plus the card's path. That is the folder `HomeActivity` gives `FileStorage`. Passing several megabytes through JNI on the main thread would stall a frame, and Java alone can decode images.
   - A single background thread decodes only the middle band of the image that the thumbnail shows (`BitmapRegionDecoder`, a centre crop), sampled down by powers of two to the thumbnail's width. A file that can't be read leaves the plate empty.
   - Decoding takes tens of milliseconds, so the host says when a new thumbnail is ready over the old one: `ShellIntent::NewImageShown`. The shell then runs a 300 ms decelerating fade on the shared `Timed` clock (`image_change.rs`). It plays from its start, takes late frames in small steps, and is instant with animations removed (ANIM-05).
   - The first image, and the same image decoded again at a new size, show at once.
5. **The host draws the card below the notifications.**
   - `ImageCard.java` sits after `EventsList` in the Events page's scrolling content. So it is below the notification-access card, "No new events" or the groups, and it still shows when Events itself has failed.
   - It has an "Image of the day" section header, drawn like "Notifications". `SectionHeader.java` now draws both.
   - The card: 16 dp side margins, 12 dp corners, `surface`, no elevation, clipped to its outline, with a ripple.
   - The thumbnail: 16:9 of the card's width, at most 240 dp tall.
   - The title: 15 sp, up to two lines. The attribution: 13 sp in the secondary colour, in full.
   - Screen readers hear the title and the attribution, with "Show image details" as the tap's action. The images themselves are hidden from them.
   - The card is clickable, so a long-press on it isn't empty space (ADR 0008).
6. **Settings → Lock-screen image → "Show card in Events".** Until #34 draws Settings, `ImageSettings.java` draws this one switch below Hidden apps, as ADR 0017 did for Hidden apps. It shows only while the lock-screen image works.
7. **The host contract.**
   - `HomeFrame` gains:
     - `image_card`: the revision, while the card shows;
     - `image_change`: the fade's progress;
     - `image_details_open`;
     - `show_image_card`: `None` while the lock-screen image doesn't work.
   - The frame grows to 94 slots:
     - 89: the card shows;
     - 90: its revision;
     - 91: the fade, 0 to 10 000;
     - 92: the details screen is open;
     - 93: "Show card in Events": 1, 0, or -1 when it isn't offered.
   - New natives:
     - `nativeImageCard()`: the title, the attribution, the image's path and its key;
     - `nativeImageCardTapped(int)`;
     - `nativeNewImageShown()`;
     - `nativeShowImageCard(boolean)`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0002, decision 2 | The shell keeps a handle on `ImageCard` and `ImageSettingsSection` for the host, and opens the image details screen when the card asks. |
| 0004, decision 2 | `StoredFile` also names the image cache's two images, `images/a` and `images/b`. The image cache's format is at version 2. |
| 0006, decision 2 | The frame has 94 slots. The host reads one stored file itself: the card's image. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| EVT-12, WALL-15: the tap opens the image details screen | Until #30, that screen shows only its name. |
| EVT-12: a thumbnail | The thumbnail is the middle of the lock-screen image at 16:9, at most 240 dp tall. |
| SET-02: Settings' Lock-screen image group | Until #34, only "Show card in Events" is drawn. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - The image cache's two files, the third image overwriting the oldest, a failed write never mixing up the images, `clear`, and loading version 1.
    - Hiding and showing the card, and a failed save.
    - The card's visibility, its revision changing only with what it shows, and stale taps.
    - The details screen covering the views, Back, Home and Home from an app, and a failing lock-screen image never covering the home.
    - Refreshing on `Resumed` and when Events shows.
    - The fade's 300 ms, the animation speed and animations removed.
    - The issue's acceptance criteria, in `tests/acceptance.rs`: the card offline and without notification access, "Show card in Events" off and on across a restart, the daily image off, the tap, and a new image's fade.
  - #27 has a tested place to put its images, and #29 a way to read them back.
- **Costs:**
  - The card's path is relative to the folder `HomeActivity` gives the Rust app. If one changes, the other must too.
  - The frame slots, the card's fields and the natives must agree between Java and Rust. Nothing checks that at build time.
  - A previous image can lose its file to a failed `put`. Its details stay, and nothing shows the previous image yet.
  - Not checked on a device: the card can't appear there until the stubs for #27 to #29 are replaced. Only the APK building, Events looking as before, and the Settings switch can be checked now. The device matrix (#43) covers the rest: the card's look in both themes, the fade, rotation, TalkBack, 200 % font size, right to left and a tablet.
