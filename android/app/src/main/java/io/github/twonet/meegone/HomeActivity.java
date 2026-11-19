package io.github.twonet.meegone;

import android.app.Activity;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.res.Configuration;
import android.graphics.Point;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.window.OnBackInvokedDispatcher;

/**
 * MeeGone's one activity: the home screen (HOME-01), which also opens from the app drawer as a
 * normal app. It hands Android's lifecycle to the Rust app (ADR 0005), which shares one
 * launcher between every instance of this activity, and shows the home views (ADR 0006).
 *
 * <p>It tells the Rust app about Home, Back and coming back to the home (ADR 0007). Whether the
 * home was out of sight is tracked here, per instance; what each case does is decided in Rust.
 *
 * <p>It draws edge to edge on every Android version, stays in portrait on phones and turns with
 * the device on large screens (HOME-11, HOME-12, ADR 0009). Size and orientation changes are
 * handled in place; any other configuration change, such as a new density when a foldable
 * changes screens, recreates it. Either way the shared launcher keeps everything (COMP-03).
 */
public final class HomeActivity extends Activity {
    static {
        System.loadLibrary("meegone_launcher");
    }

    // What the saved state holds, in case the system ends the process (REL-05).
    private static final String SAVED_VIEW = "io.github.twonet.meegone.view";
    private static final String SAVED_APPS_SCROLL = "io.github.twonet.meegone.appsScroll";
    private static final int NONE = -1;
    /** From this smallest screen width, in dp, the home turns with the device (HOME-12). */
    private static final int LARGE_SCREEN_DP = 600;

    private final int[] saved = new int[2];
    private Bridge bridge;
    private HomeViews views;
    private boolean debugRequestHome;
    /**
     * An app or a system screen covers the home: from {@code onStop} until the next {@code
     * onResume}. An instance that comes back after the system ended the process starts out of
     * sight too.
     */
    private boolean away;
    /** The system Home action arrived while the home was out of sight. */
    private boolean homeWhileAway;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        bridge = Bridge.get(this);
        int savedView = NONE;
        int savedAppsScroll = 0;
        if (savedInstanceState != null) {
            savedView = savedInstanceState.getInt(SAVED_VIEW, NONE);
            savedAppsScroll = savedInstanceState.getInt(SAVED_APPS_SCROLL, 0);
        }
        // Before MeeGone starts, so no icon or accent is drawn without it.
        DebugHooks.readIconExtras(getIntent());
        boolean started = nativeStart(bridge, getFilesDir().getPath(), savedView, savedAppsScroll);
        if (started) {
            // Checks the daily image if a check is due, and schedules its job again, which a
            // restart or a force stop drops (SRS 8.1, ADR 0029).
            ImageJobService.checkAtStart(this);
        }
        // Applications follows the system's app and profile changes (ADR 0010).
        bridge.watchApps();
        // Events' header follows the date, the clock and the next alarm (ADR 0019).
        bridge.watchClock();
        // Events' list follows the notifications and notification access (ADR 0020).
        bridge.watchNotifications();
        // Recent follows usage access, even while it shows (ADR 0023).
        bridge.watchUsageAccess();
        // Saved state with a launcher this call started: the system had ended the process while
        // the user was elsewhere. A recreation after a configuration change finds it running.
        away = started && savedInstanceState != null;
        drawEdgeToEdge();
        followScreenSize();
        views = new HomeViews(this);
        setContentView(views);
        listenForBack();
        readDebugExtras(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        if (isHome(intent)) {
            if (away) {
                homeWhileAway = true; // Handled when the home is back on screen, in onResume.
            } else {
                nativeHome();
                views.refresh();
            }
        }
        readDebugExtras(intent);
    }

    @Override
    protected void onResume() {
        super.onResume();
        bridge.setActivity(this);
        nativeResumed(away, homeWhileAway);
        away = false;
        homeWhileAway = false;
        views.onResumed();
        if (debugRequestHome) {
            debugRequestHome = false;
            nativeRequestDefaultHome();
        }
    }

    @Override
    protected void onPause() {
        bridge.clearActivity(this);
        super.onPause();
    }

    @Override
    protected void onStop() {
        away = true;
        super.onStop();
    }

    @Override
    protected void onSaveInstanceState(Bundle outState) {
        super.onSaveInstanceState(outState);
        nativeSave(saved);
        outState.putInt(SAVED_VIEW, saved[0]);
        outState.putInt(SAVED_APPS_SCROLL, saved[1]);
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        // Folding or unfolding can move the home to a screen of another size (HOME-12).
        // HomeViews follows its own new size.
        followScreenSize();
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        bridge.onActivityResult(this, requestCode, resultCode);
    }

