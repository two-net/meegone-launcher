package io.github.twonet.meegone;

import android.app.Activity;
import android.app.ActivityOptions;
import android.app.SearchManager;
import android.app.role.RoleManager;
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
import android.graphics.Insets;
import android.graphics.Rect;
import android.icu.lang.UCharacter;
import android.icu.text.Collator;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.Process;
import android.os.SystemClock;
import android.os.UserHandle;
import android.os.UserManager;
import android.provider.MediaStore;
import android.provider.Settings;
import android.provider.Telephony;
import android.telecom.TelecomManager;
import android.util.Log;
import android.view.HapticFeedbackConstants;
import android.view.View;
import android.view.WindowInsets;
import java.text.Normalizer;
import java.util.ArrayList;
import java.util.List;
import java.util.TimeZone;
import java.util.function.Supplier;
import java.util.regex.Pattern;

/**
 * What the Rust platform adapters ask of Android (ADR 0005).
 *
 * <p>Rust looks these methods up by name and signature on the main thread, so keep them in step
 * with {@code crates/core/data/src/android/system.rs} and {@code apps.rs}. Every method catches
 * what Android may throw: an exception must never reach Rust. Logs hold only yes/no answers,
 * never user data such as which apps are installed.
 */
final class Bridge {
    private static final String TAG = "MeeGone";

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
    private static final String INSTRUCTIONS =
            "To make MeeGone your home screen, open Settings, then Apps, Default apps, Home app,"
                    + " and choose MeeGone.";

    private static Bridge instance;

    private final Context context;
    private final Handler main = new Handler(Looper.getMainLooper());
    /** Tells the Rust app about a burst of app and profile changes once (ADR 0010). */
    private final Runnable reportAppsChanged = () -> HomeActivity.appsChanged(this.activity);
    /** Each app's accent colour, derived from its icon (APPS-17, ADR 0011). */
    private final AppAccents accents;
    /** The resumed activity, which can start the system's screens; null while none is. */
    private Activity activity;
    private long roleRequestedAt;
    private boolean watchingApps;

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
        Activity current = activity;
        View decor = current == null ? null : current.getWindow().peekDecorView();
        if (icon == null || decor == null) {
            return null;
        }
        int[] origin = new int[2];
        decor.getLocationOnScreen(origin);
        return ActivityOptions.makeScaleUpAnimation(decor, icon.left - origin[0],
                icon.top - origin[1], icon.width(), icon.height()).toBundle();
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
            BroadcastReceiver receiver = new BroadcastReceiver() {
                @Override
                public void onReceive(Context context, Intent intent) {
                    appsChanged();
                }
            };
            IntentFilter filter = new IntentFilter();
            for (String action : CATALOGUE_CHANGES) {
                filter.addAction(action);
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                context.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED);
            } else {
                context.registerReceiver(receiver, filter);
            }
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't watch the apps", e);
        }
    }

    private void appsChanged() {
        main.removeCallbacks(reportAppsChanged);
        main.post(reportAppsChanged);
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
