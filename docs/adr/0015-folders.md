# ADR 0015: Folders

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #14

## Context

Issue #14 adds folders to the Applications grid:
- Holding an app over another and dropping it makes a folder of both, named "Folder" until the user renames it. Dropping an app on a folder adds it (FOLD-01, FOLD-03).
- A folder's icon previews its first four apps in a 2×2 grid (FOLD-02).
- A tap opens it over a dimmed grid. Back, Home or a tap outside closes it (FOLD-04).
- Apps move within a folder and out of it. A folder left with one app turns back into that app (FOLD-05, FOLD-06).
- Motion: the icon held over grows and pulses, and the app shrinks into the folder (ANIM-14). The folder grows open from its icon (ANIM-15). Making one ticks (ANIM-24, Should).
- Screen readers name a folder and its apps, and make and empty folders with actions (A11Y-01, A11Y-05).
- Category names for new folders (FOLD-07, Could).

What was there already:
- The saved layout, reconciliation and `GridItem::Folder` support folders, and a folder of one collapses (ADR 0012). `AppsState.open_folder` and Back closing a folder exist.
- On screen, a folder was a plain cell with its name that couldn't be tapped. Nothing made, opened or emptied one.

The constraints:
- **Rust decides, Java draws** (ADR 0006). The frame is a fixed array of `int` slots, and per-cell data travels only with a revision (ADR 0010).
- **The drag** (ADR 0013, ADR 0014): one zone per cell, which the icon takes after 150 ms. A lifted icon was drawn by its own cell, so it couldn't leave its grid.
- **The arrangement** (ADR 0014): `arranging` held one app's place and the saved order. A folder of one, part-way through a drag, must not collapse.
- **Saving is atomic and on the main thread** (ADR 0004).
- **A failed store is never called again** (ARCH-09).

## Decision

1. **Two surfaces, one kind of cell.**
   - `CellKey` names what a cell shows: `App(AppId)` or `Folder(FolderId)`. `Surface` names where: `Grid` or `Folder`, the open folder's panel.
   - The lift, `Arrange`, `Move`, the app menu and the long-press take a `CellKey`. Presses, taps, long-presses and the menu name their surface. `AppsState::place_of(surface, key)` finds a cell on either.
   - `AppsState` gains the open folder's cells, layout, panel bounds, scroll and rows. The panel has 4 columns and no empty last row (`FOLDER_COLUMNS`).
   - `AppsView` keeps a revisioned cell set per surface, so each grid reads its own cells only when they change.
2. **A draft layout instead of `arranging`.**
   - While a drag changes the layout, Applications keeps `draft: Option<Layout>`. `show()` draws the draft if there is one, and the layout otherwise. It is still the only place cells are made.
   - Each step of a drag edits the draft. `Drop` tidies it, saves it once and counts the folders made. `CancelArrange` discards it.
   - A refresh reconciles both. `reconcile` no longer collapses folders of one; `tidy` does, on what is saved, so the draft can hold one mid-drag.
   - Every edit needs edit mode and a layout that was read (REL-02). The read-error path clears the draft.
   - The pure edits live in `folders.rs`:
     - `arrange`: a key to a place.
     - `combine(app, onto)`: onto an app, a new folder named `""` in the target's slot, the target first; onto a folder, the app joins its end. Folders never nest.
     - `take_out`, `arrange_in_folder` and `move_out`.
     - `tidy`: a folder of one becomes its app; an empty one goes (FOLD-06).
3. **Merge or reorder** (FOLD-01, FOLD-05). This replaces ADR 0014's single zone.
   - Over another cell in the grid, the dragged app's centre is in one of two zones:
     - within 0.4 of an icon of that cell's icon centre (`MERGE_RADIUS`): the *merge* zone;
     - anywhere else in the cell: the *reorder* zone.
   - Each change of cell or zone starts its own wait: 150 ms before a place is taken (`REORDER_DWELL`), 400 ms before a merge is armed (`FOLDER_HOVER`).
   - The hysteresis drops from 15 % to 5 % of a cell, so the gaps beside an icon still take its place.
   - A dragged folder, and any drag in the panel, have only reorder zones.
   - Lifting the finger:
     - armed: `Combine`, then the app shrinks into the folder;
     - in a reorder zone: the place, as in ADR 0014;
     - in a merge zone not yet armed: the icon settles back into its own gap.
