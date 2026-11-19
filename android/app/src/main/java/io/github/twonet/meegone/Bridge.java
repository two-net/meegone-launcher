package io.github.twonet.meegone;

import android.Manifest;
import android.app.Activity;
import android.app.ActivityOptions;
import android.app.AlarmManager;
import android.app.AppOpsManager;
import android.app.Notification;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.SearchManager;
import android.app.WallpaperManager;
import android.app.role.RoleManager;
import android.app.usage.UsageEvents;
import android.app.usage.UsageStatsManager;
import android.content.ActivityNotFoundException;
import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.ApplicationInfo;
import android.content.pm.LauncherActivityInfo;
import android.content.pm.LauncherApps;
import android.content.pm.LauncherUserInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.content.pm.ShortcutInfo;
import android.content.res.Configuration;
import android.database.ContentObserver;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.BitmapRegionDecoder;
import android.graphics.Insets;
import android.graphics.Rect;
import android.hardware.display.DisplayManager;
import android.icu.lang.UCharacter;
import android.icu.text.Collator;
import android.icu.text.DateFormat;
import android.icu.text.DisplayContext;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.net.Uri;
import android.os.BatteryManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.PowerManager;
import android.os.Process;
import android.os.SystemClock;
import android.os.UserHandle;
import android.os.UserManager;
import android.os.storage.StorageManager;
import android.provider.AlarmClock;
import android.provider.MediaStore;
import android.provider.Settings;
import android.provider.Telephony;
import android.service.notification.NotificationListenerService;
import android.service.notification.StatusBarNotification;
import android.telecom.TelecomManager;
import android.text.TextUtils;
import android.util.Log;
import android.view.Display;
import android.view.HapticFeedbackConstants;
import android.view.View;
import android.view.WindowInsets;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.net.URL;
import java.net.UnknownServiceException;
import java.text.Normalizer;
import java.util.ArrayList;
import java.util.Date;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TimeZone;
import java.util.function.Supplier;
import java.util.regex.Pattern;
import javax.net.ssl.HttpsURLConnection;
import javax.net.ssl.SSLException;

/**
 * What the Rust platform adapters ask of Android (ADR 0005).
 *
 * <p>Rust looks these methods up by name and signature on the main thread, so keep them in step
 * with {@code crates/core/data/src/android/system.rs}, {@code apps.rs}, {@code
 * notifications.rs}, {@code usage.rs}, {@code network.rs}, {@code wallpaper.rs} and {@code
 * scheduler.rs}. Every method
 * catches what Android may throw: an exception must never reach Rust. Logs hold only yes/no
 * answers and the kinds of errors, never user data such as which apps are installed or used, or
 * what a notification says (EVT-13, PRIV-03).
 */
final class Bridge {
    private static final String TAG = "MeeGone";
    /** The system Settings' wallpaper screen, which isn't among {@code Settings}' constants. */
    private static final String WALLPAPER_SETTINGS = "android.settings.WALLPAPER_SETTINGS";

    // The app catalogue's records (ADR 0010), mirrored in crates/core/data/src/catalogue.rs.
    // An app: package, activity, label, profile serial number, flags, accent (ADR 0011),
    // collation key and first install time (ADR 0012), and search key (ADR 0016).
    private static final int APP_AVAILABLE = 1;
    private static final int APP_CAN_UNINSTALL = 2;
    private static final int APP_WORK = 4;
    // A profile: serial number, flags.
    private static final int PROFILE_WORK = 1;
    private static final int PROFILE_PAUSED = 2;
    private static final int PROFILE_LOCKED = 4;
    // A shortcut (ADR 0013): id, short label, flags, rank.
    private static final int SHORTCUT_MANIFEST = 1;
    // A notification (ADR 0020), mirrored in crates/core/data/src/notification_records.rs: key,
    // package, profile serial number, the app's name, title, text, post time and flags.
    private static final int NOTIFICATION_CLEARABLE = 1;
    private static final int NOTIFICATION_ONGOING = 2;
    private static final int NOTIFICATION_SUMMARY = 4;
    private static final int NOTIFICATION_AUTO_CANCEL = 8;
    // An app used recently (ADR 0023), mirrored in crates/core/data/src/usage_records.rs:
    // package, the class of the activity last resumed, profile serial number and when it was
    // last used.
    private static final int USE_FIELDS = 4;
    // What httpsGet reports in its outcome array (ADR 0027), mirrored in
    // crates/core/data/src/https.rs: the body follows; a redirect, to the detail's absolute URL;
    // no connection; a body over the limit; not HTTPS; a secure connection that failed, such as
    // a certificate that didn't validate; anything else.
    private static final String GET_OK = "ok";
    private static final String GET_REDIRECT = "redirect";
    private static final String GET_OFFLINE = "offline";
    private static final String GET_TOO_LARGE = "too-large";
    private static final String GET_NOT_HTTPS = "not-https";
    private static final String GET_INSECURE = "insecure";
    private static final String GET_FAILED = "failed";
    // What networkState answers, mirrored in crates/core/data/src/android/network.rs.
    private static final int NETWORK_OFFLINE = 0;
    private static final int NETWORK_METERED = 1;
    private static final int NETWORK_UNMETERED = 2;
    /**
     * The user agent of every request. It names the app and nothing else, where the platform's
     * default names the device's model and build (PRIV-02).
     */
    private static final String USER_AGENT = "MeeGone";
    private static final int CONNECT_TIMEOUT_MS = 15_000;
    private static final int READ_TIMEOUT_MS = 30_000;
    /** The JPEG quality the daily image is written at once cropped (ADR 0028). */
    private static final int JPEG_QUALITY = 90;
    /**
     * The extra by which the system's notification settings learn which profile's copy of an app
     * to show: the app's uid there. Settings reads it with the package (ADR 0021).
     */
    private static final String EXTRA_APP_UID = "app_uid";
    /**
     * The extra in which every notification carries the app info of the app that posted it, as
     * {@code Notification.Builder.recoverBuilder} reads it. It names apps that Android 11's
     * package visibility hides from MeeGone, such as System UI.
     */
    private static final String EXTRA_APP_INFO = "android.appInfo";
    /**
     * The system's broadcasts about the work profile, and about the language, which changes the
     * apps' labels and their alphabetical order (L10N-04). Sent only to registered receivers.
     */
    private static final String[] CATALOGUE_CHANGES = {
        Intent.ACTION_MANAGED_PROFILE_ADDED,
        Intent.ACTION_MANAGED_PROFILE_REMOVED,
        Intent.ACTION_MANAGED_PROFILE_AVAILABLE,
        Intent.ACTION_MANAGED_PROFILE_UNAVAILABLE,
        Intent.ACTION_MANAGED_PROFILE_UNLOCKED,
        Intent.ACTION_LOCALE_CHANGED,
    };
    /**
     * The system's broadcasts that change the Events header: a new day, the clock or the 12- or
     * 24-hour setting set (both {@code TIME_SET}), a new time zone, and a new next alarm (EVT-01,
     * EVT-02, L10N-03, ADR 0019).
     */
    private static final String[] CLOCK_CHANGES = {
        Intent.ACTION_DATE_CHANGED,
        Intent.ACTION_TIME_CHANGED,
        Intent.ACTION_TIMEZONE_CHANGED,
        AlarmManager.ACTION_NEXT_ALARM_CLOCK_CHANGED,
    };
    // The styles formatTime writes, mirrored in style_code in
    // crates/core/data/src/android/system.rs: the weekday, the day and month, the time, and the
    // short weekday with the time.
    private static final int STYLE_WEEKDAY = 0;
    private static final int STYLE_DAY_MONTH = 1;
    private static final int STYLE_TIME = 2;
    private static final int STYLE_WEEKDAY_TIME = 3;
    private static final int STYLE_DAY_MONTH_TIME = 4;
    private static final char[] HEX = "0123456789abcdef".toCharArray();
    /** Accents and other marks that sit on a letter, removed for search (L10N-04). */
    private static final Pattern MARKS = Pattern.compile("\\p{Mn}+");
    /** Runs of spaces of any kind, collapsed to one for search. */
    private static final Pattern SPACES = Pattern.compile("[\\s\\p{Z}]+");
    private static final int REQUEST_HOME_ROLE = 1;
    /**
     * A role dialog that closes this fast was never shown: the system refuses after "Don't ask
     * again", or if MeeGone isn't a valid home app.
     */
    private static final long NEVER_SHOWN_MS = 500;

    private static Bridge instance;

