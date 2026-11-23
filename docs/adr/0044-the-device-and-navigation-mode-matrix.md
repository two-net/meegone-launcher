# ADR 0044: The device and navigation-mode matrix

**Status:** Accepted · **Date:** 2026-11-24 · **Issue:** #43

## Context

Issue #43 asks for proof that MeeGone works on the devices and in the modes it claims:
- **COMP-01:** Android 10 (API level 29) and later, on phones, foldables and tablets.
- **COMP-02:** every navigation mode: gestures, two buttons, three buttons.
- **SRS 2.5:** personal and work profiles.
- **COMP-04 (Should):** verified on the reference device, a Google Pixel phone, a Samsung phone, a phone from another major brand, a tablet and a foldable.
- **SRS C5:** a record, per device, of whether the lock-screen image appears, since device makers may override the lock screen.

Its acceptance criterion: a release checklist covering that matrix is run before each release, with results recorded.

What already held:
- **The design covers every case:**
  - edge to edge, the safe area, the orientation rule and size changes (ADR 0009);
  - Home and Back in every navigation mode (ADR 0007);
  - every profile's apps, with paused and locked ones dimmed (ADR 0010, ADR 0012);
  - the lock screen's read-back (ADR 0030).
- **Without a device**, `tests/acceptance.rs` runs against fakes:
  - Home and Back with gestures and with buttons;
  - folding, and a tablet's rotation;
  - work profiles, paused and locked;
  - a device that can't set wallpapers.
- **On a device**, the scripts of #37 to #42 ran on a Galaxy S22 (One UI, Android 16), in three-button navigation.

What didn't:
- **A backlog with no checklist.** 25 ADRs, from 0007 to 0041, and `tests/acceptance.rs` left checks to "the device matrix (#43)", for example:
  - Android 10 to 15, and two-button navigation;
  - tablets, foldables and work profiles;
  - TalkBack in places.

  Nothing listed or ran them.
- **Android 10 was unchecked at build time.** The release build runs lint's vital checks, which leave `NewApi` out. A call newer than API 29 without its version check would build, ship and crash there.
- **ANIM-09's reading of the navigation mode** (`Bridge.animatesReturnHome`) took "nothing to tap at the edges, and a bar there" for gestures. Two cases read gestures as buttons, so MeeGone's return motion played over the system's:
  - a taskbar on a large screen, which has buttons to tap in every mode;
  - gestures with the handle hidden (Samsung, Xiaomi), where the bar's inset is 0.
- **Work profiles below Android 15.** There, every profile but MeeGone's own counts as work (ADR 0010), so a maker's clone or dual-app profile could be called "work".

The constraints:
- **Devices.** The project has one device, the S22.
- **Two-button navigation** exists only on Android 10 and 11; Pixels dropped it in 12.
- **Emulators.** The APK is arm64 only, so its emulator needs an arm64 image, as on an Apple-silicon Mac. The user chose no emulators in CI: they would add minutes and flakiness to every change.
- **Navigation mode.** adb switches it with the system's navigation-bar overlays. Pixels and emulators follow them; other makers keep a setting of their own, which the overlays leave out of step.
- **Work profiles.** adb can't make a real work profile without a device-policy app, and can't pause one.
- **Lock-screen overlays,** such as Samsung's Dynamic Lock Screen or Glance, draw over the wallpaper without changing the system's record of it. Only a person can tell (ADR 0030).

## Decision

1. **The matrix (COMP-01, COMP-02, COMP-04).** Each row is one run of `tools/compat-check.sh --record` on a device or an emulator:

   | Row | When | For |
   |---|---|---|
   | The reference device, with `--reference`: the S22 until #48 | Every release | COMP-04's reference device |
   | Android 10 (API 29): a phone, or an arm64 emulator | Every release, in all three modes | COMP-01's oldest version, COMP-02's two-button mode |
   | A tablet, physical or emulated | Every release | COMP-01's tablets |
   | A foldable, physical or emulated | Every release | COMP-01's foldables |
   | A Google Pixel phone | May carry over | COMP-04 |
   | A Samsung phone | May carry over | COMP-04 |
   | A phone from another major brand | May carry over | COMP-04 |
   | A physical tablet, and a physical foldable | May carry over | COMP-04 |

   - **Work profile.** At least one row has a work profile, from a device-policy app such as Test DPC or an employer's.
   - **One device can fill several rows.** The S22 is also the Samsung phone.
   - **Carrying over.** A COMP-04 row recorded for an earlier version still counts, with that version in its file. It is run again if the Java host changes how it handles the window, its insets, Back, Home or profiles.
   - **Emulators** never fill a COMP-04 row.
