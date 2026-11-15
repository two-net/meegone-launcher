package io.github.twonet.meegone;

import android.content.Intent;

/**
 * Debug builds only: ways to drive MeeGone from adb before its UI is drawn (ADR 0005). Release
 * builds get the version in {@code src/release}, which does nothing (SEC-06).
 */
final class DebugHooks {
    /** {@code adb shell am start -n io.github.twonet.meegone/.HomeActivity --ez <this> true} */
    static final String EXTRA_REQUEST_HOME = "io.github.twonet.meegone.debug.REQUEST_HOME";

    private DebugHooks() {}

    /** Whether {@code intent} asks to tap the banner's "make default" button, once. */
    static boolean requestsHomeRole(Intent intent) {
        if (intent == null || !intent.getBooleanExtra(EXTRA_REQUEST_HOME, false)) {
            return false;
        }
        intent.removeExtra(EXTRA_REQUEST_HOME);
        return true;
    }
}
