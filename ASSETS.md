# Asset inventory

Every icon, illustration, font, sound and other file that MeeGone Launcher bundles is listed here with its source and licence (LEGAL-02, ADR 0037). Nothing is taken from Nokia, Microsoft or anyone else without a licence that allows it. No logo, icon, font (Nokia Pure included), sound, wallpaper, recorded animation or screenshot from the N9, MeeGo or Harmattan is used.

`tools/arch-check` enforces this list in CI. It fails if a file under `android/app/src/*/res` or `android/app/src/*/assets` isn't listed here with its source and licence, and also if a listed file is no longer bundled. Text configuration (`res/values*` and `res/xml`) isn't an asset and isn't listed.

## Bundled files

| File | Kind | Source | Licence |
|---|---|---|---|
| `android/app/src/main/res/raw/licence_mit.txt` | Licence text, shown in About (LEGAL-06) | The MIT licence as the Rust project publishes it with its standard library (`LICENSE-MIT` at github.com/rust-lang/rust) | Reproduced word for word, as the licence requires with the code it covers |
| `android/app/src/main/res/raw/licence_apache_2_0.txt` | Licence text, shown in About (LEGAL-06) | The Apache License 2.0, from apache.org/licenses/LICENSE-2.0.txt | Reproduced word for word, as the licence requires with the code it covers |
| `android/app/src/main/res/raw/licence_unicode_3_0.txt` | Licence text, shown in About (LEGAL-06) | The Unicode License v3, from unicode.org/license.txt | Reproduced word for word, as the licence requires with the data it covers |

## What isn't bundled

- **Icons.**
  - MeeGone has no launcher icon yet, so Android shows its default one. An original icon is added before release (`RELEASING.md`) and listed here.
  - The icons in Applications, Recent and Events belong to the installed apps. They are read from the system while MeeGone runs, and so is the work badge (APPS-20). None is bundled.
- **Shapes drawn in code.** Each is MeeGone's own original drawing, made of circles, lines and rounded rectangles in the theme's colours:
  - the ✕ badge of edit mode and close mode (`Badge.java`);
  - the icon plates, folder slots and new-app dot (`AppCell.java`);
  - the cards, markers, switches, cursor and button shapes (`GradientDrawable`s in the views).
- **Fonts.** None. Text uses the device's own system font (`Typeface.DEFAULT`, and `sans-serif-light` in setup).
- **Sounds.** None. MeeGone gives haptic ticks only.
- **Animations.** None recorded. Every motion is computed from the named springs and tweens of ADR 0034.
- **Images.** The daily image is downloaded from the image source while MeeGone runs, and shown with its attribution as the source provides it (LEGAL-03). It is never bundled or redistributed (SRS 8.2, LEGAL-04).

## Third-party code (LEGAL-06)

About → "Open-source licences" shows the three licence texts above, after a note on what they cover (`licences_intro`).

- **The Rust standard library** and the crates it is built from are by the Rust Project Developers and others. They are under the MIT licence or the Apache License 2.0, at the user's option. Its Unicode tables are © Unicode, Inc., under the Unicode License v3.
- **The toolchain's runtime pieces** linked into `libmeegone_launcher.so`, such as the Android NDK's LLVM libunwind, are under the Apache License 2.0 with LLVM Exceptions. That exception needs no notice for code compiled into a binary. The library links nothing else, apart from Android's own `libc`, `libm` and `libdl`, which the device provides.
- **The app's own crates and its Java host** use no third-party library (`DEPENDENCIES.md`). A third-party crate added to the app needs its licence text in `res/raw`, in About's licences (`HomeViews.LICENCES`) and in the table above.