    private final Context context;
    private final Handler main = new Handler(Looper.getMainLooper());
    /** Tells the Rust app about a burst of app and profile changes once (ADR 0010). */
    private final Runnable reportAppsChanged = () -> HomeActivity.appsChanged(this.activity);
    /** Tells the Rust app about a burst of clock and alarm changes once (ADR 0019). */
    private final Runnable reportClockChanged = () -> HomeActivity.clockChanged(this.activity);
    /** Tells the Rust app about a burst of notifications coming and going once (ADR 0020). */
    private final Runnable reportNotificationsChanged =
            () -> HomeActivity.notificationsChanged(this.activity);
    /** Tells the Rust app that usage access was granted or revoked (ADR 0023). */
    private final Runnable reportUsageAccessChanged =
            () -> HomeActivity.usageAccessChanged(this.activity);
    /** Tells the Rust app that the system animation speed changed (ANIM-05, ADR 0034). */
    private final Runnable reportAnimationScaleChanged =
            () -> HomeActivity.animationScaleChanged(this.activity);
    /** Each app's accent colour, derived from its icon (APPS-17, ADR 0011). */
    private final AppAccents accents;
    /** The resumed activity, which can start the system's screens; null while none is. */
    private Activity activity;
    private long roleRequestedAt;
    private boolean watchingApps;
    private boolean watchingClock;
    private boolean watchingNotifications;
    private boolean watchingUsageAccess;
    private boolean watchingAnimationScale;

