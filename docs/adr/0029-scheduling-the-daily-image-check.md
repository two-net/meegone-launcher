# ADR 0029: Scheduling the daily image check

**Status:** Accepted · **Date:** 2026-11-19 · **Issue:** #28

## Context

Issue #28 decides when MeeGone asks the feed for a new image, and does it while staying light on battery and data:
- **One job (ARCH-07, PWR-02).** One scheduled background job, the daily image. Everything else reacts to system events, and nothing else wakes the device while the screen is off.
- **When (WALL-03).** Once a day after local midnight. If the feed still returns the previous image, up to three more times that day, at least two hours apart.
- **Conditions (WALL-04, PWR-04).**
  - Automatic downloads use only unmetered networks unless "Use mobile data" is on (default off).
  - They wait while battery saver is on or the battery is low.
  - Data Saver is respected.
- **Budget (REL-04, PWR-03).** After a failure, back off from 15 minutes, doubling. No more than 6 network attempts a day, and typically 10 MB a day or less.
- **Fallbacks (SRS 8.1, SRS 7.6).**
  - Without background scheduling, check at the next MeeGone start.
  - After 3 days of failures, Settings shows "Daily image unavailable" with Try again.
- **Pure logic (MAINT-01).** Scheduling is tested with a fake clock and scheduler.
- **Acceptance.**
  - With no connection, the daily check leaves the wallpaper unchanged, Settings says it will try again, and no more than 6 attempts are made that day.
  - With mobile data only and "Use mobile data" off, no automatic download happens.

What was there already:
- **The adapter.** `SchedulerAdapter` (`schedule_image_job(ImageJob { not_before_ms, network })`, `cancel_image_job`) and `FakeScheduler`, which keeps one job. On Android it was a stub (ADR 0005).
- **The holder.** `LockImageHolder` scheduled the job "as soon as possible" when the daily image was turned on, and cancelled it and cleared the cache when it was turned off. The setting `use_mobile_data` was saved, but nothing wrote it.
- **The feed and the download.**
  - `BingFeed::today` makes one request and never retries (ADR 0027).
  - `Downloader::download` downloads, checks, crops and caches one image, and leaves the cache as it was on any failure (ADR 0028).
- **What ADR 0028 left here:**
  - deciding when;
  - telling the same image apart (the record kept no feed id);
  - skipping an image not offered as a wallpaper;
  - retries and back-off;
  - making sure a download in progress caches nothing once the user turns the feature off (WALL-12).
- **The process.** The `Launcher` is a main-thread `thread_local!` that exists only after `HomeActivity` starts it. The JNI binding knows only threads the JVM made. The app crate may use only `shell` and `core` crates.

## Decision

1. **The policy is pure: `checks.rs` in `meegone-lockimage-state`.**
   - **Its input** is the check log, `Checks`, and the time. The log holds:
     - the local day it is for (`day`);
     - that day's network attempts (`attempts`) and answers without a new image (`same`);
     - whether today's image is in (`updated`);
     - failures in a row (`failures`) and when the last attempt started (`last_attempt_ms`);
     - when the current streak of failures began (`failing_since_ms`), and the last problem in plain words.
   - **When the next check is due (`due_ms`):**
     - On a new local day, at once: the day's counts start again.
     - At the next local midnight, once today's image is in, after 6 attempts, or after 4 answers without a new image (WALL-03).
     - After a failure, the last attempt plus 15 minutes × 2^(failures − 1): 15, 30, 60, 120 and 240 minutes (REL-04).
     - After an answer without a new image, the last attempt plus 2 hours.
     - A retry that would fall after midnight gives way to the next day's check.
   - **Midnight is local, and right on the days the clocks change.** `utc_offset_ms` is asked at midnight itself, and where the clock skips midnight, the day starts at its first moment.
   - **An attempt counts before its request (`begin`).** It counts as failed until the feed answers (`answered` or `failed`), so one that MeeGone never finishes still keeps to the budget and the back-off.
   - **How it is going (`status`):**
     - `Fine`.
     - `Retrying(problem)`.
     - `Unavailable(problem)`, once the streak is 3 days old (SRS 7.6). An answer from the feed ends the streak.
   - **Try again** forgets the streak and its back-off, so the next check is due at once, within the day's budget (`try_again`).
2. **The updater: `ImageUpdater::check` (`updater.rs`).** It holds the settings store, the image cache, the feed, a `Downloader`, and the network, wallpaper, scheduler and system adapters. Each call:
   1. Does nothing if the daily image is off or the wallpaper can't be set. If it is off, it cancels the job.
   2. If no check is due, schedules the job for when one is (`NotDue`).
   3. **Waits without counting an attempt** while `saves_battery()` is true, or while the network is metered and "Use mobile data" is off or `saves_data()` is true (`Waiting`). The job comes back no earlier than 15 minutes later, so MeeGone and the system never loop if they disagree.
   4. Saves the attempt, and schedules its retry, before any request. If the attempt can't be saved, nothing is requested.
   5. **Asks the feed** for today's image in the region in use:
      - the same id and region as the current image is `Same`;
      - an image not offered as a wallpaper is `NotOffered`, and counts as no new image;
      - anything else goes to `Downloader::download` (`New`).
   6. Records the outcome and schedules the next check.
   7. **If the daily image was turned off meanwhile,** it clears the cache, the log included, and cancels the job (WALL-12). The download can't be interrupted, but nothing of it stays.

   Offline is not a condition to wait for. It fails as "No connection" and counts (AC-16), as a scheduled job only starts with its network anyway. One check runs at a time.
