# ADR 0017: Hiding apps

**Status:** Accepted · **Date:** 2026-11-17 · **Issue:** #16

## Context

Issue #16 lets users keep rarely used apps out of sight without uninstalling them (APPS-19, Could):
- An app's long-press menu offers Hide.
- Hidden apps are left out of the Applications grid, search and Recent.
- Settings → Applications → Hidden apps lists them and unhides each one (SET-02).
- They are kept in the settings store and survive restarts.
- The Settings item counts toward SET-04's 12 user-adjustable options.

What was there already:
- `Settings.hidden_apps` has been saved since settings v1, so the format doesn't change.
- Recent leaves hidden apps out each time it refreshes (REC-03).
- Search's candidates are the grid's apps (ADR 0016), so search leaves out whatever the grid does.
- `reconcile::grid` and `follow_order` keep layout entries that show no cell in their slots, as for a locked work profile (ADR 0012).
- Settings is drawn only as its name until #34.

The constraints:
- **One writer per settings field** (ADR 0002). ARCHITECTURE.md already named Applications as the writer of hidden apps, but `AppsHolder` had no settings store.
- **Features meet only in the shell** (ADR 0002). Settings can't depend on Applications. Content one feature shows in another is a `Slot`, and reactions across features happen in `HomeShell::dispatch`.
- **A failed feature's store is never called again** (ARCH-09).
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, and lists travel by revision (ADR 0010).

## Decision

1. **Applications owns hiding.**
   - `AppsHolder` gets the shared `SettingsStore` and writes `hidden_apps` through it, as the field's only writer.
   - A failed save changes nothing and becomes `problem`, as other menu items' failures do.
2. **A hidden app keeps its place but shows no cell.**
   - The holder keeps three lists:
     - `installed`: everything the system lists;
     - `hidden`;
     - `shown`: the installed apps that aren't hidden.
   - `reconcile` and `reconcile_draft` still see `installed`, so a hidden app keeps its layout entry, folder membership and new-app mark. Leaving it out would drop it, and it would come back at the end with a dot.
   - `grid`, the drag's edits (`arrange`, `arrange_in_folder`, `take_out`, `move_out`) and therefore search see `shown`.
   - Hiding and unhiding never change the layout, so an unhidden app comes back exactly where it was, in its folder too, and nothing is written but the settings.
   - A folder whose apps are all hidden shows no cell. If it was open, it closes, as a folder a refresh removed does, and a rename in progress ends.
3. **A hidden app is forgotten when its app is gone.**
   - The pure `reconcile::follow_hidden` keeps a hidden id exactly while `reconcile` would keep its place:
     - while its app is installed;
     - while its profile can't list its apps yet (locked, or listing none);
     - under its new id when an update renamed its launcher activity.
   - Any other id is dropped, so an app uninstalled and installed again shows, at the end with a dot (APPS-06).
   - Each refresh saves the followed set only when it changed. A failed save keeps the old set; the next refresh tries again. A layout that can't be read forgets nothing (REL-02).
4. **The menu.**
   - An app's menu is App info, Uninstall when allowed, **Hide**, Edit applications unless edit mode is on, then its shortcuts.
   - Hide is offered in and out of edit mode, in the grid and the open folder, but never in a folder's own menu.
   - A hidden app has no cell, so it has no menu, ✕ or search result either.
5. **Settings → Applications → Hidden apps.**
   - `AppsState.hidden_apps` lists the hidden apps that are installed, in the locale's alphabetical order (the collation keys, as reconcile sorts). It is replaced only when it changes.
   - `HiddenAppsSection` (`meegone-apps-ui`) is a `Slot`, like `ImageSettingsSection`. `ShellView` keeps a `Latest<AppsState>` current with the new `HomeShell::subscribe_apps`, and adds the section to Settings before the lock-screen image's, in SET-02's order. It also keeps a handle for the host, `ShellView::hidden_apps()`.
   - `AppsIntent::Unhide(app)` does nothing unless the app is hidden.
   - The host reads the list by revision and names an app by revision and place, as it does cells, so a tap on a list that changed meanwhile does nothing.
   - `HomeFrame::hidden_apps` is the revision, or `None` while Applications isn't `Ready` (`HomeShell::apps_ready`). Settings then doesn't offer the list, since a failed store would ignore every tap (ARCH-09).
