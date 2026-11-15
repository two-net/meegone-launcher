# ADR 0003: Platform adapters and test fakes

**Status:** Accepted · **Date:** 2026-11-15 · **Issue:** #2

## Context

ARCH-04 requires all access to the operating system to go through narrow platform adapters (apps, notifications, usage, wallpaper, network, scheduling, system configuration), each replaceable by a fake in tests. MAINT-01 and MAINT-02 require the core logic to be tested without a device, with at least 80 % line coverage. SRS 8.1 lists twelve capabilities the adapters wrap, and SRS 8.2 says only the image-feed adapter may know the Bing feed's format.

The skeleton from #1 had the seven traits, with only the methods it needed. Several capabilities had no home: shortcuts, uninstall requests, install and removal events, the next alarm, vibration, the web browser and the system settings screens.

## Decision

1. **Seven adapters, as ARCH-04 names them.** Each capability belongs to the adapter that owns its subject. Opening an app's info screen belongs to `AppsAdapter`, and the wallpaper settings screen to `WallpaperAdapter`. Device-wide services (the home-app role, the next alarm, haptics, the browser) join the clock and theme in `SystemConfigAdapter`. ARCHITECTURE.md has the full table.
2. **Adapters never call back.** Changes the system reports reach MeeGone as `ShellIntent`s (`SystemThemeChanged`, `AppsChanged`, `NotificationsChanged`), passed in through `Launcher::dispatch`, which is already how Home, Back and theme changes arrive. State holders stay single-threaded, and tests deliver an event by dispatching it.
3. **"If unavailable" is part of each method's contract.** A missing capability is a value: `Access::Missing`, `None`, `false`, or `DataError::Unsupported`. Each feature turns it into its own state, as SRS 8.1 describes. `DataError` gains `Offline` and `Invalid`, so failure handling can tell "No connection" apart from a rejected or changed response (WALL-16, SEC-02).
4. **One shape for every fake.** Public fields set what the "system" reports and record what was asked. An `error` field makes every fallible call fail. Fakes sit behind a `fakes` feature that arch-check allows only in `[dev-dependencies]` (SEC-06).
5. **The image feed is an interface of the lock-screen image feature.** `ImageFeed` and its format-free `FeedImage` live in `meegone-lockimage-data`, and its one implementation (#26) is built on `NetworkAdapter`.
6. **CI checks that only adapters reach the OS.**
   - arch-check scans the app's sources for `std::{env, fs, net, os, process}` and the clocks. Only files on an allowlist may use them, and the allowlist is empty until the Android adapters (HOME-01) and persistent stores (REL-03) arrive.
   - ADR 0001 turned down source scanning for layer rules, because it is easy to get around. Here the scan is a backstop: Cargo and DEPENDENCIES.md already keep out third-party crates that reach the OS.
7. **Coverage is measured on the data and state crates.**
   - That is where MAINT-01's core logic lives. `cargo coverage` (cargo-llvm-cov) runs every workspace test, leaves out UI crates, test files and fakes, and fails below 80 %.
   - Inline `#[cfg(test)]` modules still count, because stable Rust can't exclude them.

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| §7.3 diagram: the platform adapters reach the Bing image feed | `ImageFeed` belongs to the lock-screen image feature, its only user (ADR 0002). It still sits in the data layer and reaches the network only through `NetworkAdapter`. |

## Consequences

- **Benefits:**
  - Every capability in SRS 8.1 has an interface and a fake before the features that need it are built.
  - Feature issues add behaviour, not plumbing.
- **Costs:**
  - `SystemConfigAdapter` is the widest trait, with thirteen small methods. If it grows further, it can be split into narrower traits with a new ADR.
  - Methods for features not yet built are guesses that those features may refine.
  - A file that must reach the OS directly needs an allowlist entry in arch-check.
