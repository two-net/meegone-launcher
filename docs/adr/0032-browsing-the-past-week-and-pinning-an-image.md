# ADR 0032: Browsing the past week and pinning an image

**Status:** Accepted · **Date:** 2026-11-19 · **Issue:** #31

## Context

Issue #31 (Could) lets the user pick a favourite recent image instead of today's:
- **Browse (WALL-17).** The feed's images from the past seven days.
- **Pin (WALL-17).** Pinning one sets it as the lock-screen wallpaper, and pauses daily updates until it is unpinned.
- **The same rules (WALL-06, WALL-07).** HTTPS, at most 20 MB, at least 720 px on the short side, and bounded decoding.
- **Budgets.**
  - Browsing needs more than the usual one request, but typical network use stays within 10 MB a day (PWR-03).
  - Only the current and previous images are kept on disk (WALL-09), and stored data stays within 50 MB (PERF-08). How browse thumbnails are held was left to decide.
- **Acceptance.** With an image pinned, the next day's check leaves the lock-screen wallpaper as it is; after unpinning, daily updates resume.

What was there already:
- **The feed.** `BingFeed::today` asks for exactly one image, `idx=0&n=1` (ADR 0027).
- **The download.** `Downloader::download` picks, downloads, checks, crops and caches an image as the current one (ADR 0028).
- **The updater.** `ImageUpdater::run` stops early while someone else's wallpaper pauses the checks (WALL-14). "Update now" puts the current image back first, and is how Resume works (ADR 0030).
- **The job's thread.** "Update now" went from the holder to `SchedulerAdapter::update_image_now`, `Bridge.updateImageNow`, `ImageJobService.updateNow` and `nativeUpdateImageNow` to `Shared::update_image_now`. `ShellIntent::ImageChecked` ended `updating` afterwards (ADR 0030).
- **The screens.** `ImageDetailsScreen` and `ImageDetails.java`, opened from the card and the home menu, with Settings over them (ADR 0031).

The feed as recorded on 2026-11-19:
- `idx=0&n=8` answers in the same shape as `n=1`, newest first: 8 images in 5.4 KB.
- Every image is also served at `<urlbase>_800x480.jpg`, about 35 KB, without redirects.
- That day's en-US image wasn't offered as a wallpaper (`wp: false`), while the six before it were.

## Decision

1. **The feed gives the past week in one request.**
   - `ImageFeed::past_week(region)` asks for `idx=0&n=7`: today's image and the six before it, newest first (`PAST_WEEK_IMAGES`).
   - `today` and `past_week` share `parse_image`, so each image is read exactly as before. A response with no image, more than seven, the same image twice, or days out of order is `FORMAT_CHANGED`, as is one image that doesn't parse. The 64 KiB limit stays.
   - `FeedImage` gains `preview`, the 800 × 480 size, built from the checked id like the others. It isn't one of `versions`, so `pick_version` never makes it the wallpaper.
   - `bing-en-US-week.json` is the recorded response.
2. **Previews are held in memory only (PERF-08, WALL-09).**
   - `PastImages` (`past.rs`) keeps the last browse, a `Week` (its region, its local day and its images, each with an optional `Preview`), and the last problem browsing or pinning had. Nothing of it is stored, so the image cache still keeps two images.
   - A preview is checked like the daily image, with a preview's limits (SEC-02): at most `MAX_PREVIEW_BYTES` (256 KiB), a JPEG `jpeg.rs` reads, and no side over `MAX_PREVIEW_SIDE` (1 920 px), so decoding it stays bounded. The 720 px minimum is the wallpaper's, so it applies to the image that is pinned, not to its preview.
   - `Preview` shares its bytes and compares by identity, so comparing states never compares pictures, and its `Debug` prints only the size.
   - Only images offered as a wallpaper get a preview (LEGAL-04; SRS 8.2 allows "the wallpaper and its previews"). The others are listed, without a picture, as "Not offered as a wallpaper".
