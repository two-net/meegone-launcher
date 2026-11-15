# ADR 0005: The Android host and the home-app role

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #4

## Context

Issue #4 makes MeeGone a home app (HOME-01). It asks for the home-app role with the system dialog, from setup and from Settings → Access. While MeeGone isn't the default, a dismissible banner offers to make it the default, at most once a day (ONB-04). Without the role, MeeGone still runs as a normal app, but menus show no app shortcuts, because Android offers them only to the default home app.

Until now the workspace built no Android app. The constraints:

- **Android needs an app around the Rust code.** Being a home app takes a manifest entry on an activity. The role dialog needs `startActivityForResult`. Later features need Java classes Android instantiates itself (a notification listener, a job service).
- **The app's crates use only the standard library** (DEPENDENCIES.md), and only platform adapters may reach the OS (ARCH-04).
- **No UI toolkit is chosen yet** (ADR 0002). Views compile and send intents, but nothing is drawn.
- **Adapters never call back** (ADR 0003). The system's changes reach MeeGone as `ShellIntent`s.

## Decision

1. **A Gradle app with a Java host.**
   - `android/` is a Gradle project with one module, `app`. It uses the Android Gradle Plugin with the Kotlin DSL and plain Java on the Android SDK: no Kotlin, no AndroidX.
   - Its one activity, `HomeActivity`, has two intent filters: MAIN/HOME/DEFAULT, which makes it a home app (HOME-01), and MAIN/LAUNCHER, so it also runs as a normal app.
   - Application ID `io.github.twonet.meegone`, minSdk 29 (Android 10, SRS 2.5), target SDK 36. One ABI for now, arm64-v8a.
   - MeeGone requests no permissions (SEC-03).
2. **The Rust app is a native library.**
   - The root package also builds as a `cdylib`, `libmeegone_launcher.so`. Gradle builds it with cargo-ndk for each variant.
   - Its entry points (`src/android.rs`) are `HomeActivity`'s native methods: `nativeStart(bridge, filesDir)` and `nativeResumed()`.
   - A panic or start-up failure becomes a Java `RuntimeException`, so the message reaches logcat.
3. **One launcher per process.** `Launcher` lives in a main-thread `thread_local!`, and every `HomeActivity` shares it. When MeeGone is open from the app drawer, Home starts a second instance in the home task, and two launchers would both write MeeGone's files. Starting again does nothing.
4. **Adapters ask a Java `Bridge`.**
   - Rust adapters call one Java object, `Bridge`, through a minimal JNI binding written by hand (`crates/core/data/src/android/jni.rs`). The binding reads only the functions it uses from the JNI tables, by their fixed slots, checked against NDK 27.3's `jni.h`.
   - After every call it describes, clears and returns any Java exception. Strings cross as UTF-16.
   - Rust looks up the bridge's methods by name once, at start. `Bridge` catches what Android throws, so no exception crosses into Rust.
   - `Bridge` keeps the resumed activity, which it needs to open system screens.
5. **The home-app role.**
   - `SystemConfigAdapter::is_default_home` uses `RoleManager.isRoleHeld(ROLE_HOME)`, or resolves the HOME intent on a device without the role service.
   - `request_default_home` opens the role dialog with `startActivityForResult`. If the system closes it at once without showing it, as it does after "Don't ask again", `Bridge` opens the system's home-app settings instead. If no system screen opens, it returns written instructions as `Unsupported` (SRS 8.1).
   - The activity sends `ShellIntent::Resumed` on every `onResume`, including after the dialog and after system settings. The shell then refreshes Settings, which re-checks the role and the day.
6. **The banner belongs to Settings and setup.**
   - `SettingsState.home_banner` is true while MeeGone isn't the default and the banner hasn't been dismissed or answered today, in local time.
   - The settings store keeps the time of the last dismissal, so a restart doesn't bring the banner back (settings format version 2).
   - The banner is a `Slot`, `HomeBanner` (settings UI), which the shell shows over the home views (ADR 0002).
7. **Adapters not built yet are stubs.**
   - Apps, notifications, usage, wallpaper, network and scheduling answer with their "if unavailable" value from SRS 8.1 until their issues build them. Each feature then shows its reduced state.
   - The stub's `shortcuts` is `Unsupported`, so menus show none. Their implementation (#12) offers them only while `LauncherApps.hasShortcutHostPermission()` is true, that is, while MeeGone is the default.
8. **`unsafe` is OS access.**
   - arch-check also flags the `unsafe` keyword, because foreign code needs it.
   - The JNI binding, the Android `SystemConfigAdapter` (which reads the clock) and the entry points join `FileStorage` in `OS_ACCESS_ALLOWED`. No other file needs `unsafe`.
9. **Checks.**
   - CI lints the workspace for the Android target, the only build that compiles the Android-only code.
   - A second job builds the debug and release APKs.
   - Every LOAD segment of the library is 16 KB aligned, as Google Play requires, with today's Rust and NDK. `zipalign -c -P 16` checks the APK.
10. **Checking the role request on a device.**
    - Nothing is drawn yet, so debug builds accept an extra that taps the banner's button: `adb shell am start -n io.github.twonet.meegone/.HomeActivity --ez io.github.twonet.meegone.debug.REQUEST_HOME true`.
    - It lives in the `debug` source set; release builds get a version that does nothing (SEC-06).
    - It goes once the banner is drawn.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0003, decision 6 | The OS-access scan also flags `unsafe`, and the allowlist holds the Android adapters' files. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| §7.4: the settings store's fields | It also keeps when the home-app banner was last dismissed, so the banner shows at most once a day across restarts (ONB-04). It isn't a user-adjustable option, so SET-04's limit is unchanged. |
| ONB-04: "a dismissible banner … at most once a day" | Accepting the banner's request also counts as today's answer. A user who declines in the system dialog isn't asked again that day. |

## Consequences

- **Benefits:**
  - MeeGone installs, can be chosen as the home app, and the core logic of HOME-01 and ONB-04 is tested without a device.
  - Each later adapter adds methods to `Bridge` and replaces a stub, without touching the build.
  - All `unsafe` code is in three files that CI names.
- **Costs:**
  - The JNI binding is ours to maintain, and only a device checks it.
  - The Java and Rust sides must agree on method names and signatures, and nothing checks that at build time. A mismatch fails at start, in logcat.
  - Building the APK needs a JDK, the Android SDK and NDK, and cargo-ndk. The workspace checks don't.
  - The app is arm64-only, so it doesn't install on x86_64 emulators.
  - If code shrinking is turned on, it needs keep rules for `Bridge`'s methods.
