# ADR 0042: Security and privacy

**Status:** Accepted · **Date:** 2026-11-23 · **Issue:** #41

## Context

Issue #41 (Must) asks MeeGone to respect the user: minimal permissions, no tracking, one network destination, nothing sensitive stored (SEC-01 to SEC-06, PRIV-01 to PRIV-03, PRIV-05). Its acceptance criteria:
- the release build's declared permissions match the allowed list exactly, checked in CI;
- traffic captured over a day of use goes only to the image feed's hosts;
- only the home entry, the settings entry and the notification listener are exported.

Most of it was already true:
- **SEC-01:** the network security config refuses cleartext and trusts only the system's certificates, with no exception for a domain or a debug build. `https::fetch` requests only `https://` (ADR 0027).
- **SEC-02:**
  - The feed's JSON is read strictly, at most 64 KiB (ADR 0027).
  - Images are at most 20 MB, and their JPEG headers are checked before the platform decodes them, at most 8192 px a side (ADR 0028).
  - Text is plain. Links open only over HTTPS, in the browser (ADR 0031).
- **SEC-03:** the manifest asks for five permissions. The rest of SRS 8.1 needs none:
  - the home-app role is granted in the system's dialog;
  - `LauncherApps` and `<queries>` list the apps, so `QUERY_ALL_PACKAGES` isn't needed;
  - the system binds the notification listener;
  - the daily job isn't persisted, so `RECEIVE_BOOT_COMPLETED` isn't needed, and MeeGone schedules it again at each start;
  - haptics use only `performHapticFeedback`, so `VIBRATE` isn't needed.
- **SEC-04:**
  - Two components are exported: `HomeActivity` and `NotificationListener`. `HomeActivity` carries the home entry, the app's own entry and the system's settings entry.
  - `HomeActivity` reads only an intent's action, never its extras.
  - From Android 13 the runtime receivers are registered `RECEIVER_NOT_EXPORTED`. Before, they listen only to system broadcasts other apps can't send: the work profile's, the locale's, the clock's and the next alarm's.
  - MeeGone creates no `PendingIntent`.
- **SEC-05:** MeeGone sets only the lock screen's wallpaper (ADR 0030).
- **SEC-06:** fakes are turned on only in dev-dependencies (ADR 0003). The release source set has a do-nothing `DebugHooks`, so the audit, the tour, the localisation check and the probe are only in the debug and benchmark builds.
- **PRIV-01, PRIV-02:**
  - Requests carry a fixed `User-Agent: MeeGone`, which hides the device's model, and no cookies or cache (ADR 0027).
  - Every URL the feed's client builds starts with `https://www.bing.com`.
- **PRIV-03:** `StoredFile` names every file. Notification content and usage history have none.
- **PRIV-05:** every file is in the app's private storage. The backup's copy is a recorded departure (ADR 0018).

An audit of the Java host, the Rust adapters and CI found these gaps:
- **Redirects:** `https::fetch` followed them to any `https://` host, so a response could send a request elsewhere (PRIV-02).
- **Links without a default browser:** with no default browser set, a link went out as a plain view intent. An app that claims the site's links could take it instead of a browser (SEC-02).
- **Progressive JPEGs:** one may be 8192 px square. Its decoder keeps every coefficient of the whole image, 2 to 6 bytes a pixel: up to 400 MB in the home's own process (SEC-02).
- **Logs:**
  - Eight logs passed the whole exception. Its message and stack trace can name packages.
  - A Rust panic's message became the Java exception's message, and std's own panic messages can quote the string being handled, such as a notification's text. That exception's message is logged, and a crash would carry it into the crash report (PRIV-03).
- **The wallpaper after an uninstall:** the system keeps the lock screen's wallpaper after an uninstall or "Clear storage", yet the privacy notice said they remove everything MeeGone stored (PRIV-05).
- **Checks:**
  - Nothing checked the manifest, the network config or the built APK. CI built the release APK and only measured it.
  - Nothing checked where traffic goes.

## Decision

1. **Redirects stay on the request's origin (PRIV-02).**
   - `https::fetch` notes the first URL's origin: `https://` and everything before the path, query or fragment, compared without regard to case.
   - A redirect to any other origin fails as `Failed(DownloadFailed)` without being requested. User information (`www.bing.com@example.com`) and another port count as other origins.
   - The feed serves every image without redirects (ADR 0027, ADR 0032), so nothing it does changes.