    /**
     * Back always reaches the Rust app, which closes the topmost open element or does nothing
     * (HOME-08). From Android 13 that takes a callback; Android 16 no longer calls {@link
     * #onBackPressed} for apps that target it.
     */
    private void listenForBack() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT, this::back);
        }
    }

    @Override
    @SuppressWarnings("deprecation") // Only Android 10 to 12 call it; see listenForBack.
    public void onBackPressed() {
        back(); // Never finishes the home.
    }

    private void back() {
        nativeBack();
        views.refresh();
    }

    /**
     * Lays the home out behind the system bars, as Android 15 and later always do (HOME-11). Only
     * the layout changes: the bars stay visible, the status bar on every view. {@link HomeViews}
     * keeps content clear of them.
     */
    @SuppressWarnings("deprecation") // The flags are the only way on Android 10.
    private void drawEdgeToEdge() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            getWindow().setDecorFitsSystemWindows(false);
        } else {
            // Android combines every view's flags, so HomeViews' bar colours still apply.
            getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
        }
    }

    /**
     * Portrait on phones; every orientation on large screens (HOME-12). The screen decides, not
     * the window: a split screen on a tablet is still a large screen. Android 16 also ignores
     * orientation requests on large screens.
     */
    private void followScreenSize() {
        int wanted = isLargeScreen()
                ? ActivityInfo.SCREEN_ORIENTATION_FULL_USER
                : ActivityInfo.SCREEN_ORIENTATION_PORTRAIT;
        if (getRequestedOrientation() != wanted) {
            setRequestedOrientation(wanted);
        }
    }

    /** Whether the screen's smallest width is 600 dp or more: a tablet or an unfolded foldable. */
    @SuppressWarnings("deprecation") // getRealSize is the only way on Android 10.
    private boolean isLargeScreen() {
        int width;
        int height;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Rect screen = getWindowManager().getMaximumWindowMetrics().getBounds();
            width = screen.width();
            height = screen.height();
        } else {
            Point screen = new Point();
            getWindowManager().getDefaultDisplay().getRealSize(screen);
            width = screen.x;
            height = screen.y;
        }
        float density = getResources().getDisplayMetrics().density;
        return Math.min(width, height) >= LARGE_SCREEN_DP * density;
    }

    /**
     * The system changed the apps or the profiles (APPS-09, ADR 0010). The shared launcher
     * reloads the app list, and {@code resumed}, if it is a home, redraws. Called by {@link
     * Bridge} on the main thread.
     */
    static void appsChanged(Activity resumed) {
        nativeAppsChanged();
        if (resumed instanceof HomeActivity) {
            ((HomeActivity) resumed).views.refresh();
        }
    }

    /**
     * The date, the time, the time zone, the 12- or 24-hour setting or the next alarm changed
     * (EVT-01, EVT-02, ADR 0019). The shared launcher writes the Events header again, and {@code
     * resumed}, if it is a home, redraws. Called by {@link Bridge} on the main thread.
     */
    static void clockChanged(Activity resumed) {
        nativeClockChanged();
        if (resumed instanceof HomeActivity) {
            ((HomeActivity) resumed).views.refresh();
        }
    }

    /**
     * A notification was posted or removed, or notification access was granted or revoked
     * (EVT-10, ADR 0020). The shared launcher reads the notifications again, and {@code
     * resumed}, if it is a home, redraws. Called by {@link Bridge} on the main thread.
     */
    static void notificationsChanged(Activity resumed) {
        nativeNotificationsChanged();
        if (resumed instanceof HomeActivity) {
            ((HomeActivity) resumed).views.refresh();
        }
    }

    /**
     * Usage access was granted or revoked (REC-02, ADR 0023). The shared launcher reads the
     * recent apps again, and {@code resumed}, if it is a home, redraws. Called by {@link Bridge}
     * on the main thread.
     */
    static void usageAccessChanged(Activity resumed) {
        nativeUsageAccessChanged();
        if (resumed instanceof HomeActivity) {
            ((HomeActivity) resumed).views.refresh();
        }
    }

    /**
     * The daily image's check reached the feed (ADR 0029). The shared launcher, if it has
     * started, reads the image and how the checks are going again, and {@code resumed}, if it is
     * a home, redraws. Called by {@link Bridge} on the main thread.
     */
    static void imageChecked(Activity resumed) {
        nativeImageChecked();
        if (resumed instanceof HomeActivity) {
            ((HomeActivity) resumed).views.refresh();
        }
    }

    private static boolean isHome(Intent intent) {
        return Intent.ACTION_MAIN.equals(intent.getAction())
                && intent.hasCategory(Intent.CATEGORY_HOME);
    }

    private void readDebugExtras(Intent intent) {
        if (DebugHooks.requestsHomeRole(intent)) {
            debugRequestHome = true;
        }
    }

    /**
     * Starts MeeGone, once per process; later calls do nothing. {@code savedView} is the view
     * from the saved state, or -1. Returns whether this call started it.
     */
    private native boolean nativeStart(
            Bridge bridge, String filesDir, int savedView, int savedAppsScrollPx);

    /**
     * MeeGone came to the foreground: the default home app or the day may have changed. {@code
     * returning} says an app or a system screen had covered the home, and {@code byHome} that
     * the system Home action brought it back.
     */
    private native void nativeResumed(boolean returning, boolean byHome);

    /** The system Home action while the home is on screen. */
    private native void nativeHome();

    /** The system Back action. */
    private native void nativeBack();

    /** Fills {@code saved} with the current view and how far Applications is scrolled. */
    private native void nativeSave(int[] saved);

    /** An app or a profile changed; the launcher reloads the app list. */
    private static native void nativeAppsChanged();

    /** The date, the clock, the time zone or the next alarm changed (ADR 0019). */
    private static native void nativeClockChanged();

    /** A notification came or went, or notification access changed (ADR 0020). */
    private static native void nativeNotificationsChanged();

    /** Usage access was granted or revoked (ADR 0023). */
    private static native void nativeUsageAccessChanged();

    /** The daily image's check reached the feed (ADR 0029); nothing before MeeGone starts. */
    private static native void nativeImageChecked();

    /** Taps the banner's "make default" button; only debug builds' {@link DebugHooks} call it. */
    private native void nativeRequestDefaultHome();
}
