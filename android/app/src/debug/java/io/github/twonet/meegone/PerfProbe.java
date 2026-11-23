package io.github.twonet.meegone;

import android.content.Context;
import android.content.Intent;
import android.graphics.Rect;
import android.icu.text.Collator;
import android.icu.text.Transliterator;
import android.os.Looper;
import android.os.SystemClock;
import android.util.Log;
import android.view.MotionEvent;
import android.view.View;
import android.widget.CompoundButton;
import android.widget.TextView;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/**
 * Debug and benchmark builds only: marks what the performance budgets measure in logcat, under
 * {@link #TAG}, for {@code tools/perf-bench.sh} (ADR 0041). Each mark is logged as it happens,
 * so its logcat time, on the same monotonic clock as the system's own lines, is the moment:
 *
 * <ul>
 *   <li>{@code home-ready icons=<n>}: after a Home, the first frame with every icon in sight in
 *       Applications, once the main thread is idle (PERF-01); and how many icons are held.
 *   <li>{@code layout-shown}, after a cold start: the first frame with the saved layout's cells,
 *       placeholders allowed (PERF-04). Then {@code icons-shown} with every icon in sight, and
 *       {@code shift} if any cell moved between the two.
 *   <li>{@code search <ms>}: from a change of search's text to the frame with its results
 *       (PERF-05).
 *   <li>{@code launch <ms>}: from the finger lifting to the launch request leaving (PERF-06).
 *   <li>{@code trim level=<level> icons=<n>}: the icons still held after the system asked for
 *       memory back (PERF-07).
 *   <li>{@code stored <bytes>}: everything in the app's files folder (PERF-08).
 *   <li>{@code first-cell <x> <y>}: where on screen the first cell's middle is, to tap.
 *   <li>{@code setup-done}: first-run setup isn't showing, or has just been answered.
 * </ul>
 *
 * <p>The extra {@link DebugHooks#EXTRA_PERF} turns it on or off, for this and every later
 * process, as a cold start's Home carries no extras. With it, {@link DebugHooks#EXTRA_APPS}
 * pads the catalogue to that many apps with copies of the installed ones, so the budgets'
 * 300 apps need no 300 installs. Each copy is "‹label› ‹k›", its activity "‹activity›#k";
 * {@link DebugHooks#realActivity} names the real one wherever Android is asked about it, so
 * copies have real icons and launch the real app.
 *
 * <p>While it is on, it answers first-run setup as the accessibility tour does, with the daily
 * image off, so a benchmark downloads nothing and leaves the lock screen as it is.
 */
final class PerfProbe {
    static final String TAG = "MeeGonePerf";
    /** In the app's no-backup folder, which MeeGone's own storage never sees. */
    private static final String SETTINGS = "perf-probe";
    /** The fields of one app in {@code Bridge.launchableApps}. */
    private static final int APP_FIELDS = 9;
    private static final int ACTIVITY = 1;
    private static final int LABEL = 2;
    private static final int SORT_KEY = 6;
    private static final int SEARCH_KEY = 8;
    /** Between one setup step's answer and the next, for its motion. */
    private static final long SETUP_STEP_MS = 600;

    private static boolean on;
    private static int apps;
    /** When the last finger lifted, in {@link SystemClock#uptimeMillis}, or 0. */
    private static long lifted;
    /** The cells in sight when the saved layout first showed, on screen. */
    private static List<Rect> firstShown;

    private PerfProbe() {}

    /**
     * Takes {@code intent}'s settings, if it has any, and reads them for this process. Before
     * MeeGone starts, as the catalogue it reads is padded.
     */
    static void configure(Context context, Intent intent) {
        File file = new File(context.getNoBackupFilesDir(), SETTINGS);
        on = file.exists();
        apps = on ? read(file) : 0;
        if (intent == null || !intent.hasExtra(DebugHooks.EXTRA_PERF)) {
            return;
        }
        boolean wanted = intent.getBooleanExtra(DebugHooks.EXTRA_PERF, false);
        int count = intent.getIntExtra(DebugHooks.EXTRA_APPS, 0);
        intent.removeExtra(DebugHooks.EXTRA_PERF);
        intent.removeExtra(DebugHooks.EXTRA_APPS);
        // The home's intent keeps its extras, and comes again with every Home: only a change
        // is written.
        if (wanted == on && (!wanted || count == apps)) {
            return;
        }
        if (wanted) {
            try (FileOutputStream out = new FileOutputStream(file)) {
                out.write(Integer.toString(count).getBytes(StandardCharsets.US_ASCII));
            } catch (IOException e) {
                Log.w(TAG, "Couldn't keep the probe's settings", e);
            }
        } else if (file.exists() && !file.delete()) {
            Log.w(TAG, "Couldn't turn the probe off");
        }
        on = file.exists();
        apps = on ? read(file) : 0;
    }

