# ADR 0011: One icon shape and app accent colours

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #10

## Context

Issue #10 needs:
- Every icon in one rounded-square shape, with corners at about 24 % of its size. Layered (adaptive) icons are masked to it, and older single-image (legacy) icons sit on a neutral plate of the same shape (APPS-15, SRS 5.2).
- One accent colour per app, derived once from its icon and reused in Events (the group marker) and Recent (the card fill) (APPS-17, EVT-09, REC-04, SRS P4).
- The accent derived again when an update changes the icon.
- Each theme adjusting the accent to keep at least 3:1 contrast against its background, `#000000` in Dark and `#F2F2F2` in Light (THEME-07).
- The derivation and the adjustment as pure logic, tested without a device.

The constraints:

- **Icons stay in Java** (ADR 0010). They are bitmaps that only the views use.
- **The catalogue is read on the main thread** (ADR 0010), and both Applications and Recent read `App.accent` from it through `AppsAdapter::launchable_apps`. Loading 300 icons there would stall the main thread.
- **There are no Java tests.**

## Decision

1. **Icons have one shape.**
   - `AppIcons.shaped` draws every icon into a rounded square with corners at `CORNER`, 24 % of its size. `AppCell`'s placeholder uses the same constant.
   - An adaptive icon's background and foreground layers are drawn at 1.5× the size, because they are 108 dp around the 72 dp the shape shows, and then masked.
   - Any other icon is a legacy single image. It is drawn at 70 % of the size, keeping its proportions, on a white plate. White is the plate Android's own launcher gives such icons, and it is the same in both themes, so an icon and its accent never depend on the theme.
   - The mask is painted through a `BitmapShader`, which smooths its edge. `clipPath` isn't anti-aliased.
   - Icons are read unbadged and shaped first. `PackageManager.getUserBadgedIcon` then adds the work badge, so the shape never cuts it off (APPS-20).
   - In debug builds, the extra `io.github.twonet.meegone.debug.LEGACY_ICONS` draws every icon as a legacy one, so the plate can be checked on any device (SEC-06: release builds ignore it).
2. **Java samples the icon and Rust picks the colour.**
   - `AppAccents`, owned by `Bridge`, keeps each app's accent in memory for the life of the process (SRS 7.4).
   - `Bridge.launchableApps` gives each app record a sixth field: its accent as a decimal `0xRRGGBB`, or empty until it has been derived. The Rust catalogue reads an empty or malformed field as `accent::NEUTRAL` and keeps the app.
   - Apps without an accent go to the sampler in one batch per catalogue read, each app at most once. The sampler is a thread of its own at the lowest priority, apart from `AppIcons`' loader, so the grid's icons never wait for it (PERF-04).
   - For each app, the sampler draws the icon with `shaped` at 48 px, reading it at medium density, so every device derives the same accent. It passes the pixels to `nativeAccent`, which runs `meegone_core_data::accent::derive`.
   - The batch's accents reach the main thread together. `Bridge` then reports the catalogue changed, once, as it does for an install, and Applications and Recent read it again.
   - An icon that can't be read is remembered as "none", so its app stays neutral without being asked for again.
3. **`nativeAccent` runs off the main thread.** It is the one native method that does. It is pure: it reads only its `int[]` and never touches the `Launcher`, which belongs to the main thread. A failure becomes a Java exception, which `AppAccents` treats as "none". The JNI binding gains `GetIntArrayRegion`.
4. **An update derives the accent again.** `Bridge`'s `LauncherApps.Callback` forgets a package's accents when it is added, removed, changed or available again, before it reports the change. Each request carries a number, so a sample that an update overtook is dropped when it arrives.
5. **The accent is the icon's most common colourful hue** (`accent.rs`).
   - Pixels under 50 % opacity, such as the shape's corners, are left out.
   - A pixel is colourful at an HSV saturation of 0.3 or more and a value of 0.25 or more.
   - If at least 10 % of the opaque pixels are colourful, they are grouped into 24 hue sectors of 15°, centred on their hues so reds either side of 0° fall together. The accent is the mean colour of the fullest sector.
   - Otherwise the icon is greyscale. Its opaque pixels are grouped into 8 luminance bands, and the accent is the mean colour of the fullest band. A near-white icon gives near-white, and a near-black one near-black.
   - Ties go to the first sector or band, so an icon always gives the same accent. An icon with nothing opaque gives `NEUTRAL`, `#808080`.
   - So a coloured logo on a white plate gives the logo's colour. A literal "most common pixel" would give white to most of today's icons, and colour would no longer tell apps apart (SRS P4).
6. **Each theme adjusts the accent when it draws it** (THEME-07).
   - The colour maths is on `Rgb` (`colour.rs`): `luminance`, `contrast_ratio` (moved from `meegone-core-ui`) and `with_contrast(background, min)`. Being methods, they reach the UI crates with the type.
   - `with_contrast` returns the colour itself if it reaches `min`. Otherwise it keeps the HSL hue and saturation and bisects the lightness towards black against a light background, or towards white against a dark one. The result is the nearest lightness whose rounded colour reaches `min`.
   - `Tokens::app_accent` applies it against the theme's background with `APP_ACCENT_CONTRAST`, 3:1. The state keeps the accent as derived, so a theme change needs nothing derived again (THEME-06).

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0005 | One native method, `AppAccents.nativeAccent`, runs off the main thread. It is pure and never reaches the `Launcher`. |
| 0010, decision 1 | The catalogue's app records have a sixth field, the accent. It is no longer always neutral. |
| 0010, decision 7 | Icons are read unbadged, drawn in MeeGone's shape, then badged. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| APPS-17: the accent is "derived once from its icon" | It is derived once per process and icon: memory only, like the rest of `App` (SRS 7.4). The first catalogue after a start shows neutral accents for well under a second, until the batch arrives. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - The accent of solid, logo-on-plate, near-white, near-black, transparent and tied icons.
    - A sweep of 4096 colours reaching 3:1 against both backgrounds, by no more than it needs, keeping hue.
    - The catalogue's accent field.
    - The issue's acceptance scenarios, in `tests/acceptance.rs`.
  - Checked on a Galaxy S22 (One UI, Android 16), with about 70 apps:
    - Every icon has the same shape.
    - With the debug extra, every icon sits on the white plate in that shape.
    - All the accents were derived in one batch, 0.35 to 0.55 s after start.
    - Reinstalling one app derived its accent again, alone.
- **Costs:**
  - Each icon is read twice: at grid size for the grid, and at 48 px for its accent. The second read happens once per process, in the background.
  - Shaping draws each icon twice, into the content bitmap and then through the mask. Icons still cost about 110 KB each at 3× density, as in ADR 0010.
  - The record's accent field must agree between `Bridge.java` and `catalogue.rs`, like its other fields. Nothing checks that at build time.
  - Events and Recent don't draw yet. Their marker and card call `Tokens::app_accent` when #19 and #22 draw them.
