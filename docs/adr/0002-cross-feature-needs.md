# ADR 0002: Meeting cross-feature needs without feature-to-feature dependencies

**Status:** Accepted · **Date:** 2026-11-15 · **Issue:** #1

## Context

ARCH-03 says features depend only on core, and only the Home shell puts features together. Several needs cross features, and the SRS draws some of them as direct dependencies:

- The SRS §7.3 diagram has the Events UI and the Settings UI using the lock-screen image state (EVT-12 image card; the "Lock-screen image" group in SET-02).
- Theme state is used by the shell, which applies it, and by Settings, which changes it.
- Events and Recent need each app's label and accent colour (APPS-17), which the Applications component derives.
- Recent stores its removed-apps list in the settings store. Hidden apps (Applications) and the image options (Lock-screen image) live there too, yet the component table says Settings and setup owns that store.

## Decision

1. **Data that several features share lives in core.**
   - **The app catalogue** (`App`, accent colours) is read through `AppsAdapter` in `meegone-core-data`.
   - **The settings store** (`SettingsStore`, `Settings`) is in `meegone-core-data`. The Settings feature owns the settings *screens* and its own fields. Each field has exactly one writing feature (see ARCHITECTURE.md, "Sources of truth").
   - **Theme resolution** (`ThemeHolder`) is in `meegone-core-state`.
2. **Content that one feature shows inside another is a `Slot`.**
   - The owning feature's UI crate provides it; for example, `meegone-lockimage-ui` provides `ImageCard` and `ImageSettingsSection`.
   - The host view only exposes a place for slots: `EventsView::add_card` and `SettingsView::add_section`.
   - `ShellView` puts the slots in place and keeps a `Latest<State>` of the owning feature current. A slot's intents go to its own feature.
3. **Home and Back** reach every feature through one method, `StateHolder::on_back`, which the shell calls for the current view.
4. **Reactions across features**, such as re-resolving the theme after a settings change, happen in `HomeShell::dispatch` and nowhere else.
5. **The UI toolkit is deferred.** `View`, `Slot` and `Dispatch` in `meegone-core-ui` are toolkit-agnostic, and views don't draw yet. The toolkit choice gets its own ADR. It must build for Android 10+ through the NDK and meet the motion and performance requirements (ANIM, PERF).

## Departures from the SRS (MAINT-05)

| SRS | Here |
|---|---|
| §7.3 diagram: Events UI → lock-screen image state | The shell puts `ImageCard` (lockimage UI) into Events. |
| §7.3 diagram: Settings UI → lock-screen image state | The shell puts `ImageSettingsSection` (lockimage UI) into Settings. |
| §7.3 diagram: theme state as a separate holder used by shell and Settings | `ThemeHolder` lives in core state. Settings writes the choice to the settings store, and the shell asks the theme holder to re-resolve. |
| §7.3 table: Settings and setup owns the settings store | The store type is in core data. Settings owns the screens, and each feature owns its own fields. |

## Consequences

- Events and Settings compile and test without the lock-screen image feature, and that feature can fail without affecting them.
- The shell is the one place to look for how features meet.
- Core grows only with data that several features really share. Anything used by a single feature stays in that feature.