2. **`tools/compat-check.sh` checks one row.** It uses the debug build, with setup done.
   - **In each navigation mode the device offers:**
     - **Switching:** with the system's overlays on Pixels and emulators. Elsewhere it asks for the switch in the device's settings, then confirms the mode with `settings get secure navigation_mode`. Two-button mode counts as not offered after Android 11.
     - **The reading:** MeeGone reads the mode as the system sets it.
     - **Home:** from another app, Home lands on the home with nothing open (HOME-06). It is a swipe up from the bottom edge with gestures, and the key with buttons.
     - **The return motion** plays only with buttons (ANIM-09).
     - **Back:** it closes Settings, opened through the system's gear intent, and never leaves the home (HOME-08).
     - **Swipes:** a swipe across the middle changes the view. With gestures, a swipe from a side edge is Back and leaves the view as it was (HOME-13).
   - **Turning and folding:**
     - **Turning:** with Settings open, the home is turned to landscape. A phone stays portrait and a large screen turns (HOME-12). Either way the view and Settings stay as they were (COMP-03).
     - **Folding:** a foldable is folded and unfolded, with `cmd device_state` from Android 12L or by hand. Each screen gets its orientation rule, and nothing closes.
   - **Profiles:**
     - **Naming:** a work profile's apps are called work, and any other profile's aren't (APPS-20, A11Y-01). `pm list users` gives each profile's managed flag, on every version.
     - **Pausing:** with a work profile, a person pauses work apps. MeeGone must see them paused, and a person confirms they are dimmed and that a tap offers to turn them on (APPS-09).
   - **The lock screen:** the script records MeeGone's own reading (unsupported, kept, or "may not show"). Then it asks a person whether the lock screen shows the image (SRS C5). It takes no screenshot of the lock screen, which could hold the owner's notifications.
   - **Without a terminal,** it asks nothing and takes nothing as done: what needs a person is skipped, and says so. `--lock-screen yes` or `no` gives the lock screen's answer up front. `--mode` runs one mode, for a device where a person must switch it between runs.
   - **Results:**
     - **The table:** every check is ✅ or ❌, and a skipped check says why. A skip is never a pass.
     - **The lock-screen answer** is recorded, not passed or failed: a maker's overlay isn't MeeGone's to fix.
     - **The script** exits non-zero on any ❌.
3. **A debug-only probe tells the script what the home shows.**
   - **The report:** `CompatProbe`, in `src/debug`, logs one under `MeeGoneCompat` when started with `io.github.twonet.meegone.debug.COMPAT`. It holds:
     - the window's smallest width, the orientation asked for, the rotation and the hinge;
     - MeeGone's reading of the navigation mode;
     - each profile with MeeGone's flags for it;
     - the lock screen's state;
     - the view, whether anything is open, Settings, and the setup step.
   - **The marks:** while it is on, each Home and each Back is marked once the home has settled.
   - **The intent** has no Home category, so asking for a report changes nothing.
   - **The hooks:** `DebugHooks` gains `back` and `readCompatExtras`, which the release build's stubs answer with nothing (SEC-06). `HomeViews` gains two read-only accessors, `auditSettings` and `auditImageMayNotShow`.
   - **No Rust or JNI change.** The return motion's mode is read from MeeGone's own log line, which Rust's one call to `animatesReturnHome` writes on each return.
4. **Results are committed.**
   - **Where:** each row writes `docs/devices/<version>-<model>.md`, or `docs/devices/<version>-emulator-<avd>.md` for an emulator. A run with `--mode` adds the mode, as in `0.1.0-SM-S901E-gesture.md`, so the runs of one device's modes don't overwrite each other.
   - **The header** carries the date, the commit, the build and the device: Android version, size, density, smallest width and ABIs. It also carries the row's class (phone, tablet or foldable; physical or emulated; the reference device or not), the modes run, the profiles, and the lock-screen answer with MeeGone's reading.
5. **Android 10 is checked at build time (COMP-01).**
   - **Lint:** `lint { fatal += "NewApi" }` makes lint's vital checks, which `assembleRelease` already runs in CI, fail on a call newer than API 29 without its version check. No task is added. A scratch call to `getCurrentWindowMetrics` (API 30) in `Bridge` failed the build with it and passed without it.
   - **What lint sees:** its vital checks cover the release variant only. The debug hooks meet Android 10 on the matrix's API 29 row instead.
   - **`arch-check`'s `compat` rules:**
     - SRS.md's API level, `minSdk` and cargo-ndk's `-P` platform agree;
     - `NewApi` stays fatal: nothing makes it a warning or turns it off, and nothing turns off release checks or abort-on-error, or adds a baseline;
     - the shipped Java hides no method from `NewApi` with `@TargetApi`, `@RequiresApi` or `@SuppressLint("NewApi")`, except where `API_HIDING_ALLOWED` says why every caller checks the version. The two Android 12 corner methods of ADR 0009 are its entries.
