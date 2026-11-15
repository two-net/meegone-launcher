# ADR 0001: One crate per feature layer

**Status:** Accepted · **Date:** 2026-11-15 · **Issue:** #1

## Context

ARCH-02 requires three layers with one-way dependencies (UI → state → data). ARCH-03 requires six features that depend only on a shared core, with only the Home shell putting them together. Issue #1 asks for any feature-to-feature or data-to-UI dependency to fail the build in CI.

The app is written in Rust, in the existing Cargo workspace. The options were:

1. One crate per feature, with `ui`, `state` and `data` modules. Cargo enforces feature boundaries, but layer direction inside a crate can only be checked by scanning source, which is easy to get around.
2. Two crates per feature: state and data together, plus UI. Data or state can't reach the UI, but nothing stops data from using state.
3. One crate per feature layer.

## Decision

Option 3: one crate per feature layer, `crates/<area>/<layer>` named `meegone-<area>-<layer>`. A layer with nothing in it has no crate, so the workspace has 17 app crates today.

Two mechanisms enforce the boundaries:

- **Cargo.** Code can only use crates its manifest declares, so the declared dependency graph is the real one.
- **`tools/arch-check`.** It checks the declared graph against the rules in ARCHITECTURE.md and runs in CI as `cargo test -p arch-check`.

The rules are strict: a crate may depend only on its own layer or the one directly below. UI must go through state, so state crates re-export the data models their state contains.

The root package `meegone-launcher` is the one app. It may depend only on `shell` and `core`, because `HomeShell::compose` is the single place features are put together.

Failure containment (ARCH-09) relies on `std::panic::catch_unwind` in `Store`, so the workspace keeps Cargo's default `panic = "unwind"`.

## Consequences

**Benefits:**
- Boundaries are checked by the compiler and by CI, not by convention.
- Features can be built in parallel without touching each other's crates.
- Every state holder is testable without UI, because a state crate can't depend on one.

**Costs:**
- About 17 small manifests.
- State crates must re-export the models the UI needs.
- A new area means changing `AREAS` in arch-check. That is deliberate, because it changes the simplicity budget.
