# ADR 0043: Failure containment and crash-free sessions

**Status:** Accepted · **Date:** 2026-11-23 · **Issue:** #42

## Context

Issue #42 (Must) asks that launching apps always works, whatever else fails:
- **REL-01:** a failure in Events, Recent or the lock-screen image never stops Applications.
- **ARCH-09:** each feature has its own error states and never takes down the shell.
- **REL-06:** everything except downloading the image works offline.
- **REL-07:** at least 99.9 % of sessions are crash-free, as the distribution platform's own stability reporting measures them, with no third-party crash SDK.

Its acceptance criteria: AC-21 (notification access revoked while Events shows), and a fault-injection test for each row of SRS 7.6's failure table that leaves Applications working.

Most of it was already true:
- **ARCH-09:** `Store` ran every intent inside `catch_unwind` and marked a feature that panicked `Health::Failed` (ADR 0001). The shell offered a failed feature's content nowhere, and the per-feature ADRs say what each hides.
- **SRS 7.6, row by row:**
  - notification and usage access missing or revoked show their explainer cards (ADR 0020, ADR 0023);
  - the banner offers to make MeeGone the default home app (ADR 0036);
  - a feed out of reach, a changed format or an invalid image keeps the wallpaper, shows the status and backs off, and after 3 days Settings says "Daily image unavailable" with Try again (ADR 0029);
  - a device or policy that can't set the wallpaper shows the feature as unavailable, with the reason (ADR 0030);
  - low storage skips the download (ADR 0028);
  - an unreadable layout is rebuilt (ADR 0012).
- **AC-21** had its test, which ends with an app opened.
- **REL-04:** an attempt is counted before any request, so its back-off and the day's budget hold even if it stops part-way.
- **REL-06:** only the lock-screen image uses the network (ADR 0027).
- **REL-07:** the Android app adds no library, so no crash SDK.

An audit of the Rust app, the Java host and the tests found these gaps:
- **The host contained nothing.** `guarded` turns any panic that reaches an entry point into a `RuntimeException`, but `HomeViews` called the natives on the main thread with no `try`. A panic outside a `Store`, or a Java bug in any feature's views, such as Recent's list or the image card's decoding, ended the whole home.
- **A feature's code ran outside its store:**
  - each feature's part of `ShellView::frame`, on every frame;
  - the shell's calls to feature state, such as Events' `row_keys` and Recent's `columns`;
  - every reader and action of `src/android.rs`, such as `nativeEventsList` and the image screens' content;
  - `Store`'s listeners, which publish its snapshot.
- **Two bugs would stop MeeGone at every start:** a panic while reading the settings file escaped `nativeStart`, and one while reading the layout failed Applications for good.
- **A daily check that panicked** was counted but never recorded a problem, so Settings showed no status and never "Daily image unavailable". A task that panicked left the tasks after it waiting, and the job's thread caught only `RuntimeException`.
- **A failed feature just went blank,** with nothing to tell the user or a developer. `Health::Failed` kept the panic's message, which can quote what it handled (PRIV-03).
- **A failed Applications** kept its grid on screen, frozen: no tap opened anything, and it didn't scroll.
- **Nothing kept the checks true:** that panics unwind, that the build adds no SDK, and that nothing hides a crash from the platform's reports.
- **The tests:** only Events had a whole-app panic test. Most of SRS 7.6's rows had no "and an app still opens", and low storage, a changed format and an invalid image had no whole-app test at all. No test ran every view offline.

## Decision

1. **A feature's `Store` is its fuse (ARCH-09).**
   - `Health` is `Ready` or `Failed`, and keeps nothing of the panic (PRIV-03).
   - `Store::contain(step)` runs some of the feature's code outside its intents. It returns `None` once the feature has failed, and a panic inside it fails the feature. `fail()` does that from outside.
   - Intents and `back` go through the same guard, and so does publishing the new snapshot, so a listener's panic stops its feature, not the caller.
   - The rule, written on `contain`: the closure only computes. Whoever calls it changes their own state with the result, after it returns.
   - **Panics must unwind.** `meegone-core-state` doesn't compile under `cfg(panic = "abort")`, which catches a profile, `.cargo/config.toml`, `RUSTFLAGS` and `CARGO_PROFILE_*_PANIC` alike.