6. **Recent follows at once.** At the end of `dispatch`, the shell compares `hidden_apps` with the list it last saw, by pointer. When it changed, by a Hide, an Unhide or an app that is gone, Recent refreshes. A swipe therefore never shows a hidden app's card on the next page, a failed save refreshes nothing, and nothing pattern-matches intents.
7. **The host draws the list in Settings' placeholder.**
   - `HiddenAppsList`, in a `ScrollView` below Settings' name, has:
     - an "Applications" heading;
     - a "Hidden apps" heading;
     - a row per app: its 40 dp icon from the grids' `AppIcons`, its label (faded when unavailable), and **Unhide** in the accent colour;
     - "No hidden apps" when there are none.
   - Unhide is a touch target of 48 dp or more (A11Y-02). Screen readers hear "Unhide ‹app›", "work" included, and the headings are headings.
   - After an Unhide, a screen reader's focus moves to the next row's Unhide, or to "No hidden apps".
   - A failed save's message shows briefly.
8. **The host contract.**
   - The frame grows to 80 slots:
     - 78: 1 while Settings offers Hidden apps;
     - 79: the list's revision.
   - New natives on `HomeViews`:
     - `nativeHiddenApps()`: five fields per app: label, package, activity, profile serial and flags (1 work, 2 available);
     - `nativeUnhide(revision, index)`: returns the problem, or `null`.
   - The app menu's kinds gain 5, Hide.
9. **SET-04.**
   - Hidden apps is one of the 10 user-adjustable options SET-02 lists, so this issue spends no new budget. The 10 are theme, Home button opens, wrap-around, Recent's columns and time window, hidden apps, and the daily image's on/off, region, mobile data and Events card.
   - A test in `settings.rs` names every `Settings` field with no `..`. A new field doesn't compile until it is counted as an option or set aside, and the count must stay at 12 or fewer.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 80 slots. |
| 0012, decision 4 | Reconcile still sees hidden apps, so they keep their places; the cells and the drag's edits see only the apps that aren't hidden. |
| 0013, decisions 1 and 10 | An app's menu offers Hide after Uninstall. Its kind is 5. |
| 0015, decisions 2 and 7 | A folder whose apps are all hidden shows no cell, and closes if it was open. Hidden members keep their slots. |
| 0016, Departures | Hidden apps are now left out of search, as the departure foresaw: the grid leaves them out. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| SET-02: Settings has its groups | Only Applications → Hidden apps is drawn, below Settings' name. The other groups come with #34. |
| FOLD-06: a folder left with one app turns back into it | A folder whose other apps are hidden stays a folder, showing one app. Its members are still in it, and unhiding them brings them back there. |
| APPS-19: unhide in Settings | A hidden app of a locked work profile isn't listed until the profile unlocks: the system doesn't list the app meanwhile. It stays hidden. |
| APPS-19 with APPS-18: hidden apps survive restarts | An app on missing or adoptable storage is forgotten at a cold start, as it loses its place (ADR 0012). It comes back shown, at the end. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - Hiding and unhiding: the grid, search, the open folder, folders whose apps are all hidden, drags around a hidden app, failed saves, and the list's order.
    - Restarts, uninstalls, reinstalls, renamed activities, locked profiles and deleted profiles.
    - Recent's refresh, the slot's revisions and stale taps.
    - The issue's two acceptance criteria, in `tests/acceptance.rs`, and a restart in `tests/storage.rs`.
  - Checked on a Galaxy S22 (One UI 8, Android 16), with MeeGone as the default home app:
    - Calendar's menu offers Hide. Hiding it closes the grid up, and "cal" finds only Whoscall.
    - Calendar stays hidden after a force-stop.
    - Launcher settings lists it under Applications → Hidden apps. Unhide shows "No hidden apps", and Calendar is back in its old place.
- **Costs:**
  - A save failure's message is the storage adapter's English text, as for the layout's.
  - An Unhide's grid motion plays under Settings, out of sight.
  - The frame slots, the list's fields and the natives must agree between Java and Rust. Nothing checks that at build time.
  - Not checked on a device yet: TalkBack, right to left, a tablet, a locked work profile and dark mode (#32). The device matrix (#43) covers them.