4. **Out of the panel.** When the icon's centre leaves the panel's rect, the shell sends `TakeOut` to the grid's place under it, and the folder closes. The drag goes on in the grid. The draft's folder of one is tidied at the drop, so dragging one of two apps out turns the folder back into the other.
5. **The dragged icon is drawn by an overlay.**
   - `DragShadow`, a child of the home views above the pages and below Settings and the menus, draws the icon with the platform shadow in the icons' shape. Its cell is only hidden, as a gap.
   - Rust tracks the icon's centre on screen: the finger plus where it was grabbed. The frame gives the centre, the scale in icons and the opacity.
   - The settle springs to the target's icon centre as drawn, edit mode's scale included.
   - `GridLayout` gains where the cells start on screen (`left_px`, `view_top_px`) and where the icon is in its cell (`icon_px`, `icon_top_px`). A drag can't start while the panel is still opening.
6. **The motion.**
   - **Hover** (ANIM-14). The icon held over grows to 1.1 during the 400 ms wait, then pulses to 1.05 and back every 600 ms while armed.
   - **Merge** (ANIM-14). For 250 ms the app shrinks into the folder's next preview slot, which stays empty until it lands. A folder just made shows its first app shrinking from the whole icon into slot 0 as the plate fades in. Into a full preview, the app shrinks to 30 % and fades into the folder's icon.
   - **The panel** (ANIM-15, `folder.rs`). It grows from the folder's icon over a dimmed grid in 250 ms, decelerating, and shrinks back into it when it closes. A change of mind turns back from where it is. With animations removed, it is instant.
   - **Morphs** (ADR 0012). A folder just made takes the place of its first app in the cells before, and an app a folder collapsed into takes the folder's place. Their views move instead of being replaced.
   - **The tick** (ANIM-24). `AppsState.folders_made` counts folders made, and the shell ticks when it grows.
7. **Opening, closing and covering** (FOLD-04).
   - A tap on a folder opens it, in or out of edit mode. `CloseFolder` closes it after a tap outside.
   - Back closes the menu, then the folder, then edit mode. Home closes everything. The folder also closes when a refresh removes it, when the view changes and when Settings opens.
   - An open folder doesn't *cover* the home views (ADR 0008): it *locks* the carousel. A Down is still recorded, so a long-press and a press can count, but no swipe starts.
   - While it is open, long-presses and presses count only on the panel. The home menu isn't offered.
   - Edit mode works inside the panel: ✕ badges, no launches, dragging.
8. **Naming** (FOLD-03).
   - A new folder is saved as `""`. The host shows that as the translated "Folder".
   - A tap on the panel's name, or the folder menu's Rename folder, sets `renaming`. The host shows a dialog with the name in a text field.
   - OK sends `RenameFolder { folder, name }`: trimmed, at most 64 characters, saved, and ignored if the folder has gone. Cancel, Back or a tap outside sends `CancelRename`.
9. **A folder's menu.** A long-press on a folder opens a menu of Rename folder and Edit applications. Dragging on moves the folder; it can't go into anything.
10. **Screen readers** (A11Y-01, A11Y-05).
    - A folder reads "Folder: ‹name›, N apps". Its panel's pane title is "Folder: ‹name›".
    - In edit mode, an app in the grid offers "Make folder with ‹next›", or "Move into ‹folder›" when the next cell is a folder (`Group`). An app in the open folder offers "Move out of folder", which puts it just after its folder (`MoveOutOfFolder`), and the Move actions within it (`MoveInFolder`).
    - The grid and Done are hidden from screen readers while a folder is open.
11. **Health** (ARCH-09). The frame's app menu, open folder and rename come from shell flags that count only while Applications is `Ready`, so a failed Applications leaves none of them on screen.
12. **The host.**
    - `AppsGrid` draws either surface, and names it in every native call. Both grids share one `AppIcons`.
    - `FolderPanel` is a child of the Applications page, above the grid and Done, so it moves with the page:
      - a black scrim at the theme's `scrim_percent`, 40 % in Light and 60 % in Dark;
      - a panel in the surface colour with 12 dp corners and no shadow, centred in the safe area 16 dp from its edges: four of the grid's cells wide, or as wide as fits, as on a phone, and as tall as its rows up to what fits, then scrolling;
      - the folder's name above its grid.
    - Only an open panel takes touches; while it closes, they reach the grid.
    - `AppCell` draws a folder's plate in the icons' shape, in the surface colour with a faint ring, and the preview's slots in the same geometry as `preview_slot`, mirrored right to left.
    - The rename dialog is a framework `AlertDialog` in its own window, so focus, the keyboard and Back need nothing more. It follows MeeGone's theme.