3. **The log and the id live in the image cache.**
   - `ImageRecord` gains `id`, the feed's id; records saved before have none, so the next check downloads once.
   - `images/details` gains a `checks` line, read and written with `ImageCache::checks` and `update_checks`, under the cache's lock.
   - `put` keeps it, and `clear` deletes it with the images. It is the lock-screen image's own data (SRS 7.4's "last status"), so there are still three stores.
   - `IMAGE_CACHE_VERSION` is 3. Its migration gives records no id and adds an empty log, and `image-cache-v3.txt` is the new fixture.
4. **The adapters learn two conditions.**
   - `SystemConfigAdapter::saves_battery`: battery saver, or the battery's "low" flag.
   - `NetworkAdapter::saves_data`: Data Saver restricts MeeGone's background data.
   - Their fakes have `saves_battery` and `saves_data` fields. There are still seven adapters.
5. **The home and the job share one set of stores: `Shared` (`meegone-shell-state`).** It holds the platform, the settings store, the image cache, the feed and the updater, and it is `Send + Sync`.
   - `Shared::check_image` runs the updater and says whether it reached the feed.
   - `HomeShell::compose_shared` and `Launcher::start_shared` build the home on the same stores, so a setting changed in Settings counts for the next check, and turning the image off during a download is seen (decision 2.7). `compose` and `start` build their own `Shared`, as before.
   - `ShellIntent::ImageChecked` refreshes the lock-screen image after a check, so the card and Settings show what it did.
6. **The holder.**
   - **`SetEnabled(true)`** schedules the job for its next check, due at once the first time.
   - **`SetUseMobileData`** saves the setting, and schedules the waiting job with the new network at once (SET-03).
   - **`TryAgain`** is described in decision 1.
   - **`LockImageState`** gains `use_mobile_data` and `checks`.
   - **`ImageSettingsSection`** gains:
     - `use_mobile_data` and `use_mobile_data_toggled`;
     - `checks`, for #30's status line;
     - `daily_image_unavailable`, true while the image is on and its checks are `Unavailable`;
     - `try_again`.
7. **On Android.**
   - **`ImageJobService`, a `JobService`, is the one job (id 1).**
     - **The schedule.** `Bridge.scheduleImageJob(notBeforeMs, anyNetwork)` sets a minimum latency, `NETWORK_TYPE_UNMETERED` or `NETWORK_TYPE_ANY`, and `setRequiresBatteryNotLow`.
     - **Not persisted.** No `RECEIVE_BOOT_COMPLETED` is requested (SEC-03): as the home app, MeeGone starts after every boot anyway.
     - **A run.** It runs the check on one "MeeGone image" thread, calls `jobFinished(params, false)`, and tells the home with `HomeActivity.nativeImageChecked`.
     - **Stopped by the system.** `onStopJob` returns false: the check goes on to its end, and the updater plans its own retries.
     - **Scheduling during a run.** Scheduling a job with a running job's id stops it, so a schedule or cancel asked for while the job runs is kept, and carried out after `jobFinished`.
     - **Failure.** A negative answer from `JobScheduler` is `Unsupported`, with the words "Background work isn't available. The image is checked when MeeGone starts."
   - **At each start of MeeGone,** `ImageJobService.checkAtStart` runs the same check on the same thread (SRS 8.1). It does nothing unless a check is due, and schedules the job again, which a force stop drops.
   - **`nativeCheckImage`** runs on that thread. It builds the process's `Shared` in a `OnceLock` if the home hasn't, from `Bridge` and the files folder, so the job works without the home; `nativeStart` uses the same one.
   - **`AndroidScheduler`** (`android/scheduler.rs`) replaces the last stub, and `stubs.rs` is gone.
   - **The two conditions.** `Bridge.savesBattery` reads `PowerManager.isPowerSaveMode` and the sticky `ACTION_BATTERY_CHANGED`'s `EXTRA_BATTERY_LOW`, and answers false if it can't say. `Bridge.savesData` reads `getRestrictBackgroundStatus`, and answers true if it can't say.
   - **Settings.** `ImageSettings.java` draws "Use mobile data" below "Show card in Events". Above them, it draws "Daily image unavailable" with Try again, only while the frame says so. Slots 128 and 129 carry them, and `nativeUseMobileData` and `nativeImageTryAgain` report them.
   - **The manifest** declares the service with `BIND_JOB_SERVICE`, not exported.
