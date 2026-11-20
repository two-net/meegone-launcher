# ADR 0038: Named, reachable, legible and scalable

**Status:** Accepted · **Date:** 2026-11-21 · **Issue:** #37

## Context

Issue #37 asks that everyone can perceive and operate MeeGone, including with a screen reader, large text or low vision. Its requirements are Must, except A11Y-08, which is Should:
- **A11Y-01:** every interactive element has an accessible name: apps by label ("Mail, work"), folders as "Folder: ‹name›, 4 apps", and ✕ as "Uninstall Mail" or "Remove Mail from Recent".
- **A11Y-02:** touch targets are at least 48 × 48 dp.
- **A11Y-03:** text contrast is at least 4.5:1, and 3:1 for large text and meaningful graphics, in both themes.
- **A11Y-04:** layouts work at system font sizes up to 200 % without overlap, and truncated labels stay fully available to screen readers.
- **A11Y-08:** state changes are announced, such as "Applications, 2 of 3", "Edit mode" and "Mail removed from Recent".
- **Acceptance:** automated accessibility checks pass on every screen in both themes, and at 200 % no text overlaps on any home view, folder, menu, Settings or setup screen.

Gestures' alternatives, keyboards, switch access and reduced motion are #38's (A11Y-05 to A11Y-07). The device matrix is #43's.

Each feature had already done most of this as it was built:
- The names above are in place.
- "Applications, 2 of 3", "Edit mode", "Editing Recent" and "Mail removed from Recent" are announced through a live region.
- Every text is sized in sp, and the grid's rows grow with the font.
- `tokens.rs` held the text colours to 4.5:1 and the accent to 3:1.

An audit of the code, then the audit below run on a phone, found these gaps:
- **Names:** some names and announcements dropped ", work": the ✕'s actions, "removed from Recent", the moves, and menus' titles. In edit mode an app offered a tap that does nothing. The open folder's name didn't say that a tap renames it. Six full-screen containers, clickable only so that touches stay on them, read as one big button. "Unavailable", "Expanded" and "Collapsed" were silent on Android 10, which has no state description.
- **Targets:**
  - The ✕ in edit mode took a 48 dp square in the cell's own coordinates, but the cell is drawn at 92 %, so it was about 44 dp on screen.
  - Recent's first card was measured once with no plate, to find its caption's height. It could keep that height, 39 dp, leaving its plate outside its touch target.
  - Dialog buttons are 36 dp tall on the test phone.
- **Contrast:**
  - Unavailable apps were drawn at half opacity: their label at 3.89:1 in Light, and their dot and Recent plate at about 2.1:1. Hidden apps' labels were at 3.95:1.
  - Dialogs used the device's own colours, where MeeGone's accent can fall below 4.5:1: about 3.8:1 on a Material 2 dark dialog and 4.3:1 on a Material 3 light one.
  - Done's press lightened its pill under its text.
- **Scaling:**
  - At 200 %, the banner's and setup's two buttons, and a section header with Clear, were squeezed or pushed off screen.
  - The menu card couldn't scroll, so a long app menu in a short window lost items.
- **Announcements:**
  - The same words twice in a row weren't read again.
  - Search announced itself as soon as it was pulled, not once it opened.
  - Closing a folder, search or a menu left screen readers' focus nowhere.
- **Checks:** nothing checked any screen as drawn.
  - CI has no Android device, since the app is arm64-only (ADR 0005).
  - The host uses no AndroidX (ADR 0005, DEPENDENCIES.md), so Google's Accessibility Test Framework would need test libraries of its own.

## Decision

1. **Names are made in one place.**
   - `A11y.appName` gives an app's name as screen readers read it, with "work" for a work-profile app. The cells, Recent's cards, Events' rows, Hidden apps, the ✕'s actions, every announcement and menus' titles use it.
   - `A11y.describe` sets a name and a state. On Android 10 the state follows the name: "Mail, Unavailable" (`described_state`).
