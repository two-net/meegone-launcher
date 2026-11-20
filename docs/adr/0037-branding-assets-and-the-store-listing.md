# ADR 0037: Branding, assets and the store listing

**Status:** Accepted · **Date:** 2026-11-21 · **Issue:** #36

## Context

Issue #36 (Must) asks MeeGone to evoke the N9 without borrowing anything that belongs to Nokia, Microsoft or anyone else (SRS C6, section 5.4, LEGAL-01 to LEGAL-06, PRIV-04).
- **The name (LEGAL-01)** is "MeeGone Launcher". MeeGone doesn't use the Nokia, MeeGo, Harmattan, Bing or Microsoft names in a way that suggests affiliation, nor their logos or trade dress.
- **No Nokia-owned assets (LEGAL-02):** no icons, fonts (Nokia Pure included), sounds, wallpapers or recorded animations. Every icon, illustration and sound is original or openly licensed on compatible terms.
- **Attribution (LEGAL-03)** is shown as the image source provides it.
- **The image source's terms (LEGAL-04)** are checked before public release.
- **About (LEGAL-05, LEGAL-06, PRIV-04)** states "MeeGone Launcher is an independent project and is not affiliated with or endorsed by Nokia or Microsoft.", lists the licences of every third-party component and shows the privacy notice. The privacy notice is in the store listing too.
- **Acceptance:** an asset inventory records the source and licence of every bundled icon, illustration, font and sound, and the store listing is reviewed against the list above before submission.

Most of this was already true:
- About has shown the notice, the statement and the licences since ADR 0035.
- The attribution has been shown whole since ADR 0022 and ADR 0031, as `bing.rs` rejects text rather than cutting it short.
- Nothing is bundled but three licence texts.

The gaps were these:
- there was no inventory and no store listing;
- nothing kept a later change from adding an unrecorded asset or a third-party name;
- the app's label was "MeeGone".

## Decision

1. **The label is the product's name.** `app_name`, which the system shows under the icon, in the home-app choice and in App info, is "MeeGone Launcher" (LEGAL-01). MeeGone's own sentences, such as "Welcome to MeeGone", keep the short form: it names the same product, and nothing else is called MeeGone.
2. **Third-party names appear only where they must.**
   - Nokia and Microsoft appear only in the non-affiliation statement.
   - MeeGo, Harmattan and the N9 don't appear in anything a user sees: the app's text, the store listing and its graphics. The SRS and code comments may name them, as no user sees those.
   - Bing is named only to say where the daily image comes from, as in "Bing's image of the day", because the image source must be named for its attribution to make sense (LEGAL-03). The statement says MeeGone isn't affiliated with Microsoft.
3. **`ASSETS.md` is the asset inventory.** It sits beside `DEPENDENCIES.md` and works the same way.
   - It has a table of every bundled file, with its kind, source and licence, keyed by its path.
   - It says what isn't bundled and why:
     - the launcher icon, which doesn't exist yet;
     - the installed apps' icons and the work badge, read from the system;
     - the shapes MeeGone draws in code;
     - the system font;
     - no sounds, and no recorded animation;
     - the daily image, downloaded and never redistributed.
   - It names the third-party code behind About's licences, including the toolchain's runtime pieces.
     - The built library links the Android NDK's LLVM libunwind statically, under the Apache License 2.0 with LLVM Exceptions, which needs no notice in binary form.
     - Otherwise it needs only the device's `libc`, `libm` and `libdl`.
   - A "bundled file" is anything in a source set's `res` or `assets` directory, except text configuration (`res/values*`, `res/xml`) and hidden files, which the Android build skips.
4. **The store listing lives in the repository,** in the fastlane metadata layout: `fastlane/metadata/android/en-US/title.txt`, `short_description.txt` and `full_description.txt`.
   - F-Droid reads this layout as it is, and `fastlane supply` uploads it to Google Play.
   - The full description ends with About's privacy notice and the non-affiliation statement, word for word (PRIV-04, LEGAL-05).
   - There are no graphics yet (decision 7).
5. **`tools/arch-check` checks the legal rules,** in a `legal` module of pure functions, run on the repository by `tests/legal.rs` in the CI step that already runs arch-check:
   - `app_name` is "MeeGone Launcher", and so is the listing's title (LEGAL-01).
   - `about_independent` is the LEGAL-05 sentence exactly.
   - No text resource (`res/values*/*.xml`) and no listing file names Nokia, Microsoft, MeeGo, Harmattan or N9 outside that sentence.
     - The names are matched as whole words in any case, so "MeeGone" passes.
     - XML comments are skipped.
   - Every bundled file has a row in `ASSETS.md` with a source and a licence, and every row is a bundled file (LEGAL-02).
   - Every bundled `res/raw/licence_*` text is in `HomeViews.LICENCES`, so About shows it (LEGAL-06).
   - The listing's full description holds `about_privacy_notice`, read as Android reads it, and the LEGAL-05 sentence, word for word but with line breaks free.
   - Each field fits Google Play's limits: 30, 80 and 4,000 characters.

   The table-row reading, the file walk and the relative paths are shared with the dependency rules.
6. **What a check can't decide goes into `RELEASING.md`,** a checklist ticked before every release and listing submission. It holds:
   - #36's list for the listing and its graphics;
   - #44 (Q1, the name) and #45 (Q2, LEGAL-04, the image source's terms) answered;
   - the privacy-policy link;
   - checking About and the attribution on a device.

   Screenshots are taken with the daily image off. Its images are third-party works that MeeGone doesn't redistribute (SRS 8.2), and a store screenshot would.
7. **The launcher icon comes later.** Android shows its default icon until an original one is drawn. `RELEASING.md` blocks a release without it, and the inventory's check makes sure it gets a row.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0035, departures | PRIV-04's notice is in the store listing too (decision 4), so that departure is closed. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| LEGAL-04: the image source's terms checked before public release | Not something code can check: `RELEASING.md` blocks a release until #45 is answered. |
| LEGAL-01: the name "MeeGone Launcher" | The app's own sentences say "MeeGone" for short (decision 1). |
| Section 5.4: all icons original or openly licensed | True of what is bundled. There is no launcher icon yet (decision 7). |

## Consequences

- **Benefits:**
  - **Checked in CI, without a device:**
    - the name in the app and the listing;
    - no third-party names in user-facing text;
    - the inventory matching the bundled files;
    - every licence text shown in About;
    - the notice and the statement in the listing, word for word with About;
    - the listing's lengths.
  - **Each check was seen to fail** when broken on purpose, then pass again: a stray `res/drawable` file, "Nokia N9" in a string, and one word changed in About's privacy notice.
  - **The listing and About can't drift apart.** The notice is written once in `strings.xml`, and the check fails if the listing's copy differs.
- **Costs:**
  - **The marks check reads text, not meaning.** A third-party name spelt differently, or shown in a graphic, gets past it. `RELEASING.md` covers graphics and the listing by hand.
  - **The listing is English only.** Each language added later (#39, #50) needs its own folder, and the check will need to read it.
  - **The label is longer.** Narrow grids in other launchers may cut it short, as in "MeeGone Laun…".
