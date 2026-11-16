# ADR 0007: Home, Back and coming back to the home

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #6

## Context

Issue #6 makes Home and Back predictable:
- After setup and after a restart the home shows Applications (HOME-05).
- Coming back from an app shows the view the user left, or Applications if the user chose "Home button opens: Always Applications" (HOME-06).
- Home on the home closes what is open, then moves to Applications, then scrolls it to the top (HOME-07).
- Back closes the topmost open element or does nothing (HOME-08).
- The content comes back with a short scale and fade unless the system animates the return (ANIM-09).
- After the system ends MeeGone's process, the view and the scroll position come back (REL-05).

The shell already had `ShellIntent::Home` and `Back` (ADR 0003), but the Android host sent neither. The constraints:

- **Android reports a Home press the same way in both cases.** Home on the home and Home from an app both arrive as a new `MAIN`/`HOME` intent in `onNewIntent`. Only the activity's lifecycle tells them apart.
- **Coming back isn't always Home.** An app that closes, or Back from a system settings screen, also brings the home back, with no new intent.
- **Several `HomeActivity` instances can share one launcher** (ADR 0005, decision 3). An instance opened from the app drawer can stop after the home instance has resumed.
- **Android 16 stops calling `onBackPressed`** for apps that target it, which MeeGone does (target SDK 36).
- **No Android API says whether the system animates the return home.** On Android 10 and later it nearly always animates the app leaving.
- **The simplicity budget allows 3 persistent stores** (SRS 7.1). Nothing kept for REL-05 may survive a device restart, because HOME-05 starts on Applications.
- **The Applications grid isn't built yet** (#9), but Home must scroll it and REL-05 must restore its position.

## Decision

1. **The host says whether the home was out of sight; Rust decides what that means.**
   - Each `HomeActivity` keeps `away`: true from `onStop` until the end of the next `onResume`.
   - A Home intent while not `away` is Home on the home: `nativeHome()` sends `ShellIntent::Home`.
   - A Home intent while `away` is only recorded.
   - `onResume` calls `nativeResumed(returning, byHome)`. It sends `Resumed` as before and, if the home was away, `ShellIntent::Returned { home }`.
   - `away` lives in each instance, so an instance that stops late can't make the visible one think it is away.
   - A new instance starts away only when `nativeStart` reports that it started the launcher and there is saved state, that is, after the system ended the process. A recreation after a configuration change finds the launcher running.
2. **"Home button opens" applies to Home only.**
   - With **Always Applications**, `Returned { home: true }` closes Settings and everything open in every view. It shows Applications at once: the carousel is reset with no motion and no indicator.
   - With **Last view** (the default), and for every return that Home didn't bring, the view and whatever is open stay as they were. That way edit mode survives the system's uninstall dialog, and a user who granted access from Events comes back to Events.
   - Every return refreshes Recent (REC-09), so the app just used is at the top left (AC-03).
3. **Home on the home** (HOME-07):
   - It closes Settings, or everything open in the current view.
   - Otherwise, if another view shows or a drag is in progress, it moves to Applications.
   - Otherwise it scrolls Applications to the top. This includes while the row is still settling onto Applications, so two quick presses from Events move and then scroll (AC-04).
4. **Back always reaches Rust** (HOME-08).
   - From Android 13 an `OnBackInvokedCallback` is always registered, and the manifest opts in with `enableOnBackInvokedCallback`. Android 10 to 12 use `onBackPressed`, which never finishes the home.
   - Because the callback is always registered, the system shows no back-to-home animation on the home.
5. **The Applications scroll position is state.**
   - `AppsState.scroll_px` is how far the grid is scrolled, as the host reports it with `AppsIntent::Scrolled`. Home sets it to 0 with `ScrollToTop`.
   - The grid (#9) reports its position and scrolls to this one whenever the two differ.
6. **REL-05 uses the activity's saved state, not a store.**
   - `onSaveInstanceState` asks `nativeSave` for the current view and the Applications scroll position and keeps them as two `int`s in the `Bundle`. `onCreate` passes them to `nativeStart`, which sends `ShellIntent::Restore` only when it starts the launcher.
   - Android keeps this state while the home's task exists and drops it on a device restart, so a restart shows Applications (HOME-05) without a fourth store.
   - Folders, menus, search, edit mode and close mode aren't kept, so ending the process cancels them cleanly.
7. **The return motion is shell state** (ANIM-09).
   - `ReturnHome` (`meegone-shell-state`) scales the content from 94 % to 100 % and fades it in over 250 ms. It uses `decelerate`, Android's `DecelerateInterpolator`, which joins `meegone-core-state::motion`.
   - Its clock starts at the first frame after the return. It follows the system animation speed and doesn't run when animations are removed (ANIM-05). A touch ends it at once (ANIM-03).
   - `HomeFrame` gains the content's scale and opacity, in slots 10 and 11 of the frame. `HomeViews` applies them to the three pages; the background stays opaque.
8. **The system animates the return with gesture navigation.**
   - `SystemConfigAdapter::animates_return_home` decides whether ANIM-09 runs.
   - On Android, `Bridge.animatesReturnHome` reports gesture navigation: a navigation bar with nothing to tap, from the window's tappable-element insets. Its swipe-home motion carries the app into the home, so MeeGone adds nothing. With 2- or 3-button navigation, MeeGone plays ANIM-09.
   - When the insets are unknown, the answer is yes, so the two never double up. That is the case after the system ended the process: the new window isn't attached at its first `onResume`, and the system shows its own starting window instead.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0005, decision 2 | `nativeStart` takes the saved view and scroll position and returns whether it started MeeGone; `nativeResumed` says whether the home was away and whether Home brought it back. `nativeHome`, `nativeBack` and `nativeSave` are new. |
| 0006, decision 2 | The frame has 12 slots: the content's scale and opacity follow the colours. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| HOME-06: a setting offers "Always open Applications" instead of the last view | The setting is "Home button opens" (SET-02), so it applies when Home brings MeeGone back. Coming back any other way, such as Back from a system settings screen, shows the last view. |
| REL-05: restore the current view and, where possible, the scroll position | Only Applications' scroll position is kept. Events and Recent refresh on return and show their newest items first, so their old position means nothing. Settings, which covers the home views, isn't restored. |
| ANIM-09: skipped when the system already animates the return | "Already animates" is taken to mean gesture navigation (decision 8). The device matrix (#43) checks it. |

## Consequences

- **Benefits:**
  - Home, Back, the Home target, the return motion and the restore are tested without a device, in `meegone-shell-state` and `tests/acceptance.rs`.
  - The Java host only reports lifecycle facts. One boolean per instance replaces any timing guess.
  - REL-05 needs no new store and can't outlive a restart.
- **Costs:**
  - The activity keeps `android:stateNotNeeded="true"`. Android still hands back saved state after it ends the process, but only a device confirms it: open an app, run `adb shell am kill io.github.twonet.meegone`, press Home.
  - The gesture-navigation check is a heuristic. Some devices may animate differently.
  - The JNI signatures changed. As before, nothing checks them at build time; a mismatch fails in logcat.
  - The grid (#9) must report its scroll position and follow `scroll_px`.
