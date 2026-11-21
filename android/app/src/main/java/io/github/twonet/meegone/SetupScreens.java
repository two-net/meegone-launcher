package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.Resources;
import android.graphics.Bitmap;
import android.graphics.Typeface;
import android.os.Handler;
import android.os.Looper;
import android.util.TypedValue;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.Switch;
import android.widget.TextView;

/**
 * First-run setup (ONB-01, ADR 0036). It covers the home views, in their background colour,
 * until it is finished, one screen at a time:
 *
 * <ol>
 *   <li>Welcome, with Continue;
 *   <li>making MeeGone the default home app with the system's dialog, with Make default and Not
 *       now (HOME-01);
 *   <li>the daily lock-screen image: today's image when online, with its title and attribution
 *       (LEGAL-03), the switch, on until the user turns it off, Region, and Continue, which is
 *       what applies the switch (WALL-02);
 *   <li>notification access for Events and usage access for Recent, each after one plain
 *       sentence saying what is read and why, with Allow and Not now (ONB-02).
 * </ol>
 *
 * <p>It never asks about the theme (ONB-05). The Rust app decides which screen shows and moves
 * on, skipping what is already done; Back returns to the screen before. Each screen is a pane
 * with its own title, so screen readers announce it. It scrolls, so nothing overlaps at large
 * text sizes (A11Y-04). Every text the feed sent is set as plain text (SEC-02).
 */
final class SetupScreens extends Scroll {
    // The screens, as setup_step_code in src/android.rs numbers them; -1 while none shows.
    static final int WELCOME = 0;
    static final int DEFAULT_HOME = 1;
    static final int LOCK_SCREEN_IMAGE = 2;
    static final int ACCESS = 3;
    private static final int STEPS = 4;
    // The image screen's switch, as the frame's slot 152 gives it.
    static final int SWITCH_OFF = 0;
    static final int SWITCH_ON = 1;
    static final int SWITCH_UNAVAILABLE = 2;
    // The accesses put off with Not now, as bits of the frame's slot 151.
    static final int NOTIFICATIONS_PUT_OFF = 1;
    static final int USAGE_PUT_OFF = 2;
    // The fields of nativeSetupImage, mirrored in src/android.rs.
    private static final int TITLE = 0;
    private static final int ATTRIBUTION = 1;
    private static final int PREVIEW = 2;
    private static final int LOADING = 3;
    private static final int IMAGE_FIELDS = 4;
    private static final String SET = "1";
    /** The access items on the last screen, numbered as {@link AccessSettings} numbers them. */
    private static final int[] ACCESS_ITEMS = {AccessSettings.NOTIFICATIONS, AccessSettings.USAGE};
    private static final int[] ACCESS_TITLES = {
        R.string.access_notifications, R.string.access_usage,
    };
    private static final int[] ACCESS_WHY = {
        R.string.setup_notifications_why, R.string.setup_usage_why,
    };
    private static final int[] ACCESS_ALLOWED = {
        R.string.access_notifications_on, R.string.access_usage_on,
    };

    private static final float TITLE_SP = 28f;
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    /** The side margins (SRS 5.2). */
    private static final float MARGIN_DP = 16f;
    private static final float GAP_DP = 16f;
    /** A preview is 5:3, the feed's, and at most this tall. */
    private static final float MAX_PREVIEW_DP = 240f;

    /** What the user did here. */
    interface Listener {
        /** Continue, on Welcome or the image screen. */
        void next();

        /** Make default or Allow on {@code item}, numbered as {@link AccessSettings} does. */
        void allow(int item);

        /** Not now on {@code item}, numbered as {@link AccessSettings} does. */
        void notNow(int item);

        /** The image screen's switch was tapped. */
        void imageSwitched(boolean on);

        /** Region was tapped: its choices open. */
        void region();

        /** Today's preview as read at {@code revision}, or null. */
        byte[] preview(int revision);
    }