8. **Left to later issues.** Setting the wallpaper from the record, and its time and marker (#29). Until then, the lock-screen image is unavailable on Android, so no job is scheduled there yet. "Update now" (#29), and the status line with when the image last updated (#30).

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 1 | `NetworkAdapter` gains `saves_data`, and `SystemConfigAdapter` gains `saves_battery`. There are still seven adapters. |
| 0005, decisions 3 and 7 | The launcher shares its stores with the job through one `Shared` per process, which `ImageJobService`'s thread may build first. Scheduling is built, and no adapter is a stub any more; unbuilt methods answer with their "if unavailable" value. |
| 0022, decision 1 | Each record keeps the feed's id, and `images/details` keeps the check log (`IMAGE_CACHE_VERSION` 3). `clear` deletes it too. |
| 0027, decision 1 | The updater is `today`'s caller: once per attempt, and never more than 6 times a day. It leaves an image not offered as a wallpaper. |
| 0028, decision 8 | #28's items are done: when to download, the same image, images not offered, retries and back-off, and caching nothing after the feature is turned off. |

## Departures from the SRS (MAINT-05)

None.

## Consequences

- **Benefits:**
  - **Tested without a device:**
    - **The policy:**
      - midnight in UTC+7, and on the day the clocks go forward;
      - same-image retries at 2 hours, at most three;
      - back-off of 15 to 240 minutes, and the cap of 6;
      - a new day's counts, an attempt cut short, and 3 days of failures;
      - Try again, within the budget and after it is spent.
    - **The updater:**
      - a new image, the same image, and one not offered as a wallpaper;
      - mobile data with "Use mobile data" off, with it on, and with Data Saver;
      - battery saver;
      - offline all day: 6 attempts, then the next day's midnight;
      - the attempt saved before the request;
      - turning the image off during a check;
      - the image off, a device that can't set it, no background scheduling, and storage that can't record the attempt.
    - **The cache:** the log is kept, migrated and cleared.
    - **The shell:** a check on another thread is seen once the home hears of it.
    - **The acceptance scenarios** (`tests/acceptance.rs`):
      - AC-16;
      - "Use mobile data" off with mobile data only;
      - 3 days of failures, then Try again;
      - a day in Bangkok: the image after local midnight, then four same-image checks the next day.
  - **Checked on a Galaxy S22 (Android 16, Bangkok, on AC power and Wi-Fi)** with a temporary debug build, not kept. It reported the lock screen as supported, turned the image on and off from adb, and logged each check. For one run it also took an image not offered as a wallpaper as offered, since none of the feed's 12 regions offered one that day.
    - **Battery saver on** (`cmd power set-mode 1`):
      - turning the image on scheduled the job at once, and the system ran it at once while MeeGone was on screen;
      - the check saw battery saver and waited: no request, and no attempt recorded;
      - the job came back with a 15-minute latency.
    - **Battery saver off, Data Saver on, on Wi-Fi:**
      - the bridge read Data Saver as on and the network as unmetered, and a forced run made its one request;
      - today's en-GB image wasn't offered as a wallpaper, so nothing was downloaded;
      - the attempt and the answer were recorded, and the job came back at 2 hours.
    - **The download path,** with the temporary bypass:
      - turning the image off and on ran the job at once;
      - the check took 1.2 s: the feed, the UHD download, and a 549 KB crop in `images/a`, its record carrying the feed's id;
      - the job came back for 00:00 Bangkok time, unmetered and battery-not-low.
    - **Early runs and the setting:**
      - a forced early run made no request and kept the time;
      - "Use mobile data", switched from adb and from the drawn switch, changed the job's network between unmetered and any at once, and saved the setting.
    - **Runs and restarts:**
      - every run ended "app called jobFinished", none stopped by a schedule made during it;
      - a force stop dropped the job, and the next start's check made no request and scheduled it again for midnight.
    - **Not checked on the device:**
      - a metered network and no connection, as switching networks would drop the wireless adb connection;
      - same-image retries and 3 days of failures, which need the clock to move.

      The tests cover them.
    - **Afterwards:** the test image was deleted, battery saver and Data Saver were off, and the clean build was installed. MeeGone started and drew as before, with no job scheduled.
- **Costs:**
  - **A wait can mean wake-ups.** While battery saver is on and MeeGone is on screen, the system may run the job every 15 minutes, only for it to wait. Off screen, battery saver defers the job itself.
  - **Try again doesn't go beyond the day's 6 attempts.** If they are spent, the check comes after midnight. "Update now" (#29) is the way to fetch at once.
  - **A download can't be stopped part-way.** After the feature is turned off, it finishes and is deleted.
  - **The first check after the update downloads once,** as older records have no id.
  - **Kept in step by hand:** `scheduleImageJob`, `cancelImageJob`, `savesBattery` and `savesData` between `Bridge.java` and the Rust adapters, and slots 128 and 129 between `src/android.rs` and `HomeViews.java`.