    /** The apps the settings in {@code file} ask for, or 0. */
    private static int read(File file) {
        try {
            String text = new String(Files.readAllBytes(file.toPath()), StandardCharsets.US_ASCII);
            return Integer.parseInt(text.trim());
        } catch (IOException | NumberFormatException e) {
            return 0;
        }
    }

    /**
     * The home views were made. {@code started} says this process started MeeGone: a cold
     * start, whose layout and icons are marked as they show (PERF-02, PERF-04).
     */
    static void created(HomeViews views, boolean started) {
        if (!on) {
            return;
        }
        Log.i(TAG, "created started=" + started + " apps=" + apps);
        views.postDelayed(() -> answerSetup(views), SETUP_STEP_MS);
        if (started) {
            firstShown = null;
            afterFrame(views, () -> layoutShown(views));
        }
    }

    private static boolean layoutShown(HomeViews views) {
        List<AppCell> inSight = views.appsGrid().cellsInSight();
        if (inSight.isEmpty()) {
            return false;
        }
        Log.i(TAG, "layout-shown cells=" + inSight.size());
        firstShown = rects(inSight);
        afterFrame(views, () -> iconsShown(views));
        return true;
    }

    private static boolean iconsShown(HomeViews views) {
        AppsGrid grid = views.appsGrid();
        if (!grid.iconsShownInSight()) {
            return false;
        }
        Log.i(TAG, "icons-shown");
        List<Rect> now = rects(grid.cellsInSight());
        if (firstShown != null && !now.subList(0, Math.min(now.size(), firstShown.size()))
                .equals(firstShown.subList(0, Math.min(now.size(), firstShown.size())))) {
            Log.i(TAG, "shift");
        }
        firstCell(grid);
        Log.i(TAG, "stored " + size(views.getContext().getFilesDir()));
        return true;
    }

    /**
     * The system's Home action reached the home. Marks the first frame after it with every
     * icon in sight, once the main thread is idle: drawn and ready for a touch (PERF-01).
     */
    static void home(HomeViews views) {
        if (!on) {
            return;
        }
        afterFrame(views, () -> {
            AppsGrid grid = views.appsGrid();
            if (views.showsApplications() && !grid.iconsShownInSight()) {
                return false;
            }
            Looper.myQueue().addIdleHandler(() -> {
                Log.i(TAG, "home-ready icons=" + views.icons().size());
                return false;
            });
            return true;
        });
    }

    /** Search's text changed: marks how long until the frame with its results (PERF-05). */
    static void typed(HomeViews views) {
        if (!on) {
            return;
        }
        long at = SystemClock.uptimeMillis();
        views.getViewTreeObserver().registerFrameCommitCallback(() ->
                Log.i(TAG, "search " + (SystemClock.uptimeMillis() - at)));
    }

    static void touched(MotionEvent event) {
        if (on && event.getActionMasked() == MotionEvent.ACTION_UP) {
            lifted = event.getEventTime();
        }
    }

    /** A launch request has left: marks how long since the finger lifted (PERF-06). */
    static void launched() {
        if (on && lifted > 0) {
            Log.i(TAG, "launch " + (SystemClock.uptimeMillis() - lifted));
            lifted = 0;
        }
    }

    static void trimmed(HomeViews views, int level) {
        if (on) {
            Log.i(TAG, "trim level=" + level + " icons=" + views.icons().size());
        }
    }