2. **Links open only in a browser (SEC-02).**
   - `Bridge.openInBrowser` still sends a link to the default browser first.
   - Where none is set, or it refuses, the view intent gets a selector for `ACTION_MAIN` with `CATEGORY_APP_BROWSER`, so only apps in the browser category can answer, and the user picks one of them.
3. **A progressive image may have at most 16 million pixels (SEC-02).**
   - `jpeg::read_frame` also says whether the frame is progressive.
   - `check_image` refuses a progressive image over `MAX_PROGRESSIVE_PIXELS` as `ImageTooLarge`. That is under 100 MB to decode, and about twice the feed's largest image, 3840 × 2160.
   - Baseline images keep the 8192 px limit, as their decoder holds only the rows it keeps.
4. **Logs name only the kind of failure (PRIV-03).**
   - The Java host logs an exception's class, never the exception itself.
   - `src/panics.rs` replaces the panic hook, once, with one that keeps only the panic's file and line. `guarded` throws that, never the panic's message.
   - The default hook's printout is dropped too, as it carries the same message.
5. **The privacy notice names the wallpaper (PRIV-05, PRIV-04).**
   - About's notice, and the listing word for word, end: "The lock screen keeps the last daily image as its wallpaper until you change it, or reset it in Settings first."
6. **Security rules in arch-check (`tools/arch-check/src/security.rs`).** `tests/security.rs` runs them on the sources in CI's check job.
   - **`Manifest`** has two readers that give the same shape: one for a source manifest, and one for the release APK's, as `aapt2 dump xmltree` prints it. Components are named by their simple class name.
   - **Permissions (SEC-03):** the app asks for exactly `PERMISSIONS_ALLOWED`, each with the row of SRS 8.1 it serves. `uses-permission-sdk-23` counts too.
   - **Exported components (SEC-04):**
     - Exactly the components of `EXPORTED_ALLOWED` are reachable, each with its guarding permission and each of its intent filters. A component with a filter and no `android:exported` counts as reachable, as on Android.
     - A new filter on `HomeActivity` is a new way in, so it fails.
     - In the source, every component must say whether it is exported.
   - **Roles and the lock screen (SEC-03, SEC-05):** no component is guarded by `BIND_ACCESSIBILITY_SERVICE` or `BIND_DEVICE_ADMIN`. None sets `showWhenLocked` or `turnScreenOn`.
   - **Cleartext and debug builds (SEC-01, SEC-06):**
     - The application names its network security config and doesn't allow cleartext.
     - It isn't debuggable, test-only or profileable from the shell.
     - The config refuses cleartext, trusts exactly the system's certificates, and has no `debug-overrides` or `domain-config`.
   - **The shipped Java** (`main` and `release`):
     - It uses none of `FORBIDDEN`:
       - web views, HTML and links in text (SEC-02);
       - showing over, waking or dismissing the lock screen (SEC-05);
       - device and SIM identifiers (PRIV-01);
       - stack traces in logcat (PRIV-03).
     - Only the files in `NETWORK_ALLOWED` may connect, and today that is only `Bridge.java` (PRIV-02). Rust can't reach the network except through it (ARCH-04).
     - No `Log` call passes a third argument, an exception (PRIV-03).
   - **Shipped XML:** no `android:autoLink` (SEC-02).
   - **The allow-lists stay true:** an entry of `PERMISSIONS_ALLOWED`, `EXPORTED_ALLOWED` or `NETWORK_ALLOWED` that matches nothing fails.
7. **The release APK is checked in CI (SEC-03, SEC-04, SEC-06).**
   - `tests/release_apk.rs` reads `app-release-unsigned.apk` with the SDK's `aapt2` and `dexdump`, from the newest build tools. It runs the manifest rules on the APK's own manifest.
   - It also fails on any class that only the debug build's source sets define. Today those are `A11yAudit`, `A11yTour`, `L10nCheck` and `PerfProbe`, listed from the source tree.
   - It needs the APK, so it is ignored by default. The Android job runs it after the build, with `--ignored`.
   - This is what holds the issue's first and third criteria: the built release, not only its source, asks for exactly SRS 8.1's access and exports exactly those entries.
