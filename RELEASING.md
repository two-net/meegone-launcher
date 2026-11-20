# Releasing

Before a public release, and before any store-listing change is submitted, go through this list and tick every item (#36, ADR 0037). CI checks what it can. The rest needs a person, and the release waits until every item is ticked.

The store listing is in `fastlane/metadata/android/`, in the layout F-Droid reads and `fastlane supply` uploads to Google Play.

## Checked by CI

- [ ] CI is green. `cargo test -p arch-check` checks these (ADR 0037):
  - the product is named "MeeGone Launcher" in the app and as the listing's title (LEGAL-01);
  - no text in the app or the listing names Nokia, Microsoft, MeeGo, Harmattan or the N9, except in the non-affiliation statement;
  - every bundled file is in `ASSETS.md` with its source and licence (LEGAL-02);
  - every bundled licence text is shown in About (LEGAL-06);
  - About and the listing's full description carry the non-affiliation statement (LEGAL-05) and the privacy notice (PRIV-04) word for word;
  - the listing's fields fit the stores' length limits.
- [ ] `cargo test -p arch-check` and `cargo test --workspace` also check the accessibility rules CI can (ADR 0038):
  - every colour the host draws is one of the theme's tokens, and every pairing of them meets its contrast in both themes (A11Y-03);
  - every text size is in sp (A11Y-04).

## Open questions that block a release

- [ ] #44 (Q1): "MeeGone" is cleared as a product name.
- [ ] #45 (Q2, LEGAL-04): the image source's terms allow the automated daily download and the image's use as a device wallpaper in a distributed app, and the attribution MeeGone shows, the feed's credit line in full, is the form they require.

## The store listing

- [ ] The name is "MeeGone Launcher". Nothing suggests MeeGone is made, endorsed or sponsored by Nokia, Microsoft or anyone else, and no third-party names are used as keywords (LEGAL-01).
- [ ] The text names Bing only as the source of the daily image, and Nokia and Microsoft only in the non-affiliation statement.
- [ ] There are no Nokia, MeeGo, Harmattan, Bing or Microsoft logos, icons, fonts, sounds, wallpapers or trade dress, and no N9 screenshots or photos, in any graphic or video.
- [ ] The icon, the feature graphic and every other graphic are original or openly licensed on compatible terms, and recorded in `ASSETS.md`.
- [ ] The screenshots show MeeGone itself:
  - with the daily image off, so no image from the source appears in them, since those images are third-party works that MeeGone doesn't redistribute (SRS 8.2);
  - on a device with few apps beyond the system's own;
  - with notifications made up for the purpose, never anyone's real messages.
- [ ] The store's privacy-policy field links to the same notice as About. Google Play asks for one from apps with notification or usage access.

## On a device, with the release build

- [ ] The launcher icon is MeeGone's own original icon, recorded in `ASSETS.md`, not Android's default.
- [ ] Settings → About shows the version, the privacy notice, the non-affiliation statement and "Open-source licences", whose texts open in full (SET-05, LEGAL-05, LEGAL-06).
- [ ] The image-of-the-day card, the image details screen, the past week and setup each show the image's attribution in full (LEGAL-03).
- [ ] `tools/a11y-audit.sh` passes with the debug build of the release's commit, on a test device with a folder, notifications and a work profile, and again with `--first-run` for setup's screens. It checks every screen in both themes at font scale 1.0 and 2.0: names, 48 dp targets, contrast, and text cut off or overlapping (A11Y-01 to A11Y-04, ADR 0038).
- [ ] With TalkBack on, the home views, a folder, edit mode, Recent's close mode and Settings read as ADR 0038 says: apps by name with "work" for a work app, "Folder: ‹name›, N apps", "Uninstall ‹app›" and "Remove ‹app› from Recent", and "Applications, 2 of 3", "Edit mode" and "‹app› removed from Recent" as they happen (A11Y-01, A11Y-08).