    /**
     * Pads the catalogue's {@code fields} to the apps asked for, with copies of the apps in it.
     * Each copy's label, sort key and search key are made as {@code Bridge} makes them.
     */
    static void padApps(List<String> fields, Collator collator, Transliterator latin) {
        int real = fields.size() / APP_FIELDS;
        if (!on || real == 0 || apps <= real) {
            return;
        }
        List<String> copies = new ArrayList<>();
        for (int k = 1; real + k <= apps; k++) {
            int from = ((k - 1) % real) * APP_FIELDS;
            List<String> copy = new ArrayList<>(fields.subList(from, from + APP_FIELDS));
            String label = copy.get(LABEL).isEmpty() ? copy.get(0) : copy.get(LABEL);
            String name = String.format(Locale.ROOT, "%s %d", label, k);
            copy.set(ACTIVITY, copy.get(ACTIVITY) + "#" + k);
            copy.set(LABEL, name);
            copy.set(SORT_KEY, Bridge.hex(collator.getCollationKey(name).toByteArray()));
            copy.set(SEARCH_KEY, Bridge.fold(name, latin));
            copies.addAll(copy);
        }
        fields.addAll(copies);
    }

    /**
     * Answers setup's step on screen, Continue or Not now, with the daily image's switch off,
     * then the next, until setup ends (ONB-01, ADR 0036).
     */
    private static void answerSetup(HomeViews views) {
        int step = views.auditSetupStep();
        if (step < 0) {
            Log.i(TAG, "setup-done");
            return;
        }
        if (step == SetupScreens.LOCK_SCREEN_IMAGE) {
            A11yAudit.each(views.getRootView(), view -> view instanceof CompoundButton
                    && view.isShown() && ((CompoundButton) view).isChecked(), View::performClick);
        }
        if (!click(views, R.string.setup_not_now) && !click(views, R.string.setup_continue)) {
            Log.w(TAG, "No answer to setup's step " + step);
            return;
        }
        Log.i(TAG, "setup answered step=" + step);
        views.postDelayed(() -> answerSetup(views), SETUP_STEP_MS);
    }

    /** Clicks the shown element whose text is {@code text}, or what holds it. */
    private static boolean click(HomeViews views, int text) {
        String wanted = views.getResources().getString(text);
        List<View> found = new ArrayList<>();
        A11yAudit.each(views.getRootView(), view -> view instanceof TextView && view.isShown()
                && wanted.contentEquals(((TextView) view).getText()), found::add);
        for (View view : found) {
            for (View at = view; at != null;
                    at = at.getParent() instanceof View ? (View) at.getParent() : null) {
                if (at.isClickable() && at.isEnabled()) {
                    return at.performClick();
                }
            }
        }
        return false;
    }

    /** A copy's activity as the real app names it. */
    static String realActivity(String activity) {
        int copy = activity.indexOf('#');
        return copy < 0 ? activity : activity.substring(0, copy);
    }

    /** Runs {@code check} after each frame until it returns true. */
    private static void afterFrame(HomeViews views, Check check) {
        views.getViewTreeObserver().registerFrameCommitCallback(() -> {
            if (!check.done()) {
                afterFrame(views, check);
            }
        });
    }

    private interface Check {
        boolean done();
    }

    private static void firstCell(AppsGrid grid) {
        List<AppCell> inSight = grid.cellsInSight();
        if (!inSight.isEmpty()) {
            Rect rect = rects(inSight.subList(0, 1)).get(0);
            Log.i(TAG, "first-cell " + rect.centerX() + " " + rect.centerY());
        }
    }

    private static List<Rect> rects(List<AppCell> cells) {
        List<Rect> rects = new ArrayList<>();
        int[] at = new int[2];
        for (AppCell cell : cells) {
            cell.getLocationOnScreen(at);
            rects.add(new Rect(at[0], at[1], at[0] + cell.getWidth(), at[1] + cell.getHeight()));
        }
        return rects;
    }

    private static long size(File file) {
        if (file.isFile()) {
            return file.length();
        }
        long total = 0;
        File[] children = file.listFiles();
        if (children != null) {
            for (File child : children) {
                total += size(child);
            }
        }
        return total;
    }
}
