# ADR 0030: Setting the lock-screen wallpaper

**Status:** Accepted · **Date:** 2026-11-19 · **Issue:** #29

## Context

Issue #29 puts the daily image where MeeGone shows photography: the system lock screen's wallpaper, and nowhere else (SRS C1, P3):
- **Lock screen only (WALL-01, SEC-05).** Only the lock screen's wallpaper changes. The home wallpaper, the home views and the system lock screen itself are untouched.
- **Only once confirmed (WALL-02).** Nothing changes on the lock screen before the user turns the feature on, in setup or in Settings.
- **Only a good image (WALL-08, WALL-10).**
  - The wallpaper is set only after the download, the checks and the crop have all succeeded. Any failure keeps the current wallpaper.
  - With no image yet, the lock screen stays as it is.
- **Update now (WALL-11).** Fetch the image at once, over any available connection.
- **Turning it off (WALL-12).** Downloads stop and the cached images go, but the wallpaper stays. Settings offers "Reset lock screen to home wallpaper" and a shortcut to the system's wallpaper settings.
- **Unavailable (WALL-13).** Where the device can't set wallpapers, the feature shows as unavailable, with the reason.
- **Overrides (SRS C5).** Detect what can be detected, and say when the image may not appear.
- **The record (SRS 7.4, 7.5).** Keep the time applied and an applied-wallpaper marker with the image.
- **Someone else's wallpaper (WALL-14, Should).** Where the platform can tell, pause daily updates when someone else changes the lock screen's wallpaper, and ask whether to resume.
- **Acceptance (AC-15, AC-17).**
  - A new image is on the lock screen after that day's checks.
  - Turning the feature off and resetting stops downloads, deletes the images, and shows the home wallpaper.
  - Leaving setup before Continue changes nothing.

What was there already:
- **The adapter.** `WallpaperAdapter` had `lock_screen_support`, `set_lock_screen`, `reset_lock_screen` and `open_wallpaper_settings`, and its fake (ADR 0003). On Android all four answered "Not available yet", so the lock-screen image reported itself unavailable and no job was scheduled (ADR 0028, ADR 0029).
- **The image.**
  - `Downloader::download` caches a checked crop as the current image (ADR 0028).
  - `ImageUpdater::check` decides when, on the job's thread (ADR 0029).
  - The record had `applied_at_ms`, which nothing wrote.
- **What ADR 0028 left here:**
  - setting the wallpaper from the record, and its time and marker;
  - how tablets and foldables crop. The crop was the default display's size in its natural orientation, which is portrait on a phone.
- **What ADR 0029 left here:** "Update now", the way past the day's 6 attempts.
- **Settings.** Until #34, Settings is drawn as far as each issue needs. The daily image's switch wasn't drawn, so on Android only adb could turn it on.

## Decision

1. **The updater sets the wallpaper, in the order of SRS 7.5:** cache the image, set it, record the time and the marker.
   - **`apply`.**
     - It reads the settings again first, so nothing is set once the image is off (WALL-02, WALL-12).
     - It reads the image back from the cache and calls `set_lock_screen`.
     - It records the result with the new `ImageCache::applied(file, at_ms, marker)`. That changes the current record only if it is still the one in that file.
   - **When it is set.**
     - A new image is set once `Downloader::download` has cached it.
     - **The same image, cached but not on the lock screen,** is set without downloading it again. This covers a set the system refused, or MeeGone stopping between caching and setting.
     - Today's image counts as in only once it is on the lock screen. A refused set is the attempt's failure, with its back-off, and the retry's feed answer finds the image cached (WALL-08, REL-04).
   - **`Checked::New`** now means cached and set.
   - **Nothing is set otherwise:** with no image (WALL-10), for an image not offered as a wallpaper, or after any failed step (WALL-08). There is no placeholder.