3. **The updater browses and pins, on the job's thread.**
   - **`browse(past)`** reads the week for the region in use, then each preview, over any connection, because the user asked. It reuses what it read for the rest of the local day, unless a newer image of the day has come in since. A failed preview leaves only that picture out. Nothing is kept once the daily image is turned off meanwhile (WALL-12). It never touches the cache or the check log.
   - **`pin(id, past)`** holds `checking`, as a check does:
     - an image not listed is `NOT_LISTED`, and one not offered is `NOT_OFFERED` (LEGAL-04);
     - the current or the previous image is chosen from the cache, without a download;
     - any other goes through `Downloader::download` unchanged: 20 MB, 720 px, the crop and low storage (WALL-06 to WALL-08).
     - It is then set, as in ADR 0030, and the job is cancelled. It isn't one of the day's 6 checks.
     - A set the system refuses is tried again after 15 minutes, on any network, since setting needs none.
   - **While pinned,** `run` returns the new `Checked::Pinned`, which isn't an attempt: no request, no job (the issue's acceptance criterion). A pinned image not yet on the lock screen is set first. One that someone else replaced stays replaced (WALL-14); pinning it again puts it back.
   - **Unpin is "Update now",** as Resume is (ADR 0030). `update_now` lets go of the pin first, then runs as before, which also schedules the next check, so daily updates resume. Today's image comes back from the cache when it is the previous one: `ask` now chooses a previous record that is today's image instead of downloading it.
4. **The pin lives on the record (`IMAGE_CACHE_VERSION` 5).**
   - `ImageRecord` gains `pinned`, its last field. Only the current record's counts.
   - `Downloader::download(image, region, pinned)` writes it in `put`'s one atomic write, so no crash can leave a pinned image unpinned. `put` unpins the record it makes previous.
   - **`ImageCache::choose(file, pinned)`** makes the record in `file` current, swapping the two records if needed, and sets its pin, in one details write that never touches an image.
     - A previous record comes back without its time and marker. Its old marker isn't the lock screen's any more, and would otherwise look like someone else's change (WALL-14). The updater then sets it as not yet set.
   - The migration adds `false` to each record, and `image-cache-v5.txt` is the new fixture.
5. **What the user asks of the job's thread waits in a queue.**
   - `ImageTasks` (`tasks.rs`) holds `ImageTask::UpdateNow`, `Browse` and `Pin(id)`: the ones waiting, and the one running.
   - The holder queues a task, at most once, and asks `SchedulerAdapter::run_image_tasks()`, `update_image_now` renamed, which still takes no arguments. If the platform refuses, the task is taken back.
   - `Shared::run_image_tasks` runs them one after another on the job's thread, each marked running until it returns or unwinds.
   - **`updating`, `browsing` and `pinning` come from the queue.** Before, any `ImageChecked`, such as an automatic check's, ended "Updating…" even while the update still waited behind it on the same thread.
   - `PastImages` and `ImageTasks` are memory only, so they come together as `ImageMemory`, which `Shared` holds for the home and the job alike.
6. **The holder.**
   - **`LockImageState` gains:**
     - `pinned`;
     - `past`, each `PastEntry` with its written date (L10N-03), whether it is offered, and whether it is the current image;
     - `browsing`, `pinning` and `past_problem`;
     - `past_offered()`, while the daily image is on, with an image yet or not;
     - `may_pin(id)`.
   - **Intents:**
     - `OpenPastImages`, which the shell intercepts;
     - `Browse` and `Pin(id)`, each only while it may.
   - **While pinned:**
     - `paused` is false;
     - "Daily image unavailable" doesn't show;
     - `arm` schedules nothing, as the checks wait anyway.
   - `SetEnabled(false)` empties `PastImages` with the cache.
7. **The screen.**
   - **`PastImagesScreen` (lockimage UI)** is built like the details screen. Its `PastContent` has the images, `loading` and `problem`. Each image says what it offers (`PastAction`): Pin, Pin but not now, Pinning…, Unpin, Unpinning…, or Not offered as a wallpaper.
   - **Its actions:**
     - `pin(revision, index)` asks only for the image shown at that revision;
     - `unpin` sends `UpdateNow`;
     - `try_again` sends `Browse`.
   - **The ways in:** "Past week" on the details screen, and "Choose from the past week" in Settings, which works before any image is cached, as when today's isn't offered.
   - **The shell** keeps `past_images_open`. Opening it sends `Browse`. It is the topmost screen: Back and Home close it first, then Settings, then the details. `returned` closes it too, and so does `keep_past_images_in_step` once the daily image isn't on. The details' Settings opens nothing under it.
   - **`DetailsContent` and `UpdateStatus` gain `pinned`,** so the status line reads "Pinned. Daily updates are paused." in place of the checks, with Unpin.
