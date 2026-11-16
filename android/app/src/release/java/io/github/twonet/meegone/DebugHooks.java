package io.github.twonet.meegone;

import android.content.Intent;

/** Release builds have no debug features (SEC-06); see {@code src/debug} for what this replaces. */
final class DebugHooks {
    private DebugHooks() {}

    static boolean requestsHomeRole(Intent intent) {
        return false;
    }

    static void readIconExtras(Intent intent) {}

    static boolean legacyIcons() {
        return false;
    }
}
