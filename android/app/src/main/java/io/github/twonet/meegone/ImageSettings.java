package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.content.res.Resources;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.RippleDrawable;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.Switch;
import android.widget.TextView;

/**
 * Settings → Lock-screen image (SET-02, ADR 0022, ADR 0029 to ADR 0032), as far as #21, #28 to
 * #31 draw it:
 *
 * <ul>
 *   <li>"Daily lock-screen image", the switch that confirms the feature, before which the lock
 *       screen never changes (WALL-02), disabled with the reason where the device can't set it
 *       (WALL-13);
 *   <li>while it is on, when the image last updated, and the last problem in plain words, such
 *       as "No connection. Will try again later." (WALL-16);
 *   <li>while someone else has changed the lock screen, that daily updates are paused, with
 *       Resume (WALL-14);
 *   <li>while an image is pinned, that daily updates are paused, with Unpin (WALL-17);
 *   <li>where the device didn't keep the image, that it may not appear on the lock screen (SRS
 *       C5);
 *   <li>after three days of failed checks, "Daily image unavailable" with Try again (SRS 7.6);
 *   <li>"Show card in Events", which hides or shows the image-of-the-day card (EVT-12);
 *   <li>"Use mobile data", which lets the daily image download over mobile data (WALL-04);
 *   <li>while it is on, "Choose from the past week", which opens the past week's images
 *       (WALL-17);
 *   <li>"Update now", which reads "Updating…" while it runs (WALL-11);
 *   <li>once the daily image is off, "Reset lock screen to home wallpaper" and "Wallpaper
 *       settings" (WALL-12).
 * </ul>
 *
 * Changes take effect at once (SET-03). Until #34 draws the rest of Settings, it sits below
 * Hidden apps.
 */
final class ImageSettings extends LinearLayout {
    /** What the user did here. */
    interface Listener {
        /** "Daily lock-screen image" was switched. */
        void dailyImage(boolean on);

        /** "Update now", or Resume while daily updates are paused, was tapped. */
        void updateNow();

        /** Unpin was tapped: today's image comes back (WALL-17). */
        void unpin();

        /** "Choose from the past week" was tapped (WALL-17). */
        void pastImages();

        /** "Show card in Events" was switched. */
        void showCard(boolean on);

        /** "Use mobile data" was switched. */
        void useMobileData(boolean on);

        /** Try again was tapped. */
        void tryAgain();

        /** "Reset lock screen to home wallpaper" was tapped. */
        void resetLockScreen();

        /** "Wallpaper settings" was tapped. */
        void openWallpaperSettings();
    }

    /** The daily image's switch, as the frame gives it. */
    static final int DAILY_IMAGE_OFF = 0;
    static final int DAILY_IMAGE_ON = 1;
    static final int DAILY_IMAGE_UNAVAILABLE = 2;
    /** "Update now", as the frame gives it. */
    static final int UPDATE_NOW_OFFERED = 0;
    static final int UPDATE_NOW_RUNNING = 1;
    // The fields of nativeImageStatus, mirrored in src/android.rs: when the image last updated,
    // the last problem, how the checks are going, and "1" while the image is pinned.
    static final int STATUS_FIELDS = 4;
    private static final int STATUS_UPDATED = 0;
    private static final int STATUS_PROBLEM = 1;
    private static final int STATUS_KIND = 2;
    private static final int STATUS_PINNED = 3;
    private static final String RETRYING = "retrying";
    private static final String UNAVAILABLE = "unavailable";

    private static final float GROUP_SP = 14f;
    private static final float TEXT_SP = 16f;
    private static final float NOTE_SP = 14f;
    private static final float ROW_DP = 56f;
    private static final float GAP_DP = 16f;
    /** The smallest a touch target gets (A11Y-02). */
    private static final float TARGET_DP = 48f;
    private static final int MEDIUM = 500;
    /** How faint a control that takes no input is drawn. */
    private static final float DISABLED_ALPHA = 0.38f;

    private final TextView group;
    private final Switch dailyImage;
    private final TextView unavailableReason;
    private final TextView updated;
    private final TextView problem;
    private final LinearLayout paused;
    private final TextView pausedText;
    private final TextView resume;
    private final LinearLayout pinned;
    private final TextView pinnedText;
    private final TextView unpin;
    private final TextView mayNotShow;
    private final LinearLayout unavailable;
    private final TextView unavailableText;
    private final TextView tryAgain;
    private final Switch showCard;
    private final Switch useMobileData;
    private final TextView pastImages;
    private final TextView updateNow;
    private final TextView resetLockScreen;
    private final TextView wallpaperSettings;
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    ImageSettings(Context context, Listener listener) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        setOrientation(VERTICAL);
        setPadding(0, Math.round(GAP_DP * density), 0, 0);