8. **On Android.**
   - **`PastImages.java`** is a `ScrollView` over the home views in the surface colour, above Settings and the details:
     - the title, then Loading… while there is nothing yet, or the problem with Try again;
     - a card for each image: the preview at 5:3, at most 240 dp tall, the title, the attribution in full, "‹date› · On the lock screen" for the current image, and a 56 dp row with what it offers.
     - Pin and Unpin are named for screen readers, such as "Pin Step into the great outdoors" (A11Y-01).
     - All text is plain (SEC-02), and it scrolls at 200 % text (A11Y-04).
   - **Previews:** `fill` fetches each preview's bytes on the main thread with `nativePastImagePreview`, as the launcher lives there. `ImageCard.decode(byte[], w, h)` decodes it on the decoder's thread, at the size shown. Decoded previews are kept by image id while the screen shows, and dropped when it hides (PERF-07).
   - **`ImageDetails.java`** gains Unpin while pinned, and "Past week". `ImageSettings.java` gains the pinned line with Unpin, and "Choose from the past week".
   - **The frame grows to 139 slots:**
     - 137: the screen is open;
     - 138: its revision, or −1.
   - **Natives:**
     - new: `nativePastImages` (two fields, then seven for each image), `nativePastImagePreview`, `nativePinImage`, `nativeUnpinImage`, `nativePastImagesTryAgain` and `nativeOpenPastImages`;
     - changed: `nativeImageDetails` and `nativeImageStatus` gain a pinned field;
     - renamed: `nativeUpdateImageNow` becomes `nativeRunImageTasks`, and the bridge's `updateImageNow` becomes `runImageTasks`, with `ImageJobService.runTasks`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 1 | `SchedulerAdapter::update_image_now` becomes `run_image_tasks`, which runs the lock-screen image's queue. There are still seven adapters. |
| 0022, decision 1 | Each record keeps whether it is pinned (`IMAGE_CACHE_VERSION` 5), and `ImageCache::choose` makes a cached record current. |
| 0027, decision 1 | `past_week` asks for seven images in one request, and each image has an 800 × 480 `preview`. |
| 0028, decision 1 | `download` takes the pin, which `put` writes with the image. |
| 0029, decision 2; 0030, decision 3 | `Checked::Pinned` is new. "Update now" lets go of a pin first, and runs through the queue; Unpin is "Update now", as Resume is. `updating` comes from the queue, not from `ImageChecked`. |
| 0031, decisions 2 and 7 | The details screen gains Unpin and Past week, and the past week's screen opens over it and over Settings. The frame gains slots 137 and 138, and the details and status natives a pinned field. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| 8.2: request inputs are the region, which day (today) and how many images (one) | Browsing also asks for the past seven days in one request. |
| 8.2: normally one details request and one image download a day | A browse adds one details request and up to seven previews (about 0.25 MB), at most once per local day and region; a pin adds one download. |

## Consequences

- **Benefits:**
  - **The budgets.**
    - A browse is about 5 KB and 6 × 35 KB.
    - A pin is one image, about 3.7 MB as UHD on a phone.
    - An unpin the same day downloads nothing.
    - A day with a check, two browses, a pin and an unpin stays under 10 MB: about 7.6 MB in the acceptance test.
    - Nothing new is stored (PERF-08).
  - **Tested without a device:**
    - **The feed:** the recorded week, every prefix of it, a week of one, too many images, repeats, days out of order, and an unlisted region refused without a request.
    - **Previews:** their limits, and comparing and printing them.
    - **The cache:**
      - the v5 migration and fixture;
      - the pin through `put`;
      - `choose`: current, previous with its marker cleared, a file no record holds, and a failed save.
    - **The queue:** order, no repeats, a task queued while another runs, a task taken back, and a task that unwinds.
    - **The updater:**
      - a browse once a day, again the next day or after a newer image;
      - previews only for images offered, and a failed preview left out;
      - a failed browse, and one turned off midway;
      - pinning a new, the current, and the previous image;
      - images not offered or no longer listed;
      - a refused set retried;
      - someone else's wallpaper while pinned;
      - checks while pinned asking nothing;
      - unpinning from the cache the same day, and with a download a later day.
    - **The holder:** the queue-derived flags against an automatic check's report; browse, pin and unpin through the job's thread; no pause and no job while pinned; nothing while off.
    - **The screens and the shell:** what each image offers, pin by revision, Unpin and Try again; the screen over the details and over Settings, Back and Home, `returned`, and closing once the image is off.
    - **The acceptance scenarios** (`tests/acceptance.rs`):
      - the issue's criterion, through the details screen;
      - pinning before any image is cached, through Settings;
      - PWR-03's day of browsing and pinning.
  - **Built.** `cargo clippy` for `aarch64-linux-android`, and `assembleDebug` and `assembleRelease`. The APK's library exports the new natives, and the bridge's `runImageTasks` matches the lookup at start-up.
