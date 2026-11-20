package io.github.twonet.meegone;

import android.content.Intent;

/** Release builds have no debug features (SEC-06); see {@code src/debug} for what this replaces. */
final class DebugHooks {
    private DebugHooks() {}

    static void readIconExtras(Intent intent) {}

    static void readAuditExtras(HomeActivity activity, HomeViews views, Intent intent) {}

    static boolean legacyIcons() {
        return false;
    }
}
