# ADR 0028: Downloading, checking, cropping and caching the daily image

**Status:** Accepted · **Date:** 2026-11-19 · **Issue:** #27

## Context

Issue #27 turns the feed's image into a safe, correctly framed lock-screen image:
- **Picking and cropping (WALL-06).** Pick a portrait version when the feed offers one at least as large as the screen, otherwise the largest. Centre-crop it to the lock screen's proportions.
- **Safety (WALL-07, SEC-02).**
  - Only HTTPS, and no download over 20 MB.
  - Only content that decodes as an image at least 720 px on its short side, with bounded decoding. Anything else is discarded.
- **Handing it on (WALL-08).** An image goes on to become the wallpaper only after download, validation and cropping have all succeeded.
- **Caching (WALL-09).** The current and previous images are kept with their details, and older ones deleted.
- **Low storage (SRS 7.6).** Skip the download and report the status.
- **Pure logic.** Selection and validation are unit-tested without a device.
- **Storage budget (PERF-08, Should).** Stored data, image cache included, stays at or below 50 MB.
- **Acceptance.**
  - A 25 MB download is rejected.
  - An image 600 px on its short side, or content that isn't an image, is discarded, and the current wallpaper is kept.
  - A third day's image deletes the oldest.

What was there already:
- **The feed.** `BingFeed::today` gives a `FeedImage` with three `versions`: UHD (3840×2160), 1920×1080 and 1080×1920, each on `https://www.bing.com` (ADR 0027).
- **The network.** `NetworkAdapter::get(url, max_bytes)` requests only `https://`, follows only HTTPS redirects, refuses a body over `max_bytes`, and blocks (ADR 0027).
- **The cache.** `ImageCache::put(record, image)` keeps two images, in `images/a` and `images/b`. Each new image goes in the file the current one doesn't use, so the oldest is overwritten (ADR 0022).
- **The lock screen's size.** `WallpaperAdapter::lock_screen_size`, "the size the lock-screen image is cropped to", which nothing asked. On Android the whole wallpaper adapter was a stub until #29.
- **The budget.** `FileStorage` keeps every write within 50 MB in total, and reports a full disk as "Not enough storage space" (ADR 0004).
- **Free storage.** Nothing could tell how much there was.

The constraints:
- **Only the standard library** (DEPENDENCIES.md). Rust can't decode a JPEG image; Java can.
- **Decisions belong in Rust** (ADR 0006), and are tested without a device (MAINT-01).
- **Seven adapters, as ARCH-04 names them** (ADR 0003).
- **No caller yet.** The updater that decides when to download arrives with #28, and #29 sets the wallpaper.

## Decision

1. **`Downloader::download(image, region)`** (`download.rs` in `meegone-lockimage-data`) is how an image enters the cache. It holds the network, wallpaper and system adapters and the cache, and runs these steps in order:
   1. Read the lock screen's size. If it is unknown, it fails without a request.
   2. Skip the download when storage is low (decision 5).
   3. Pick a version (decision 2).
   4. Download it with a 20 MB limit (`MAX_DOWNLOAD_BYTES`). A larger body is refused unread, as "Too large".
   5. Check the image (decision 3).
   6. Plan the crop (decision 4), and have the platform carry it out (decision 6).
   7. Check what comes back: a JPEG of exactly the planned size.
   8. Cache it with the image's date, region, title, attribution and info link, as the current image (WALL-09).

   It returns the record as cached, and only then is the image handed on (WALL-08):
   - Any failure leaves the cache as it was, so the current image, and the wallpaper set from it, stay.
   - It makes at most one request and never retries; the updater keeps to the call budget (REL-04).
   - It blocks, so it runs on a JVM thread other than the main one, as `get` does.
   - `ImageCache::put` now returns the record it cached, naming its file, so the record handed on names the image #29 sets.
   - The cache keeps the crop, never the download: about 0.5 MB for the S22 rather than 3.7 MB. Two crops stay well inside PERF-08's 50 MB, which `FileStorage` still enforces on every write.
2. **Picking a version (`pick_version`, WALL-06).**
   - Only `https://` versions count (WALL-07).
   - If any portrait version is at least as wide and as tall as the lock screen, take the smallest such, which downloads the least (PWR-03).
   - Otherwise take the largest by pixel count. Of two equally large, take the one that keeps more pixels once cropped to the screen's proportions. That is the portrait version on a phone.
   - From the feed's three versions, this gives:
     - UHD on the S22 (1080×2340), whose screen is taller than the portrait version, and on a landscape tablet;
     - the portrait version on a 1080×1920 or 720×1280 screen.
