package io.github.twonet.meegone;

import android.content.Context;
import android.content.pm.LauncherActivityInfo;
import android.graphics.Bitmap;
import android.os.Handler;
import android.os.Looper;
import android.os.UserHandle;
import android.os.UserManager;
import android.util.DisplayMetrics;
import android.util.Log;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Each app's accent colour, derived once from its icon (APPS-17, ADR 0011). It travels in the
 * app catalogue that {@link Bridge} hands the Rust app, which shows it in Events and Recent and
 * adjusts it to each theme there (THEME-07). Accents are kept in memory for the life of the
 * process (SRS 7.4).
 *
 * <p>An icon is drawn in MeeGone's shape at {@link #SAMPLE_PX}, the same on every device, on a
 * thread of its own at the lowest priority, so the grid's icons never wait for it (PERF-04).
 * Its pixels go to the Rust app's {@code nativeAccent}, the pure logic that picks the colour.
 * The accents of a batch reach the main thread together, and then {@code onDerived} runs once,
 * so the catalogue is read again.
 *
 * <p>When a package changes, as on an update, its accents are forgotten and derived again from
 * the new icon. Everything but the sampling runs on the main thread.
 */
final class AppAccents {
    private static final String TAG = "MeeGone";
    /** Icons are drawn this many px square to derive their accent. */
    private static final int SAMPLE_PX = 48;
    /** The density icons are read at: 48 px is a 48 dp legacy icon's own size there. */
    private static final int SAMPLE_DENSITY = DisplayMetrics.DENSITY_MEDIUM;
    /** An icon that couldn't be read: its app is neutral until its package changes. */
    private static final int NONE = -1;
    private static final ExecutorService SAMPLER = Executors.newSingleThreadExecutor(task -> {
        Thread thread = new Thread(task, "MeeGone accents");
        thread.setPriority(Thread.MIN_PRIORITY);
        return thread;
    });

    private final Context context;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final Runnable onDerived;
    /** Derived accents as {@code 0xRRGGBB}, or {@link #NONE}, by app. */
    private final Map<String, Integer> accents = new HashMap<>();
    /**
     * Apps whose accent is on its way, by app, with the number of the request. A request that
     * {@link #forget} overtook, because its package changed, is dropped when it arrives.
     */
    private final Map<String, Integer> asked = new HashMap<>();
    /** Apps asked for since the last batch went to the sampler. */
    private final Map<String, LauncherActivityInfo> waiting = new LinkedHashMap<>();
    private int requests;

    AppAccents(Context context, Runnable onDerived) {
        this.context = context.getApplicationContext();
        this.onDerived = onDerived;
    }

    /**
     * The catalogue's accent field for an app in profile {@code serial}: its accent as a decimal
     * {@code 0xRRGGBB}, or empty until it has been derived. An app without one is asked for, once,
     * and {@link #deriveWaiting} sends it to the sampler.
     */
    String field(long serial, LauncherActivityInfo info) {
        String key = key(serial, info.getComponentName().getPackageName(),
                info.getComponentName().getClassName());
        Integer accent = accents.get(key);
        if (accent != null) {
            return accent == NONE ? "" : Integer.toString(accent);
        }
        if (!asked.containsKey(key)) {
            asked.put(key, ++requests);
            waiting.put(key, info);
        }
        return "";
    }

    /** Sends the apps {@link #field} asked for to the sampler, as one batch. */
    void deriveWaiting() {
        if (waiting.isEmpty()) {
            return;
        }
        List<String> keys = new ArrayList<>(waiting.keySet());
        List<LauncherActivityInfo> apps = new ArrayList<>(waiting.values());
        List<Integer> numbers = new ArrayList<>();
        for (String key : keys) {
            numbers.add(asked.get(key));
        }
        waiting.clear();
        SAMPLER.execute(() -> {
            int[] derived = new int[apps.size()];
            for (int i = 0; i < derived.length; i++) {
                derived[i] = sample(apps.get(i));
            }
            main.post(() -> arrived(keys, numbers, derived));
        });
    }

    /** Forgets a package's accents, so the next catalogue derives them again from its icon. */
    void forget(String packageName, UserHandle user) {
        try {
            long serial = context.getSystemService(UserManager.class).getSerialNumberForUser(user);
            String prefix = key(serial, packageName, "");
            removeStartingWith(accents, prefix);
            removeStartingWith(asked, prefix);
            removeStartingWith(waiting, prefix);
        } catch (RuntimeException e) {
            // The profile is gone, and its apps with it.
        }
    }

    private void arrived(List<String> keys, List<Integer> numbers, int[] derived) {
        int stored = 0;
        for (int i = 0; i < derived.length; i++) {
            String key = keys.get(i);
            if (numbers.get(i).equals(asked.get(key))) {
                asked.remove(key);
                accents.put(key, derived[i]);
                stored++;
            }
        }
        if (stored > 0) {
            Log.i(TAG, "Accents derived: " + stored);
            onDerived.run();
        }
    }

    /** On the sampler thread: the accent of an app's icon, or {@link #NONE}. */
    private static int sample(LauncherActivityInfo info) {
        try {
            Bitmap icon = AppIcons.shaped(info.getIcon(SAMPLE_DENSITY), SAMPLE_PX);
            int[] pixels = new int[SAMPLE_PX * SAMPLE_PX];
            icon.getPixels(pixels, 0, SAMPLE_PX, 0, 0, SAMPLE_PX, SAMPLE_PX);
            icon.recycle();
            return nativeAccent(pixels);
        } catch (RuntimeException e) {
            // An icon the system couldn't load, or that MeeGone couldn't read.
            return NONE;
        }
    }

    private static <V> void removeStartingWith(Map<String, V> map, String prefix) {
        for (Iterator<String> it = map.keySet().iterator(); it.hasNext(); ) {
            if (it.next().startsWith(prefix)) {
                it.remove();
            }
        }
    }

    /** Profile, package and activity, as {@link AppIcons} keys its icons. */
    private static String key(long serial, String packageName, String activity) {
        return serial + "/" + packageName + "/" + activity;
    }

    /**
     * The accent of an icon drawn in MeeGone's shape, from its {@code 0xAARRGGBB} pixels, as
     * {@code 0xRRGGBB}. Pure, so it runs on the sampler thread (ADR 0011). Throws if MeeGone
     * fails.
     */
    private static native int nativeAccent(int[] argb);
}
