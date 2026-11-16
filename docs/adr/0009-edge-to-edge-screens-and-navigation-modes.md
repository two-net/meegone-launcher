# ADR 0009: Edge to edge, screen sizes and navigation modes

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #8

## Context

Issue #8 asks the home to look right and keep its state on every Android 10+ phone, foldable and tablet:
- The home views draw the theme's solid background edge to edge, behind the system bars, and keep content clear of the bars, display cut-outs and rounded corners (HOME-11).
- The system status bar stays visible on all three views.
- Phones stay in portrait. Large screens, with a smallest width of 600 dp or more, turn to every orientation and adapt the layout (HOME-12).
- Cut-outs, rounded corners, folding, unfolding and window-size changes lose no state (COMP-03).
- Every navigation mode works the same: gestures, two buttons, three buttons (COMP-02).

The constraints:

- **Android 15 draws edge to edge, Android 10 to 14 don't.** From target SDK 35, Android 15 and later always lay an app out behind the bars, and Android 16 removes the opt-out. Earlier versions need the app to ask. MeeGone targets SDK 36 and supports SDK 29.
- **Each version reports the obstacles differently.** Bars and cut-outs are window insets, read through `WindowInsets.Type` from Android 11 and through older calls on Android 10. Rounded corners are reported only from Android 12, as a radius per corner.
- **Insets arrive before layout.** The window's new size isn't known yet when they do.
- **The home is drawn by Java views** (ADR 0006), and the Rust app keeps all state in one launcher per process (ADR 0005). A recreated activity finds it running (ADR 0007).
- **The content isn't built yet.** Each view shows only its name until #9, #19 and #22, so how the grid or the cards adapt can't be decided here.
- **On gesture navigation the screen's side edges belong to the system Back gesture.** View swipes already never depend on them (HOME-13).
- **No Java tests.** The host has no test source set, and only a device shows what is drawn.

## Decision

1. **Edge to edge on every version.**
   - `HomeActivity` lays the window out behind the bars: `setDecorFitsSystemWindows(false)` from Android 11, and the layout-only `SYSTEM_UI_FLAG_LAYOUT_*` flags on Android 10. Android combines every view's flags, so `HomeViews`' bar-icon colours still apply.
   - The theme:
     - Draws into cut-outs: `shortEdges` on Android 10, `always` from Android 11 (`values-v30`).
     - Turns off the system's contrast scrim, so the three-button bar shows the home background. The bar icons already follow the background (ADR 0006, decision 7).
   - Nothing hides a bar: the status bar shows on every view and over Settings.
2. **A safe area for content.**
   - `HomeViews.onApplyWindowInsets` computes how far content stays from each edge:
     - The bars and the cut-out.
     - From Android 12, any rounded corner that would still cut the safe area's corners. The top and bottom stay as the bars set them. Each corner can move its side in by as far as its curve reaches at that height: `r − √(r² − (r − t)²)` for radius `r`, where the safe area is `t` from the top or bottom edge.
   - The formula uses only the radius, so it needs no window size. When the window doesn't reach the screen's corner, it moves the side in further than needed, never less.
   - The background stays edge to edge on `HomeViews`.
   - Each page and Settings get the safe area as padding, so their content lays out inside it. Content that scrolls may draw under the bars by turning off `clipToPadding` (#9, #19, #22).
   - The view indicator sits below its top, and the home menu stays inside it, on all four sides.
   - It is recomputed whenever the insets change: rotation, folding, a new window size or a new navigation mode.
3. **Portrait on phones, every orientation on large screens.**
   - On create and on every configuration change, `HomeActivity` asks for `SCREEN_ORIENTATION_PORTRAIT` when the screen's smallest width is under 600 dp. Otherwise it asks for `SCREEN_ORIENTATION_FULL_USER`, which follows the user's rotation lock.
   - The screen decides, not the window: `getMaximumWindowMetrics()` from Android 11, the display's real size on Android 10. A split screen on a tablet is still a large screen, and folding or unfolding switches between the two.
   - Android 16 itself ignores orientation requests on large screens for apps that target it, with the same threshold.
   - The rule lives in Java, like the bar-icon colours: it is a window concern and keeps no state.
4. **Size changes keep everything open.**
   - Resizing, rotating, folding and unfolding stay handled in place (`configChanges`). `HomeViews` sends the new `Layout` and the carousel takes the new width.
   - Any other configuration change, such as a foldable's screens having different densities, recreates the activity. The shared launcher keeps everything. Handling density in place would mean rebuilding every pixel and text size `HomeViews` sets.
   - A new `Layout` keeps the current view, Settings, the home menu and everything open in the views: folders, edit mode, close mode, and the Applications scroll position. Only a drag in progress ends; the system cancels its touches anyway.
   - The home menu stays open, centred. It opened where the finger was, which means nothing in the new layout.
5. **Navigation modes need no mode of their own.**
   - Back reaches Rust through `OnBackInvokedCallback`, or `onBackPressed` before Android 13, and Home through `onNewIntent`, in every mode (ADR 0007).
   - The only difference is ANIM-09's return motion, which plays only with buttons (ADR 0007, decision 8).
   - View swipes start anywhere, past the touch slop (HOME-13). With gesture navigation, the system takes a swipe from a side edge as Back before the carousel's touch slop and cancels the touch, so the views never move with it.
   - MeeGone excludes no system-gesture area.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 7 | The home views also draw edge to edge on Android 10 to 14, with no scrim behind three-button navigation, and keep content inside the safe area. |
| 0008, decision 3 | The card stays inside the safe area on all four sides, not only below the status bar. |
| 0008, decision 4 | A change of layout no longer closes the home menu: it stays open, centred. |

## Departures from the SRS (MAINT-05)

None. For HOME-12's "adapt its layout", the pages, the carousel, the safe area, the menu and Settings adapt now. Each view's content adapts when it is built: the Applications columns (APPS-02, #9) and Recent's cards (#22).

## Consequences

- **Benefits:**
  - The home looks the same on Android 10 to 16. Content stays clear of cut-outs and corners in every orientation and window.
  - Folding, rotating and resizing keep all state, including the home menu. `meegone-shell-state` and `tests/acceptance.rs` test this without a device, as they do the same Home and Back in every navigation mode.
  - Later views get the safe area as their page's padding and need no inset code of their own.
- **Costs:**
  - The safe area and the orientation rule are Java, so only a device checks them.
  - The rounded-corner formula assumes the window reaches the screen's corner. In a smaller window it may move content in a few pixels more than needed.
  - `AppsState.scroll_px` is in pixels. When folding changes the grid's columns or density, the grid (#9) must turn it into the same first visible item.
  - Two-button navigation and the Android 10 to 14 path need devices the project doesn't have yet. They belong to the device matrix (#43).