6. **MeeGone reads gestures from the Back edge too (ANIM-09).** `animatesReturnHome` now answers gestures if either holds:
   - **A side edge** takes system gestures beyond any bar there. That is Back's swipe edge, which only gesture navigation has, taskbar or not.
   - **Nothing at the edges can be tapped**, yet the bar or the system's gestures take the bottom. That is the handle, shown or hidden.

   Three-button landscape stays buttons: its bar's gesture inset is the bar itself, no wider. The log line now carries which sign fired and the raw insets, so a device that reads wrong shows why.
7. **A clone profile called "work" is recorded, not guessed at.** Below Android 15, no public call tells a work profile from a maker's clone or dual-app profile. The matrix shows where it happens: a ❌, which gets an issue. A fix without a device that has one would be a guess.
8. **The backlog is mapped.** Each check the earlier ADRs left to #43 is now covered by one of:
   - `tools/compat-check.sh`;
   - the accessibility audit, run on the API 29, tablet and foldable rows;
   - an existing item of `RELEASING.md`;
   - a new by-hand item of its device matrix;
   - or nothing yet.

   | ADR | Left to #43 | Covered by |
   |---|---|---|
   | 0007 | ANIM-09 skipped only with gestures | compat-check: the reading and the return motion, in every mode |
   | 0009 | Two-button navigation; Android 10 to 14 | compat-check on the API 29 row in all three modes; later versions on the other rows |
   | 0010 | The launch transition with MeeGone as the default home (ANIM-08) | By hand, on the reference device |
   | 0013 | Shortcuts with MeeGone as the default home; TalkBack | By hand, on the reference device; the TalkBack item |
   | 0014 | TalkBack; right to left; animations removed; the tick | The TalkBack, Arabic or Hebrew, and Remove animations items; the tick by finger |
   | 0015 | TalkBack; right to left; the tick; a tablet's panel | The same; the audit on the tablet row, which needs a folder |
   | 0016 | "cafe" finding "Café"; TalkBack's Search apps; the field after a theme change; Android 10's keyboard inset; right to left; a tablet; removed animations | The "lodz" item; the TalkBack item; by hand; by hand on the API 29 row; the audit's ar-XB pass; the audit on the tablet row; the Remove animations item |
   | 0017 | TalkBack; right to left; a tablet; a locked work profile; dark mode | The TalkBack item; the audit on the tablet row, in both themes and ar-XB; compat-check's paused work apps, and a locked one by hand |
   | 0018 | A transfer between devices; a cloud restore; Android 10 and 11's backup rules; a work profile | **Nothing yet:** it needs two devices and a store release |
   | 0019 | The alarm row; 24-hour time; a new time zone; midnight; 200 %; TalkBack; right to left; a tablet; dark mode | By hand; the 24-hour item; the audit on the matrix rows; the TalkBack item |
   | 0020 | The motion frame by frame; media playback; a work profile; the access switch itself; 200 %; TalkBack; right to left; a tablet; dark mode | By hand, except the motion frame by frame, which is `tools/perf-bench.sh`'s frames; the audit; the TalkBack item |
   | 0021 | Clear; a real content intent with auto-cancel; a work profile; TalkBack; 200 %; right to left; a tablet; dark mode | By hand, on a test device; the audit; the TalkBack item |
   | 0022 | The card in both themes; the fade; rotation; TalkBack; 200 %; right to left; a tablet | The audit on the tablet and foldable rows; compat-check's turning; the fade by hand; the TalkBack item |
   | 0023 | TalkBack; 200 %; right to left; dark mode; a tablet; a foldable | The audit on the tablet and foldable rows; the TalkBack item |
   | 0024 | The pinch and its reflow; TalkBack's actions; 200 %; right to left; a tablet; a foldable | The pinch by hand on the tablet row; the TalkBack item; the audit |
   | 0025 | TalkBack; right to left; animations removed; a tablet; dark mode | The TalkBack, Arabic or Hebrew, and Remove animations items; the audit on the tablet row |
   | 0026 | TalkBack's radio group; 200 %; right to left | The TalkBack item; the audit |
   | 0033 | Android 10 and 11's edge glow; TalkBack; recreation by other configuration changes | By hand on the API 29 row; the TalkBack item; compat-check's folding, which recreates the activity where the screens' densities differ |
   | 0034 | Remove animations and the accessibility shortcut; catches by a real finger; the badges' early taps; TalkBack's tick; PERF-03's frames | The Remove animations item; by finger; the TalkBack item; `tools/perf-bench.sh` |
   | 0035 | A confirmed reset; the system's gear itself; the instructions; TalkBack; Android 10 to 15 | By hand, on a test device; compat-check opens Settings through the gear's intent; the audit on the API 29 row; the TalkBack item |
   | 0036 | The default-home step and its dialog from setup; the banner; Back through the steps; TalkBack; Android 10 to 15 | `tools/a11y-audit.sh --first-run` and by hand on the API 29 row; the TalkBack item |
   | 0038 | TalkBack's speech; Android 10's state in names; right to left; a tablet; a work profile's ", work" names | The TalkBack item; the audit on the API 29 and tablet rows; compat-check's profile names |
   | 0039 | TalkBack touching empty space; Switch Access; a keyboard's Esc and Meta+/; right to left; a tablet; Android 10's glow | The TalkBack, keyboard and Switch Access items; the audit; by hand on the API 29 row |
   | 0040 | The pseudo-locales on a device; Arabic and Hebrew; a tablet | The audit's and the Arabic or Hebrew items; the audit on the tablet row |
   | 0041 | The reference device; a 60 Hz screen; Android 10 to 13's trims; a reboot's cold start; memory in the background; the audit since its capture changed | #48 and `tools/perf-bench.sh` on it; the audit on the matrix rows. **Nothing yet** for the rest |
   | `tests/acceptance.rs` | AC-18 to AC-20 on a device | The Remove animations, TalkBack, keyboard and Switch Access items, and `tools/perf-bench.sh` |

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0007, decision 8 | "Gesture navigation" is read from a Back edge as well as from the bottom having nothing to tap (decision 6). |
| 0009, decision 5 | The same. The two-button and Android 10 to 14 checks it left are the matrix's API 29 row. |
| 0010 | Below Android 15, a profile called work that isn't one is a ❌ the matrix records (decision 7). |
| 0030 | Whether the image appears is recorded for each device in `docs/devices/`. |
| 0007 to 0041 | What each left to "the device matrix (#43)" is covered as decision 8's table says. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| COMP-04: verified on the reference device, a Pixel, a Samsung, another brand, a tablet and a foldable | Every release runs the reference device, Android 10, a tablet and a foldable, which may be emulated. The physical Pixel, Samsung, other brand, tablet and foldable may carry over from an earlier release until a change to the host's window, insets, Back, Home or profile code. The project owns one device. |

