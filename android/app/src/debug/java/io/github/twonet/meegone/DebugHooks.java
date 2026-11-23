package io.github.twonet.meegone;

import android.content.Context;
import android.content.Intent;
import android.icu.text.Collator;
import android.icu.text.Transliterator;
import android.view.MotionEvent;
import java.util.List;

/**
 * Debug builds only, and the benchmark build, which shares them (ADR 0041): ways to drive
 * MeeGone from adb that its UI doesn't offer (ADR 0011). Release builds get the version in
 * {@code src/release}, which does nothing (SEC-06).
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

    /**
     * Turns {@link PerfProbe}'s marks on, with true, or off, for this and every later process
     * (ADR 0041): {@code tools/perf-bench.sh} sets it with {@link #EXTRA_APPS}.
     */
    static final String EXTRA_PERF = "io.github.twonet.meegone.debug.PERF";

    /** With {@link #EXTRA_PERF}: how many apps the catalogue is padded to, such as 300. */
    static final String EXTRA_APPS = "io.github.twonet.meegone.debug.APPS";

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

    /** Takes the probe's settings from {@code intent}, before MeeGone starts (ADR 0041). */
    static void readPerfExtras(Context context, Intent intent) {
        PerfProbe.configure(context, intent);
    }

    /** The home views were made; {@code started} says this process started MeeGone. */
    static void created(HomeViews views, boolean started) {
        PerfProbe.created(views, started);
    }

    /** The system's Home action reached the home (PERF-01). */
    static void home(HomeViews views) {
        PerfProbe.home(views);
    }

    /** Search's text changed (PERF-05). */
    static void typed(HomeViews views) {
        PerfProbe.typed(views);
    }

    /** A touch reached the home views (PERF-06). */
    static void touched(MotionEvent event) {
        PerfProbe.touched(event);
    }

    /** A launch request has left MeeGone (PERF-06). */
    static void launched() {
        PerfProbe.launched();
    }

    /** The home views gave memory back (PERF-07). */
    static void trimmed(HomeViews views, int level) {
        PerfProbe.trimmed(views, level);
    }

    /** Pads the catalogue to the benchmark's apps, if it asked for more (ADR 0041). */
    static void padApps(List<String> fields, Collator collator, Transliterator latin) {
        PerfProbe.padApps(fields, collator, latin);
    }

    /** The activity Android knows, for a padded copy's (ADR 0041). On any thread. */
    static String realActivity(String activity) {
        return PerfProbe.realActivity(activity);
    }
}
