package io.github.twonet.meegone;

import android.app.job.JobInfo;
import android.app.job.JobParameters;
import android.app.job.JobScheduler;
import android.app.job.JobService;
import android.content.ComponentName;
import android.content.Context;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * MeeGone's one scheduled job, the daily image's check (ARCH-07, WALL-03, WALL-04, ADR 0029).
 *
 * <p>The Rust app decides everything: whether a check is due, whether the battery and the
 * network allow it, what to fetch, and when the job runs next, which it asks for through {@link
 * #schedule}. {@code JobScheduler} runs the job no earlier than asked, on the network asked for,
 * and not while the battery is low. The job isn't persisted, so MeeGone needs no permission to
 * start at boot (SEC-03): as the home app it starts anyway, and {@link #checkAtStart} schedules
 * the job again. Each check runs on one thread of its own, never the main one, as its requests
 * block.
 */
public final class ImageJobService extends JobService {
    static {
        // The system may start this service before, or without, the home.
        System.loadLibrary("meegone_launcher");
    }

    private static final String TAG = "MeeGone";
    /** The job's id; MeeGone schedules no other (ARCH-07). */
    static final int JOB_ID = 1;
    /** Checks run here, one at a time. */
    private static final ExecutorService CHECKER =
            Executors.newSingleThreadExecutor(task -> new Thread(task, "MeeGone image"));
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    /** Guards {@link #running} and the pending request, and keeps requests in order. */
    private static final Object LOCK = new Object();
    /** Whether the job is running. */
    private static boolean running;
    /** Whether a request came while the job ran: {@link #pendingJob}, or a cancel if null. */
    private static boolean pending;
    private static JobInfo pendingJob;

    @Override
    public boolean onStartJob(JobParameters params) {
        synchronized (LOCK) {
            running = true;
        }
        Context app = getApplicationContext();
        CHECKER.execute(() -> {
            boolean attempted = check(app);
            MAIN.post(() -> {
                jobFinished(params, false);
                finished(app);
                if (attempted) {
                    Bridge.imageChecked();
                }
            });
        });
        return true;
    }

    /**
     * The system stopped the job, as when its network went. The check goes on to its end, and
     * a request cut short counts as a failure, which the next run retries (REL-04).
     */
    @Override
    public boolean onStopJob(JobParameters params) {
        return false;
    }

    /**
     * Runs the check once at MeeGone's start, which may be the only one where background
     * scheduling isn't available (SRS 8.1). It does nothing unless a check is due, and
     * schedules the job for the next one. Called by {@link HomeActivity} on the main thread.
     */
    static void checkAtStart(Context context) {
        Context app = context.getApplicationContext();
        CHECKER.execute(() -> {
            if (check(app)) {
                MAIN.post(Bridge::imageChecked);
            }
        });
    }

    /**
     * Schedules the job to run no earlier than {@code notBeforeMs}, on any network or only an
     * unmetered one, replacing the one scheduled (WALL-04). While the job runs, the request
     * waits until it finishes, as scheduling it would stop it. Returns null, or a short
     * message when the system can't schedule it. Called from Rust, through {@link Bridge}.
     */
    static String schedule(Context context, long notBeforeMs, boolean anyNetwork) {
        long delay = Math.max(0, notBeforeMs - System.currentTimeMillis());
        JobInfo job = new JobInfo.Builder(JOB_ID, new ComponentName(context, ImageJobService.class))
                .setMinimumLatency(delay)
                .setRequiredNetworkType(anyNetwork
                        ? JobInfo.NETWORK_TYPE_ANY
                        : JobInfo.NETWORK_TYPE_UNMETERED)
                .setRequiresBatteryNotLow(true)
                .build();
        return request(context, job);
    }

    /** Cancels the job, as {@link #schedule} schedules it. Called from Rust, through Bridge. */
    static String cancel(Context context) {
        return request(context, null);
    }

    private static String request(Context context, JobInfo job) {
        synchronized (LOCK) {
            if (running) {
                pending = true;
                pendingJob = job;
                return null;
            }
            return apply(context, job);
        }
    }

    /** The job has finished: a request that came meanwhile is carried out. */
    private static void finished(Context context) {
        synchronized (LOCK) {
            running = false;
            if (pending) {
                pending = false;
                String problem = apply(context, pendingJob);
                pendingJob = null;
                if (problem != null) {
                    Log.w(TAG, "Couldn't schedule the daily image's check");
                }
            }
        }
    }

    /** Schedules {@code job}, or cancels the job if it is null. Under {@link #LOCK}. */
    private static String apply(Context context, JobInfo job) {
        try {
            JobScheduler scheduler = context.getSystemService(JobScheduler.class);
            if (scheduler != null) {
                if (job == null) {
                    scheduler.cancel(JOB_ID);
                    return null;
                }
                if (scheduler.schedule(job) == JobScheduler.RESULT_SUCCESS) {
                    return null;
                }
            }
        } catch (RuntimeException e) {
            Log.w(TAG, "Couldn't schedule the daily image: " + e.getClass().getSimpleName());
        }
        return context.getString(R.string.image_job_unavailable);
    }

    /** Runs one check. Returns whether it reached the feed. Never on the main thread. */
    private static boolean check(Context app) {
        try {
            return nativeCheckImage(Bridge.get(app), app.getFilesDir().getPath());
        } catch (RuntimeException e) {
            // A bug in the check; the next start or run tries again.
            Log.w(TAG, "The daily image's check failed: " + e.getMessage());
            return false;
        }
    }

    /**
     * Runs the daily image's check if one is due, and schedules the job for the next one.
     * Returns whether it reached the feed.
     */
    private static native boolean nativeCheckImage(Bridge bridge, String filesDir);
}
