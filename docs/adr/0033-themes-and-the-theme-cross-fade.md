# ADR 0033: Themes and the theme cross-fade

**Status:** Accepted · **Date:** 2026-11-19 · **Issue:** #32

## Context

Issue #32 (Must) adds three themes, with a true-black Dark like the N9's AMOLED home:
- **The choice (THEME-01).** Settings offers System (the default), Light and Dark. Setup doesn't ask (ONB-05).
- **System (THEME-02)** follows the device's dark-theme setting while MeeGone runs, including scheduled changes.
- **Every surface (THEME-03):** the home views, folders, menus, dialogs, search, Settings and setup. Dark's home background is true black and Light's near-white (THEME-04). The system bars' icons stay legible (THEME-05).
- **A change (THEME-06, ANIM-19)** cross-fades the whole screen in 250 ms without a restart, keeping the view, the scroll position, an open folder and edit or close mode.
- **Accents (THEME-07)** keep 3:1 against the background.
- **Acceptance:**
  - AC-05: with System and a folder open, the device turning dark turns MeeGone dark within 300 ms, with the folder still open and the scroll position kept.
  - AC-06: with Light, the device turning dark leaves MeeGone light.

What was there already:
- **In the Rust app,** most of it:
  - `ThemeChoice` in the settings file, System by default and backed up (ADR 0018);
  - `resolve_theme` and `ThemeHolder` in `meegone-core-state`, which Settings writes and the shell reads (ADR 0002);
  - the tokens of SRS 5.3 with their contrast tests, and `Tokens::app_accent` (ADR 0011);
  - `ShellIntent::SystemThemeChanged`;
  - `motion::THEME_CROSSFADE` (250 ms), which nothing ran.

  The frame already carried every colour the host draws, and tests covered AC-05 and AC-06 without a device.
