# ADR 0016: App search

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #15

## Context

Issue #15 adds search to Applications:
- Pulling down while the grid is at its top reveals a field and opens the keyboard (SRCH-01). The field follows the pull; past a threshold it springs open (ANIM-16).
- Results follow each keystroke and match labels whatever their case and accents. Matches at a word's start rank above matches inside a word, and ties go in the locale's alphabetical order (SRCH-02, L10N-04).
- The keyboard's action key launches the first result (SRCH-03).
- Back, Home or scrolling down past the field closes search and restores the grid and its scroll position (SRCH-04, HOME-07, HOME-08).
- Hidden apps, once hiding ships, are left out (APPS-19).
- Results update within 50 ms of a keystroke with 300 apps (PERF-05). The ranking is pure logic, tested without a device (MAINT-01).
- "Search the web for '…'" when nothing matches (SRCH-05, Could).

The constraints:
- **The app's crates use only the standard library** (DEPENDENCIES.md), which has no Unicode accent folding or collation. The locale's order already comes from the platform, as each app's collation key (ADR 0012).
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, and cells travel by revision (ADR 0010).
- **Rust never saw the grid's vertical drags.** `AppsGrid` is a `ScrollView`, and it takes a vertical gesture from the carousel as soon as it scrolls (ADR 0010).
- **No text was typed into the home views.** The manifest panned the window for the keyboard, and the folder's rename dialog has a window of its own (ADR 0015).
- **The web search adapter existed**, as `SystemConfigAdapter::web_search` and `has_browser`, but on Android it was a stub waiting for this issue.

## Decision

1. **The platform folds; Rust ranks.**
   - `App` gains `search_key`: the label folded for search. `Bridge` makes it for each catalogue record: NFKD, the nonspacing marks removed, ICU's full case folding, and runs of spaces collapsed to one. "Café" and " CAFE " are both `cafe`.
   - The catalogue record gains it as a 9th field. A record without it is left out, as one without a sort key is.
   - `AppsAdapter::search_key(text)` folds a query with the same `Bridge` method, so a query and the labels always agree. If the bridge can't answer, the query is only lowercased.
   - `meegone-apps-state::search::rank` is pure. It takes the grid's apps, those in folders too, each once, and matches the folded query in each `search_key`:
     - first, where it starts a word: at the start, or after anything that isn't a letter or digit;
     - then, where it is only inside a word;
     - each group in `alphabetical` order, the collation key's, which `reconcile` shares;
     - nothing for an empty query.
2. **Applications owns search.**
   - `AppsState` gains:
     - `search`: the query while search is open;
     - `results`: apps' cells, keeping the new-app dot;
     - the results grid's layout, scroll, and the field's band (`search_band`);
     - `web_search_offered`.
   - `Surface::Search` is a third surface beside the grid and the open folder. Its rows have no empty last row.
   - The intents:
     - `OpenSearch`: ignored in edit mode or with a folder open. It closes a menu and reads `has_browser()`.
     - `Search(query)`: ignored while search is closed. It folds the query through the adapter, ranks, and shows the results from their top.
     - `SubmitSearch { from }`: launches the first result from its icon, or searches the web when nothing matches and the offer shows.
     - `SearchWeb`, `CloseSearch`, `SearchLayout { layout, band }`, `ResultsScrolled`.
   - A refresh ranks again, so installs and removals reach the results.
   - Any launch closes search, so coming back shows the grid. A web search closes it too. A failure stays open and becomes `problem`.
   - Back closes the menu, then the folder, then search, then edit mode. Opening a folder, a menu or edit mode closes search first, such as a tap on a folder in the grid under an empty field.
   - Search never touches `scroll_px`. Closing it restores the grid where it was, even when a screen reader opened it part-way down.
