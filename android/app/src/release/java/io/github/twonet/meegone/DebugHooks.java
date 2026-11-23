package io.github.twonet.meegone;

import android.content.Context;
import android.content.Intent;
import android.icu.text.Collator;
import android.icu.text.Transliterator;
import android.view.MotionEvent;
import java.util.List;

/** Release builds have no debug features (SEC-06); see {@code src/debug} for what this replaces. */
final class DebugHooks {
    private DebugHooks() {}

    static void readIconExtras(Intent intent) {}

    static void readAuditExtras(HomeActivity activity, HomeViews views, Intent intent) {}

    static boolean legacyIcons() {
        return false;
    }

    static void readPerfExtras(Context context, Intent intent) {}

    static void created(HomeViews views, boolean started) {}

    static void home(HomeViews views) {}

    static void back(HomeViews views) {}

    static void readCompatExtras(HomeActivity activity, HomeViews views, Intent intent) {}

    static void typed(HomeViews views) {}

    static void touched(MotionEvent event) {}

    static void launched() {}

    static void trimmed(HomeViews views, int level) {}

    static void padApps(List<String> fields, Collator collator, Transliterator latin) {}

    static String realActivity(String activity) {
        return activity;
    }

    static void readFaultExtras(Intent intent) {}

    static void fault(int feature) {}
}
