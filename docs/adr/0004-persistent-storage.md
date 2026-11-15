# ADR 0004: Versioned, atomic storage for the three persistent stores

**Status:** Accepted · **Date:** 2026-11-16 · **Issue:** #3

## Context

The layout, the settings and the image cache were kept in memory only (`Memory*` stores from #1). Several requirements apply as they become persistent:

- **Atomic saves.** A crash or power loss never leaves a half-written layout or settings (REL-03).
- **Versions.** Every saved format carries a version and forward migrations, and an unreadable or newer file resets to defaults instead of crashing (MAINT-04).
- **Exactly three stores** (SRS 7.1). Notification content and usage history are never stored. The only usage-related data kept is the list of apps removed from Recent, with the time of removal (PRIV-03, EVT-13).
- **Removal.** Uninstalling MeeGone or clearing its data removes everything it stored (PRIV-05).
- **Size.** Stored data stays within 50 MB (PERF-08).

Writing files is OS access, which ARCH-04 sends through narrow, fakeable adapters. ADR 0003 left arch-check's OS-access allowlist empty until the persistent stores arrived. The app's own crates use only the standard library (DEPENDENCIES.md).

## Decision

1. **A storage adapter.**
   - `StorageAdapter` in `meegone-core-data` (`storage/`) reads a file, replaces it atomically, or removes it.
   - It reaches the shell in `Platform`, next to the other adapters, so the host hands over MeeGone's folder at start and tests restart the app on the same `Fakes`.
   - `FakeStorage` keeps files in memory and has the usual `error` field.
2. **Three stores, named by a closed enum.**
   - `StoredFile` is the only way to name a file: `layout`, `settings` and `images/details`. Nothing else can be written.
   - The image cache is a folder, so the images themselves (#27) can join their details without moving anything.
   - The stores are `PersistentLayoutStore` (apps data), `PersistentSettingsStore` (core data) and `PersistentImageCache` (lockimage data). Each replaces its `Memory*` version, so there is one implementation per store, tested on `FakeStorage`.
3. **One file touches the file system.**
   - `FileStorage` (`storage/files.rs`), the only entry in `OS_ACCESS_ALLOWED`, keeps each file under a root folder that the host provides.
   - On Android that folder is in the app's private internal storage (`Context.getFilesDir()`). Android deletes it on uninstall and on "Clear storage" (PRIV-05). MeeGone never uses shared or external storage (SEC-03).
4. **Atomic replacement.**
   - A write creates `<file>.tmp` in the same folder, writes it and flushes it to the device. It then renames it over the target, which is atomic within one file system, and flushes the folder so the rename survives a power loss.
   - Writes are serialised, so two writes never share a temporary file.
   - A crash leaves at most a stale `.tmp`, which reads ignore and the next write replaces.
5. **A small, hand-written text format** (`storage/format.rs`):
   - A header, `meegone-<kind> <version>`, keeps a file from being read by the wrong store.
   - One record per line, `key field field …`, with escaped strings.
   - A trailer, `end <checksum>`, holds the 64-bit FNV-1a checksum of everything before it. A file without it was cut short; a file whose checksum doesn't match was damaged.
   - Parsing is strict, and any problem makes the whole file unreadable.
   - Text keeps the app std-only, and a saved file can be read in a bug report.
6. **Versions and migrations.**
   - Each store implements `Format`: a kind, a `VERSION`, a parser for the current version only, and `MIGRATIONS`, one text-to-text step per version.
   - The migrations array is declared as `[Migration; VERSION - 1]`, so a new version without its migration doesn't compile.
   - Each store keeps a fixture file for every version it has ever written. Tests load each one through every migration step, and check that the newest fixture is exactly what is written today.
7. **Unreadable means defaults.**
   - A missing, damaged, wrong-kind or newer-version file, or one that can't be read, loads as the store's defaults. Rebuilding an unreadable layout from the installed apps is #11's (REL-02).
   - A failed save returns a `DataError` and changes nothing. The settings store updates its kept copy only after a successful save, and each feature shows the error as its `problem`.
8. **A storage budget.** `write` fails and changes nothing if the stored files would then take more than `MAX_STORED_BYTES`, 50 MB (PERF-08). Both `FileStorage` and `FakeStorage` check it.

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| ARCH-04 lists seven adapters: apps, notifications, usage, wallpaper, network, scheduling, system configuration | `StorageAdapter` is an eighth narrow trait, for MeeGone's own private files. It amends decision 1 of ADR 0003. |

## Consequences

- **Benefits:**
  - Every requirement above is checked without a device:
    - crashes are injected part-way through writes, and before the rename;
    - every truncation and every single-bit change of a saved file is rejected;
    - fixtures cover every format version;
    - a whole-app test checks that nothing from notifications or usage history is stored.
  - Adding a field is a new version, a migration and a fixture, with the build failing if the migration is missing.
- **Costs:**
  - The storage module is ours to maintain: about 650 lines of code and 600 of tests.
  - Text is larger than a binary format, which matters little for files of a few kilobytes.
  - After a downgrade, a newer file loads as defaults, and the next save replaces it. The user loses settings saved by the newer version. MAINT-04 accepts this.
  - Each write rewrites its whole file. That suits files of this size; the images (#27) get files of their own.