3. **Checking the image (`check_image` and `jpeg.rs`; WALL-07, SEC-02).** A small, strict JPEG header reader checks the file's shape and reads its size, without decoding a pixel:
   - It must start with SOI and end with EOI.
   - Segment lengths must stay inside the file, and only fill bytes may sit between segments. No scan may come before the frame header.
   - The frame must be baseline, extended or progressive, Huffman-coded, with 8-bit samples, a width and a height, and 1 or 3 components, each with sampling factors from 1 to 4. Lossless, arithmetic-coded, hierarchical, 12-bit and CMYK files are refused, as is a height left for later (DNL).
   - The image must be at least 720 px on its short side and at most 8192 px on either side, so decoding stays bounded.
   - Anything else is discarded as "The image couldn't be read", "The image is too small" or "The image is too large". The reader never panics.
4. **Planning the crop (`plan_crop`; WALL-06, SEC-02).**
   - **The region** is the image's middle in the lock screen's proportions, as large as fits, rounded to the nearest pixel and never outside the image.
   - **The result** is the lock screen's size, or the region's if that is smaller. It is never scaled up, since the system scales the wallpaper to the screen anyway. The feed's UHD image gives 997×2160 on the S22.
   - **Decoding** happens at 1/`sample` of the region's size. `sample` is the largest power of two that still gives the result's size or more, so the decoded picture is less than twice the result on each side.
5. **Low storage (SRS 7.6).**
   - Before any request, `SystemConfigAdapter::free_storage_bytes` is asked how much more MeeGone can store before the device runs low.
   - Under one maximum download, 20 MB, nothing is requested, and the download fails with `Failed("Not enough storage space")`: the same words a full disk gives.
   - If the system can't say, the download goes ahead, and a full disk still fails the write.
6. **The platform crops: `WallpaperAdapter::crop_image(jpeg, crop)`.**
   - It belongs to the wallpaper adapter, which already gave the size the lock-screen image is cropped to, so there are still seven adapters. It decodes only `crop`'s region, at 1/`sample`, scales it to the result's size, and writes a new JPEG without the original's metadata.
   - `ImageCrop` (`meegone-core-data`) carries the image's size as its header gives it, the region, `sample` and the result's size.
   - An image that doesn't decode, or not at its header's size, is `Invalid`. The downloader reports it as "The image couldn't be read". Other failures, such as running out of memory, are passed on as they are.
   - `FakeWallpaper` records every crop asked for and returns a header-only JPEG of the planned size (`fakes::jpeg`). `cropped` overrides what it returns, and `error` makes it fail.
7. **On Android.**
   - **`AndroidWallpaper`** (`android/wallpaper.rs`) replaces the stub.
     - `lock_screen_size` and `crop_image` are built.
     - `lock_screen_support`, `set_lock_screen`, `reset_lock_screen` and `open_wallpaper_settings` stay "Not available yet" until #29. The lock-screen image therefore still reports itself unavailable, and nothing changes on screen.
   - **`Bridge.lockScreenSize()`** gives the default display's `Display.Mode` physical size. That is its natural orientation, so turning the device doesn't change it.
   - **`Bridge.cropImage(jpeg, width, height, left, top, right, bottom, sample, outWidth, outHeight)`:**
     - It runs `BitmapRegionDecoder` on the bytes, requires the decoder's size to equal the header's, and calls `decodeRegion` with `inSampleSize = sample`.
     - It then calls `createScaledBitmap` to the result's size and `compress` to JPEG at quality 90.
     - It recycles every bitmap, and catches `IOException`, `RuntimeException` and `OutOfMemoryError`, answering null. Logs hold only the kind of failure.
     - On the main thread it answers null at once, as `httpsGet` does.
   - **`Bridge.freeStorageBytes()`** uses `StorageManager.getAllocatableBytes` for the files folder. That counts cache the system would clear, and leaves out the space it keeps for itself when storage runs low. It falls back to `getUsableSpace`, and answers -1 if neither works.
   - **The JNI binding** gains:
     - `new_bytes` (`NewByteArray`, slot 176, and `SetByteArrayRegion`, slot 208), checked against NDK 27.3's `jni.h`;
     - `call_ints`, for an `int[]` that Java returns.