## Consequences

- **Benefits:**
  - **Every release has one checklist** for the matrix and the whole backlog, and every run leaves a file that says what passed, on what, and when.
  - **Android 10 is checked on every build** of the shipped code, not only on a device that has it.
  - **Taskbars and hidden handles no longer get a doubled return motion.**
  - **The lock screen's fate per device is on record**, including the overlays MeeGone can't see.
- **Costs:**
  - **A person runs each row,** about 10 minutes, plus an emulator for each emulated row. The script's questions need their answers.
  - **Overlay switching works only on Pixels and emulators;** elsewhere a person switches the mode.
  - **A swipe that adb injects** may not trigger every maker's gestures. A miss shows as a ❌ to look at, not as a pass.
  - **The debug hooks aren't linted.**
- **Checked on a device:** the Galaxy S22 (One UI, Android 16) over wireless debugging, as the reference device, with the debug build. Its two modes were run apart, a person switching between them, and recorded in `docs/devices/0.1.0-SM-S901E-three.md` and `-gesture.md`.
  - **Every check passed in both modes,** including the swipe up to Home and the side-edge Back that adb injected, which One UI took as its own gestures.
  - **The reading:** in three-button mode, neither sign fired: the bar had 144 px to tap. In gesture mode, both did: Back edges of 90 px each side, and a 45 px handle with nothing to tap.
  - **The lock screen showed the image,** as MeeGone's read-back said.
  - **The phone has no work profile.** Its Secure Folder isn't one, and launchers don't see it.
  - **One miss.** In the first run, straight after the install, Home's mark didn't come within 8 s. It didn't happen again in four runs, one of them with the app's compiled code reset as after an install.
- **Not checked yet:**
  - **A hidden handle.** Writing One UI's gesture-hint setting from adb didn't hide it, so the case decision 6 adds for it hasn't been seen.
  - **A taskbar,** two-button navigation, Android 10 to 15, a tablet, a foldable and a work profile: the other rows.
  - **Backups between devices,** and performance beyond the reference device (ADR 0018, ADR 0041), are still not covered.
