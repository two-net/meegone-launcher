package io.github.twonet.meegone;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;

/**
 * MeeGone's one activity: the home screen (HOME-01), which also opens from the app drawer as a
 * normal app. It hands Android's lifecycle to the Rust app (ADR 0005), which shares one
 * launcher between every instance of this activity and draws nothing yet.
 */
public final class HomeActivity extends Activity {
    static {
        System.loadLibrary("meegone_launcher");
    }

    private Bridge bridge;
    private boolean debugRequestHome;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        bridge = Bridge.get(this);
        nativeStart(bridge, getFilesDir().getPath());
        readDebugExtras(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        readDebugExtras(intent);
    }

    @Override
    protected void onResume() {
        super.onResume();
        bridge.setActivity(this);
        nativeResumed();
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
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        bridge.onActivityResult(this, requestCode, resultCode);
    }

    private void readDebugExtras(Intent intent) {
        if (DebugHooks.requestsHomeRole(intent)) {
            debugRequestHome = true;
        }
    }

    /** Starts MeeGone, once per process; later calls do nothing. */
    private native void nativeStart(Bridge bridge, String filesDir);

    /** MeeGone came to the foreground: the default home app or the day may have changed. */
    private native void nativeResumed();

    /** Taps the banner's "make default" button; only debug builds' {@link DebugHooks} call it. */
    private native void nativeRequestDefaultHome();
}
