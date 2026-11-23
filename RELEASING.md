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
- [ ] `cargo test -p arch-check` and `cargo test --workspace` also check the accessibility rules CI can (ADR 0038, ADR 0039):
  - every colour the host draws is one of the theme's tokens, and every pairing of them meets its contrast in both themes, the focus ring's included (A11Y-03, A11Y-06);
  - every text size is in sp (A11Y-04);
  - AC-19's tasks can be done with no gesture, and with animations removed every flow is over as it happens (A11Y-05, AC-18).
- [ ] `cargo test -p arch-check` and `cargo test --workspace` also check the localisation rules (ADR 0040):
  - neither the Java host nor the Rust app writes text for the user outside string resources, and every problem is worded from one (L10N-01);
  - every string resource has a comment for translators, numbers its arguments when it has several, and only the product's name is untranslatable;
  - no number reaches the screen in ASCII digits whatever the language (L10N-03);
  - right to left, the views' order, the swipes and edit mode's drags are mirrored (L10N-02).
- [ ] `cargo bench --bench budgets` and the APK job also check the performance budgets as far as CI can (ADR 0041):
  - with 300 apps, the Rust app's share of Home, a cold start, a frame, a keystroke and a launch each stays within a tenth of its budget (PERF-01 to PERF-06);
  - the release APK is 15 MB or less (PERF-08).
- [ ] `cargo test -p arch-check`, `cargo test --workspace` and the APK job also check the security and privacy rules (#41, ADR 0042):
  - the release APK asks for exactly the permissions SRS 8.1 lists, and nothing else (SEC-03);
  - other apps can reach only the home entry, the settings entry and the notification listener, each by its own intent filters (SEC-04);
  - no component is an accessibility service or a device administrator, or shows over the lock screen (SEC-03, SEC-05);
  - every connection is HTTPS with the system's certificates, with no exception for a domain or a debug build (SEC-01);
  - the release APK isn't debuggable, test-only or profileable, and holds none of the debug build's classes (SEC-06);
  - the shipped Java has no web view, links in text or device identifiers, connects only from the bridge, and logs no exception (SEC-02, PRIV-01, PRIV-02, PRIV-03);
  - a redirect never leaves the image feed's origin, and a day's requests all go to `https://www.bing.com/` (PRIV-02).

## Open questions that block a release

- [ ] #44 (Q1): "MeeGone" is cleared as a product name.
- [ ] #48 (Q5): the reference device is chosen. Every performance budget applies to it (SRS 1.3, PERF-01 to PERF-08).
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
- [ ] `tools/a11y-audit.sh` passes with the debug build of the release's commit, on a test device with Android 13 or later, a folder, notifications and a work profile, and again with `--first-run` for setup's screens. It checks every screen in both themes at font scale 1.0, at 2.0, and with animations removed: names, 48 dp targets, contrast, text cut off or overlapping, what screen readers and Tab reach, and the focus ring (A11Y-01 to A11Y-07, ADR 0038, ADR 0039).
  - It does so again in the pseudo-locales en-XA, whose text is about a third longer, and ar-XB, right to left (ADR 0040). In each pass, it checks the views' order and a finger's swipes, and search's folding. Below Android 13 it skips the pseudo-locales and says so; that doesn't count as passing.
- [ ] With TalkBack on, the home views, a folder, edit mode, Recent's close mode and Settings read as ADR 0038 says: apps by name with "work" for a work app, "Folder: ‹name›, N apps", "Uninstall ‹app›" and "Remove ‹app› from Recent", and "Applications, 2 of 3", "Edit mode" and "‹app› removed from Recent" as they happen (A11Y-01, A11Y-08).
- [ ] AC-19's six tasks, done three ways with no gesture: switch views, launch an app, move an icon, make a folder, clear Recent and open Settings (A11Y-05, A11Y-06, ADR 0039).
  - **With TalkBack:**
    - Touching empty space reads the view, such as "Applications, 2 of 3".
    - Its actions menu offers Next view, Previous view, Search apps and Bigger or More cards.
    - A double-tap there opens the home menu, or Edit Recent on Recent.
  - **With only a hardware keyboard:**
    - Tab, the arrows and Enter reach everything, and ← or → past a view's edge goes to the next view.
    - Menu or Shift+F10 opens a menu, and Esc is Back.
    - In edit mode, Ctrl with the arrows moves an app, and the app's menu makes a folder.
    - The ring shows wherever the focus is, and Meta+/ lists the keys.
  - **With Switch Access** (Universal Switch on Samsung): every element and each view's element can be reached and acted on.
- [ ] With Accessibility → Remove animations on, the same tasks and every screen are instant, or fades of 100 ms or less, and nothing is missing (AC-18, A11Y-07).
- [ ] With the device in Arabic or Hebrew (L10N-02, ADR 0040):
  - Recent is on Applications' left and Events on its right, and a swipe to the right goes from Applications to Recent.
  - With a hardware keyboard, → past a view's end goes to the next view, and ← past its start to the one before.
  - TalkBack's page actions go the same way.
  - Settings, a folder, edit mode and Recent's close mode lay out from the right, with each ✕ at the start corner.
- [ ] `tools/perf-bench.sh --record` passes on the reference device, with the benchmark build of the release's commit (`(cd android && ./gradlew installBenchmark)`), and its results file in `docs/benchmarks/` is committed (#40, ADR 0041). The run covers:
  - Home pressed 50 times from an app, 95th percentile within 300 ms (PERF-01, AC-20);
  - 20 cold starts: Applications with its icons within 1.5 s, and the saved layout of 300 apps within 500 ms without a cell moving (PERF-02, PERF-04);
  - 30 s each of view swiping, scrolling and motion by key, with at most 1 % of frames past their deadline and none over 700 ms (PERF-03, AC-20);
  - search within 50 ms of a keystroke, and a launch leaving within 50 ms of the tap (PERF-05, PERF-06);
  - at most 150 MB at rest with 300 apps, giving icons back when the system asks (PERF-07);
  - stored data within 50 MB and the APK within 15 MB (PERF-08).
- [ ] `tools/net-audit.sh --stop --record` passes with the release build of the release's commit, after a day of use started with `tools/net-audit.sh --start`, and its results file in `docs/privacy/` is committed (#41, ADR 0042). It needs PCAPdroid on the device and Wireshark's tshark. During the day, with the daily image on:
  - the daily job runs, and Update now is tapped once;
  - the details screen and More info are opened;
  - the past week is browsed, and an image is pinned and unpinned.

  Every connection goes to `www.bing.com` over HTTPS (PRIV-02, SEC-01).
- [ ] With no default browser set, More info offers only browsers, never another app that opens the site's links (SEC-02, ADR 0042).
- [ ] Dates, times and numbers (L10N-03, L10N-04):
  - With 24-hour time on, the Events alarm and "Last updated" show 24-hour times; with it off, 12-hour ones.
  - In Arabic, Events' counts use Arabic-Indic digits.
  - In Swedish, Settings → Applications → Hidden apps, and a first start's grid, put "Ö…" after "Z…".
  - Searching "lodz" finds an app called "Łódź…".
