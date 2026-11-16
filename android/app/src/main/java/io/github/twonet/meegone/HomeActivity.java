package io.github.twonet.meegone;

import android.app.Activity;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.window.OnBackInvokedDispatcher;

/**
 * MeeGone's one activity: the home screen (HOME-01), which also opens from the app drawer as a
 * normal app. It hands Android's lifecycle to the Rust app (ADR 0005), which shares one
 * launcher between every instance of this activity, and shows the home views (ADR 0006).
 *
 * <p>It tells the Rust app about Home, Back and coming back to the home (ADR 0007). Whether the
 * home was out of sight is tracked here, per instance; what each case does is decided in Rust.
 */
public final class HomeActivity extends Activity {
    static {
        System.loadLibrary("meegone_launcher");
    }

    // What the saved state holds, in case the system ends the process (REL-05).
    private static final String SAVED_VIEW = "io.github.twonet.meegone.view";
    private static final String SAVED_APPS_SCROLL = "io.github.twonet.meegone.appsScroll";
    private static final int NONE = -1;

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
        boolean started = nativeStart(bridge, getFilesDir().getPath(), savedView, savedAppsScroll);
        // Saved state with a launcher this call started: the system had ended the process while
        // the user was elsewhere. A recreation after a configuration change finds it running.
        away = started && savedInstanceState != null;
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

    /** Taps the banner's "make default" button; only debug builds' {@link DebugHooks} call it. */
    private native void nativeRequestDefaultHome();
}