    private final Listener listener;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final float density;
    private final LinearLayout[] steps = new LinearLayout[STEPS];
    private final TextView[] titles = new TextView[STEPS];
    private final TextView[] bodies = new TextView[STEPS];
    private final TextView welcomeNext;
    private final TextView homeNotNow;
    private final TextView makeDefault;
    private final TextView homeInstructions;
    private final FrameLayout previewBox;
    private final ImageView previewImage;
    private final TextView loading;
    private final TextView imageTitle;
    private final TextView imageAttribution;
    private final Switch imageSwitch;
    private final TextView unavailable;
    private final LinearLayout regionRow;
    private final TextView regionTitle;
    private final TextView regionValue;
    private final TextView imageNext;
    private final TextView[] accessTitles = new TextView[ACCESS_ITEMS.length];
    private final TextView[] accessWhy = new TextView[ACCESS_ITEMS.length];
    private final TextView[] accessStatus = new TextView[ACCESS_ITEMS.length];
    private final ButtonRow[] accessButtons = new ButtonRow[ACCESS_ITEMS.length];
    private final TextView[] accessNotNow = new TextView[ACCESS_ITEMS.length];
    private final TextView[] accessAllow = new TextView[ACCESS_ITEMS.length];
    private final TextView[] accessInstructions = new TextView[ACCESS_ITEMS.length];
    private int step = -1;
    /** The image screen's revision as last read, or -1. */
    private int imageRevision = -1;
    private boolean decoding;
    private int foreground = -1;
    private int dim = -1;
    private int background = -1;
    private int accent = -1;