2. **The ✕ stays a named action,** as in Android's own launchers: "Uninstall Mail, work" in edit mode and "Remove Mail, work from Recent" in close mode. It isn't a focusable element of its own, so edit mode adds no swipe stops. In edit mode an app no longer offers a tap; its Move actions stay, and a folder still opens.
3. **Containers that only keep touches aren't buttons.** Settings, the menu card, the image details screen, the past week, setup and the banner stay clickable, because the home views take empty space under a finger for their own long-press (ADR 0008). `A11y.notAButton` takes the click out of what screen readers see. The open folder's name offers "Rename folder" and is a heading, as is Settings' title.
4. **One announcer.**
   - Every announcement goes through `HomeViews.announce`, into the polite live region.
   - The same words again get a zero-width space, so they are read again.
   - Search's pane title is set only once search is open.
   - When a menu, a folder or search closes, screen readers' focus goes back to the cell or row it was for, or to the grid's first visible cell. They then read it, for example "Folder: Games, 3 apps". This is the platform's way of saying a pane closed.
   - The open folder's title stays "Folder: ‹name›": its cell says how many apps it holds before it opens and after it closes, and changing a visible pane's title would announce it again.
5. **Targets are 48 dp on screen.**
   - `A11y.TARGET_DP` is the only 48 dp constant.
   - `Badge.target` divides the ✕'s square by its view's scale.
   - The alarm row is at least 48 dp wide.
   - Recent's cards are measured again whenever their plate changes.
   - Dialog buttons are given a 48 dp minimum.
6. **Every colour comes from the tokens, and every pairing is held to its ratio.**
   - `PAIRINGS` in `crates/core/ui/src/tokens.rs` lists each pairing of tokens the host draws, the Java that draws it, and its need: text 4.5:1, large text or a graphic 3:1. One test checks them all in both themes.
   - Unavailable apps keep a grey icon at half opacity, but their label, dot and Recent plate are drawn in the secondary text colour (APPS-09).
   - Hidden apps' unavailable labels are drawn the same way.
   - `Dialogs` lays dialogs out as the device's light or dark ones, but draws them in MeeGone's colours: a 12 dp card in the surface colour over the scrim, text in the primary colour, buttons and radio buttons in the accent.
   - Every press is `SettingsRows.pressed`, the primary text colour at `PRESSED_PERCENT` (12 %), Done's included.
   - Not held to a ratio, each for its reason:
     - **Press highlights** last only while a finger covers them. Primary text and Done's text are still held to 4.5:1 on them. Holding the secondary and accent text too would need a lighter Dark secondary colour, which then breaks the view indicator's 3:1.
     - **Surfaces against the background, dividers, and the rings around folders and the ✕** only decorate. Cards, menus, the search field and folders are known by their text.
     - **Disabled rows**, at 38 %, are inactive, which WCAG exempts. The reason they are disabled is shown in secondary text.
     - **App icons** are identified by their labels, and each app's accent has its own 3:1 rule (THEME-07).
   - The focus indicator is #38's (A11Y-06).
7. **Large text has room.**
   - `ButtonRow` puts buttons side by side at the end while they fit, and otherwise one above another, in the same order. The banner and setup use it.
   - `SectionHeader` measures its button first, and the title wraps into what is left.
   - The menu card's items scroll inside it.
8. **CI checks what it can without a device** (`cargo test`, `cargo test -p arch-check`):
   - `PAIRINGS` in both themes, the press, and Done pressed.
   - `tools/arch-check/src/a11y.rs` fails on any colour literal in the Java host outside `COLOUR_ALLOWED`. Each allowed colour has its reason, and an entry that matches nothing is reported. The allowed colours are:
     - ripple masks;
     - the transparent status line;
     - the black scrim;
     - the legacy icon's plate;
     - the press;
     - the decorative rings;
     - the audit's own arithmetic.
   - The same module fails on any `setTextSize` in a unit other than sp.