2. **The marker (WALL-14, SRS C5).**
   - **The adapter.** `set_lock_screen` returns the system's marker for the new wallpaper, a `WallpaperMarker`, or `None` where the platform can't tell wallpapers apart. `lock_screen_marker` says which wallpaper the lock screen has now, whoever set it.
   - **The record.** `ImageRecord` gains `marker: Option<Marker>`:
     - `Marker::Set(m)`: right after setting, the lock screen had `m`.
     - `Marker::NotKept`: it had another, so the device didn't keep the image, which may not appear (C5). This read-back is the one override check: a list of override apps would show only that they are installed, not that they are on. The S22 has Samsung's Dynamic Lock Screen installed, for example.
     - `None`: not set yet, or the platform can't tell.
   - **Paused (`changed_by_someone_else`).** The record is `Set(m)` and the lock screen now has another marker.
     - Nothing is stored for it: the record and the system say it together.
     - It is never paused where the platform can't tell, nor for an image marked `NotKept`.
   - **While paused,**
     - the automatic check returns the new `Checked::Paused`, asks nothing, and cancels the job;
     - Settings says "The lock screen's wallpaper was changed. Daily images are paused." with Resume.
   - **Resume** is "Update now" (decision 3), which puts the image back first.
   - **The format.**
     - `IMAGE_CACHE_VERSION` is 4: each record gains a last field, a number, `not-kept`, or `-`.
     - The migration gives older records none, and `image-cache-v4.txt` is the new fixture.
     - A marker is read only as written, so `+1` or `01` doesn't load.
3. **"Update now" (WALL-11).**
   - **`ImageUpdater::update_now`** shares `check`'s run, with three differences:
     - It first puts the current image back if it isn't on the lock screen, without the network, so Resume works offline.
     - It skips the time it is due, the wait for the battery and an unmetered network, and the day's 6 attempts.
     - It still counts its attempt, so the automatic checks that day use what is left.
   - **`LockImageIntent::UpdateNow`.**
     - It is offered only while the daily image is on, and runs one at a time.
     - It sets `updating` and asks the new `SchedulerAdapter::update_image_now`, since the update blocks and the holder runs on the main thread.
     - `ShellIntent::ImageChecked` now sends `LockImageIntent::Checked`, which reads the image again and ends `updating`. Other reads, such as `Resumed`, don't end it.
   - **`Shared::update_image_now`** runs it on the job's thread.