    SetupScreens(Context context, Listener listener) {
        super(context);
        this.listener = listener;
        density = getResources().getDisplayMetrics().density;
        // Touches on it stay there.
        setClickable(true);
        A11y.notAButton(this);
        setFillViewport(true);
        FrameLayout content = new FrameLayout(context);
        int margin = dp(MARGIN_DP);
        content.setPadding(margin, dp(48), margin, dp(24));
        addView(content, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        int[] stepTitles = {
            R.string.setup_welcome_title, R.string.setup_default_home_title, R.string.daily_image,
            R.string.setup_access_title,
        };
        int[] stepBodies = {
            R.string.setup_welcome_body, R.string.setup_default_home_body,
            R.string.setup_image_body, R.string.setup_access_body,
        };
        for (int at = 0; at < STEPS; at++) {
            LinearLayout screen = new LinearLayout(context);
            screen.setOrientation(LinearLayout.VERTICAL);
            screen.setVisibility(GONE);
            // Screen readers announce each screen as it appears (A11Y-08).
            screen.setAccessibilityPaneTitle(getResources().getString(stepTitles[at]));
            titles[at] = text(screen, TITLE_SP, 0);
            titles[at].setText(stepTitles[at]);
            titles[at].setTypeface(Typeface.create("sans-serif-light", Typeface.NORMAL));
            titles[at].setAccessibilityHeading(true);
            bodies[at] = text(screen, BODY_SP, dp(GAP_DP));
            bodies[at].setText(stepBodies[at]);
            steps[at] = screen;
            content.addView(screen, new FrameLayout.LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        }

        // 1. Welcome.
        welcomeNext = buttons(steps[WELCOME], null, R.string.setup_continue)[1];
        welcomeNext.setOnClickListener(view -> listener.next());

        // 2. The default home app.
        homeInstructions = SettingsRows.note(steps[DEFAULT_HOME]);
        TextView[] home = buttons(steps[DEFAULT_HOME], R.string.setup_not_now,
                R.string.access_make_default);
        homeNotNow = home[0];
        makeDefault = home[1];
        homeNotNow.setOnClickListener(view -> listener.notNow(AccessSettings.DEFAULT_HOME));
        makeDefault.setOnClickListener(view -> listener.allow(AccessSettings.DEFAULT_HOME));

        // 3. The daily lock-screen image.
        LinearLayout image = steps[LOCK_SCREEN_IMAGE];
        previewBox = new FrameLayout(context) {
            @Override
            protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
                int width = MeasureSpec.getSize(widthMeasureSpec);
                super.onMeasure(MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY),
                        MeasureSpec.makeMeasureSpec(previewHeight(width), MeasureSpec.EXACTLY));
            }
        };
        // The title says what it shows, so screen readers skip it.
        previewBox.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
        previewImage = new ImageView(context);
        previewImage.setScaleType(ImageView.ScaleType.CENTER_CROP);
        previewBox.addView(previewImage, new FrameLayout.LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        // Until today's preview is decoded, or offline, it takes no room.
        previewBox.setVisibility(GONE);
        LinearLayout.LayoutParams boxParams = matchWidth();
        boxParams.topMargin = dp(GAP_DP);
        image.addView(previewBox, boxParams);
        loading = text(image, BODY_SP, dp(GAP_DP));
        loading.setText(R.string.image_past_loading);
        loading.setVisibility(GONE);
        imageTitle = text(image, BODY_SP + 3, dp(GAP_DP / 2));
        // LEGAL-03: in full, never cut short.
        imageAttribution = text(image, BODY_SP, dp(GAP_DP / 4));
        imageTitle.setVisibility(GONE);
        imageAttribution.setVisibility(GONE);
        LinearLayout options = new LinearLayout(context);
        SettingsRows.startGroup(options);
        image.addView(options, matchWidth());
        imageSwitch = SettingsRows.switchRow(options, R.string.daily_image);
        imageSwitch.setOnClickListener(view -> listener.imageSwitched(imageSwitch.isChecked()));
        unavailable = SettingsRows.note(options);
        regionTitle = new TextView(context);
        regionValue = new TextView(context);
        regionRow = SettingsRows.titled(options, regionTitle, regionValue, R.string.image_region,
                listener::region);
        imageNext = buttons(image, null, R.string.setup_continue)[1];
        imageNext.setOnClickListener(view -> listener.next());

        // 4. Access, each after its one sentence (ONB-02).
        LinearLayout access = steps[ACCESS];
        Resources res = getResources();
        for (int at = 0; at < ACCESS_ITEMS.length; at++) {
            int item = ACCESS_ITEMS[at];
            accessTitles[at] = text(access, SettingsRows.TEXT_SP, dp(GAP_DP * 1.5f));
            accessTitles[at].setText(ACCESS_TITLES[at]);
            accessWhy[at] = text(access, BODY_SP, dp(GAP_DP / 4));
            accessWhy[at].setText(ACCESS_WHY[at]);
            accessStatus[at] = text(access, BODY_SP, dp(GAP_DP / 4));
            accessStatus[at].setVisibility(GONE);
            accessInstructions[at] = SettingsRows.note(access);
            TextView[] answers = buttons(access, R.string.setup_not_now, R.string.access_allow);
            accessButtons[at] = (ButtonRow) answers[1].getParent();
            accessNotNow[at] = answers[0];
            accessAllow[at] = answers[1];
            String title = res.getString(ACCESS_TITLES[at]);
            // "Notification access: Allow" (A11Y-01).
            accessNotNow[at].setContentDescription(res.getString(
                    R.string.access_button_description, title, res.getString(R.string.setup_not_now)));
            accessAllow[at].setContentDescription(res.getString(
                    R.string.access_button_description, title, res.getString(R.string.access_allow)));
            accessNotNow[at].setOnClickListener(view -> listener.notNow(item));
            accessAllow[at].setOnClickListener(view -> listener.allow(item));
        }
    }

    /**
     * Shows screen {@code step}, or none with -1, at its top. Returns whether it changed, so the
     * host can close a dialog that belonged to the screen before.
     */
    boolean show(int step) {
        if (step == this.step) {
            return false;
        }
        this.step = step;
        setVisibility(step >= 0 ? VISIBLE : GONE);
        for (int at = 0; at < STEPS; at++) {
            steps[at].setVisibility(at == step ? VISIBLE : GONE);
        }
        scrollTo(0, 0);
        if (step != LOCK_SCREEN_IMAGE) {
            forgetImage();
        }
        return true;
    }

    /** The written instructions below Make default, or none with null (SRS 8.1). */
    void showDefaultHome(String instructions) {
        homeInstructions.setText(instructions);
        homeInstructions.setVisibility(instructions != null ? VISIBLE : GONE);
    }

    /**
     * The image screen's switch, as the frame's slot 152 gives it, with {@code reason} below it
     * while unavailable (WALL-13), and the region chosen by name. With -1, the lock-screen image
     * isn't working, and the screen offers Continue alone (ARCH-09).
     */
    void showImage(int switchState, String reason, String region) {
        boolean working = switchState >= 0;
        imageSwitch.setVisibility(working ? VISIBLE : GONE);
        regionRow.setVisibility(working ? VISIBLE : GONE);
        imageSwitch.setChecked(switchState == SWITCH_ON);
        boolean usable = switchState == SWITCH_ON || switchState == SWITCH_OFF;
        imageSwitch.setEnabled(usable);
        imageSwitch.setAlpha(usable ? 1f : SettingsRows.DISABLED_ALPHA);
        unavailable.setText(reason);
        unavailable.setVisibility(reason != null ? VISIBLE : GONE);
        regionValue.setText(region);
        if (!working) {
            forgetImage();
        }
    }

    /** Today's image as read at {@code revision}: its title, attribution and preview. */
    void fillImage(int revision, String[] fields) {
        if (fields == null || fields.length < IMAGE_FIELDS || revision == imageRevision) {
            return;
        }
        imageRevision = revision;
        String title = fields[TITLE];
        imageTitle.setText(title);
        imageAttribution.setText(fields[ATTRIBUTION]);
        imageTitle.setVisibility(title.isEmpty() ? GONE : VISIBLE);
        imageAttribution.setVisibility(title.isEmpty() ? GONE : VISIBLE);
        loading.setVisibility(SET.equals(fields[LOADING]) && title.isEmpty() ? VISIBLE : GONE);
        if (SET.equals(fields[PREVIEW])) {
            decode(revision);
        } else {
            previewImage.setImageDrawable(null);
            previewBox.setVisibility(GONE);
        }
    }

    /**
     * The access screen: each item allowed, put off with Not now, or still asked, and the
     * written instructions below the item at {@code instructed}, numbered as {@link
     * AccessSettings} does, or none.
     */
    void showAccess(boolean notifications, boolean usage, int putOff, int instructed,
            String instructions) {
        boolean[] allowed = {notifications, usage};
        boolean[] declined = {
            (putOff & NOTIFICATIONS_PUT_OFF) != 0, (putOff & USAGE_PUT_OFF) != 0,
        };
        for (int at = 0; at < ACCESS_ITEMS.length; at++) {
            boolean answered = allowed[at] || declined[at];
            accessButtons[at].setVisibility(answered ? GONE : VISIBLE);
            accessStatus[at].setVisibility(answered ? VISIBLE : GONE);
            accessStatus[at].setText(allowed[at] ? ACCESS_ALLOWED[at] : R.string.setup_put_off);
            boolean here = ACCESS_ITEMS[at] == instructed && instructions != null && !answered;
            accessInstructions[at].setText(here ? instructions : null);
            accessInstructions[at].setVisibility(here ? VISIBLE : GONE);
        }
    }

    /** The theme's colours (THEME-03). */
    void colour(int background, int foreground, int dim, int accent) {
        if (background == this.background && foreground == this.foreground && dim == this.dim
                && accent == this.accent) {
            return;
        }
        this.background = background;
        this.foreground = foreground;
        this.dim = dim;
        this.accent = accent;
        setBackgroundColor(background);
        Themed.scrolling(this, dim);
        for (int at = 0; at < STEPS; at++) {
            titles[at].setTextColor(foreground);
            bodies[at].setTextColor(dim);
        }
        for (TextView button : new TextView[] {welcomeNext, homeNotNow, makeDefault, imageNext}) {
            SettingsRows.colourButton(button, foreground, dim, accent);
        }
        homeInstructions.setTextColor(foreground);
        loading.setTextColor(dim);
        imageTitle.setTextColor(foreground);
        imageAttribution.setTextColor(dim);
        SettingsRows.colourSwitch(imageSwitch, foreground, dim, accent);
        unavailable.setTextColor(dim);
        regionTitle.setTextColor(foreground);
        regionValue.setTextColor(dim);
        regionRow.setBackground(SettingsRows.ripple(foreground));
        for (int at = 0; at < ACCESS_ITEMS.length; at++) {
            accessTitles[at].setTextColor(foreground);
            accessWhy[at].setTextColor(dim);
            accessStatus[at].setTextColor(foreground);
            accessInstructions[at].setTextColor(foreground);
            SettingsRows.colourButton(accessNotNow[at], foreground, dim, accent);
            SettingsRows.colourButton(accessAllow[at], foreground, dim, accent);
        }
    }

    /** Decodes today's preview, as large as its box, behind the scenes. */
    private void decode(int revision) {
        if (decoding) {
            return;
        }
        byte[] jpeg = listener.preview(revision);
        if (jpeg == null) {
            return;
        }
        decoding = true;
        int width = steps[LOCK_SCREEN_IMAGE].getWidth();
        if (width <= 0) {
            // Not laid out yet: as wide as the screen.
            width = Math.max(1, getResources().getDisplayMetrics().widthPixels - 2 * dp(MARGIN_DP));
        }
        int boxWidth = width;
        int boxHeight = previewHeight(width);
        ImageCard.DECODER.execute(() -> {
            Bitmap bitmap = ImageCard.decode(jpeg, boxWidth, boxHeight);
            main.post(() -> decoded(revision, bitmap));
        });
    }

    private void decoded(int revision, Bitmap bitmap) {
        decoding = false;
        if (bitmap == null || revision != imageRevision || step != LOCK_SCREEN_IMAGE) {
            // Unreadable, or the screen changed meanwhile.
            return;
        }
        previewImage.setImageBitmap(bitmap);
        previewBox.setVisibility(VISIBLE);
    }

    /** Lets today's preview go once the image screen hides (PERF-07). */
    private void forgetImage() {
        imageRevision = -1;
        previewImage.setImageDrawable(null);
        previewBox.setVisibility(GONE);
    }

    /** A preview's height for {@code width}: 5:3, and at most {@link #MAX_PREVIEW_DP}. */
    private int previewHeight(int width) {
        return Math.max(1, Math.min(width * 3 / 5, dp(MAX_PREVIEW_DP)));
    }

    /** A line of text, {@code top} below the one before. */
    private TextView text(LinearLayout parent, float sp, int top) {
        TextView text = new TextView(parent.getContext());
        text.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp);
        // SEC-02: what the feed sent is only ever text.
        text.setLinksClickable(false);
        LinearLayout.LayoutParams params = matchWidth();
        params.topMargin = top;
        parent.addView(text, params);
        return text;
    }

    /**
     * A row of buttons at the screen's end edge: {@code second}, the step forward, last, after
     * {@code first} if there is one, or one above the other at a large font size (A11Y-04).
     * Returns both, the first null if there is none.
     */
    private TextView[] buttons(LinearLayout parent, Integer first, int second) {
        ButtonRow row = new ButtonRow(parent.getContext());
        LinearLayout.LayoutParams params = matchWidth();
        params.topMargin = dp(GAP_DP);
        parent.addView(row, params);
        TextView[] made = new TextView[2];
        if (first != null) {
            made[0] = new TextView(parent.getContext());
            SettingsRows.button(made[0], first);
            row.addView(made[0]);
        }
        made[1] = new TextView(parent.getContext());
        SettingsRows.button(made[1], second);
        row.addView(made[1]);
        return made;
    }

    private int dp(float value) {
        return Math.round(value * density);
    }

    private static LinearLayout.LayoutParams matchWidth() {
        return new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT);
    }
}
