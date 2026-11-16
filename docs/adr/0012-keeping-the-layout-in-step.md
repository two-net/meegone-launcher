# ADR 0012: Keeping the layout in step with the installed apps

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #11

## Context

Issue #11 makes the user's arrangement last, and keeps it in step with what is installed. It needs:
- The first grid alphabetical by the locale's order (APPS-04, L10N-04), with the default phone, messaging, browser and camera apps first (APPS-05, Could).
- New apps at the end (APPS-06), each with a dot until it is first opened (APPS-07, Could).
- An uninstalled app gone at once, also from its folder, and an updated app keeping its place with its new label and icon (APPS-08, FOLD-06).
- The layout kept across restarts and updates, and reconciled with the installed apps at every start (APPS-18).
- An unreadable layout rebuilt alphabetically, keeping what can be recovered, without crashing repeatedly (REL-02).
- A new icon that scales and fades in, and a removed one that shrinks away while the grid closes up, over 200 ms (ANIM-21).
- Reconciliation as pure logic, tested without a device (MAINT-01).

ADR 0010 already reconciled the saved order with the catalogue, collapsed folders and refreshed on `AppsChanged`. What it lacked:
- **The locale's order.** It sorted by lowercased label.
- **A first start told apart from a damaged file.** Both loaded as an empty layout.
- **The install time.** It was always 0.
- **The motion.**
- **Stable cell views.** `AppsGrid` rebound its views by position, so a removal mid-grid made every later icon flash its placeholder.

The constraints:
- **The app's crates use only the standard library** (DEPENDENCIES.md), so there is no Unicode collation in Rust.
- **Exactly three stores** (ADR 0004): the new-app marks have to live in the layout.
- **Motion is state** (ADR 0006), and the host draws by revision (ADR 0010).
- **After a restart, a work profile with its own lock lists no apps until it is unlocked.** Taking that silence for uninstalls would move every work app to the end.

## Decision

1. **Load outcomes.**
   - `format::read` tells a missing file from a readable one and from an unreadable one, and keeps the unreadable bytes. `load` is built on it, so settings and the image cache are unchanged.
   - `LayoutStore::load` returns `Missing`, `Read` or `Recovered`, or fails when storage itself can't be read.
2. **Recovery** (REL-02).
   - `format::salvage` reads an unreadable file record by record. The header must be whole and no newer than today's version. The checksum is ignored, a last line without its line break is dropped, and each record is migrated and read on its own.
   - The layout's records stand alone (`app`, `folder` and `new`), so recovery keeps every cell that still reads, in order. Parsing a whole file stays strict.
3. **Format v2.**
   - `Layout` gains `new`, the apps not yet opened, written as `new <app>` records after the cells.
   - The v1→v2 migration is the identity, because a v1 body is a v2 body without marks. `layout-v2.txt` joins the fixtures.
4. **Reconciliation** (`meegone-apps-state::reconcile`), pure and run on every refresh:
   - The saved order is kept, and each app once. Apps that are gone leave, also from folders; a folder of one becomes that app, and an empty one goes (FOLD-06). Folder ids are kept unique, since recovery can repeat one.
   - **Apps of a profile that is locked, or that lists no apps at all, keep their places.** They stay in the layout but out of the grid until the system lists them. Apps of a profile that no longer exists leave.
   - **An update that renames an app's launcher activity keeps its place and mark.** That is exactly one saved app gone and one unplaced app with the same profile and package.
   - **Apps not placed yet join the end:**
     - First start: the default apps first, in role order, then the rest alphabetically. Nothing is marked.
     - Saved layout: oldest install first, then alphabetically, each marked new.
     - Recovered layout, or storage that can't be read: alphabetically, with nothing marked, since these aren't new installs.
   - **Alphabetical** is `(sort_key, label, work, id)`. Equal labels put the personal app first, and the order is total.
5. **The holder.**
   - `AppsHolder` keeps the layout in memory once read, as its only writer. It saves when the layout changes, and again at the next refresh after a failed save. A recovered layout is saved at once, so the damaged file is replaced.
   - A read error shows the rebuilt grid and its `problem`, but never saves: the next refresh reads the file again.
   - A successful launch from Applications removes the app's mark. `GridItem::App` carries `new`.
6. **The catalogue.**
   - Records gain the label's collation key and the first install time. The key comes from `android.icu.text.Collator` at primary strength, so it ignores case and accents, in lowercase hex, and the install time from `LauncherActivityInfo.getFirstInstallTime`.
   - Profiles gain `LOCKED`, from `UserManager.isUserUnlocked`.
   - `Bridge` also refreshes on `ACTION_MANAGED_PROFILE_UNLOCKED` and `ACTION_LOCALE_CHANGED`, so labels and keys follow the language.
   - `AppsAdapter::default_apps` asks for the default dialer (`TelecomManager`) and SMS app (`Telephony.Sms`). For the browser and camera, it asks which activity handles an `https` link and `STILL_IMAGE_CAMERA`; the system's chooser (`android`) means none was chosen. The manifest's `<queries>` declare those two intents. They are not permissions.
