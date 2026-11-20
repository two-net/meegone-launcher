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

    /**
     * Goes through every screen in both themes and checks each against #37's accessibility rules,
     * logging what fails under {@code MeeGoneA11y} (ADR 0038): {@code adb shell am start -n
     * io.github.twonet.meegone/.HomeActivity --ez <this> true}, or {@code tools/a11y-audit.sh},
     * which runs it at the normal and the largest font size.
     */
    static final String EXTRA_A11Y_AUDIT = "io.github.twonet.meegone.debug.A11Y_AUDIT";

    /** Read on the icon threads. */
    private static volatile boolean legacyIcons;

    private DebugHooks() {}

    /** Notes whether {@code intent} asks to draw every icon as a legacy one. */
    static void readIconExtras(Intent intent) {
        if (intent != null && intent.getBooleanExtra(EXTRA_LEGACY_ICONS, false)) {
            legacyIcons = true;
        }
    }

    /**
     * Starts the accessibility tour if {@code intent} asks for it. The extra is taken off, so the
     * activity made again after a configuration change doesn't start it again.
     */
    static void readAuditExtras(HomeActivity activity, HomeViews views, Intent intent) {
        if (intent != null && intent.getBooleanExtra(EXTRA_A11Y_AUDIT, false)) {
            intent.removeExtra(EXTRA_A11Y_AUDIT);
            A11yTour.start(activity, views);
        }
    }

    /** Whether every icon is drawn as a legacy one. On any thread. */
    static boolean legacyIcons() {
        return legacyIcons;
    }
}