3. **The shell owns the pull** (`search.rs` in `meegone-shell-state`).
   - It sees every touch, after the dragged icon and before the carousel. Once the finger passes the touch slop vertically, as the carousel would decline it, the pull takes the gesture:
     - *Open*: down, while Applications is at rest at the top of its grid, within a pixel, with nothing covering it, open over it or lifted, and the band laid out;
     - *Away*: up or down, while search is open with nothing typed.
   - `nativePointer`'s answer includes the pull, so `HomeViews` intercepts and the grid gets a cancel. At the top of Applications, the pull replaces the grid's stretch.
   - The band shows one for one with the finger, then a third of the rest, up to half a band more (`SEARCH_PULL_RESISTANCE`, `SEARCH_PULL_STRETCH`).
   - Lifting the finger decides the side:
     - opened: the whole band showed, or a fling down (500 dp/s);
     - closed: pushed all the way away, or a fling up;
     - otherwise it goes back to the side it was on.
   - A release that changes the side sends `OpenSearch` or `CloseSearch`.
   - The field springs with `SETTLE`, keeping the release speed unless it would overshoot by more than 3 % of the band. Stretched past the band, it heads straight back. With animations removed it is there at once.
   - A touch on the moving field holds it. Moving up or down pulls it from there; a tap or a sideways move lets it carry on, so a tap on the field as it opens never closes search.
   - `keep_search_in_step` springs the field when Applications opens or closes search itself: the screen-reader action, Back, Home, a launch, another view or Settings.
   - While search shows, which counts only while Applications is `Ready`:
     - the carousel is locked, as for a folder;
     - no long-press, home menu or cell menu is offered;
     - presses count on the results, or on the grid below an empty field.
   - A new layout keeps search and ends a pull (COMP-03).
4. **Screen readers.** The home views offer "Search apps" while `ShellState::search_offered`: on Applications, with nothing covering it or open over it, outside edit mode. The shell also refuses `OpenSearch` anywhere else (A11Y-05). The field is an edit box with a hint, "No apps found" is a polite live region, and the grid is hidden from screen readers while results replace it.
5. **The host.**
   - **`SearchPanel`**, a child of the Applications page above the grid and below Done and the open folder, holds:
     - the field, a pill in the surface colour, 48 dp tall, 8 dp below the safe area and 16 dp in from its sides, with the hint "Search apps";
     - the results' `AppsGrid`, whose cells start below the band;
     - "No apps found" and "Search the web for “…”" in the accent colour.
   - While the field is out of sight the panel is invisible but laid out, so the results grid reports the band with its layout.
   - The field slides down by the frame's reveal and fades in with it. The grid below an empty field moves down by the same amount. Once something is typed, the results show and the grid is hidden but keeps its scroll position.
   - The field is a filter field without suggestions, with Go as its action. Each change of its text goes to Rust. Go, or a hardware Enter, sends the first result's icon rect.
   - When search opens, the field takes focus and the keyboard. When it closes, the keyboard goes and the field empties without sending anything. A new `HomeViews`, such as after a theme change, refills the field from `nativeSearchQuery()`.
   - **The keyboard's height.**
     - The manifest asks for `adjustResize`, so the keyboard arrives as an inset; edge to edge, the window itself never resizes.
     - Only the results grid takes it, as bottom padding, so the last results scroll above the keyboard.
     - Android 10 reports the keyboard inside the bottom inset. The safe area takes away whatever exceeds the stable inset, so the keyboard never changes the pages' safe area or Rust's layout.
   - Search's cells have no menu and no edit actions.
   - **Web search.** `Bridge.hasBrowser()` asks whether anything opens an `https:` link. `Bridge.webSearch` starts `ACTION_WEB_SEARCH` with the query, which is never logged. No `<queries>` entry is needed to start it. If nothing handles it, its message is a toast.
6. **The host contract.**
   - The frame grows to 78 slots:
     - 73: search's bits: 1 open, 2 results shown, 4 nothing found, 8 web search offered, 16 pulling, 32 offered to screen readers;
     - 74: how much of the band shows, in px;
     - 75–77: the results' revision, scroll and rows.
   - Surface 2 is search's results, in every grid native. `nativeAppsLayout` takes the band as surface 2's panel rect.
   - New natives on `HomeViews`: `nativeOpenSearch`, `nativeSearchChanged(String)`, `nativeSearchSubmitted(int, int, int, int)`, `nativeSearchWeb` and `nativeSearchQuery`.
   - New `Bridge` methods: `searchKey(String)`, `hasBrowser()` and `webSearch(String)`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decisions 2 and 5 | The frame has 78 slots. `nativePointer` says the gesture is taken while the search field is pulled, too. |
