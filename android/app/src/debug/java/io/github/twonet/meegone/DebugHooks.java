package io.github.twonet.meegone;

import android.content.Intent;

/**
 * Debug builds only: ways to drive MeeGone from adb that its UI doesn't offer (ADR 0011).
 * Release builds get the version in {@code src/release}, which does nothing (SEC-06).
 */
final class DebugHooks {
    /**
     * Draws every icon as a single-image legacy one, on the neutral plate (APPS-15, ADR 0011),
     * for the rest of the process. Few apps still have such icons, so this shows the plate on
     * any device. Force-stop MeeGone first, so no icon is already drawn: {@code adb shell am
     * force-stop io.github.twonet.meegone}, then {@code adb shell am start -n
     * io.github.twonet.meegone/.HomeActivity --ez <this> true}
     */
    static final String EXTRA_LEGACY_ICONS = "io.github.twonet.meegone.debug.LEGACY_ICONS";

    /** Read on the icon threads. */
    private static volatile boolean legacyIcons;

    private DebugHooks() {}

    /** Notes whether {@code intent} asks to draw every icon as a legacy one. */
    static void readIconExtras(Intent intent) {
        if (intent != null && intent.getBooleanExtra(EXTRA_LEGACY_ICONS, false)) {
            legacyIcons = true;
        }
    }

    /** Whether every icon is drawn as a legacy one. On any thread. */
    static boolean legacyIcons() {
        return legacyIcons;
    }
}
