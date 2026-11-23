package io.github.twonet.meegone;

import android.app.Activity;
import android.content.Intent;
import android.util.Log;

/**
 * Restart MeeGone, on a card that says a part of it stopped working (ARCH-09, ADR 0043). A part
 * that failed stays off for the rest of the process, so the process ends and MeeGone starts
 * again in a new one, with every part working.
 *
 * <p>Its home activity is started first, so it is the one Android brings back. It is
 * {@code singleTask} and {@code stateNotNeeded}, so it comes back fresh rather than restoring the
 * failed state (ADR 0005). The process then ends with code 0, which Android records as the app
 * exiting itself, never as a crash, and so do the platform's stability reports. Nothing is lost:
 * every file is written atomically (REL-03), and what is only in memory, such as "Updating…",
 * starts again.
 */
final class Restart {
    private static final String TAG = "MeeGone";

    private Restart() {
    }

    /** Ends this process and starts MeeGone's home activity again in a new one. */
    static void now(Activity activity) {
        Log.i(TAG, "Restarting at the user's request");
        Intent home = new Intent(activity, HomeActivity.class)
                .setAction(Intent.ACTION_MAIN)
                // In the home's own task while MeeGone is the home app, or a normal app's.
                .addCategory(Bridge.get(activity).isDefaultHome()
                        ? Intent.CATEGORY_HOME
                        : Intent.CATEGORY_LAUNCHER)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            activity.startActivity(home);
        } catch (RuntimeException e) {
            // The home still comes back: Android restarts the default home app on its own.
            Log.w(TAG, "Couldn't ask for the home again: " + e.getClass().getSimpleName());
        }
        System.exit(0);
    }
}