- **On Android,** none of it:
  - `SystemConfigAdapter::is_dark` was `false` (#32), and nothing sent `SystemThemeChanged`.
  - The host couldn't choose a theme: Settings drew no item for it, and `SettingsView::theme_chosen` was called only by tests.
  - `uiMode` wasn't among the activity's `configChanges` (ADR 0009), so the device turning dark **recreated** the activity. The new views took the shared launcher's state, but nothing was left to fade from, and a name being typed in the rename dialog was lost (ADR 0015).
  - Android's own widgets took some colours from the activity's DayNight theme, that is, the device's:
    - the search field's cursor, selection handles and highlight;
    - the press ripple of radio-button and switch rows;
    - scrollbar thumbs and, on Android 10 and 11, the glow at a list's ends;
    - the rename dialog's buttons and field.

    These matched MeeGone only while its theme matched the device's.

## Decision

1. **The device's dark theme is a question and an event (ADR 0003).**
   - `Bridge.isDark()` answers from the application's configuration (`UI_MODE_NIGHT_YES`), which follows the device even while the activity handles the change itself. `AndroidSystem::is_dark` asks it, and reads `false` if the bridge can't answer.
   - `HomeActivity` handles `uiMode` in place. When the night bits change, `onConfigurationChanged` calls `nativeSystemThemeChanged`, which dispatches `SystemThemeChanged`, and redraws. A home out of sight hears of it when it shows again.
   - **`Resumed` refreshes the theme too,** so a change while another app showed is never missed. It covers the user and the device's schedule alike.
2. **Nothing is made again.** With `uiMode` handled in place, every view keeps its place and state:
   - the scroll positions, an open folder, edit or close mode, search's field with its text and keyboard;
   - the rename dialog with the name as it is being typed.

   Other configuration changes, such as a new density or language, still recreate the activity (ADR 0009).
3. **Only a change in the theme drawn fades.**
   - `HomeShell` remembers the theme it last drew. After every intent, a different one starts the cross-fade (`keep_theme_in_step`), whether the device or Settings changed it.
   - A refresh that resolves to the same theme fades nothing: another setting, Dark chosen while the device is dark, or the device turning dark while Light is chosen.
   - MeeGone starts in its theme without a fade.
4. **One cross-fade for the image card and the theme.**
   - The card's thumbnail fade (ADR 0022) becomes `CrossFade`, with its length: 300 ms for the image (ANIM-22) and 250 ms for the theme (ANIM-19).
   - Both run on the shared `Timed` clock, decelerating. A late frame advances them by at most 34 ms, and with animations removed they are instant (ANIM-05).
   - `ShellState.image_change` and `theme_change` are `CrossFadeState`s.
5. **The host fades a picture of the old screen out over the new one.**
   - When a frame brings a new background while the fade has just started, `HomeViews` calls `ThemeFade.capture()` first, while the screen still shows the old theme. It draws itself into a bitmap of its size: the pages, Settings, the details screens, the menu and the dragged icon.
   - The picture goes into `HomeViews`' overlay, which draws above everything and takes no touches, so the new screen works at once (ANIM-03).
   - Each frame sets its opacity from the fade's progress, and it goes once the fade ends, or when the view detaches.
   - A theme changing again mid-fade takes a new picture of the screen as it is, fade and all.
   - With no memory for the bitmap, the change is just instant.
   - Interpolating the tokens instead would have recoloured every view, and re-read Events' and Recent's rows (whose accents depend on the theme), on every frame.
6. **The system bars and dialogs switch at once.** They are other windows: the bar icons follow the new background on the fade's first frame (ADR 0006). An open rename dialog closes and opens again in the new theme, at once, keeping the text and the selection.
7. **MeeGone's colours, not the device's, on Android's own widgets (THEME-03).** `Themed` sets them from the frame's tokens:
   - a text field's cursor, selection handles and highlight in the accent: search's and the rename dialog's;
   - every scroll view's edge glow and scrollbar thumb in the secondary colour.

   In addition:
   - The radio-button and switch rows of Settings get an explicit press ripple, as MeeGone's other rows have.
   - The rename dialog stays the device's light or dark dialog matched to MeeGone's theme, and its buttons and field take MeeGone's accent.
   - Recent's cards that are shrinking away take a new theme too.
   - The window turns off Android's "override force-dark", so it never darkens the Light theme.
8. **Settings → Appearance → Theme (THEME-01).**
   - The theme is Settings' own item, the first of SET-02's groups. Until #34 draws the rest of Settings, it is drawn ahead of the sections drawn so far, as #25 did for Recent's time window (ADR 0026). A failed Settings doesn't offer it (ARCH-09).
   - `RecentSettings` becomes `ChoiceSettings`, a group heading, a title and a radio button per choice. It is used for the theme and for Recent's time window.
   - A tap saves the choice and the screen cross-fades at once (SET-03). A failed save keeps the theme, and the next frame puts the check back.
   - SET-04's count is unchanged: the theme was already one of its 10 options.
9. **The host contract.**
   - **`HomeFrame` gains** `theme_change` (0 to 10 000) and `theme_choice: Option<ThemeChoice>`, and the theme's fade counts in `animating`.
   - **The frame grows to 141 slots:**
     - slot 139 is the fade's progress;
     - slot 140 is the choice: 0 System, 1 Light, 2 Dark, or -1 while Settings isn't working.
   - **New natives:**
     - `HomeActivity.nativeSystemThemeChanged()`;
     - `HomeViews.nativeTheme(int theme)`, which returns null or a message. Other numbers do nothing.
   - **New bridge method:** `boolean isDark()`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 141 slots. |
| 0009, decision 4 | `configChanges` also covers `uiMode`: the device's dark theme is handled in place. |
| 0015, THEME-06 departure | Lifted: a name being typed stays when the theme changes, as the dialog opens again in the new theme with it. |
| 0016, decision 5 | A theme change keeps the field; it is refilled from `nativeSearchQuery()` only after another configuration change made the views anew. |
| 0022, decision 4 | The fade's `image_change.rs` is now `crossfade.rs`, with the image's 300 ms. |
| 0026, decision 5 | `RecentSettings` is now `ChoiceSettings`, and Appearance → Theme is drawn above Hidden apps. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| THEME-06, ANIM-19: the whole screen cross-fades | The system bars and an open dialog switch at once, as they are other windows. Everything MeeGone draws in its own window fades. |
| THEME-03: dialogs follow the theme | The rename dialog follows MeeGone's light or dark, and takes its accent; its surface is the device's dialog colour. Toasts are drawn by the system, from Android 11 in the device's theme. |

## Consequences

- **Benefits:**
  - **Tested without a device:**
    - `CrossFade`: 250 ms and 300 ms, decelerating, and ANIM-05 at twice the length and instant.
    - The shell:
      - the device turning dark, and a choice in Settings, each fading in 250 ms, and instantly with animations removed;
      - no fade at start, for another setting, for Dark while the device is dark, or for the device turning light while Dark is chosen;
      - a change made while away, taken on at `Resumed`.
    - **The acceptance scenarios** (`tests/acceptance.rs`):
      - AC-05, now with the fade over within 300 ms and the folder and scroll position kept after it;
      - THEME-01: the choice in the frame, Dark's tokens and true black, Light staying light while the device is dark, and System following it again, with Settings still open;
      - THEME-06: edit mode and the scroll position below Settings; search with its query and results; Recent in close mode.
  - **Built.** `cargo clippy` for `aarch64-linux-android`, and `assembleDebug` and `assembleRelease`.
  - **Checked on a Galaxy S22 (One UI 8, Android 16)** with the debug build, switching the device with `cmd uimode night`:
    - **System:** the grid went from `#F2F2F2` with dark bar icons to true black with light ones, in the same process: nothing was recreated.
    - **AC-05:** with the grid scrolled to its "Game" folder and the folder open, the device turning dark faded the screen to Dark. A recording showed the old and the new screens crossing, done about 280 ms after the switch. One UI dims the whole screen briefly at the switch itself. The folder stayed open over the same rows.
    - **AC-06:** with Light chosen in Settings → Appearance → Theme, the device turning light and dark again left Settings light, with dark bar icons, and the choice checked.
    - **Search:** with "gm" typed and Gmail found, the device turning light kept the field, its text, its focus, the keyboard and the result; the cursor was in each theme's accent.
    - **The rename dialog:** with "Arcade" typed over "Game", the device turning dark opened the dialog again, dark, with "Arcade" and the accent's buttons. Cancel kept the name "Game".
    - **Afterwards** the device's dark theme was back on its schedule (`auto`), and this build stayed installed.
  - **Not checked on the device:** Android 10 and 11, whose edge glow `Themed` colours; TalkBack; recreation by other configuration changes, unchanged. The device matrix (#43) covers these.
- **Costs:**
  - **The window before the first frame** still follows the device: a cold start in Light on a dark device starts black for a moment. The choice is known only once the Rust app has read the settings file.
  - **The picture costs memory** for 250 ms: a screen-sized ARGB bitmap, about 10 MB on a 1080 × 2340 phone.
  - **Android's animation speed is still read as 1** until #33, so the fade plays even with animations removed on a device. The Rust app already makes it instant at speed 0.
  - **Kept in step by hand:**
    - slots 139 and 140, and the theme's numbers between `theme_code` and `HomeViews.THEMES`;
    - `isDark` between `Bridge.java` and `android/system.rs`;
    - `nativeSystemThemeChanged` and `nativeTheme` with `HomeActivity.java` and `HomeViews.java`.
