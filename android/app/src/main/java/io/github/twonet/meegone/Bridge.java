package io.github.twonet.meegone;

import android.app.Activity;
import android.app.role.RoleManager;
import android.content.ActivityNotFoundException;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.SystemClock;
import android.provider.Settings;
import android.util.Log;
import java.util.TimeZone;

/**
 * What the Rust platform adapters ask of Android (ADR 0005).
 *
 * <p>Rust looks these methods up by name and signature on the main thread, so keep them in step
 * with {@code crates/core/data/src/android/system.rs}. Every method catches what Android may
 * throw: an exception must never reach Rust. Logs hold only yes/no answers, never user data.
 */
final class Bridge {
    private static final String TAG = "MeeGone";
    private static final int REQUEST_HOME_ROLE = 1;
    /**
     * A role dialog that closes this fast was never shown: the system refuses after "Don't ask
     * again", or if MeeGone isn't a valid home app.
     */
    private static final long NEVER_SHOWN_MS = 500;
    private static final String INSTRUCTIONS =
            "To make MeeGone your home screen, open Settings, then Apps, Default apps, Home app,"
                    + " and choose MeeGone.";

    private static Bridge instance;

    private final Context context;
    /** The resumed activity, which can start the system's screens; null while none is. */
    private Activity activity;
    private long roleRequestedAt;

    private Bridge(Context context) {
        this.context = context.getApplicationContext();
    }

    /** The one bridge of this process, shared by every activity. */
    static synchronized Bridge get(Context context) {
        if (instance == null) {
            instance = new Bridge(context);
        }
        return instance;
    }

    void setActivity(Activity activity) {
        this.activity = activity;
    }

    void clearActivity(Activity activity) {
        if (this.activity == activity) {
            this.activity = null;
        }
    }

    /** Whether MeeGone holds the home-app role (HOME-01, ONB-04). Called from Rust. */
    boolean isDefaultHome() {
        boolean held;
        try {
            RoleManager roles = context.getSystemService(RoleManager.class);
            if (roles != null && roles.isRoleAvailable(RoleManager.ROLE_HOME)) {
                held = roles.isRoleHeld(RoleManager.ROLE_HOME);
            } else {
                Intent home = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME);
                ResolveInfo info = context.getPackageManager()
                        .resolveActivity(home, PackageManager.MATCH_DEFAULT_ONLY);
                held = info != null
                        && info.activityInfo != null
                        && context.getPackageName().equals(info.activityInfo.packageName);
            }
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't check the default home app", e);
            held = false;
        }
        Log.i(TAG, "Default home app: " + held);
        return held;
    }

    /**
     * Opens the system dialog for choosing the default home app, or else the system's home-app
     * settings. Returns null, or written instructions if no system screen could be opened
     * (SRS 8.1). Called from Rust.
     */
    String requestDefaultHome() {
        Activity current = activity;
        if (current == null) {
            return INSTRUCTIONS;
        }
        try {
            RoleManager roles = current.getSystemService(RoleManager.class);
            if (roles != null && roles.isRoleAvailable(RoleManager.ROLE_HOME)) {
                // The dialog reads the calling package, so it needs startActivityForResult.
                roleRequestedAt = SystemClock.elapsedRealtime();
                current.startActivityForResult(
                        roles.createRequestRoleIntent(RoleManager.ROLE_HOME), REQUEST_HOME_ROLE);
                Log.i(TAG, "Asked for the home role");
                return null;
            }
            return openHomeSettings(current) ? null : INSTRUCTIONS;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't ask for the home role", e);
            return INSTRUCTIONS;
        }
    }

    /**
     * After the role dialog. If the system closed it at once without showing it, opens the
     * home-app settings instead, so the user's tap still leads somewhere.
     */
    void onActivityResult(Activity from, int requestCode, int resultCode) {
        if (requestCode != REQUEST_HOME_ROLE) {
            return;
        }
        boolean neverShown = resultCode != Activity.RESULT_OK
                && SystemClock.elapsedRealtime() - roleRequestedAt < NEVER_SHOWN_MS;
        Log.i(TAG, "Role dialog closed; accepted: " + (resultCode == Activity.RESULT_OK)
                + ", shown: " + !neverShown);
        if (neverShown && !isDefaultHome()) {
            openHomeSettings(from);
        }
    }

    /** The UTC offset of the device's time zone at {@code atMs}, in ms. Called from Rust. */
    long utcOffsetMs(long atMs) {
        try {
            return TimeZone.getDefault().getOffset(atMs);
        } catch (RuntimeException e) {
            return 0;
        }
    }

    private static boolean openHomeSettings(Activity from) {
        String[] actions = {
            Settings.ACTION_HOME_SETTINGS, Settings.ACTION_MANAGE_DEFAULT_APPS_SETTINGS
        };
        for (String action : actions) {
            try {
                from.startActivity(new Intent(action));
                return true;
            } catch (ActivityNotFoundException | SecurityException e) {
                // Try the next screen.
            }
        }
        return false;
    }
}