2. **Every call into Events, Recent, the lock-screen image or Settings is contained.**
   - **The frame.** `ShellView::frame` builds each feature's part in its own `contain`: Events' header, list and menu; Recent's cards and time window; the image's card, details, past week, Settings group and setup step; Settings' own groups and setup. A failed part is drawn as absent, as before.
   - **The shell.** `HomeShell` wraps its calls to feature state the same way, and `HomeShell::contain(feature, step)` lets the host do it by `Feature`.
   - **The host's readers and actions.** Each reader in `src/android.rs` builds its fields inside `contained`, and passes them to Java outside it. The `with_*_message` helpers become one, `with_feature_message`, which runs its step contained.
   - **In the same frame.** `HomeShell::failures` names the failed features. `Launcher::frame` dispatches `ShellIntent::Failed` for each new one, which closes what it covered, such as the image details, then draws. A part that fails while it is drawn is drawn as failed before the frame returns. One that fails in a reader is drawn at the next idle, as `nativeIdle` says.
   - **Left out.** Applications' own views and the shell's own logic stay uncontained: a bug there is a crash, which the platform's reports count. The image job's thread has its own containment (decision 7).
3. **The host contains each feature's views.**
   - `HomeViews.apply` draws each feature's views inside its own `try`: Events' header and list; the image's card, details, past week, Settings group and setup step; Recent's cards and time window; Settings' groups, setup and banner. Inline blocks keep the frame free of allocation.
   - It catches `RuntimeException`, which `guarded` also throws, and `OutOfMemoryError`, as `ThemeFade` does for a bitmap. Not `Error` in general: a broken build, a broken VM or a stack overflow should crash, and be counted.
   - A catch hides that feature's views, changing only their visibility so none of their code runs. It logs the feature's number and the exception's kind, and calls `nativeFeatureFailed`, so Rust stops the feature too.
   - The frame's slot 155 lists the failed features, so the host also stops drawing a feature that failed in Rust.
   - Nothing else in the host runs a stopped part's views either: Events' and Recent's icons, Recent's sizes and the long-press's row or card are read only while the part is drawn, and a stopped part's dialog, such as Region's, closes. A view that threw part-way through filling itself may be half-built, and nothing would catch it again.
   - `stop` never throws itself: the native it calls is caught, as the view stops drawing the part either way.
   - `HomeActivity` needs no guard: its natives contain a feature's panic now, and its refresh goes through `apply`.
4. **A part that stopped says so, with Restart MeeGone.**
   - The Rust app decides where each card shows, `HomeFrame::stopped`, and the frame's slot 156 carries it:
     - Applications': in the home-app banner's place, on Applications with nothing over it. The banner gives way to it.
     - Events' and Recent's: at the top of their pages.
     - The lock-screen image's: in Events where its card was, only if it showed, and in Settings in place of its group.
     - Settings': at the top of Settings, in place of Appearance, Home and Access.
   - A long-press on Applications' card, or on its button, opens nothing behind it, as on the banner. Settings opened at the image's group, from the home menu or its details, shows its card in that place.
   - `StoppedCard` is styled as the home-app banner: the surface colour, 16 sp text, one button, a polite live region so TalkBack says it on the view showing. `HomeViews` owns all six, so a failed part's code never runs to show its card. Their words are string resources, `stopped_*` (L10N-01).
   - **Restart MeeGone** (`Restart.java`) starts the home activity again, in the home's task while MeeGone holds the role, then ends the process with `System.exit(0)`. Android records that as the app exiting itself, never as a crash. `HomeActivity` is `singleTask` and `stateNotNeeded`, so it comes back fresh. Files are written atomically (REL-03), and what was only in memory, such as "Updating…", starts again.
5. **A failed Applications still opens apps (REL-01).**
   - Its last grid stays drawn. A tap there that a finger could reach opens the app through the catalogue, from the shell (`launch_from_last_grid`). A panic in the catalogue is contained too, and the shell says why a launch failed, `launch_problem`.
   - The grid still scrolls, as the shell keeps where it is. Home takes it to the top, and the process's saved state keeps it (REL-05).
   - Search, menus, folders and edit mode stay off, and the home menu leaves out Edit applications.
   - If Applications fails at its first refresh, its grid is empty. Recent and Events can still open apps.