- **Checked on a Galaxy S22 (One UI, Android 16, Bangkok, Wi-Fi)** with the debug build. Before the check, the daily image was off, its cache was empty, and the lock screen showed home wallpaper 971 (lock id −1).
  - **Turned on in Settings,** the first check made one attempt. That day's en-GB image wasn't offered as a wallpaper, so nothing was cached, and the status read "Not updated yet". The cache file was saved as version 5. "Choose from the past week" showed anyway, before any image.
  - **The past week:**
    - It read "Loading…" for a moment, then listed the seven days from 19 to 13 November.
    - The 19th's sculpture had no preview and read "Not offered as a wallpaper". The six others had their previews, full width at 5:3, each with Pin.
    - The accessibility tree named each button by its image, such as "Pin The only way is up".
  - **Pinning "Hidden depths" (13 November):**
    - The lock screen got wallpaper 972 within half a second. Its record was current, pinned, with marker 972, and a 566 KB crop in `images/a`.
    - The check log still held its one attempt.
    - The card read "13 November · On the lock screen" with "Unpin Hidden depths", and the list kept its scroll position.
    - MeeGone's job was no longer registered.
  - **Pinned:**
    - Settings and the details screen read "Last updated 19 November at 9:39 pm" and "Pinned. Daily updates are paused.". Settings had Unpin; the details screen had More info, Unpin, Update now, Past week and Settings.
    - The Events card showed the pinned image.
  - **The daily check while pinned:**
    - `cmd jobscheduler run -f` found no job to run.
    - After a force stop, MeeGone's start check (a new process) recorded no attempt, scheduled no job, and left lock wallpaper 972 pinned.
    - **For contrast,** after unpinning, a cancelled job and the same restart: the start check scheduled the job again. Pinned, the check takes the pinned path.
  - **At 200 % text,** opened from the details screen, every line wrapped without overlap, and the screen scrolled to its last card. The activity was recreated by the change, and the screen stayed open, its previews decoded again.
  - **Unpin** made one feed request, the day's second attempt, and the record lost its pin. That day's image still wasn't offered, so the lock screen kept "Hidden depths". The job came back for 2 hours later (WALL-03), so daily updates resumed.
  - **Not checked on the device:**
    - TalkBack speaking, beyond the names in the accessibility tree;
    - "Pinning…" and "Unpinning…", which each finished in under half a second;
    - the next day, as moving the phone's clock would change its owner's settings; the start check takes the same path;
    - pinning the previous image from the cache, which needs two cached images, while only one could be cached that day;
    - the Dark theme, which comes with #32.

    The tests cover them.
  - **Afterwards:** the daily image was off and its cache empty, with no job. The lock screen was reset to the home wallpaper (lock id −1; One UI renumbered the home wallpaper 974, as in ADR 0030), and the text size was 100 %. This build stayed installed.
- **Costs:**
  - **A browse can be a day stale** for an image the feed adds after it, until a newer image of the day comes in or the next local day; Try again shows only after a problem.
  - **Previews are fetched again after the process restarts,** as they are never stored.
  - **Pinning someone else's replaced wallpaper back** means pinning it again: while pinned, Settings doesn't offer Resume, since Resume would unpin.
  - **Kept in step by hand:**
    - slots 137 and 138;
    - the fields of `nativePastImages`, `nativeImageDetails` and `nativeImageStatus` with `PastImages.java`, `ImageDetails.java` and `ImageSettings.java`;
    - `runImageTasks` between `Bridge.java` and `android/scheduler.rs`;
    - `nativeRunImageTasks` with `ImageJobService.java`.
