package io.github.twonet.meegone;

import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.LauncherActivityInfo;
import android.content.pm.LauncherApps;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.drawable.BitmapDrawable;
import android.graphics.drawable.Drawable;
import android.os.Handler;
import android.os.Looper;
import android.os.UserHandle;
import android.os.UserManager;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * The Applications grid's icons (ADR 0010). They are pixels only the views use, so they stay in
 * Java and never cross into Rust.
 *
 * <p>Each icon is the system's, with the work badge on work-profile apps (APPS-20). It is loaded
 * on one background thread, in the order the grid asks, and drawn once into a bitmap of the
 * size the grid shows, so a few hundred icons stay small and quick to draw. Until it arrives,
 * the cell shows a placeholder of the same size, so the layout never shifts (PERF-04).
 *
 * <p>When a package changes, as on an update, its icons are loaded again. Everything else runs
 * on the main thread.
 */
final class AppIcons {
    /** One thread for the whole process: icons load in order, behind the scenes. */
    private static final ExecutorService LOADER = Executors.newSingleThreadExecutor(task -> {
        Thread thread = new Thread(task, "MeeGone icons");
        thread.setPriority(Thread.NORM_PRIORITY - 1);
        return thread;
    });

    private final Context context;
    private final int sizePx;
    private final int densityDpi;
    private final Handler main = new Handler(Looper.getMainLooper());
    /** Runs when icons arrive or change, so the grid asks again. */
    private final Runnable onChange;
    private final Map<String, Drawable> loaded = new HashMap<>();
    /** Icons on their way, or that couldn't be loaded until their package changes. */
    private final Set<String> asked = new HashSet<>();
    private final LauncherApps.Callback forgetChanged = new LauncherApps.Callback() {
        @Override
        public void onPackageAdded(String packageName, UserHandle user) {
            forget(packageName, user);
        }

        @Override
        public void onPackageRemoved(String packageName, UserHandle user) {
            forget(packageName, user);
        }

        @Override
        public void onPackageChanged(String packageName, UserHandle user) {
            forget(packageName, user);
        }

        @Override
        public void onPackagesAvailable(String[] packageNames, UserHandle user,
                boolean replacing) {
            for (String packageName : packageNames) {
                forget(packageName, user);
            }
        }

        @Override
        public void onPackagesUnavailable(String[] packageNames, UserHandle user,
                boolean replacing) {}
    };
    /** Bumped by {@link #clear}, so icons loaded before it are dropped. */
    private int generation;
    private boolean watching;

    AppIcons(Context context, int sizePx, Runnable onChange) {
        this.context = context.getApplicationContext();
        this.sizePx = sizePx;
        this.densityDpi = context.getResources().getDisplayMetrics().densityDpi;
        this.onChange = onChange;
    }

    /**
     * The icon of an app, or null until it arrives: then {@code onChange} runs. {@code profile}
     * is the profile's serial number.
     */
    Drawable get(long profile, String packageName, String activity) {
        String key = key(profile, packageName, activity);
        Drawable icon = loaded.get(key);
        if (icon != null || !asked.add(key)) {
            return icon;
        }
        int askedIn = generation;
        LOADER.execute(() -> {
            Drawable found = load(profile, packageName, activity);
            main.post(() -> {
                if (askedIn != generation) {
                    return;
                }
                if (found != null) {
                    asked.remove(key);
                    loaded.put(key, found);
                    onChange.run();
                }
            });
        });
        return null;
    }

    /** Follows package changes while the grid is on screen. */
    void watch() {
        if (watching) {
            return;
        }
        watching = true;
        try {
            context.getSystemService(LauncherApps.class).registerCallback(forgetChanged, main);
        } catch (RuntimeException e) {
            // Icons then stay as they are until the grid comes back.
        }
    }

    /**
     * Stops following package changes, and drops every icon: any of them may change while the
     * grid is away.
     */
    void stopWatching() {
        if (!watching) {
            return;
        }
        watching = false;
        try {
            context.getSystemService(LauncherApps.class).unregisterCallback(forgetChanged);
        } catch (RuntimeException e) {
            // Nothing to undo.
        }
        clear();
    }

    private void clear() {
        generation++;
        loaded.clear();
        asked.clear();
    }

    private void forget(String packageName, UserHandle user) {
        UserManager users = context.getSystemService(UserManager.class);
        String prefix = key(users.getSerialNumberForUser(user), packageName, "");
        boolean changed = removeStartingWith(loaded.keySet(), prefix);
        changed |= removeStartingWith(asked, prefix);
        if (changed) {
            onChange.run();
        }
    }

    private static boolean removeStartingWith(Set<String> keys, String prefix) {
        boolean removed = false;
        for (Iterator<String> it = keys.iterator(); it.hasNext(); ) {
            if (it.next().startsWith(prefix)) {
                it.remove();
                removed = true;
            }
        }
        return removed;
    }

    /** On the loader thread. */
    private Drawable load(long profile, String packageName, String activity) {
        try {
            UserHandle user = context.getSystemService(UserManager.class)
                    .getUserForSerialNumber(profile);
            if (user == null) {
                return null;
            }
            Intent intent = new Intent(Intent.ACTION_MAIN)
                    .addCategory(Intent.CATEGORY_LAUNCHER)
                    .setComponent(new ComponentName(packageName, activity));
            LauncherActivityInfo info =
                    context.getSystemService(LauncherApps.class).resolveActivity(intent, user);
            if (info == null) {
                return null;
            }
            Drawable icon = info.getBadgedIcon(densityDpi);
            Bitmap bitmap = Bitmap.createBitmap(sizePx, sizePx, Bitmap.Config.ARGB_8888);
            icon.setBounds(0, 0, sizePx, sizePx);
            icon.draw(new Canvas(bitmap));
            return new BitmapDrawable(context.getResources(), bitmap);
        } catch (RuntimeException e) {
            return null;
        }
    }

    /** Profile, package and activity; a package's icons share the start of their keys. */
    private static String key(long profile, String packageName, String activity) {
        return profile + "/" + packageName + "/" + activity;
    }
}