6. **A saved file that a bug can't read is unreadable (REL-02, MAINT-04).** `format::decode`, which every store reads through, catches a panic in a migration or a parser and gives `None`. So settings load as the defaults and a layout is salvaged. `format::salvage` does the same for each record, so one bad record costs only itself.
7. **The daily check and the job's thread (REL-04).**
   - A panic in the check, once it is counted, becomes `Failed(DownloadFailed)`. It records the problem, so the status shows, the back-off and the day's budget hold, and after 3 days the image shows as unavailable.
   - `Shared::run_image_tasks` runs each task in its own `catch_unwind`, so the rest of the queue still runs. A browse or a pin that panics sets the past week's problem, so it offers Try again.
   - `ImageJobService` also catches `OutOfMemoryError`, logging its kind, so the job always finishes and its next run isn't lost.
8. **Logs say where, once (PRIV-03).** The panic hook in `src/panics.rs` also writes "MeeGone panicked at ‹file›:‹line›" to logcat through liblog, once for every panic, contained or not. It never writes the message. `HomeViews` logs each feature that stops, by number.
9. **Every row of SRS 7.6 has a test that ends with an app opened.**
   - `tests/acceptance.rs`'s section "Failure handling (SRS 7.6)":
     - usage access revoked;
     - the default-home role lost while running;
     - the feed's format changed, through the real feed client;
     - an invalid image, through the real downloader;
     - the wallpaper no longer allowed;
     - low storage;
     - a daily check that panics for 3 days.
   - The unreadable-layout test (`rel02_…`) and AC-16 each gain a launch at the end. AC-21 already had one.
   - `rel06_…` uses every view, Settings, search and the image's screens offline with no request, then shows the daily check asks only the feed.
   - `tests/containment.rs` makes each feature fail where its code runs, through the `faults` feature of `meegone-core-state`, and checks that the rest goes on and an app still opens:
     - Events and Recent while drawn;
     - the image while its details show;
     - Settings during setup;
     - Recent as a pinch starts;
     - Applications;
     - a failure the host reports.

     Unit tests cover the store, decoding, the updater, the task queue and the shell's fallback.
10. **Reliability rules in arch-check (`tools/arch-check/src/reliability.rs`, REL-07).** `tests/reliability.rs` runs them in CI's check job.
    - **The shipped Java:**
      - It never sets an uncaught-exception handler, kills a process or halts the VM: a handler could hide a crash, and a kill would be recorded as a signal.
      - It ends the process only where `EXIT_ALLOWED` says: `Restart.java`.
    - **The Gradle build:**
      - It applies only the plugins in `PLUGINS_ALLOWED`, which lists the Android one.
      - It declares no `dependencies`, `implementation`, `classpath` or version-catalogue `alias`.
      - It turns the Rust app's `faults` feature on only on the line that limits it to the debug build.
    - **The release APK,** in the ignored test CI's Android job runs:
      - every class is in MeeGone's own package;
      - its native library, stored uncompressed, has no `nativeArmFault`.
    - **Crate manifests:** the fakes rule now covers `faults` too, so it is turned on only in `[dev-dependencies]`.
    - **The allow-lists stay true:** an entry that matches nothing fails.
11. **A fault check on a device (`tools/fault-check.sh`).**
    - **Only the debug build can fail on purpose.**
      - Its Rust app is built with the root crate's `faults` feature, which Gradle passes to cargo-ndk only for the debug build type.
      - The debug `DebugHooks` reads a `FAULT` extra, such as `events-java,recent-rust`. It arms Java faults itself, and Rust ones through `nativeArmFault`, which exists only with the feature.
      - The release `DebugHooks` does nothing, and the benchmark build, which shares the debug sources but not the debug library, logs that it can't.
    - **For each fault in a feature's Java views, its Rust code, or what the host reads of Events and Recent,** the script checks that:
      - the part says it stopped;
      - the process lives on, with no crash in the system's crash log or drop box;
      - an app opens from Applications.
    - **Then it arms Applications' fault on the running home,** not in the intent that started it, which Android relaunches with, **taps Restart MeeGone** and checks:
      - a new process, in which no part stops;
      - `HomeActivity` resumed;
      - `EXIT_SELF` in the exit info, from Android 11;
      - no crash;
      - an app opens.
    - RELEASING.md runs it before each release.