13. **The host contract.**
    - The frame grows to 73 slots. 32–35 now hold the dragged icon's cell, its centre on screen and its scale in icons. The new ones are:
      - 52–54: the dragged icon's surface, opacity and whether it is drawn;
      - 55: the pressed cell's surface;
      - 56–57: the cell held over and its icon's scale;
      - 58–61: the folder taking an app in, its slot, whether it was just made, and how far;
      - 62–65: the open folder, whether it is open or closing, its cell and how far it has grown;
      - 66–68: the open folder's cells revision, scroll and rows;
      - 69–70: the folder being renamed and its cell;
      - 71: the menu's surface;
      - 72: the scrim's opacity.
    - Cells have 8 fields: a folder's app count and its preview's apps are new. Flags gain 32, a folder. Menu items gain kind 4, Rename folder.
    - `nativeAppsLayout(surface, density, layout, grid)` takes the layout as an `int[]`: width, row height, the room above and below, height, where the cells and the grid start on screen, the icon's size and top, and the panel's rect.
    - New native methods:
      - `nativeAppGrouped` and `nativeAppMovedOut`, for screen readers;
      - `nativeCloseFolder`;
      - `nativeRenameRequested`, `nativeFolderRenamed` and `nativeRenameCancelled`.

## Amendments to earlier ADRs

| ADR | Change |
|---|---|
| 0006, decision 2 | The frame has 73 slots. |
| 0008 | An open folder locks the carousel instead of covering the home views, and the home menu isn't offered while it is open. |
| 0010, decisions 5 and 6 | The grid natives name a surface. Cells have 8 fields and flag 32, a folder. `nativeAppsLayout` takes an `int[]`. A tap on a folder opens it, even in edit mode. |
| 0012 | `reconcile` keeps folders of one; `tidy` collapses them in what is saved. A folder just made, and an app a folder collapsed into, keep the view of the cell they replace. |
| 0013, decision 3 | The dragged icon is drawn by `DragShadow`, tracked by its centre on screen, and its cell is a gap. A folder has a menu of its own. The app menu counts only while Applications is `Ready`. |
| 0014, decisions 1 and 2 | A draft layout replaces `arranging`. Each cell has a merge zone and a reorder zone, and the hysteresis is 5 %. Lifting the finger over an icon not yet armed puts the icon back. |

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| FOLD-07: category names for new folders (Could) | Deferred. A new folder is "Folder" until the user renames it. |
| APPS-07: a dot on a new app | Apps inside a folder show no dot, and a folder shows none for them. |
| THEME-06: a theme change keeps what is open | The folder stays open, but a name being typed in the rename dialog is lost: the dialog belongs to the activity, which a theme change recreates. The dialog opens again with the saved name. |

## Consequences

- **Benefits:**
  - Tested without a device:
    - The pure edits: combining, nesting, taking out, order within a folder, tidying and ids, around entries that show no cell.
    - The draft through a refresh, commit and cancel, renaming, opening and closing, Back's order and the screen readers' actions.
    - The zones, their waits and hysteresis, the three drop rules, leaving the panel, auto-scroll in it, merging, and the shadow's settle.
    - The panel's motion, the carousel's lock, ticks, and a failed Applications.
    - AC-05 with an open folder, AC-08, and FOLD-03 to FOLD-05 in `tests/acceptance.rs`.
  - Checked on a Galaxy S22 (One UI, Android 16), with MeeGone as the default home app:
    - Holding AR Doodle over 7-Eleven and dropping it makes "Folder", previewing both, in 7-Eleven's place.
    - A tap opens it over the scrim, in and out of edit mode. Back, Home and a tap outside close it, and the tap launches nothing.
    - Renaming it to "Games" from its name, and from its menu, which Back cancels. The name is saved.
    - Dragging AR Doodle out of the panel closes the folder and turns it back into 7-Eleven.
    - Dropping Camera on a folder adds it. A dragged folder held over ChatGPT only takes its place. Moving Calendar to the end of a folder is saved and shows in its preview.
    - With animations removed, the folder opens and closes at once, and a dropped app goes straight in.
- **Costs:**
  - Not checked on a device yet: TalkBack's actions and announcements, right to left, the tick, and a tablet's panel. The device matrix (#43) covers them.
  - Following the system's dark mode isn't wired on Android yet (#32), so a theme change with a folder open is checked only in `tests/acceptance.rs`.
  - The frame slots, the cell fields and the native methods must agree between Java and Rust. Nothing checks that at build time.
  - The two grids share one motion. Each replays only its own change, so a change on one surface while the other's plays cuts the other's short.
  - While an icon is held over another, the host draws frames for the pulse.