9. **A device checks every screen as drawn.** In debug builds only (SEC-06):
   - **`A11yAudit`** checks a window the way Google's Accessibility Scanner does, with nothing but the Android SDK:
     - **Names:** every element screen readers can act on has a name, and every action it offers has a label.
     - **Targets:** every element taking a tap or a long-press, and every target a view draws itself (`Audited`), is 48 dp each way on screen.
     - **Contrast:**
       - Every text on top is compared, in the window's own pixels, with what is behind it. Dialogs are drawn into the pixels, as their windows have room for a shadow around them.
       - Large text needs 3:1: at least 24 dp, or 18.66 dp bold, as drawn, which is 18 pt or 14 pt bold in WCAG's CSS pixels.
       - Disabled text is exempt.
     - **Scaling:**
       - No text is cut off by its view or by an ancestor that doesn't scroll.
       - No two texts overlap.
       - A text cut short reaches screen readers whole: through its own node, or through the description of the element they read instead.
   - **`A11yTour`** goes through every screen in Light, then Dark. It gets there as a screen reader would, through the home views' and the elements' actions, and waits for everything to rest before each check. Every scrolling screen is checked a page at a time.
     - **The screens:**
       - Events and a notification's menu;
       - Applications, the home menu and an app's menu;
       - edit mode, a folder, a folder in edit mode, and the rename dialog;
       - search with results, and with nothing found;
       - Recent in both densities, and in close mode;
       - Settings, with the region, licences and Reset launcher dialogs, each cancelled;
       - the image details screen and the past week;
       - setup's steps, while setup shows.
     - It puts back the theme and the density it found.
   - **`tools/a11y-audit.sh`** starts the tour at font scale 1.0, then 2.0, puts the device's own scale back, and fails on any `FAIL` line.
     - `--first-run` clears MeeGone's data first, so setup is checked too.
     - `RELEASING.md` asks for both before a release.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, consequences | Accessibility still comes from the platform's views, and now the debug build's audit checks it on a device (decision 9). |
| 0023, decision 2 | An unavailable app's card has a grey icon at half opacity, on a plate in the secondary text colour, with its label in it (decision 6). |
| 0033 and 0035, dialogs | Dialogs keep the device's light or dark layout, but are drawn in MeeGone's colours, with 48 dp buttons (decisions 5 and 6). |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| A11Y-03: text contrast in both themes | Press highlights hold only primary text and Done's text to 4.5:1 (decision 6). Secondary and accent text show under a finger at 3.6:1 to 4.4:1. |
| AC: automated accessibility checks pass on every screen in both themes | They run on a device from one command, not in CI, which has no device for the arm64-only app (ADR 0005). CI checks the colours and the text sizes in code. |

## Consequences

- **Benefits:**
  - **Every pairing of colours is checked in CI.** No colour reaches the screen without a token or a stated reason.
  - **Names, targets, contrast and large text are checked on every screen, in both themes, with one command.**
  - **The audit found what reading the code didn't:** Recent's first card at 39 dp, and the 36 dp dialog buttons.
  - **Each check was seen to fail when broken on purpose, then pass again.**
    - In the audit: app cells without names, menu items 30 dp tall, and section headers in the divider colour.
    - In CI: a stray `Color.RED` and `COMPLEX_UNIT_PX`, and a lighter Light accent.
- **Checked on a device:** a Galaxy S22 on Android 16, with MeeGone as its home app, at font scale 1.0 and 2.0, in both themes. The tour checked 116 screens, with about 5,200 checks, and every check passed.
- **Costs:**
  - **The audit needs a device.** It checks only the screens the device's state allows. On the test phone there was no folder and no notification, and setup had been done. Those screens need a device with them, or `--first-run` on a test device.
  - **The audit reads pixels and views, not meaning.**
    - A name that is present but unhelpful passes.
    - So does a text cut short whose element has some description.
    - TalkBack itself is still checked by hand (`RELEASING.md`, #43).
  - **Dialogs reach into the platform's layout** for the title's id, `alertTitle`. A device without it keeps its own title colour.
  - **Not checked on a device yet:**
    - TalkBack's actual speech;
    - Android 10's state in names;
    - right to left;
    - a tablet;
    - a work profile's ", work" names. The test phone's other profile, a Secure Folder, can't be reached from adb.

    The device matrix (#43) covers them.