12. **What CI can't check is in RELEASING.md:**
    - the fault check;
    - the stopped cards with TalkBack, a keyboard and Switch Access;
    - the store's crash rate after a release.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0001 | `Store` contains a feature's code wherever it runs, through `contain`, not only its intents. `Health::Failed` keeps nothing of the panic. That panics unwind is checked by the compiler, not only by convention. |
| 0005 | A feature's panic no longer reaches the host: it stops that feature, and the panic hook logs where it was. A panic outside any feature still becomes a Java exception, and a crash. |
| 0006 | The frame has slots 155, the failed features, and 156, where the stopped cards show. |
| 0008 | While Applications has failed, the home menu leaves out Edit applications. |
| 0010 | A tap on an app in a failed Applications' last grid still opens it, and the grid still scrolls. |
| 0016 | Search stays off while Applications has failed, as before; the card says taps still work. |
| 0029 | A check that panics is a failed attempt, with its problem recorded. A task that panics ends, and the queue goes on. The job's thread also catches `OutOfMemoryError`. |
| 0036 | The home-app banner gives way to Applications' stopped card. |
| 0040 | `PROSE_ALLOWED` no longer holds a panic's message, and `compile_error!`'s message is a developer's. |
| 0042 | The panic hook writes each panic's file and line to logcat. "MeeGone creates no `PendingIntent`" still holds: Restart starts an explicit intent. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| REL-07: at least 99.9 % of sessions crash-free, as the distribution platform measures them | Google Play's Android vitals measure crashes per daily user, the user-perceived crash rate, not per session. MeeGone holds that at 0.1 % or less over 28 days. That is at least as strict as sessions only while a user who crashes does so no more often than other users open MeeGone. A crash at every start breaks this: 1 user in 1 000 is 0.1 % of users but far more of the sessions. So decision 6 removes the start-up crashes found, and RELEASING.md looks for a crash cluster with many more reports than users. F-Droid reports no stability data, so REL-07 is measured on Play alone. |
| ARCH-09: every feature fails on its own | Applications' own views and the shell's own logic aren't contained. A bug there is a crash, which the platform's reports count and a release fixes. A failed Applications' store, the likelier failure, still opens apps (decision 5). |

## Consequences

- **Benefits:**
  - **A bug in Events, Recent, the lock-screen image or Settings no longer ends the home.** Wherever its code runs, in Rust or in its Java views, that part stops alone, says so, and offers a restart. Every row of SRS 7.6, and every place a feature's code runs, has a whole-app test that ends with an app opened.
  - **Two crashes at every start are gone.**
  - **A daily check with a bug** now backs off, says so, and gives up visibly after 3 days, like any other failure.
  - **The checks fail when broken on purpose.** Each was broken, seen to fail, then put back:
    - `panic = "abort"` in the release profile;
    - a Gradle `dependencies` block with an `implementation`;
    - `setDefaultUncaughtExceptionHandler` in `Bridge.java`;
    - Events' part of the frame outside its `contain`;
    - `decode` without its catch;
    - the daily check without its catch.
- **Costs:**
  - **Containing costs a frame little:** a `catch_unwind` that doesn't unwind is nearly free, and `cargo bench --bench budgets` still holds.
  - **A part that fails is off until the process restarts.** Its holder may be half-way through a change, so it is never run again, and Restart MeeGone ends the process.
  - **A contained failure is invisible to the store's crash reports.** Only logcat and the fault check see one. A part that keeps failing for users would not show in the vitals.
  - **Switching the Android build** between debug and release rebuilds the Rust app, as the `faults` feature differs.
  - **Six strings** for translators.
- **Not checked yet:**
  - **`tools/fault-check.sh` hasn't run on a device.** In particular, that Android relaunches the home after `System.exit(0)` and records it as `EXIT_SELF`. If it doesn't, Restart would start a small activity in its own process to relaunch the home first. The first run on a test device settles it.
  - **The stopped cards haven't been through the accessibility audit.** They reuse the home-app banner's audited parts, and RELEASING.md checks them by hand until `tools/a11y-audit.sh`'s tour can show them.
  - **Java exceptions outside `apply`** aren't caught: in the framework's measure, layout or draw of a feature's views, in a click it posts, or as an accessibility node is made. Each would need a wrapper around the feature's views. That's for a later issue, if the vitals show one.
  - **The vitals** can only be read after a release.