4. **Off, unavailable and the reset (WALL-12, WALL-13).**
   - **Turning it off** cancels the job and clears the cache, as before. The lock screen keeps its wallpaper.
   - **While off,** Settings offers:
     - "Reset lock screen to home wallpaper" (`ResetLockScreen`, which does nothing while the image is on, as a reset would look like someone else's change);
     - "Wallpaper settings" (`OpenWallpaperSettings`).
   - **The reason it is unavailable** is `lock_screen_support`'s, shown below the disabled switch, as before but now drawn.
5. **Tablets and foldables crop to a square (ADR 0028's open question).**
   - **The adapter.** `lock_screen_size` becomes `screen()`, the default display's natural size and density, and `folds()`, whether the device has a hinge.
   - **The decision is the pure `lock_screen_target(screen, folds)`:**
     - a phone keeps its upright crop;
     - a screen 600 dp or more across (`LARGE_SCREEN_DP`, as HOME-12 lets turn), or a foldable, gets a square as wide as the screen's longest side.
     - The system's own centre crop then fills the lock screen either way up, and on a foldable's other screen.
     - `plan_crop` still never scales up, so the feed's UHD image gives 2160 × 2160 on a tablet.
   - **Why only the default display.** A monitor never shows the lock screen, and would make a phone look like a tablet. A foldable shows one screen at a time as its default display, so the hinge stands for the other.
6. **On Android.**
   - **The bridge.**
     - `lockScreenUnsupported` uses `WallpaperManager.isWallpaperSupported` and `isSetWallpaperAllowed`, and says which is false.
     - `setLockScreen(jpeg)`:
       - calls `setStream(…, null, false, FLAG_LOCK)`: no crop hint, no backup, the lock screen only;
       - refuses the main thread;
       - takes `setStream`'s 0 as a refusal, as the system answers 0 when it wrote nothing.
     - `lockScreenWallpaperId` is `getWallpaperId(FLAG_LOCK)`, which is negative while the lock screen shows the home wallpaper.
     - `resetLockScreen` is `clear(FLAG_LOCK)`.
     - `openWallpaperSettings` tries the system Settings' `android.settings.WALLPAPER_SETTINGS`, then `ACTION_SET_WALLPAPER`, then answers with written instructions (SRS 8.1).
     - `screen` reads the display's mode and density, and `folds` reads `FEATURE_SENSOR_HINGE_ANGLE`.
     - `updateImageNow` hands the update to `ImageJobService.updateNow`. That runs `nativeUpdateImageNow` on the "MeeGone image" thread without `JobScheduler`, so it works where background scheduling doesn't, and always tells the home afterwards.
   - **The manifest** asks for `SET_WALLPAPER`, granted at install, which SRS 8.1 lists (SEC-03).
   - **Settings.** `ImageSettings.java` draws:
     - the "Daily lock-screen image" switch, with the reason below it while it is disabled;
     - the paused message with Resume;
     - "This device may not show the image on its lock screen";
     - "Update now", which reads "Updating…" while it runs;
     - while off, the reset and "Wallpaper settings". A reset that worked says "The lock screen shows the home wallpaper".
     - Frame slots 130 to 134 carry them. `nativeDailyImage`, `nativeImageUpdateNow`, `nativeResetLockScreen`, `nativeOpenWallpaperSettings` and `nativeImageUnavailableReason` report them.
   - **Available.** The lock-screen image is no longer unavailable on Android, so the switch works, the job is scheduled while the image is on, and the card in Events shows.
7. **Setup (WALL-02).** First-run setup is #35. Its lock-screen image step will send `SetEnabled(true)` on Continue. Until then, the daily image is off unless the user turns it on, and while it is off nothing is checked, fetched or set, even by "Update now" or MeeGone's start.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 1 | `WallpaperAdapter`'s `lock_screen_size` becomes `screen` and `folds`; `set_lock_screen` returns a marker, and `lock_screen_marker` is new. `SchedulerAdapter` gains `update_image_now`. There are still seven adapters. |
| 0005, decision 7 | The wallpaper adapter is built whole. Opening links (#30) is the one method that still answers "Not available yet". |
| 0022, decision 1 | Each record keeps its marker (`IMAGE_CACHE_VERSION` 4), and `ImageCache::applied` records when it was set. |
| 0028, decisions 1 and 8 | The download crops to `lock_screen_target(screen, folds)`: a square on screens that turn and on foldables. #29's items are done. |
| 0029, decisions 2, 6 and 8 | The updater sets the image: `Checked::New` means set, and `Checked::Paused` is new. "Update now" is built, and `ImageChecked` ends it. |

## Departures from the SRS (MAINT-05)

None.

## Consequences

- **Benefits:**
  - **Tested without a device:**
    - **The cache:** the marker kept, migrated and saved in every form, and `applied` only for the image still current.
    - **The crop:** phones, tablets in either orientation, exactly 600 dp, a foldable's two screens, and an unknown screen.
    - **The updater:**
      - a new image set once cached, with its time and marker;
      - nothing set after a failed download, crop or save, nor with no image;
      - a refused set retried without a download;
      - the image on the lock screen not set again;
      - turning it off during a check;
      - someone else's wallpaper pausing the checks, and Resume putting the image back offline;
      - "Update now" past the time, the battery, the network and the budget, and while off;
      - an image the device didn't keep, and a platform that can't tell.
    - **The holder:** "Update now" one at a time until `Checked`; paused and may-not-show; the reset only while off; and their failures.
    - **The shell:** "Update now" on another thread, heard by the home.
    - **The acceptance scenarios** (`tests/acceptance.rs`):
      - AC-15 and AC-17;
      - WALL-02 (never confirmed, as when setup is left before Continue);
      - WALL-10, WALL-11, WALL-13 and WALL-14;
      - wall03 and wall04, which now also see the image set.
  - **Checked on a Galaxy S22 (One UI, Android 16, Bangkok, Wi-Fi)** with the debug build. The lock screen started with no wallpaper of its own (lock id −1), showing home wallpaper 967.
    - **Turning it on** from the drawn switch: the device said wallpapers can be set, so the switch was enabled.
      - The check ran at once, and today's en-GB image wasn't offered as a wallpaper.
      - None of the feed's 12 regions offered it that day (only en-AU and en-NZ did), so nothing was downloaded or set. The attempt was recorded and the job came back at 2 hours.
    - **The download path,** with a temporary build that took the image as offered, as ADR 0029's check did:
      - "Update now" read "Updating…" and came back.
      - It made one feed request and one UHD download, and cached a 997 × 2160 crop (549 KB).
      - The lock screen got wallpaper 968, the marker recorded with its time.
      - The home wallpaper kept 967, now for the home screen only.
    - **Paused:**
      - The record's marker was changed to one the lock screen didn't have, as when someone else sets a wallpaper. Settings showed the message with Resume.
      - A forced job run asked nothing, recorded no attempt and left no job.
      - Resume set the image again (lock 969, recorded), made one feed request, and scheduled the job for local midnight.
    - **Off and reset:**
      - Turning it off deleted the images and cancelled the job. The lock screen kept 969.
      - The reset showed its message, and the lock screen had no wallpaper of its own again, showing the home wallpaper. One UI renumbered that home wallpaper 971 as it took the lock screen back; its record was the same.
    - **Wallpaper settings:**
      - One UI's `ACTION_SET_WALLPAPER` offers only a chooser of apps (Live wallpapers, Microsoft Launcher, Photos).
      - `android.settings.WALLPAPER_SETTINGS` opens Settings' own "Choose wallpaper from", which the shortcut now tries first. Back returned to MeeGone.
    - **Not checked on the device:**
      - another app changing the lock screen, as that would change the owner's wallpapers or need their photos;
      - an image the device doesn't keep;
      - a policy that forbids wallpapers;
      - tablets and foldables;
      - the lock screen itself, as the phone has a secure lock. `dumpsys wallpaper` showed each change instead.

      The tests cover them.
    - **Afterwards:** the image was off, the lock screen reset, and the clean build installed. `clear` leaves an empty `images` folder, as before.
- **Costs:**
  - **Overrides a wallpaper can't see.** Lock-screen services that draw over the wallpaper, such as Samsung's Dynamic Lock Screen or Glance, keep the system's record as MeeGone set it. The read-back can't tell, and the image may not show (C5).
  - **A pause is noticed when MeeGone looks:** at `Resumed`, in Settings, and at each job run. A run always looks before it fetches, so it never sets an image over someone else's wallpaper.
  - **"Update now" spends the day's attempts.** After it, fewer automatic checks may be left that day.
  - **A refused set shows in Events first.** The card shows the cached image, while the lock screen keeps the previous one until the retry, 15 minutes later.
  - **The shortcut's first try isn't a public constant.** `android.settings.WALLPAPER_SETTINGS` is the system Settings' own action, so `ACTION_SET_WALLPAPER` stays as the fallback.
  - **A square crop is larger:** about 1.5 MB on a tablet, within PERF-08's budget.
  - **Kept in step by hand:**
    - `lockScreenUnsupported`, `screen`, `folds`, `setLockScreen`, `lockScreenWallpaperId`, `resetLockScreen`, `openWallpaperSettings` and `updateImageNow` between `Bridge.java` and the Rust adapters;
    - `nativeUpdateImageNow` with `ImageJobService.java`;
    - slots 130 to 134 between `src/android.rs` and `HomeViews.java`.
