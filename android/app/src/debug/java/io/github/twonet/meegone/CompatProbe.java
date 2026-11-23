package io.github.twonet.meegone;

import android.content.pm.ActivityInfo;
import android.content.pm.LauncherApps;
import android.content.res.Configuration;
import android.os.Looper;
import android.os.Process;
import android.os.UserHandle;
import android.os.UserManager;
import android.util.Log;
import android.view.Display;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * Debug builds only: what {@code tools/compat-check.sh} needs to know of the home on each
 * device and in each navigation mode, logged under {@link #TAG} (COMP-01, COMP-02, ADR 0044).
 * The extra {@link DebugHooks#EXTRA_COMPAT} turns it on for the rest of the process and logs a
 * report once the home has settled:
 *
 * <ul>
 *   <li>{@code device sdk=<n> sw=<dp> window=<w>x<h> requested=<orientation> rotation=<0-3>
 *       folds=<bool> gestures=<bool>}: the window's smallest width and size in dp, the
 *       orientation the home asks for (ADR 0009), the display's rotation, whether the device
 *       has a hinge, and whether MeeGone takes the navigation mode for gestures (ANIM-09).
 *   <li>{@code profile user=<id> serial=<n> own=<bool> work=<bool> paused=<bool> locked=<bool>
 *       apps=<n>}: one line for each profile a launcher sees, with the flags MeeGone gives it
 *       (APPS-01, APPS-09, APPS-18, APPS-20).
 *   <li>{@code lock unsupported=<code|none> id=<n> may-not-show=<bool>}: why the lock-screen
 *       image can't be set, the lock screen's wallpaper id, and whether Settings says the device
 *       may not show the image (WALL-13, WALL-14, C5, ADR 0030).
 *   <li>{@code state view=<0-2> open=<bool> settings=<0-2> setup=<step>}: the view shown,
 *       whether anything Back closes is open, Settings, and the setup step shown, or -1.
 *   <li>{@code done}: the report's end.
 * </ul>
 *
 * <p>While it is on, each Home and Back is marked once the home has settled after it: {@code
 * home view=… open=… settings=… setup=…} and {@code back …} the same.
 */
final class CompatProbe {
    static final String TAG = "MeeGoneCompat";

    /** Main thread. */
    private static boolean on;

    private CompatProbe() {}

    /** Turns the probe on and logs the report once the home has settled. */
    static void report(HomeActivity activity, HomeViews views) {
        on = true;
        whenSettled(views, () -> {
            Bridge bridge = Bridge.get(activity);
            Configuration config = activity.getResources().getConfiguration();
            Display display = views.getDisplay();
            Log.i(TAG, "device sdk=" + android.os.Build.VERSION.SDK_INT
                    + " sw=" + config.smallestScreenWidthDp
                    + " window=" + config.screenWidthDp + "x" + config.screenHeightDp
                    + " requested=" + orientation(activity.getRequestedOrientation())
                    + " rotation=" + (display == null ? -1 : display.getRotation())
                    + " folds=" + bridge.folds()
                    + " gestures=" + bridge.animatesReturnHome());
            profiles(activity, bridge);
            String unsupported = bridge.lockScreenUnsupported();
            Log.i(TAG, "lock unsupported=" + (unsupported == null ? "none" : unsupported)
                    + " id=" + bridge.lockScreenWallpaperId()
                    + " may-not-show=" + views.auditImageMayNotShow());
            Log.i(TAG, "state " + state(views));
            Log.i(TAG, "done");
        });
    }

    /** The system's Home action reached the home. */
    static void home(HomeViews views) {
        if (on) {
            whenSettled(views, () -> Log.i(TAG, "home " + state(views)));
        }
    }

    /** Back reached the home and the Rust app has answered it. */
    static void back(HomeViews views) {
        if (on) {
            whenSettled(views, () -> Log.i(TAG, "back " + state(views)));
        }
    }

    private static void profiles(HomeActivity activity, Bridge bridge) {
        LauncherApps launcher = activity.getSystemService(LauncherApps.class);
        UserManager users = activity.getSystemService(UserManager.class);
        Map<Long, Integer> flags = new HashMap<>();
        String[] fields = bridge.profiles();
        for (int i = 0; fields != null && i + 1 < fields.length; i += 2) {
            flags.put(Long.parseLong(fields[i]), Integer.parseInt(fields[i + 1]));
        }
        for (UserHandle user : launcher.getProfiles()) {
            long serial = users.getSerialNumberForUser(user);
            int of = flags.getOrDefault(serial, 0);
            List<?> apps = launcher.getActivityList(null, user);
            Log.i(TAG, "profile user=" + id(user)
                    + " serial=" + serial
                    + " own=" + user.equals(Process.myUserHandle())
                    + " work=" + ((of & Bridge.PROFILE_WORK) != 0)
                    + " paused=" + ((of & Bridge.PROFILE_PAUSED) != 0)
                    + " locked=" + ((of & Bridge.PROFILE_LOCKED) != 0)
                    + " apps=" + apps.size());
        }
    }

    /** The user id in {@code UserHandle{10}}: the SDK has no public getter for it. */
    private static String id(UserHandle user) {
        return user.toString().replaceAll("\\D", "");
    }

    private static String orientation(int requested) {
        switch (requested) {
            case ActivityInfo.SCREEN_ORIENTATION_PORTRAIT:
                return "portrait";
            case ActivityInfo.SCREEN_ORIENTATION_FULL_USER:
                return "full-user";
            case ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED:
                return "unspecified";
            default:
                return Integer.toString(requested);
        }
    }

    private static String state(HomeViews views) {
        return "view=" + views.auditView()
                + " open=" + views.auditOpen()
                + " settings=" + views.auditSettings()
                + " setup=" + views.auditSetupStep();
    }

    /** Runs {@code then} once a frame has shown the home with nothing moving and it is idle. */
    private static void whenSettled(HomeViews views, Runnable then) {
        views.getViewTreeObserver().registerFrameCommitCallback(() -> {
            if (!views.auditSettled()) {
                whenSettled(views, then);
                return;
            }
            Looper.myQueue().addIdleHandler(() -> {
                then.run();
                return false;
            });
        });
        views.invalidate();
    }
}