    private Bridge(Context context) {
        this.context = context.getApplicationContext();
        // New accents change the catalogue, as an install would.
        accents = new AppAccents(this.context, this::appsChanged);
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
            return homeInstructions();
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
            return openHomeSettings(current) ? null : homeInstructions();
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't ask for the home role", e);
            return homeInstructions();
        }
    }

    /**
     * How to make MeeGone the home app without a system screen to open, in the device's language
     * (SRS 8.1, L10N-01).
     */
    private String homeInstructions() {
        return context.getString(R.string.default_home_instructions);
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

    /**
     * Whether the device's dark theme is on now, set by the user or by its schedule (THEME-02,
     * ADR 0033). The application's configuration follows the device even while the home
     * handles the change itself, so this answers for every instance of it. Called from Rust.
     */
    boolean isDark() {
        int night = context.getResources().getConfiguration().uiMode
                & Configuration.UI_MODE_NIGHT_MASK;
        return night == Configuration.UI_MODE_NIGHT_YES;
    }

    /**
     * Whether the system animates the home's return from an app itself, so MeeGone's own return
     * motion would double up (ANIM-09, ADR 0007). It does with gesture navigation, whose
     * swipe-home motion carries the app into the home: the navigation bar is then there but
     * has nothing to tap. With unknown insets the answer is yes, so the two never double up.
     * Called from Rust.
     */
    @SuppressWarnings("deprecation") // getTappableElementInsets is the only way on Android 10.
    boolean animatesReturnHome() {
        Activity current = activity;
        try {
            View decor = current == null ? null : current.getWindow().peekDecorView();
            WindowInsets insets = decor == null ? null : decor.getRootWindowInsets();
            if (insets == null) {
                return true;
            }
            Insets tappable;
            Insets bars;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                tappable = insets.getInsets(WindowInsets.Type.tappableElement());
                bars = insets.getInsets(WindowInsets.Type.navigationBars());
            } else {
                tappable = insets.getTappableElementInsets();
                bars = insets.getSystemWindowInsets();
            }
            boolean gestures = tappable.left == 0 && tappable.right == 0 && tappable.bottom == 0
                    && (bars.left > 0 || bars.right > 0 || bars.bottom > 0);
            Log.i(TAG, "Gesture navigation: " + gestures);
            return gestures;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't check the navigation mode", e);
            return true;
        }
    }

    /**
     * A light haptic tick, as on a long-press (ANIM-24). A view plays it, so it follows the
     * system's touch-feedback setting and needs no permission (SEC-03). Returns whether a view
     * could play it. Called from Rust.
     */
    boolean hapticTick() {
        Activity current = activity;
        try {
            View decor = current == null ? null : current.getWindow().peekDecorView();
            return decor != null
                    && decor.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't play the haptic tick", e);
            return false;
        }
    }

    /**
     * Whether anything opens an {@code https:} link, so web search can be offered (SRCH-05).
     * The manifest's queries let MeeGone see browsers. Called from Rust.
     */
    boolean hasBrowser() {
        try {
            Intent browse = new Intent(Intent.ACTION_VIEW, Uri.parse("https://"))
                    .addCategory(Intent.CATEGORY_BROWSABLE);
            return context.getPackageManager()
                    .resolveActivity(browse, PackageManager.MATCH_DEFAULT_ONLY) != null;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't look for a browser: " + e.getClass().getSimpleName());
            return false;
        }
    }

    /**
     * Searches the web for {@code query} with the device's default search handler (SRCH-05,
     * ADR 0016). Returns null, or a short message if nothing could search. The query is never
     * logged. Called from Rust.
     */
    String webSearch(String query) {
        try {
            Intent search = new Intent(Intent.ACTION_WEB_SEARCH)
                    .putExtra(SearchManager.QUERY, query)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            Activity current = activity;
            (current != null ? current : context).startActivity(search);
            return null;
        } catch (RuntimeException e) {
            // ActivityNotFoundException: nothing handles web searches.
            Log.w(TAG, "Couldn't search the web: " + e.getClass().getSimpleName());
            return context.getString(R.string.web_search_unavailable);
        }
    }

    /**
     * Opens {@code url} in the device's browser, outside MeeGone (WALL-15, SEC-02, ADR 0031).
     * Anything but an {@code https:} link with a host is refused. Where a default browser is
     * set, the link goes to it, so an app that claims the site's links doesn't take it. Returns
     * null, or a short message if no browser could open it. The link is never logged. Called
     * from Rust.
     */
    String openInBrowser(String url) {
        try {
            Uri link = Uri.parse(url);
            if (!"https".equals(link.getScheme()) || TextUtils.isEmpty(link.getHost())) {
                return context.getString(R.string.browser_unavailable);
            }
            Intent view = new Intent(Intent.ACTION_VIEW, link)
                    .addCategory(Intent.CATEGORY_BROWSABLE)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            String browser = defaultBrowser();
            Activity current = activity;
            Context from = current != null ? current : context;
            try {
                from.startActivity(new Intent(view).setPackage(browser));
            } catch (ActivityNotFoundException e) {
                if (browser == null) {
                    throw e;
                }
                // The default browser can't take it after all: any browser may.
                from.startActivity(view);
            }
            return null;
        } catch (RuntimeException e) {
            // ActivityNotFoundException: no browser is installed.
            Log.w(TAG, "Couldn't open a link: " + e.getClass().getSimpleName());
            return context.getString(R.string.browser_unavailable);
        }
    }

    /**
     * The default browser's package, or null where none is set and the system would ask which
     * app to use. The manifest's queries let MeeGone see browsers.
     */
    private String defaultBrowser() {
        Intent browse = new Intent(Intent.ACTION_VIEW, Uri.parse("https://"))
                .addCategory(Intent.CATEGORY_BROWSABLE);
        ResolveInfo found = context.getPackageManager()
                .resolveActivity(browse, PackageManager.MATCH_DEFAULT_ONLY);
        if (found == null || found.activityInfo == null) {
            return null;
        }
        // Without a default, the system's chooser answers, from the "android" package.
        String browser = found.activityInfo.packageName;
        return "android".equals(browser) ? null : browser;
    }

    /**
     * {@code text} folded for search (SRCH-02, L10N-04, ADR 0016). Called from Rust for each
     * query, and folds labels exactly as the catalogue does, so the two always agree.
     */
    String searchKey(String text) {
        try {
            return fold(text);
        } catch (RuntimeException e) {
            return null;
        }
    }

    /**
     * Compatibility-decomposed, without the marks that sit on letters, case-folded and with runs
     * of spaces collapsed to one: "Café" and " CAFE " are both "cafe", and "ﬁle" is "file".
     */
    private static String fold(String text) {
        String bare = MARKS.matcher(Normalizer.normalize(text, Normalizer.Form.NFKD))
                .replaceAll("");
        return SPACES.matcher(UCharacter.foldCase(bare, true)).replaceAll(" ").trim();
    }

    /**
     * Every launchable app in every profile, except MeeGone itself, which is already on screen
     * (APPS-01, ADR 0010). One record per app: package, activity, label, profile serial number,
     * flags, accent, collation key, first install time and search key. An app is unavailable
     * while its profile is paused or it is suspended (APPS-09). Its accent is empty until it has
     * been derived from its icon, in the background; the catalogue then changes (APPS-17,
     * ADR 0011).
     * The collation key orders labels as the device's language does, ignoring case and accents
     * (L10N-04, ADR 0012). The search key is the label folded for search (SRCH-02, ADR 0016).
     * Returns null if the system couldn't answer, so the saved layout keeps its apps. Called
     * from Rust.
     */
    String[] launchableApps() {
        try {
            LauncherApps launcher = context.getSystemService(LauncherApps.class);
            UserManager users = context.getSystemService(UserManager.class);
            // Made for each read, so it follows the current language.
            Collator collator = Collator.getInstance();
            collator.setStrength(Collator.PRIMARY);
            List<String> fields = new ArrayList<>();
            for (UserHandle user : launcher.getProfiles()) {
                int profile = profileFlags(launcher, users, user);
                long serialNumber = users.getSerialNumberForUser(user);
                String serial = Long.toString(serialNumber);
                for (LauncherActivityInfo info : launcher.getActivityList(null, user)) {
                    ComponentName component = info.getComponentName();
                    if (context.getPackageName().equals(component.getPackageName())) {
                        continue;
                    }
                    ApplicationInfo app = info.getApplicationInfo();
                    boolean suspended = (app.flags & ApplicationInfo.FLAG_SUSPENDED) != 0;
                    int flags = 0;
                    if ((profile & PROFILE_PAUSED) == 0 && !suspended) {
                        flags |= APP_AVAILABLE;
                    }
                    if ((app.flags & ApplicationInfo.FLAG_SYSTEM) == 0) {
                        flags |= APP_CAN_UNINSTALL;
                    }
                    if ((profile & PROFILE_WORK) != 0) {
                        flags |= APP_WORK;
                    }
                    CharSequence label = info.getLabel();
                    String text = label == null ? "" : label.toString().trim();
                    fields.add(component.getPackageName());
                    fields.add(component.getClassName());
                    fields.add(text);
                    fields.add(serial);
                    fields.add(Integer.toString(flags));
                    fields.add(accents.field(serialNumber, info));
                    // An app without a label is named by its package in Rust; sort and find it
                    // so too.
                    String name = text.isEmpty() ? component.getPackageName() : text;
                    fields.add(hex(collator.getCollationKey(name).toByteArray()));
                    fields.add(Long.toString(Math.max(0, info.getFirstInstallTime())));
                    fields.add(fold(name));
                }
            }
            accents.deriveWaiting();
            return fields.toArray(new String[0]);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't list the apps", e);
            return null;
        }
    }

    /**
     * Every profile that holds apps: its serial number and flags, work, paused and locked
     * (APPS-09, APPS-18, APPS-20). Returns null if the system couldn't answer. Called from Rust.
     */
    String[] profiles() {
        try {
            LauncherApps launcher = context.getSystemService(LauncherApps.class);
            UserManager users = context.getSystemService(UserManager.class);
            List<String> fields = new ArrayList<>();
            for (UserHandle user : launcher.getProfiles()) {
                fields.add(Long.toString(users.getSerialNumberForUser(user)));
                fields.add(Integer.toString(profileFlags(launcher, users, user)));
            }
            return fields.toArray(new String[0]);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't list the profiles", e);
            return null;
        }
    }

    private static int profileFlags(LauncherApps launcher, UserManager users, UserHandle user) {
        int flags = 0;
        if (isWork(launcher, user)) {
            flags |= PROFILE_WORK;
        }
        if (users.isQuietModeEnabled(user)) {
            flags |= PROFILE_PAUSED;
        }
        // A profile with its own lock lists no apps until it is unlocked after a restart; the
        // layout keeps their places meanwhile (APPS-18).
        if (!users.isUserUnlocked(user)) {
            flags |= PROFILE_LOCKED;
        }
        return flags;
    }

    /** Lowercase hex, two digits a byte, as catalogue.rs reads it. */
    private static String hex(byte[] bytes) {
        char[] text = new char[bytes.length * 2];
        for (int i = 0; i < bytes.length; i++) {
            text[2 * i] = HEX[(bytes[i] >> 4) & 0xF];
            text[2 * i + 1] = HEX[bytes[i] & 0xF];
        }
        return new String(text);
    }

    /**
     * The packages of the default phone, messaging, browser and camera apps, in that order,
     * leaving out those the system doesn't identify, such as the phone app on a tablet (APPS-05,
     * ADR 0012). The first grid puts them first. Called from Rust.
     */
    String[] defaultApps() {
        List<String> packages = new ArrayList<>();
        addDefault(packages,
                () -> context.getSystemService(TelecomManager.class).getDefaultDialerPackage());
        addDefault(packages, () -> Telephony.Sms.getDefaultSmsPackage(context));
        addDefault(packages, () -> handler(new Intent(Intent.ACTION_VIEW, Uri.parse("https://"))
                .addCategory(Intent.CATEGORY_BROWSABLE)));
        addDefault(packages, () -> handler(new Intent(MediaStore.INTENT_ACTION_STILL_IMAGE_CAMERA)));
        return packages.toArray(new String[0]);
    }

    private static void addDefault(List<String> packages, Supplier<String> lookup) {
        try {
            String packageName = lookup.get();
            if (packageName != null && !packageName.isEmpty()) {
                packages.add(packageName);
            }
        } catch (RuntimeException e) {
            // This one isn't known; the others still come first.
        }
    }

    /**
     * The package of the app the user chose for {@code intent}, or null if they haven't chosen
     * one: the system would ask, which its chooser in the "android" package does.
     */
    private String handler(Intent intent) {
        ResolveInfo info = context.getPackageManager()
                .resolveActivity(intent, PackageManager.MATCH_DEFAULT_ONLY);
        if (info == null || info.activityInfo == null) {
            return null;
        }
        String packageName = info.activityInfo.packageName;
        return "android".equals(packageName) ? null : packageName;
    }

    /**
     * Whether {@code user} is a work profile. From Android 15 the system says which kind of
     * profile it is; before, every profile but MeeGone's own that a launcher can see is one.
     */
    private static boolean isWork(LauncherApps launcher, UserHandle user) {
        if (user.equals(Process.myUserHandle())) {
            return false;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.VANILLA_ICE_CREAM) {
            try {
                LauncherUserInfo info = launcher.getLauncherUserInfo(user);
                return info != null
                        && UserManager.USER_TYPE_PROFILE_MANAGED.equals(info.getUserType());
            } catch (RuntimeException e) {
                // Fall back to the answer of older versions.
            }
        }
        return true;
    }

    /**
     * Starts or resumes an app (APPS-10). Its window grows from the icon at the given screen
     * rect with the platform's launch-from-bounds transition (ANIM-08); an empty rect gets the
     * system's default. An unavailable app is launched too: the system shows why it can't open,
     * and offers to unpause a work profile (APPS-09). Returns null, or a short message if the
     * system couldn't start it. Called from Rust.
     */
    String launch(String packageName, String activityName, long profile, int left, int top,
            int right, int bottom) {
        try {
            LauncherApps launcher = context.getSystemService(LauncherApps.class);
            UserHandle user = context.getSystemService(UserManager.class)
                    .getUserForSerialNumber(profile);
            if (user == null) {
                return context.getString(R.string.app_unavailable);
            }
            Rect icon = right > left && bottom > top ? new Rect(left, top, right, bottom) : null;
            Bundle options = growFrom(icon);
            Log.i(TAG, "Launching from the icon: " + (options != null));
            launcher.startMainActivity(
                    new ComponentName(packageName, activityName), user, icon, options);
            return null;
        } catch (RuntimeException e) {
            // ActivityNotFoundException, SecurityException or IllegalStateException: the app
            // or its profile is gone, or the system refused.
            Log.w(TAG, "Couldn't launch an app: " + e.getClass().getSimpleName());
            return context.getString(R.string.app_unavailable);
        }
    }

    /**
     * The enabled shortcuts of one app, for its long-press menu (APPS-11, ADR 0013). One record
     * per shortcut: id, short label, flags and rank. The system offers shortcuts only to the
     * default home app: returns null otherwise, or if the system couldn't answer, such as while
     * a work profile is locked. Rust orders them. Called from Rust.
     */
    String[] shortcuts(String packageName, String activityName, long profile) {
        try {
            LauncherApps launcher = context.getSystemService(LauncherApps.class);
            UserHandle user = user(profile);
            if (user == null || !launcher.hasShortcutHostPermission()) {
                return null;
            }
            LauncherApps.ShortcutQuery query = new LauncherApps.ShortcutQuery()
                    .setPackage(packageName)
                    .setActivity(new ComponentName(packageName, activityName))
                    .setQueryFlags(LauncherApps.ShortcutQuery.FLAG_MATCH_MANIFEST
                            | LauncherApps.ShortcutQuery.FLAG_MATCH_DYNAMIC);
            List<ShortcutInfo> found = launcher.getShortcuts(query, user);
            if (found == null) {
                return null;
            }
            List<String> fields = new ArrayList<>();
            for (ShortcutInfo shortcut : found) {
                if (!shortcut.isEnabled()) {
                    continue;
                }
                CharSequence label = shortcut.getShortLabel();
                fields.add(shortcut.getId());
                fields.add(label == null ? "" : label.toString());
                fields.add(Integer.toString(
                        shortcut.isDeclaredInManifest() ? SHORTCUT_MANIFEST : 0));
                fields.add(Integer.toString(Math.max(0, shortcut.getRank())));
            }
            return fields.toArray(new String[0]);
        } catch (RuntimeException e) {
            // SecurityException without the home role, IllegalStateException while the profile
            // is locked.
            Log.w(TAG, "Couldn't list shortcuts: " + e.getClass().getSimpleName());
            return null;
        }
    }

    /**
     * Starts one of an app's shortcuts (APPS-11). Returns null, or a short message if the
     * system couldn't start it, such as when the app disabled it meanwhile. Called from Rust.
     */
    String launchShortcut(String packageName, String id, long profile) {
        try {
            UserHandle user = user(profile);
            if (user == null) {
                return context.getString(R.string.shortcut_unavailable);
            }
            context.getSystemService(LauncherApps.class)
                    .startShortcut(packageName, id, null, null, user);
            return null;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't start a shortcut: " + e.getClass().getSimpleName());
            return context.getString(R.string.shortcut_unavailable);
        }
    }

    /**
     * Opens the system's confirmation to uninstall an app, in its own profile (APPS-13).
     * MeeGone never uninstalls anything itself: the user confirms, and the system reports the
     * removal like any other. It needs REQUEST_DELETE_PACKAGES, granted at install (SEC-03).
     * Returns null, or a short message if the system refused. Called from Rust.
     */
    String requestUninstall(String packageName, String activityName, long profile) {
        try {
            UserHandle user = user(profile);
            if (user == null) {
                return context.getString(R.string.uninstall_unavailable);
            }
            Intent intent = new Intent(Intent.ACTION_DELETE,
                    Uri.fromParts("package", packageName, activityName))
                    .putExtra(Intent.EXTRA_USER, user)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK
                            | Intent.FLAG_ACTIVITY_EXCLUDE_FROM_RECENTS);
            Activity current = activity;
            (current != null ? current : context).startActivity(intent);
            return null;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't ask to uninstall: " + e.getClass().getSimpleName());
            return context.getString(R.string.uninstall_unavailable);
        }
    }

    /**
     * Opens the system's App info screen for an app, in its own profile (APPS-11). Returns null,
     * or a short message if the system couldn't open it. Called from Rust.
     */
    String openAppInfo(String packageName, String activityName, long profile) {
        try {
            UserHandle user = user(profile);
            if (user == null) {
                return context.getString(R.string.app_info_unavailable);
            }
            context.getSystemService(LauncherApps.class).startAppDetailsActivity(
                    new ComponentName(packageName, activityName), user, null, null);
            return null;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't open App info: " + e.getClass().getSimpleName());
            return context.getString(R.string.app_info_unavailable);
        }
    }

    /** The profile with this serial number, or null if it is gone. */
    private UserHandle user(long profile) {
        return context.getSystemService(UserManager.class).getUserForSerialNumber(profile);
    }

    /** The window grows from {@code icon}, a rect on the screen (ANIM-08), or null for none. */
    private Bundle growFrom(Rect icon) {
        ActivityOptions options = growOptions(icon);
        return options != null ? options.toBundle() : null;
    }

    /** Options for a window that grows from {@code icon} (ANIM-08), or null for none. */
    private ActivityOptions growOptions(Rect icon) {
        Activity current = activity;
        View decor = current == null ? null : current.getWindow().peekDecorView();
        if (icon == null || decor == null) {
            return null;
        }
        int[] origin = new int[2];
        decor.getLocationOnScreen(origin);
        return ActivityOptions.makeScaleUpAnimation(decor, icon.left - origin[0],
                icon.top - origin[1], icon.width(), icon.height());
    }

    /**
     * Starts telling the Rust app when apps are installed, updated, removed, suspended or
     * unsuspended, when a work profile is added, removed, paused, resumed or unlocked, and when
     * the language changes (APPS-09, APPS-18, L10N-04, ADR 0010, ADR 0012). Once per process;
     * later calls do nothing. A burst of changes, such as an update's removal and addition, is
     * reported once, on the main thread.
     */
    void watchApps() {
        if (watchingApps) {
            return;
        }
        watchingApps = true;
        try {
            context.getSystemService(LauncherApps.class).registerCallback(new AppsCallback(), main);
            receive(CATALOGUE_CHANGES, this::appsChanged);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't watch the apps", e);
        }
    }

    private void appsChanged() {
        main.removeCallbacks(reportAppsChanged);
        main.post(reportAppsChanged);
    }

    /**
     * Starts telling the Rust app when the day, the clock, the 12- or 24-hour setting, the time
     * zone or the next alarm changes, so the Events header follows them (EVT-01, EVT-02,
     * L10N-03, ADR 0019). Once per process; later calls do nothing. A burst of changes, such as
     * a new time zone that also moves the date, is reported once, on the main thread.
     */
    void watchClock() {
        if (watchingClock) {
            return;
        }
        watchingClock = true;
        try {
            receive(CLOCK_CHANGES, () -> {
                main.removeCallbacks(reportClockChanged);
                main.post(reportClockChanged);
            });
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't watch the clock", e);
        }
    }

    /** Runs {@code report} on the main thread for each of the system's {@code actions}. */
    private void receive(String[] actions, Runnable report) {
        BroadcastReceiver receiver = new BroadcastReceiver() {
            @Override
            public void onReceive(Context context, Intent intent) {
                report.run();
            }
        };
        IntentFilter filter = new IntentFilter();
        for (String action : actions) {
            filter.addAction(action);
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED);
        } else {
            context.registerReceiver(receiver, filter);
        }
    }

    /**
     * Every change to the apps of any profile. A package that is added, removed, changed or
     * becomes available again may have a new icon, so its accents are derived again (ADR 0011).
     */
    private final class AppsCallback extends LauncherApps.Callback {
        @Override
        public void onPackageAdded(String packageName, UserHandle user) {
            accents.forget(packageName, user);
            appsChanged();
        }

        @Override
        public void onPackageRemoved(String packageName, UserHandle user) {
            accents.forget(packageName, user);
            appsChanged();
        }

        @Override
        public void onPackageChanged(String packageName, UserHandle user) {
            accents.forget(packageName, user);
            appsChanged();
        }

        @Override
        public void onPackagesAvailable(String[] packageNames, UserHandle user,
                boolean replacing) {
            for (String packageName : packageNames) {
                accents.forget(packageName, user);
            }
            appsChanged();
        }

        @Override
        public void onPackagesUnavailable(String[] packageNames, UserHandle user,
                boolean replacing) {
            appsChanged();
        }

        @Override
        public void onPackagesSuspended(String[] packageNames, UserHandle user) {
            appsChanged();
        }

        @Override
        public void onPackagesUnsuspended(String[] packageNames, UserHandle user) {
            appsChanged();
        }
    }

    /**
     * {@code atMs} in the style numbered by {@code style}, in the device's language, time zone
     * and 12- or 24-hour setting, or null (EVT-01, EVT-02, WALL-16, L10N-03, ADR 0019,
     * ADR 0031). The language's
     * own patterns order the fields, such as "17 November" or "November 17", and a weekday or a
     * month shown on its own is capitalised as the language does at the start of a line. Called
     * from Rust.
     */
    String formatTime(long atMs, int style) {
        try {
            boolean hour24 = android.text.format.DateFormat.is24HourFormat(context);
            String skeleton;
            switch (style) {
                case STYLE_WEEKDAY:
                    skeleton = "EEEE";
                    break;
                case STYLE_DAY_MONTH:
                    skeleton = "MMMMd";
                    break;
                case STYLE_TIME:
                    skeleton = hour24 ? "Hm" : "hm";
                    break;
                case STYLE_WEEKDAY_TIME:
                    skeleton = hour24 ? "EHm" : "Ehm";
                    break;
                case STYLE_DAY_MONTH_TIME:
                    skeleton = hour24 ? "MMMMdHm" : "MMMMdhm";
                    break;
                default:
                    return null;
            }
            DateFormat format = DateFormat.getInstanceForSkeleton(skeleton, Locale.getDefault());
            // The time zone as of now, which the system sets before TIMEZONE_CHANGED arrives.
            format.setTimeZone(
                    android.icu.util.TimeZone.getTimeZone(TimeZone.getDefault().getID()));
            format.setContext(DisplayContext.CAPITALIZATION_FOR_STANDALONE);
            return format.format(new Date(atMs));
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't write a time: " + e.getClass().getSimpleName());
            return null;
        }
    }

    /**
     * Starts telling the Rust app when notifications are posted or removed, and when the
     * listener is connected or disconnected, as when the user grants or revokes notification
     * access (EVT-10, ADR 0020). Called once the home has started; until then, a listener the
     * system binds before any home exists tells nobody, and the home reads the notifications as
     * it starts.
     */
    void watchNotifications() {
        watchingNotifications = true;
    }

    /**
     * A notification was posted or removed, or the listener was connected or disconnected.
     * Reported once per burst, on the main thread, once the home is watching. Called by {@link
     * NotificationListener} on the main thread.
     */
    static void notificationsChanged() {
        Bridge bridge;
        synchronized (Bridge.class) {
            bridge = instance;
        }
        if (bridge == null || !bridge.watchingNotifications) {
            return;
        }
        bridge.main.removeCallbacks(bridge.reportNotificationsChanged);
        bridge.main.post(bridge.reportNotificationsChanged);
    }

    /**
     * Whether the user has granted MeeGone's listener notification access (EVT-03). If they
     * have but the system hasn't bound the listener, as can happen after MeeGone is updated, it
     * asks the system to bind it; the connection then reports itself. Called from Rust.
     */
    boolean notificationAccess() {
        ComponentName listener = new ComponentName(context, NotificationListener.class);
        boolean granted;
        try {
            granted = context.getSystemService(NotificationManager.class)
                    .isNotificationListenerAccessGranted(listener);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't check notification access: " + e.getClass().getSimpleName());
            return false;
        }
        if (granted && NotificationListener.connected() == null) {
            try {
                NotificationListenerService.requestRebind(listener);
            } catch (RuntimeException e) {
                Log.w(TAG, "Couldn't bind the listener: " + e.getClass().getSimpleName());
            }
        }
        return granted;
    }

    /**
     * Every other app's active notifications, one record each (EVT-04, EVT-05, ADR 0020): key,
     * package, profile serial number, the app's name, title, text, post time and flags. None
     * while the listener isn't connected yet; its connection reports itself. Null if the system
     * couldn't list them. What they say crosses into Rust's memory only, and is never logged
     * (EVT-13). Called from Rust.
     */
    String[] activeNotifications() {
        NotificationListener listener = NotificationListener.connected();
        if (listener == null) {
            return new String[0];
        }
        try {
            StatusBarNotification[] active = listener.getActiveNotifications();
            if (active == null) {
                return new String[0];
            }
            UserManager users = context.getSystemService(UserManager.class);
            Map<String, String> labels = new HashMap<>();
            List<String> fields = new ArrayList<>();
            for (StatusBarNotification posted : active) {
                String packageName = posted.getPackageName();
                long serial = users.getSerialNumberForUser(posted.getUser());
                // MeeGone's own are left out (EVT-05).
                if (context.getPackageName().equals(packageName) || serial < 0) {
                    continue;
                }
                Notification notification = posted.getNotification();
                Bundle extras = notification.extras != null ? notification.extras : new Bundle();
                String app = serial + "/" + packageName;
                String label = labels.get(app);
                if (label == null) {
                    label = appLabel(packageName, posted.getUser(), extras);
                    labels.put(app, label);
                }
                fields.add(posted.getKey());
                fields.add(packageName);
                fields.add(Long.toString(serial));
                fields.add(label);
                fields.add(text(extras, Notification.EXTRA_TITLE, Notification.EXTRA_TITLE_BIG));
                fields.add(text(extras, Notification.EXTRA_TEXT, Notification.EXTRA_BIG_TEXT));
                fields.add(Long.toString(posted.getPostTime()));
                fields.add(Integer.toString(notificationFlags(posted, notification, extras)));
            }
            return fields.toArray(new String[0]);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the notifications: " + e.getClass().getSimpleName());
            return null;
        }
    }

    /**
     * The name of the app that posted a notification: its label, read from the system, or from
     * the app info the notification carries for an app MeeGone can't see, such as System UI.
     * Empty if neither names it.
     */
    @SuppressWarnings("deprecation") // getParcelable(String, Class) needs Android 13.
    private String appLabel(String packageName, UserHandle user, Bundle extras) {
        PackageManager packages = context.getPackageManager();
        try {
            return packages.getApplicationLabel(context.getSystemService(LauncherApps.class)
                    .getApplicationInfo(packageName, 0, user)).toString().trim();
        } catch (PackageManager.NameNotFoundException | RuntimeException e) {
            // Hidden by package visibility: try the notification's own app info.
        }
        try {
            Object carried = extras.getParcelable(EXTRA_APP_INFO);
            if (carried instanceof ApplicationInfo
                    && packageName.equals(((ApplicationInfo) carried).packageName)) {
                return packages.getApplicationLabel((ApplicationInfo) carried).toString().trim();
            }
        } catch (RuntimeException e) {
            // Unreadable: the Rust app names it by its package.
        }
        return "";
    }

    /** The first of {@code keys} that holds text, or an empty string. */
    private static String text(Bundle extras, String... keys) {
        for (String key : keys) {
            CharSequence text = extras.getCharSequence(key);
            if (text != null && text.toString().trim().length() > 0) {
                return text.toString();
            }
        }
        return "";
    }

    /**
     * Whether the user can clear a notification, whether it is ongoing, and whether it
     * summarises a bundle. Ongoing ones, which Events leaves out, are ongoing events such as a
     * call, a foreground service's, media playback (even paused) and navigation (EVT-05).
     */
    private static int notificationFlags(
            StatusBarNotification posted, Notification notification, Bundle extras) {
        int flags = 0;
        if (posted.isClearable()) {
            flags |= NOTIFICATION_CLEARABLE;
        }
        boolean ongoing = (notification.flags
                        & (Notification.FLAG_ONGOING_EVENT | Notification.FLAG_FOREGROUND_SERVICE))
                != 0
                || Notification.CATEGORY_TRANSPORT.equals(notification.category)
                || Notification.CATEGORY_NAVIGATION.equals(notification.category)
                || extras.containsKey(Notification.EXTRA_MEDIA_SESSION);
        if (ongoing) {
            flags |= NOTIFICATION_ONGOING;
        }
        if ((notification.flags & Notification.FLAG_GROUP_SUMMARY) != 0) {
            flags |= NOTIFICATION_SUMMARY;
        }
        if ((notification.flags & Notification.FLAG_AUTO_CANCEL) != 0) {
            flags |= NOTIFICATION_AUTO_CANCEL;
        }
        return flags;
    }

    /**
     * Opens a notification's content (EVT-06): sends its content intent as if MeeGone started
     * it, with the window growing from the tapped row, a rect on the screen (ANIM-08). Without
     * one, or if its app cancelled it, opens the app instead. It never removes the notification:
     * the Rust app dismisses one meant to disappear when opened. One that is already gone opens
     * nothing, as the list follows. Returns null, or a short message if nothing could open.
     * Called from Rust.
     */
    String openNotification(String key, int left, int top, int right, int bottom) {
        NotificationListener listener = NotificationListener.connected();
        if (listener == null) {
            return context.getString(R.string.notification_unavailable);
        }
        Rect row = right > left && bottom > top ? new Rect(left, top, right, bottom) : null;
        StatusBarNotification posted;
        try {
            StatusBarNotification[] found = listener.getActiveNotifications(new String[] {key});
            posted = found != null && found.length > 0 ? found[0] : null;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't find a notification: " + e.getClass().getSimpleName());
            return context.getString(R.string.notification_unavailable);
        }
        if (posted == null) {
            return null;
        }
        PendingIntent content = posted.getNotification().contentIntent;
        if (content != null) {
            Activity current = activity;
            try {
                content.send(current != null ? current : context, 0, null, null, null, null,
                        startFromHome(growOptions(row)));
                return null;
            } catch (PendingIntent.CanceledException | RuntimeException e) {
                Log.w(TAG, "Couldn't open a notification: " + e.getClass().getSimpleName());
            }
        }
        if (openApp(posted.getPackageName(), posted.getUser(), row)) {
            return null;
        }
        return context.getString(R.string.notification_unavailable);
    }

    /**
     * Starts an app's first launcher activity in {@code user}, growing from {@code from}.
     * Returns false if it has none, or the system refused.
     */
    private boolean openApp(String packageName, UserHandle user, Rect from) {
        try {
            LauncherApps launcher = context.getSystemService(LauncherApps.class);
            List<LauncherActivityInfo> activities = launcher.getActivityList(packageName, user);
            if (activities == null || activities.isEmpty()) {
                return false;
            }
            launcher.startMainActivity(
                    activities.get(0).getComponentName(), user, from, growFrom(from));
            return true;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't open an app: " + e.getClass().getSimpleName());
            return false;
        }
    }

    /**
     * Removes notifications, in one request (EVT-07, EVT-08). The system keeps any that can't
     * be cleared, and reports the others as removed, which updates the list. Returns null, or a
     * short message if it refused. Called from Rust.
     */
    String dismissNotifications(String[] keys) {
        NotificationListener listener = NotificationListener.connected();
        if (listener == null) {
            return context.getString(R.string.notifications_dismiss_unavailable);
        }
        try {
            listener.cancelNotifications(keys);
            return null;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't dismiss notifications: " + e.getClass().getSimpleName());
            return context.getString(R.string.notifications_dismiss_unavailable);
        }
    }

    /**
     * Opens the system's notification settings for an app, in its own profile (EVT-08,
     * APPS-20). Settings learns another profile's copy by its uid, in {@link #EXTRA_APP_UID}.
     * If that screen doesn't open, the app's App info does. Returns null, or a short message if
     * neither opens. Called from Rust.
     */
    String openNotificationSettings(String packageName, long profile) {
        Activity current = activity;
        Context from = current != null ? current : context;
        UserHandle user = user(profile);
        if (user == null) {
            return context.getString(R.string.notification_settings_unavailable);
        }
        LauncherApps launcher = context.getSystemService(LauncherApps.class);
        try {
            Intent settings = new Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                    .putExtra(Settings.EXTRA_APP_PACKAGE, packageName)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            if (!user.equals(Process.myUserHandle())) {
                settings.putExtra(EXTRA_APP_UID,
                        launcher.getApplicationInfo(packageName, 0, user).uid);
            }
            from.startActivity(settings);
            return null;
        } catch (PackageManager.NameNotFoundException | RuntimeException e) {
            Log.w(TAG, "Couldn't open notification settings: " + e.getClass().getSimpleName());
        }
        try {
            List<LauncherActivityInfo> activities = launcher.getActivityList(packageName, user);
            if (activities != null && !activities.isEmpty()) {
                launcher.startAppDetailsActivity(
                        activities.get(0).getComponentName(), user, null, null);
                return null;
            }
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't open App info: " + e.getClass().getSimpleName());
        }
        return context.getString(R.string.notification_settings_unavailable);
    }

    /**
     * Opens MeeGone's page of the system's notification-access screen, or the screen itself,
     * where the user grants access (EVT-03). Returns null, or a short message if neither opens.
     * Called from Rust.
     */
    String openNotificationAccess() {
        Activity current = activity;
        Context from = current != null ? current : context;
        ComponentName listener = new ComponentName(context, NotificationListener.class);
        List<Intent> screens = new ArrayList<>();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            screens.add(new Intent(Settings.ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS)
                    .putExtra(Settings.EXTRA_NOTIFICATION_LISTENER_COMPONENT_NAME,
                            listener.flattenToString()));
        }
        screens.add(new Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS));
        for (Intent screen : screens) {
            try {
                from.startActivity(screen.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                return null;
            } catch (ActivityNotFoundException | SecurityException e) {
                // Try the next screen.
            }
        }
        Log.w(TAG, "Couldn't open the notification-access screen");
        return context.getString(R.string.notification_access_unavailable);
    }

    /**
     * Starts telling the Rust app when the user grants or revokes usage access, even while
     * Recent shows (REC-02, ADR 0023). Once per process; later calls do nothing. The system
     * reports it as a change of MeeGone's own app op, which is never polled (PWR-02).
     */
    void watchUsageAccess() {
        if (watchingUsageAccess) {
            return;
        }
        watchingUsageAccess = true;
        try {
            context.getSystemService(AppOpsManager.class).startWatchingMode(
                    AppOpsManager.OPSTR_GET_USAGE_STATS, context.getPackageName(),
                    (op, packageName) -> {
                        // On a binder thread.
                        main.removeCallbacks(reportUsageAccessChanged);
                        main.post(reportUsageAccessChanged);
                    });
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't watch usage access: " + e.getClass().getSimpleName());
        }
    }

    /**
     * The system animation speed (ANIM-05, A11Y-07, ADR 0034), in thousandths: 1000 is normal,
     * and 0 means animations are removed, as Developer options' "Animator duration scale" and
     * Accessibility's "Remove animations" set it. 1000 if the setting can't be read. Called from
     * Rust.
     */
    int animationScale() {
        try {
            float scale = Settings.Global.getFloat(context.getContentResolver(),
                    Settings.Global.ANIMATOR_DURATION_SCALE, 1f);
            if (Float.isNaN(scale) || scale < 0f) {
                return 1000;
            }
            return Math.round(Math.min(scale, 10f) * 1000f);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the animation speed: " + e.getClass().getSimpleName());
            return 1000;
        }
    }

    /**
     * Tells the Rust app whenever the system animation speed changes, even while the home shows,
     * such as from the accessibility shortcut (ANIM-05, ADR 0034). Once per process.
     */
    void watchAnimationScale() {
        if (watchingAnimationScale) {
            return;
        }
        watchingAnimationScale = true;
        try {
            context.getContentResolver().registerContentObserver(
                    Settings.Global.getUriFor(Settings.Global.ANIMATOR_DURATION_SCALE), false,
                    new ContentObserver(main) {
                        @Override
                        public void onChange(boolean selfChange) {
                            // On the main thread, through the handler.
                            main.removeCallbacks(reportAnimationScaleChanged);
                            main.post(reportAnimationScaleChanged);
                        }
                    });
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't watch the animation speed: " + e.getClass().getSimpleName());
        }
    }

    /** Whether the user has granted MeeGone usage access (REC-02). Called from Rust. */
    boolean usageAccess() {
        try {
            int mode = context.getSystemService(AppOpsManager.class).unsafeCheckOpNoThrow(
                    AppOpsManager.OPSTR_GET_USAGE_STATS, Process.myUid(),
                    context.getPackageName());
            if (mode == AppOpsManager.MODE_DEFAULT) {
                return context.checkSelfPermission(Manifest.permission.PACKAGE_USAGE_STATS)
                        == PackageManager.PERMISSION_GRANTED;
            }
            return mode == AppOpsManager.MODE_ALLOWED;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't check usage access: " + e.getClass().getSimpleName());
            return false;
        }
    }

    /**
     * Every other app used at or after {@code sinceMs}, one record each (REC-03, ADR 0023):
     * package, the class of the activity last resumed, profile serial number, and when it last
     * came to the front or left it, in ms since the epoch. An app used for an hour reads as used
     * when the user left it. MeeGone itself is left out. The system reports only this profile's
     * apps. Null if the system couldn't list them. Called from Rust.
     */
    String[] recentUses(long sinceMs) {
        try {
            UsageEvents events = context.getSystemService(UsageStatsManager.class)
                    .queryEvents(sinceMs, System.currentTimeMillis());
            if (events == null) {
                return null;
            }
            String own = context.getPackageName();
            Map<String, Long> lastUsed = new HashMap<>();
            Map<String, String> activities = new HashMap<>();
            UsageEvents.Event event = new UsageEvents.Event();
            while (events.getNextEvent(event)) {
                int type = event.getEventType();
                boolean resumed = type == UsageEvents.Event.ACTIVITY_RESUMED;
                if (!resumed && type != UsageEvents.Event.ACTIVITY_PAUSED) {
                    continue;
                }
                String pkg = event.getPackageName();
                if (pkg == null || pkg.equals(own)) {
                    continue;
                }
                long at = event.getTimeStamp();
                Long before = lastUsed.get(pkg);
                if (before == null || at >= before) {
                    lastUsed.put(pkg, at);
                }
                if (resumed && event.getClassName() != null) {
                    // Events come oldest first, so the last one resumed stays.
                    activities.put(pkg, event.getClassName());
                }
            }
            String serial = Long.toString(context.getSystemService(UserManager.class)
                    .getSerialNumberForUser(Process.myUserHandle()));
            List<String> fields = new ArrayList<>(lastUsed.size() * USE_FIELDS);
            for (Map.Entry<String, Long> used : lastUsed.entrySet()) {
                String activity = activities.get(used.getKey());
                fields.add(used.getKey());
                fields.add(activity != null ? activity : "");
                fields.add(serial);
                fields.add(Long.toString(used.getValue()));
            }
            return fields.toArray(new String[0]);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the recent apps: " + e.getClass().getSimpleName());
            return null;
        }
    }

    /**
     * Opens MeeGone's page of the system's usage-access screen, or the screen itself, where the
     * user grants access (REC-02). Returns null, or a short message if neither opens. Called
     * from Rust.
     */
    String openUsageAccess() {
        Activity current = activity;
        Context from = current != null ? current : context;
        Intent[] screens = {
            new Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS,
                    Uri.fromParts("package", context.getPackageName(), null)),
            new Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS),
        };
        for (Intent screen : screens) {
            try {
                from.startActivity(screen.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                return null;
            } catch (ActivityNotFoundException | SecurityException e) {
                // Try the next screen.
            }
        }
        Log.w(TAG, "Couldn't open the usage-access screen");
        return context.getString(R.string.usage_access_unavailable);
    }

    /**
     * When the next alarm rings, in ms since the epoch, or -1 if none is set (EVT-02). It needs
     * no permission. Called from Rust.
     */
    long nextAlarmMs() {
        try {
            AlarmManager.AlarmClockInfo next =
                    context.getSystemService(AlarmManager.class).getNextAlarmClock();
            return next != null ? next.getTriggerTime() : -1;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the next alarm: " + e.getClass().getSimpleName());
            return -1;
        }
    }

    /**
     * Opens what the app that set the next alarm shows for it, or the clock app's alarms if it
     * shows nothing (EVT-02, ADR 0019). Returns null, or a short message if nothing could open.
     * Called from Rust.
     */
    String openNextAlarm() {
        Activity current = activity;
        Context from = current != null ? current : context;
        try {
            AlarmManager.AlarmClockInfo next =
                    context.getSystemService(AlarmManager.class).getNextAlarmClock();
            PendingIntent show = next != null ? next.getShowIntent() : null;
            if (show != null) {
                show.send(from, 0, null, null, null, null, startFromHome(null));
                return null;
            }
        } catch (PendingIntent.CanceledException | RuntimeException e) {
            Log.w(TAG, "Couldn't open the alarm's app: " + e.getClass().getSimpleName());
        }
        try {
            from.startActivity(new Intent(AlarmClock.ACTION_SHOW_ALARMS)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            return null;
        } catch (RuntimeException e) {
            // ActivityNotFoundException: no clock app shows alarms.
            Log.w(TAG, "Couldn't show the alarms: " + e.getClass().getSimpleName());
            return context.getString(R.string.alarm_unavailable);
        }
    }

    /**
     * Lets an intent another app made, such as an alarm's or a notification's, start its screen
     * as if MeeGone did, while MeeGone shows, with {@code options} if any. From Android 14 the
     * sender must say so, or the system may block it because the app that made it is in the
     * background.
     */
    @SuppressWarnings("deprecation") // ALLOWED is the only mode on Android 14 and 15.
    private static Bundle startFromHome(ActivityOptions options) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            return options != null ? options.toBundle() : null;
        }
        int mode = Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA
                ? ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOW_IF_VISIBLE
                : ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOWED;
        return (options != null ? options : ActivityOptions.makeBasic())
                .setPendingIntentBackgroundActivityStartMode(mode)
                .toBundle();
    }

    /** The UTC offset of the device's time zone at {@code atMs}, in ms. Called from Rust. */
    long utcOffsetMs(long atMs) {
        try {
            return TimeZone.getDefault().getOffset(atMs);
        } catch (RuntimeException e) {
            return 0;
        }
    }

    /**
     * When this copy of MeeGone was first installed, in ms since the epoch, or 0 if the system
     * can't say. A reinstall or a new device gives a new time, so a layout saved by another copy
     * is known to come from a backup (APPS-21, ADR 0018). Called from Rust.
     */
    long installedMs() {
        try {
            return context.getPackageManager()
                    .getPackageInfo(context.getPackageName(), 0)
                    .firstInstallTime;
        } catch (PackageManager.NameNotFoundException | RuntimeException e) {
            Log.w(TAG, "Couldn't read MeeGone's install time: " + e.getClass().getSimpleName());
            return 0;
        }
    }

    /**
     * The device's first language as a BCP 47 tag, such as "de-AT", from which the image feed's
     * Automatic region is derived (WALL-05, ADR 0027). Null if the system can't say. Called from
     * Rust.
     */
    String locale() {
        try {
            return Locale.getDefault().toLanguageTag();
        } catch (RuntimeException e) {
            return null;
        }
    }

    /**
     * How many more bytes MeeGone can store before the device runs low on storage, or -1 if the
     * system can't say (SRS 7.6, ADR 0028). {@code StorageManager.getAllocatableBytes} counts
     * cached files the system would clear, and leaves out the space it keeps for itself when
     * storage runs low. It may take a moment, so it is asked off the main thread. Called from
     * Rust.
     */
    long freeStorageBytes() {
        File files = context.getFilesDir();
        try {
            StorageManager storage = context.getSystemService(StorageManager.class);
            return storage.getAllocatableBytes(storage.getUuidForPath(files));
        } catch (IOException | RuntimeException e) {
            Log.w(TAG, "Couldn't read the allocatable storage: " + e.getClass().getSimpleName());
        }
        try {
            return files.getUsableSpace();
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the free storage: " + e.getClass().getSimpleName());
            return -1;
        }
    }

    /**
     * Whether battery saver is on or the battery is low, when automatic downloads wait (WALL-04,
     * PWR-04, ADR 0029). The battery's "low" flag comes from the system's last battery report,
     * which needs no receiver. False if the system can't say. Called from Rust.
     */
    boolean savesBattery() {
        try {
            PowerManager power = context.getSystemService(PowerManager.class);
            if (power != null && power.isPowerSaveMode()) {
                return true;
            }
            Intent battery =
                    context.registerReceiver(null, new IntentFilter(Intent.ACTION_BATTERY_CHANGED));
            return battery != null
                    && battery.getBooleanExtra(BatteryManager.EXTRA_BATTERY_LOW, false);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the battery: " + e.getClass().getSimpleName());
            return false;
        }
    }

    /**
     * Whether Data Saver keeps MeeGone's background work off metered networks (PWR-04, ADR
     * 0029). True if the system can't say, so automatic downloads wait for Wi-Fi. Called from
     * Rust.
     */
    boolean savesData() {
        try {
            return context.getSystemService(ConnectivityManager.class)
                    .getRestrictBackgroundStatus()
                    == ConnectivityManager.RESTRICT_BACKGROUND_STATUS_ENABLED;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read Data Saver: " + e.getClass().getSimpleName());
            return true;
        }
    }

    /**
     * Schedules the daily image's job to run no earlier than {@code notBeforeMs}, on any network
     * if {@code anyNetwork} is 1 or only an unmetered one (WALL-04, ADR 0029). Returns null, or a
     * short message when the system can't schedule it. Called from Rust.
     */
    String scheduleImageJob(long notBeforeMs, int anyNetwork) {
        return ImageJobService.schedule(context, notBeforeMs, anyNetwork != 0);
    }

    /** Cancels the daily image's job (WALL-12). As {@link #scheduleImageJob}. Called from Rust. */
    String cancelImageJob() {
        return ImageJobService.cancel(context);
    }

    /**
     * Runs what the user asked of the daily image on its own thread at once, over any
     * connection: "Update now" (WALL-11, ADR 0030), browsing the past week and pinning one of its
     * images (WALL-17, ADR 0032). Returns null: they are on their way. Called from Rust.
     */
    String runImageTasks() {
        ImageJobService.runTasks(context);
        return null;
    }

    /**
     * The daily image's check reached the feed, on its own thread: the home reads the image and
     * how the checks are going again, on the main thread (ADR 0029). Does nothing before there
     * is a bridge.
     */
    static void imageChecked() {
        Bridge bridge;
        synchronized (Bridge.class) {
            bridge = instance;
        }
        if (bridge != null) {
            bridge.main.post(() -> HomeActivity.imageChecked(bridge.activity));
        }
    }

    /**
     * Whether the device is online, and if so whether its network may cost the user (WALL-04):
     * {@link #NETWORK_OFFLINE}, {@link #NETWORK_METERED} or {@link #NETWORK_UNMETERED}. Offline
     * if the system can't say. Called from Rust.
     */
    int networkState() {
        try {
            ConnectivityManager connectivity =
                    context.getSystemService(ConnectivityManager.class);
            Network network = connectivity.getActiveNetwork();
            NetworkCapabilities capabilities =
                    network != null ? connectivity.getNetworkCapabilities(network) : null;
            if (capabilities == null
                    || !capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) {
                return NETWORK_OFFLINE;
            }
            return capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
                    ? NETWORK_UNMETERED
                    : NETWORK_METERED;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the network state: " + e.getClass().getSimpleName());
            return NETWORK_OFFLINE;
        }
    }

    /**
     * Makes one HTTPS GET request and returns its body, or null (SEC-01, SEC-02, PRIV-02, ADR
     * 0027). {@code outcome}, two elements long, gets what happened and a detail: {@link
     * #GET_OK} with the body, or why there is none, such as {@link #GET_REDIRECT} with the
     * absolute URL to go to instead. Rust decides which redirects to follow: only to https.
     *
     * <p>The platform validates the certificate and the host name, and the network security
     * config refuses cleartext. The request carries the URL and nothing else that could tell
     * the device or the user apart: a fixed user agent, no cookies, as MeeGone never installs a
     * cookie handler, and no cache. A body longer than {@code maxBytes} is never read in full.
     * It blocks, so it answers {@link #GET_FAILED} on the main thread without a request. Logs
     * hold only the kind of failure. Called from Rust, off the main thread.
     */
    byte[] httpsGet(String url, int maxBytes, String[] outcome) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            Log.w(TAG, "Refused a network request on the main thread");
            return answer(outcome, GET_FAILED, null);
        }
        HttpsURLConnection connection = null;
        try {
            URL target = new URL(url);
            if (!"https".equals(target.getProtocol())) {
                return answer(outcome, GET_NOT_HTTPS, null);
            }
            connection = (HttpsURLConnection) target.openConnection();
            connection.setInstanceFollowRedirects(false);
            connection.setUseCaches(false);
            connection.setConnectTimeout(CONNECT_TIMEOUT_MS);
            connection.setReadTimeout(READ_TIMEOUT_MS);
            connection.setRequestProperty("User-Agent", USER_AGENT);
            int status = connection.getResponseCode();
            if (status >= 300 && status < 400) {
                String location = connection.getHeaderField("Location");
                if (location == null) {
                    Log.w(TAG, "A redirect had no location: HTTP " + status);
                    return answer(outcome, GET_FAILED, null);
                }
                return answer(outcome, GET_REDIRECT, new URL(target, location).toString());
            }
            if (status < 200 || status >= 300) {
                Log.w(TAG, "A request failed: HTTP " + status);
                return answer(outcome, GET_FAILED, null);
            }
            if (connection.getContentLengthLong() > maxBytes) {
                return answer(outcome, GET_TOO_LARGE, null);
            }
            byte[] body = readAtMost(connection.getInputStream(), maxBytes);
            if (body == null) {
                return answer(outcome, GET_TOO_LARGE, null);
            }
            answer(outcome, GET_OK, null);
            return body;
        } catch (UnknownServiceException e) {
            // The network security config refused cleartext (SEC-01).
            return answer(outcome, GET_NOT_HTTPS, null);
        } catch (SSLException e) {
            Log.w(TAG, "A secure connection failed: " + e.getClass().getSimpleName());
            return answer(outcome, GET_INSECURE, null);
        } catch (IOException e) {
            if (networkState() == NETWORK_OFFLINE) {
                return answer(outcome, GET_OFFLINE, null);
            }
            Log.w(TAG, "A request failed: " + e.getClass().getSimpleName());
            return answer(outcome, GET_FAILED, null);
        } catch (RuntimeException e) {
            Log.w(TAG, "A request failed: " + e.getClass().getSimpleName());
            return answer(outcome, GET_FAILED, null);
        } finally {
            if (connection != null) {
                connection.disconnect();
            }
        }
    }

    /** Fills {@code outcome} with {@code kind} and {@code detail}, and returns no body. */
    private static byte[] answer(String[] outcome, String kind, String detail) {
        if (outcome != null && outcome.length >= 2) {
            outcome[0] = kind;
            outcome[1] = detail;
        }
        return null;
    }

    /** All of {@code in}, or null if it holds more than {@code maxBytes} (SEC-02). */
    private static byte[] readAtMost(InputStream in, int maxBytes) throws IOException {
        try (InputStream stream = in) {
            ByteArrayOutputStream body = new ByteArrayOutputStream();
            byte[] buffer = new byte[16 * 1024];
            int read;
            while ((read = stream.read(buffer)) != -1) {
                if ((long) body.size() + read > maxBytes) {
                    return null;
                }
                body.write(buffer, 0, read);
            }
            return body.toByteArray();
        }
    }

    /**
     * The screen the lock screen shows on, which the daily image is cropped for (WALL-06, ADR
     * 0028, ADR 0030): the default display's width and height in pixels, in its natural
     * orientation, from its mode, so turning the device doesn't change them, and its density in
     * dpi. Other displays, such as a monitor, never show the lock screen. Null if the system
     * can't say. Called from Rust.
     */
    int[] screen() {
        try {
            Display display =
                    context.getSystemService(DisplayManager.class)
                            .getDisplay(Display.DEFAULT_DISPLAY);
            if (display == null) {
                return null;
            }
            Display.Mode mode = display.getMode();
            int width = mode.getPhysicalWidth();
            int height = mode.getPhysicalHeight();
            int density = context.getResources().getDisplayMetrics().densityDpi;
            return width > 0 && height > 0 ? new int[] {width, height, density} : null;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the display's size: " + e.getClass().getSimpleName());
            return null;
        }
    }

    /**
     * Whether the device folds, from its hinge-angle sensor, so the lock screen may show on a
     * screen of another shape (ADR 0030). Called from Rust.
     */
    boolean folds() {
        try {
            return context.getPackageManager()
                    .hasSystemFeature(PackageManager.FEATURE_SENSOR_HINGE_ANGLE);
        } catch (RuntimeException e) {
            return false;
        }
    }

    /**
     * Why the lock-screen wallpaper can't be set, or null if it can (WALL-13, ADR 0030): a
     * device without wallpapers, or a policy that forbids changing them. Called from Rust.
     */
    String lockScreenUnsupported() {
        try {
            WallpaperManager wallpapers = WallpaperManager.getInstance(context);
            if (!wallpapers.isWallpaperSupported()) {
                return context.getString(R.string.wallpaper_not_supported);
            }
            if (!wallpapers.isSetWallpaperAllowed()) {
                return context.getString(R.string.wallpaper_not_allowed);
            }
            return null;
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't ask about wallpapers: " + e.getClass().getSimpleName());
            return context.getString(R.string.wallpaper_not_supported);
        }
    }

    /**
     * Sets a JPEG image as the lock screen's wallpaper only, without backup (WALL-01, ADR 0030):
     * {@code FLAG_LOCK} leaves the home wallpaper as it is, and nothing here touches the system
     * lock screen itself (SEC-05). Returns the new wallpaper's id, or -1 if it couldn't be set:
     * {@code setStream} answers 0 when the system refused it and wrote nothing. It blocks while
     * the system writes the image, so it answers -1 on the main thread without setting
     * anything. Called from Rust, off the main thread.
     */
    int setLockScreen(byte[] jpeg) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            Log.w(TAG, "Refused to set a wallpaper on the main thread");
            return -1;
        }
        try {
            int id = WallpaperManager.getInstance(context)
                    .setStream(new ByteArrayInputStream(jpeg), null, false,
                            WallpaperManager.FLAG_LOCK);
            if (id <= 0) {
                Log.w(TAG, "The system didn't set the lock screen's wallpaper");
                return -1;
            }
            return id;
        } catch (IOException | RuntimeException e) {
            Log.w(TAG, "Couldn't set the lock screen's wallpaper: "
                    + e.getClass().getSimpleName());
            return -1;
        }
    }

    /**
     * The id of the lock screen's wallpaper, whoever set it, negative while the lock screen has
     * none of its own and shows the home wallpaper, or {@code Integer.MIN_VALUE} if the system
     * can't say (WALL-14, ADR 0030). Called from Rust.
     */
    int lockScreenWallpaperId() {
        try {
            return WallpaperManager.getInstance(context).getWallpaperId(WallpaperManager.FLAG_LOCK);
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't read the lock screen's wallpaper: "
                    + e.getClass().getSimpleName());
            return Integer.MIN_VALUE;
        }
    }

    /**
     * Removes the lock screen's own wallpaper, so it shows the home wallpaper (WALL-12, ADR
     * 0030). Returns null, or a short message if it couldn't. Called from Rust.
     */
    String resetLockScreen() {
        try {
            WallpaperManager.getInstance(context).clear(WallpaperManager.FLAG_LOCK);
            return null;
        } catch (IOException | RuntimeException e) {
            Log.w(TAG, "Couldn't reset the lock screen: " + e.getClass().getSimpleName());
            return context.getString(R.string.lock_screen_reset_failed);
        }
    }

    /**
     * Opens the system's wallpaper settings, where the user picks any wallpaper (WALL-12, ADR
     * 0030): the system Settings' own wallpaper screen where it has one, as Samsung's and
     * Google's do, or else the system's choice of wallpaper apps. Returns null, or written
     * instructions if neither opens. Called from Rust.
     */
    String openWallpaperSettings() {
        Activity current = activity;
        Context from = current != null ? current : context;
        Intent[] screens = {
            new Intent(WALLPAPER_SETTINGS), new Intent(Intent.ACTION_SET_WALLPAPER),
        };
        for (Intent screen : screens) {
            try {
                from.startActivity(screen.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                return null;
            } catch (ActivityNotFoundException | SecurityException e) {
                // Try the next screen.
            }
        }
        Log.w(TAG, "Couldn't open the wallpaper settings");
        return context.getString(R.string.wallpaper_settings_unavailable);
    }

    /**
     * The region {@code left}, {@code top}, {@code right}, {@code bottom} of a JPEG image
     * {@code width} × {@code height} pixels, decoded at 1/{@code sample} of its size, scaled to
     * {@code outWidth} × {@code outHeight} and written as a new JPEG. Null if it can't be made
     * (WALL-06, WALL-07, SEC-02, ADR 0028).
     *
     * <p>Rust has already checked the image's header and planned the crop; this only carries it
     * out. Only the region is decoded, and no larger than the result needs, so memory stays
     * bounded. An image that doesn't decode, or not at the size its header gave, is refused.
     * Writing it anew keeps none of the original's metadata. It blocks, so it answers null on
     * the main thread without decoding. Logs hold only the kind of failure. Called from Rust,
     * off the main thread.
     */
    @SuppressWarnings("deprecation") // newInstance(byte[], int, int) arrived in Android 12.
    byte[] cropImage(
            byte[] jpeg,
            int width,
            int height,
            int left,
            int top,
            int right,
            int bottom,
            int sample,
            int outWidth,
            int outHeight) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            Log.w(TAG, "Refused to decode an image on the main thread");
            return null;
        }
        BitmapRegionDecoder decoder = null;
        Bitmap region = null;
        Bitmap scaled = null;
        try {
            decoder = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
                    ? BitmapRegionDecoder.newInstance(jpeg, 0, jpeg.length)
                    : BitmapRegionDecoder.newInstance(jpeg, 0, jpeg.length, false);
            if (decoder == null
                    || decoder.getWidth() != width
                    || decoder.getHeight() != height) {
                Log.w(TAG, "An image didn't decode at the size its header gave");
                return null;
            }
            BitmapFactory.Options options = new BitmapFactory.Options();
            options.inSampleSize = sample;
            options.inPreferredConfig = Bitmap.Config.ARGB_8888;
            region = decoder.decodeRegion(new Rect(left, top, right, bottom), options);
            if (region == null) {
                Log.w(TAG, "An image didn't decode");
                return null;
            }
            scaled = region.getWidth() == outWidth && region.getHeight() == outHeight
                    ? region
                    : Bitmap.createScaledBitmap(region, outWidth, outHeight, true);
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            if (!scaled.compress(Bitmap.CompressFormat.JPEG, JPEG_QUALITY, out)) {
                Log.w(TAG, "A cropped image couldn't be written");
                return null;
            }
            return out.toByteArray();
        } catch (IOException | RuntimeException | OutOfMemoryError e) {
            Log.w(TAG, "Couldn't crop an image: " + e.getClass().getSimpleName());
            return null;
        } finally {
            if (scaled != null && scaled != region) {
                scaled.recycle();
            }
            if (region != null) {
                region.recycle();
            }
            if (decoder != null) {
                decoder.recycle();
            }
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
