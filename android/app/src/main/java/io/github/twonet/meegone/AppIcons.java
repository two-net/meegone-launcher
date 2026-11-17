package io.github.twonet.meegone;

import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.LauncherActivityInfo;
import android.content.pm.LauncherApps;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.graphics.BitmapShader;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Shader;
import android.graphics.drawable.AdaptiveIconDrawable;
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
 * <p>Every icon has one shape, a rounded square (APPS-15, ADR 0011): {@link #shaped} masks an
 * adaptive icon's layers to it, and draws a legacy single-image icon on a neutral plate of the
 * same shape. The work badge goes on after, so the shape never cuts it off (APPS-20).
 *
 * <p>Icons load on one background thread, in the order the grid asks, and are drawn once into
 * a bitmap of the size the grid shows, so a few hundred icons stay small and quick to draw.
 * Until one arrives, the cell shows a placeholder of the same size and shape, so the layout
 * never shifts (PERF-04).
 *
 * <p>When a package changes, as on an update, its icons are loaded again. Everything else runs
 * on the main thread.
 */
final class AppIcons {
    /** The corner radius of every icon's rounded square, as a share of its size (SRS 5.2). */
    static final float CORNER = 0.24f;
    /** An adaptive icon's layers are 108 dp around the 72 dp its shape shows. */
    private static final float ADAPTIVE_LAYERS = 108f / 72f;
    /** A legacy icon's longer side, as a share of its plate, which leaves the icon's own shape. */
    private static final float LEGACY_SCALE = 0.7f;
    /**
     * The plate behind a legacy icon: white, as Android's own launcher gives such icons, and the
     * same in both themes, so the icon and its accent never depend on the theme.
     */
    private static final int PLATE = Color.WHITE;

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
     * is the profile's serial number. An empty {@code activity} asks for the app's own icon, for
     * an app with no icon in the grid, such as one in Events (ADR 0020).
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
            Drawable source = activity.isEmpty()
                    ? appIcon(packageName, user)
                    : activityIcon(packageName, activity, user);
            if (source == null) {
                return null;
            }
            Drawable icon = new BitmapDrawable(context.getResources(), shaped(source, sizePx));
            Drawable badged = context.getPackageManager().getUserBadgedIcon(icon, user);
            if (badged == icon) {
                return icon;
            }
            // A work app: draw its badge in, so the grid still draws one bitmap per icon.
            Bitmap bitmap = Bitmap.createBitmap(sizePx, sizePx, Bitmap.Config.ARGB_8888);
            badged.setBounds(0, 0, sizePx, sizePx);
            badged.draw(new Canvas(bitmap));
            return new BitmapDrawable(context.getResources(), bitmap);
        } catch (RuntimeException e) {
            return null;
        }
    }

    /** A launcher activity's icon, or null if it has gone. On the loader thread. */
    private Drawable activityIcon(String packageName, String activity, UserHandle user) {
        Intent intent = new Intent(Intent.ACTION_MAIN)
                .addCategory(Intent.CATEGORY_LAUNCHER)
                .setComponent(new ComponentName(packageName, activity));
        LauncherActivityInfo info =
                context.getSystemService(LauncherApps.class).resolveActivity(intent, user);
        return info != null ? info.getIcon(densityDpi) : null;
    }

    /**
     * An app's own icon, or the system's default app icon for one MeeGone can't see, such as
     * System UI under Android 11's package visibility. On the loader thread.
     */
    private Drawable appIcon(String packageName, UserHandle user) {
        PackageManager packages = context.getPackageManager();
        try {
            return packages.getApplicationIcon(context.getSystemService(LauncherApps.class)
                    .getApplicationInfo(packageName, 0, user));
        } catch (PackageManager.NameNotFoundException | RuntimeException e) {
            return packages.getDefaultActivityIcon();
        }
    }

    /**
     * {@code icon} drawn in MeeGone's one shape, {@code sizePx} square (APPS-15). An adaptive
     * icon's layers are masked to the rounded square. Any other icon is a legacy single image:
     * it is scaled onto a neutral plate of the same shape, keeping its proportions. On any
     * thread.
     */
    static Bitmap shaped(Drawable icon, int sizePx) {
        Bitmap content = Bitmap.createBitmap(sizePx, sizePx, Bitmap.Config.ARGB_8888);
        Canvas canvas = new Canvas(content);
        if (icon instanceof AdaptiveIconDrawable && !DebugHooks.legacyIcons()) {
            AdaptiveIconDrawable adaptive = (AdaptiveIconDrawable) icon;
            int outset = Math.round(sizePx * (ADAPTIVE_LAYERS - 1f) / 2f);
            drawLayer(adaptive.getBackground(), canvas, -outset, sizePx + outset);
            drawLayer(adaptive.getForeground(), canvas, -outset, sizePx + outset);
        } else {
            canvas.drawColor(PLATE);
            int width = icon.getIntrinsicWidth();
            int height = icon.getIntrinsicHeight();
            if (width <= 0 || height <= 0) {
                width = height = 1;
            }
            float scale = sizePx * LEGACY_SCALE / Math.max(width, height);
            int drawnWidth = Math.round(width * scale);
            int drawnHeight = Math.round(height * scale);
            int left = (sizePx - drawnWidth) / 2;
            int top = (sizePx - drawnHeight) / 2;
            icon.setBounds(left, top, left + drawnWidth, top + drawnHeight);
            icon.draw(canvas);
        }
        // Painting the content through a rounded rect, unlike clipping to one, smooths the
        // shape's edge.
        Bitmap shaped = Bitmap.createBitmap(sizePx, sizePx, Bitmap.Config.ARGB_8888);
        Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        paint.setShader(new BitmapShader(content, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP));
        float corner = sizePx * CORNER;
        new Canvas(shaped).drawRoundRect(0, 0, sizePx, sizePx, corner, corner, paint);
        content.recycle();
        return shaped;
    }

    /** One of an adaptive icon's layers, which may be missing, drawn over a square. */
    private static void drawLayer(Drawable layer, Canvas canvas, int start, int end) {
        if (layer != null) {
            layer.setBounds(start, start, end, end);
            layer.draw(canvas);
        }
    }

    /** Profile, package and activity; a package's icons share the start of their keys. */
    private static String key(long profile, String packageName, String activity) {
        return profile + "/" + packageName + "/" + activity;
    }
}