| 0009, decision 2 | The window asks for `adjustResize`. The keyboard's inset goes only to search's results, and on Android 10 the safe area leaves it out. |
| 0010, decisions 1 and 5 | The catalogue's records have a 9th field, the search key. Grids name a third surface, search's results. At the top of Applications, a pull reveals search instead of stretching the grid. |
| 0012 | The platform makes the search key as it makes the collation key; Rust only ranks. |
| 0015, decision 7 | Back closes the menu, then the folder, then search, then edit mode. An open folder and open search never show together. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| ANIM-23: lists stretch at their ends | At the top of Applications, a pull reveals the search field instead. The bottom still stretches, as do the results and the open folder. |
| SRCH-04, HOME-08: Back closes search | While the keyboard shows, the first Back is the keyboard's own and hides it. The next one closes search. |
| SRCH-04: "scrolling down past the field" closes search | While nothing is typed, pushing the field up past its band closes search. Once something is typed, the results scroll under a field that stays, and Back or Home close search. |
| APPS-19: hidden apps are left out of search | App hiding isn't built yet. Search's candidates are the grid's apps, so an app the grid leaves out will be left out of search too. |
| A11Y-06: everything works with a hardware keyboard | Enter launches the first result. Typing on Applications doesn't open search yet; the screen-reader action and the pull do. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - The ranking: word starts before insides, ties in the collation keys' order, apps in folders once each, work apps, unavailable apps, new-app dots, and 300 apps.
    - Search's state: opening, typing, refreshing, launching, web search and its failures, Back's order, and what closes it.
    - The pull: following the finger, the stretch, both thresholds, flings, at most 3 % overshoot, catching, cancelling, removed animations, and where a pull counts.
    - The shell's lock, menus, presses, Home, Back, other views, Settings, layouts and a failed Applications.
    - The issue's three acceptance criteria, and SRCH-01, SRCH-04 and SRCH-05, in `tests/acceptance.rs`.
  - The ranking runs on keys folded once per catalogue read. A keystroke costs one fold through the bridge and a scan of short strings.
  - Checked on a Galaxy S22 (One UI 8, Android 16), with MeeGone as the default home app:
    - A pull at the top of Applications slides the field in and moves the grid down. Past the band it springs open and Gboard rises.
    - "ma" shows LINE MAN, Magnum3.0 and Maps, then Gmail and Smart Switch. "CLO" finds Clock.
    - Enter launches the first result, and so does a tap on a result. Home then shows the grid with search closed.
    - The first Back hides the keyboard, and the second closes search with the grid at its top. Home closes search too.
    - Pushing the empty field up closes search. A pull further down the grid only scrolls it, and a sideways swipe with search open doesn't change view.
    - "zzqx" shows "No apps found" and "Search the web for “zzqx”". With no default search app set, Android asks which app should search, and search closes behind it.
- **Costs:**
  - Folding is the platform's, so only the fake's stand-in is tested without a device. No app on the test device has an accented label, and adb can't type one, so "cafe" finding "Café" is checked only through the fake. The device matrix (#43) covers it.
  - PERF-05 is measured only on the reference device (#48), with the benchmarks of #40.
  - Not checked on a device yet: TalkBack's "Search apps", the field refilled after a theme change (#32 isn't wired), Android 10's keyboard inset, right to left, a tablet, and removed animations (#33). The device matrix (#43) covers them.
  - Results have no long-press menu, and can't be dragged.
  - The frame slots, the catalogue's records, the surfaces and the natives must agree between Java and Rust. Nothing checks that at build time.
  - A pull, like the carousel, starts only from a grid at rest at its top. A fling that reaches the top doesn't turn into a pull in the same gesture.