8. **Left to later issues.**
   - **#28, the updater:**
     - deciding when to download;
     - telling the same image apart (WALL-03), as the record keeps no feed id yet;
     - leaving out an image not offered as a wallpaper (ADR 0027);
     - retries and back-off;
     - making sure a download in progress caches nothing after the user turns the feature off (WALL-12).
   - **#29:** setting the wallpaper from the record, and its time and marker. Also how tablets and foldables crop.
   - **#30:** showing the status words.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 1 | `WallpaperAdapter` gains `crop_image`, and `SystemConfigAdapter` gains `free_storage_bytes`. There are still seven adapters. |
| 0005, decision 7 | The wallpaper adapter is built for the lock screen's size and cropping. Setting the wallpaper is still a stub until #29, and scheduling until #28. |
| 0022, decision 1 | `ImageCache::put` returns the record as cached. The cache holds the cropped image, never the download. |
| 0027, decision 1 | #27 picks one of `versions` and downloads it with a 20 MB limit. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| WALL-07: content that decodes as an image | Only JPEG is accepted: baseline, extended or progressive, 8-bit, greyscale or colour. The feed serves nothing else, and this keeps Android's other image decoders away from downloaded bytes. |

## Consequences

- **Benefits:**
  - **Tested without a device:**
    - **Picking:** phones, tablets, ties and plain-HTTP versions.
    - **The JPEG reader:**
      - four real files made from plain colours: baseline, progressive, greyscale, and one with Exif, IPTC and a restart interval;
      - every prefix and every damaged byte of those files;
      - and 39 files it refuses.
    - **The checks:** both size limits on each side.
    - **The crop plan:** worked examples, and a sweep of 121 image sizes on 7 screens. Every region stays inside its image, and every decode covers the result at less than twice its size.
    - **The downloader through each step**, with fakes:
      - the issue's three acceptance criteria;
      - an image that doesn't decode, and a wrong crop;
      - plain-HTTP versions only, offline, low storage, unknown storage, an unknown screen size, and a failed save.
  - **Checked on a Galaxy S22 (Android 16)** with a temporary build that ran the feed and the downloader on a background thread. The build was not kept.
    - **The device's answers:**
      - The lock screen is 1080×2340.
      - Free storage was 23.2 GB, against 23.7 GB available by `df`: the system's reserve is left out.
    - **Today's en-GB image** (not offered as a wallpaper, which #28 will skip):
      - UHD was picked, and 3.65 MB downloaded in 178 ms.
      - The 997×2160 crop, at sample 1, took 96 ms and gave a 549 KB JPEG with no metadata. It is the middle of the image.
      - A whole `download` took 0.3 to 0.4 s.
    - **Three downloads** went to `a`, `b` and `a`, and only `details`, `a` and `b` were kept.
    - **Refused, with the cache unchanged:**
      - the feed's JSON offered as an image ("The image couldn't be read");
      - Bing's 640×360 version ("The image is too small");
      - a JPEG header with nothing to decode (the decoder threw `IOException`).
    - **The four fixtures** cropped correctly on Android, progressive included.
    - **The main thread:** a crop and a request there were both refused without any work.
    - **Afterwards:** the images were cleared and the clean build installed. MeeGone started and drew as before.
  - **What later issues get:** #28 gets one call that downloads and caches an image; #29 gets a record naming the image to set; #30 gets the status words.
- **Costs:**
  - **Memory:** a download crosses JNI twice, to Rust and back to Java. A 20 MB image therefore holds about 60 MB for a moment on the job thread. The feed's UHD image is about 4 MB.
  - **Damaged pixel data:** after a good header it still decodes, with grey where it can't be read, as in any viewer. Only a decoder could tell, and the one decoder is the platform's.
  - **Kept in step by hand:** `cropImage` and `lockScreenSize` between `Bridge.java` and `wallpaper.rs`, and `freeStorageBytes` with `system.rs`.
  - **The low-storage threshold** is MeeGone's own: one maximum download beyond the system's reserve.
  - **The crop's orientation:** the lock screen's size is the display's in its natural orientation, which is portrait on phones. Tablets turn and foldables change screens, so their right crop is #29's to decide.