        group = new TextView(context);
        group.setText(R.string.menu_lock_screen_image);
        group.setTextSize(TypedValue.COMPLEX_UNIT_SP, GROUP_SP);
        group.setAccessibilityHeading(true);
        addView(group, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));

        // Each its own label, so screen readers hear "Daily lock-screen image, switch, off"
        // (A11Y-01). A click, unlike a change of state, comes only from the user.
        dailyImage = row(context, R.string.daily_image, density);
        dailyImage.setOnClickListener(view -> listener.dailyImage(dailyImage.isChecked()));
        unavailableReason = note(context);
        // WALL-16: when the image last updated, right below the switch.
        updated = note(context);

        // What needs the user's answer comes next, where it is seen, and only while it applies.
        pausedText = new TextView(context);
        pausedText.setText(R.string.image_paused);
        resume = new TextView(context);
        paused = withAction(context, pausedText, resume, R.string.image_resume, density);
        resume.setOnClickListener(view -> listener.updateNow());
        pinnedText = new TextView(context);
        pinnedText.setText(R.string.image_pinned);
        unpin = new TextView(context);
        pinned = withAction(context, pinnedText, unpin, R.string.image_unpin, density);
        unpin.setOnClickListener(view -> listener.unpin());
        mayNotShow = note(context);
        mayNotShow.setText(R.string.image_may_not_show);
        unavailableText = new TextView(context);
        unavailableText.setText(R.string.image_unavailable);
        tryAgain = new TextView(context);
        unavailable = withAction(context, unavailableText, tryAgain, R.string.image_try_again,
                density);
        tryAgain.setOnClickListener(view -> listener.tryAgain());
        // The last problem, below "Daily image unavailable" when that shows (WALL-16).
        problem = note(context);

        showCard = row(context, R.string.show_image_card, density);
        showCard.setOnClickListener(view -> listener.showCard(showCard.isChecked()));
        useMobileData = row(context, R.string.use_mobile_data, density);
        useMobileData.setOnClickListener(
                view -> listener.useMobileData(useMobileData.isChecked()));

        pastImages = action(context, R.string.image_choose_past_week, density);
        pastImages.setOnClickListener(view -> listener.pastImages());
        updateNow = action(context, R.string.image_update_now, density);
        updateNow.setOnClickListener(view -> listener.updateNow());
        resetLockScreen = action(context, R.string.reset_lock_screen, density);
        resetLockScreen.setOnClickListener(view -> listener.resetLockScreen());
        wallpaperSettings = action(context, R.string.wallpaper_settings, density);
        wallpaperSettings.setOnClickListener(view -> listener.openWallpaperSettings());
    }

    private Switch row(Context context, int label, float density) {
        Switch row = new Switch(context);
        row.setText(label);
        row.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        row.setMinHeight(Math.round(ROW_DP * density));
        addView(row, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        return row;
    }

    /** A line of smaller text below the row it explains, hidden until it applies. */
    private TextView note(Context context) {
        TextView note = new TextView(context);
        note.setTextSize(TypedValue.COMPLEX_UNIT_SP, NOTE_SP);
        note.setVisibility(GONE);
        addView(note, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        return note;
    }

    /** A whole row that does something when tapped, such as "Update now". */
    private TextView action(Context context, int label, float density) {
        TextView action = new TextView(context);
        action.setText(label);
        action.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        action.setGravity(Gravity.CENTER_VERTICAL);
        action.setMinHeight(Math.round(ROW_DP * density));
        action.setVisibility(GONE);
        addView(action, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        return action;
    }

    /** A message with a button at its end, such as Try again, hidden until it applies. */
    private LinearLayout withAction(
            Context context, TextView text, TextView button, int label, float density) {
        LinearLayout row = new LinearLayout(context);
        row.setOrientation(HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setMinimumHeight(Math.round(ROW_DP * density));
        row.setVisibility(GONE);
        text.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        row.addView(text, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f));
        button.setText(label);
        button.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        button.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
        button.setGravity(Gravity.CENTER);
        int target = Math.round(TARGET_DP * density);
        button.setMinHeight(target);
        button.setMinWidth(target);
        int gap = Math.round(GAP_DP * density);
        button.setPadding(gap, 0, gap, 0);
        row.addView(button, new LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(row, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        return row;
    }

    /**
     * Shows the section as the Rust app says: the daily image's switch ({@code DAILY_IMAGE_*})
     * and, while it is unavailable, {@code reason}; "Update now" ({@code UPDATE_NOW_*}, or
     * negative while it isn't offered); and which messages and actions apply.
     */
    void show(
            int daily,
            String reason,
            int update,
            boolean pausedOn,
            boolean mayNotShowOn,
            boolean imageUnavailable,
            boolean showCardOn,
            boolean useMobileDataOn,
            boolean offActions) {
        boolean on = daily == DAILY_IMAGE_ON;
        if (dailyImage.isChecked() != on) {
            dailyImage.setChecked(on);
        }
        boolean available = daily != DAILY_IMAGE_UNAVAILABLE;
        dailyImage.setEnabled(available);
        dailyImage.setAlpha(available ? 1f : DISABLED_ALPHA);
        boolean withReason = daily == DAILY_IMAGE_UNAVAILABLE && reason != null;
        unavailableReason.setText(withReason ? reason : null);
        unavailableReason.setVisibility(withReason ? VISIBLE : GONE);
        paused.setVisibility(pausedOn ? VISIBLE : GONE);
        mayNotShow.setVisibility(mayNotShowOn ? VISIBLE : GONE);
        unavailable.setVisibility(imageUnavailable ? VISIBLE : GONE);
        if (showCard.isChecked() != showCardOn) {
            showCard.setChecked(showCardOn);
        }
        if (useMobileData.isChecked() != useMobileDataOn) {
            useMobileData.setChecked(useMobileDataOn);
        }
        pastImages.setVisibility(on ? VISIBLE : GONE);
        updateNow.setVisibility(update >= 0 ? VISIBLE : GONE);
        boolean running = update == UPDATE_NOW_RUNNING;
        updateNow.setText(running ? R.string.image_updating : R.string.image_update_now);
        // Resume and Unpin are Update now too (WALL-14, WALL-17).
        for (TextView button : new TextView[] {updateNow, resume, unpin}) {
            button.setEnabled(!running);
            button.setTextColor(running ? dim : accent);
        }
        resetLockScreen.setVisibility(offActions ? VISIBLE : GONE);
        wallpaperSettings.setVisibility(offActions ? VISIBLE : GONE);
    }

    /**
     * Shows the status line as the Rust app says, in {@link #STATUS_FIELDS} fields, or hides it
     * with null while the daily image is off (WALL-16).
     */
    void showStatus(String[] status) {
        boolean shown = status != null && status.length >= STATUS_FIELDS;
        Resources res = getResources();
        updated.setText(shown ? updatedLine(res, status) : null);
        updated.setVisibility(shown ? VISIBLE : GONE);
        boolean pinnedOn = shown && pinned(status);
        pinned.setVisibility(pinnedOn ? VISIBLE : GONE);
        // While pinned, the checks wait, so their last problem isn't news (WALL-17).
        String line = shown && !pinnedOn ? problemLine(res, status) : null;
        problem.setText(line);
        problem.setVisibility(line != null ? VISIBLE : GONE);
    }

    /** Whether the status says the image is pinned, so daily updates wait (WALL-17). */
    static boolean pinned(String[] status) {
        return "1".equals(status[STATUS_PINNED]);
    }

    /** When the image last updated, in words: "Last updated 19 November, 07:00" (WALL-16). */
    static String updatedLine(Resources res, String[] status) {
        String when = status[STATUS_UPDATED];
        return when.isEmpty()
                ? res.getString(R.string.image_not_updated_yet)
                : res.getString(R.string.image_last_updated, when);
    }

    /**
     * The last problem in words, "No connection. Will try again later." while the checks retry,
     * or just "No connection." below "Daily image unavailable"; null while nothing fails
     * (WALL-16, SRS 7.6).
     */
    static String problemLine(Resources res, String[] status) {
        String problem = status[STATUS_PROBLEM];
        if (problem.isEmpty()) {
            return null;
        }
        switch (status[STATUS_KIND]) {
            case RETRYING:
                return res.getString(R.string.image_will_try_again, problem);
            case UNAVAILABLE:
                return res.getString(R.string.image_last_problem, problem);
            default:
                return null;
        }
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int accent) {
        if (foreground == this.foreground && dim == this.dim && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.accent = accent;
        group.setTextColor(dim);
        unavailableReason.setTextColor(dim);
        updated.setTextColor(dim);
        problem.setTextColor(foreground);
        mayNotShow.setTextColor(dim);
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        for (TextView text : new TextView[] {pausedText, pinnedText, unavailableText}) {
            text.setTextColor(foreground);
        }
        for (TextView button : new TextView[] {
            resume, unpin, tryAgain, pastImages, updateNow, resetLockScreen, wallpaperSettings
        }) {
            button.setTextColor(button.isEnabled() ? accent : dim);
            button.setBackground(
                    new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
        }
        int[][] states = {{android.R.attr.state_checked}, {}};
        for (Switch row : new Switch[] {dailyImage, showCard, useMobileData}) {
            row.setTextColor(foreground);
            row.setThumbTintList(new ColorStateList(states, new int[] {accent, foreground}));
            row.setTrackTintList(new ColorStateList(states, new int[] {accent, dim}));
        }
    }
}