7. **The motion** (ANIM-21).
   - **Who animates.** When a refresh brings or removes cells, the shell starts `grid_change`. It is like the return-home motion: pure, with its clock starting at the first frame, 200 ms decelerating at the system animation speed, and instant with animations off.
     - New cells grow from 50 % and fade in, cells that left shrink to 50 % and fade out, and the rest slide from their old places.
     - A late frame advances it by at most 34 ms.
     - The pressed icon is released, since its place may now hold another cell.
   - **Where each cell came from.** `AppsView` gives each cell its place in the cells it handed out before (`previous_places`, by app or folder).
   - **What the host reads.** Cells go from 5 to 6 fields, adding `from`, and flags gain `NEW = 8`. The frame goes from 24 to 30 slots: appear scale and opacity, leave scale and opacity, and shift in ten-thousandths, plus the accent for the dot.
   - **How `AppsGrid` uses it.**
     - It reuses each view by `from`, moving it without detaching it from the window, so icons and labels go with their app.
     - Views no cell takes become leaving views. They can't be tapped, are hidden from screen readers, and stay at their old places until they have faded out.
     - A revision in which every cell keeps its place, such as the accent a new app gets moments after it is installed, carries the change on unchanged.
   - **The dot.** `AppCell` draws a 6 dp dot in the theme's accent before the label, keeping the pair centred and mirroring right to left. Screen readers hear "Maps, new".

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0004, decision 7 | An unreadable layout is recovered record by record and rebuilt around what was kept, instead of loading as empty. A storage read error is no longer an empty layout. |
| 0010, decision 1 | App records have 8 fields, adding the collation key and install time, and profiles have the `LOCKED` flag. |
| 0010, decision 3 | Installs and removals move the grid. Unlocking a profile or changing the language also refreshes it. |
| 0010, decision 5 | Cells have 6 fields, adding where each was before, and the frame has 30 slots. |
| 0010, decision 6 | `AppsGrid` keeps each app's view across revisions instead of rebinding views by position. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| APPS-07: the dot stays "until it is first opened" | It goes when the app is opened from MeeGone. An app first opened elsewhere, such as from the store's "Open" button, keeps its dot until then: MeeGone requests no permissions, and usage access is optional. |
| APPS-18: the layout persists, with ADR 0010 leaving apps on missing storage to #11 | Apps on missing or adoptable storage still leave the grid and come back at the end. At a cold start the system doesn't list them, and without broader package visibility MeeGone can't tell them from uninstalled apps. Locked profiles, the common case, keep their places. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - every reconciliation rule;
    - properties over 2,000 generated layouts: each installed app once, the kept order, no repeated folder ids, and reconciling twice changing nothing;
    - every truncation and single-bit change of a saved layout going through recovery and the holder without a panic, showing every app, and leaving a readable file;
    - the load outcomes, the motion, and each cell's previous place;
    - the issue's scenarios, in `tests/acceptance.rs`.
  - Checked on a Galaxy S22 (One UI, Android 16), with MeeGone as an ordinary app, using two code-less test apps:
    - an install joins the end with a dot and grows in;
    - opening it from the grid removes the dot;
    - removing one mid-row shrinks it away while the next cell slides into its place, with no placeholder flash;
    - junk written over `files/layout` gives an alphabetical grid, no crash, and a valid v2 file at once.
  - A user's v1 layout is read unchanged and rewritten as v2 only when it next changes.
- **Costs:**
  - The cells' `from`, the flags and the frame slots must agree between Java and Rust, and nothing checks that at build time.
  - The catalogue is read on the main thread, as ADR 0010 noted. On the S22, with 72 apps, a read took 35 to 244 ms, of which the collation keys took about 1 ms (4 ms cold).
    - A new app's accent arrives moments after the install and reads the catalogue again. The frame after the motion's first came 75 to 92 ms late, so a late frame advances the motion by at most 34 ms.
    - The whole change takes about 250 ms there, within ANIM-06's 150 to 350 ms.
    - The device was found popping new icons in before two fixes: a refresh revision now keeps a change playing, and the per-frame step is capped.
  - A change made while the home is out of sight plays when the home is drawn again, or not at all if it finished meanwhile.
  - Leaving views can be under a long-press for 200 ms, so a long-press there opens the home menu as on empty space.
  - Folders still show as plain cells (#14). A folder that turns back into its app leaves and the app joins, rather than morphing.