8. **A day's requests are checked on the host (PRIV-02).** An acceptance test runs a day with the fakes:
   - an offline morning with its retries;
   - the image set;
   - the details and More info;
   - the past week, a pin and an unpin;
   - Update now, and the job until midnight.

   Every request goes to `https://www.bing.com/`, and More info's link goes to the browser, not the network.
9. **A day's traffic is checked on a device (PRIV-02, SEC-01): `tools/net-audit.sh`.**
   - **Capturing:** PCAPdroid, a no-root local VPN, captures the release build's traffic alone. It is driven over adb through its control activity, with `--start` and `--stop`.
   - **Checking:** tshark reads the capture. Each TCP and UDP stream is named by its TLS server name or DNS query, and its server is the end with the lower port.
   - **What passes:** only `www.bing.com` over TCP 443, or a DNS query for it. It also fails on another name, a stream with no name, plain HTTP, QUIC, or an empty capture.
   - **Recording:** `--record` refuses a capture under 24 hours. It writes the table to `docs/privacy/<version>-<model>.md`, the way `tools/perf-bench.sh` records benchmarks.
   - **Elsewhere:** RELEASING.md runs it before each release. `--check FILE` reads a capture made another way.
10. **What CI doesn't check needs a device or a person.** RELEASING.md adds the capture and the link without a default browser.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0005 | A panic reaches logcat as its file and line, not its message. |
| 0027 | `https::fetch` follows redirects only on the first URL's origin, still at most 3 and only to `https://`. |
| 0028 | A progressive image may also have at most 16 million pixels. |
| 0031 | Without a default browser, a link goes only to an app in the browser category. |
| 0037 | The privacy notice also says the lock screen keeps the last image. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| PRIV-05: uninstalling MeeGone or clearing its data removes everything it stored | MeeGone's own files still go. The lock screen's wallpaper is the system's: it keeps the last daily image until the user changes it, and no app hears its own uninstall to reset it first. The privacy notice says so, and Settings' "Reset lock screen to home wallpaper" puts the home wallpaper back beforehand (WALL-12). |

## Consequences

- **Benefits:**
  - **Every criterion has a check:**
    - CI holds the release APK's permissions and exported components to their lists;
    - a host test holds a day's requests to the feed's origin;
    - the device tool holds a day's real traffic to its host.
  - **The checks fail when broken on purpose.** Each was broken, seen to fail with its requirement, then put back:
    - adding `CAMERA`;
    - exporting `ImageJobService`;
    - a `VIEW` filter on `HomeActivity`;
    - `debug-overrides` in the network config;
    - logging an exception;
    - a `WebView`;
    - `openConnection` outside the bridge;
    - removing the redirect's origin check.

    The benchmark APK, put in the release APK's place, failed on its profileability and its four debug classes.
  - **A response can no longer send MeeGone elsewhere,** and a feed link can no longer reach a non-browser app.
- **Costs:**
  - **The Android job** compiles arch-check, a few seconds.
  - **Before a release,** a day-long capture needs a person, PCAPdroid on the device and Wireshark's tshark.
  - **A feed that moved its images** behind a redirect to another host would stop the daily image. Changing that is a change to `bing.rs` and to this ADR.
  - **Logs and crash reports** lose a panic's message.
- **Not checked yet:**
  - **`tools/net-audit.sh` hasn't run on a device.** Its PCAPdroid steps come from PCAPdroid's documentation (`CaptureCtrl`, `pcap_file` written to `Download/PCAPdroid/`, `app_filter`, `api_key`). The analysis was tried on made-up streams. The first day on the S22 settles both, and its record is committed.
  - **On a device without a default browser,** the browser-only selector hasn't been tried.
  - **The progressive limit's memory** hasn't been measured on a device.
  - **The debug and benchmark builds' hooks** read intent extras without checking who sent them, so any app could start the audit's tour or the probe. Neither build is released, and the APK check keeps their classes out of the release.
  - **`Log.w(TAG, e)`,** with the exception as the second argument, gets past the scan, which reads code, not types.
