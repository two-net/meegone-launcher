package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.Resources;
import android.graphics.Bitmap;
import android.os.Handler;
import android.os.Looper;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.io.File;

/**
 * The image details screen (WALL-15, ADR 0031). It covers the home views in the surface colour:
 * today's lock-screen image across the top, centre-cropped, then its title, its attribution as
 * the source provides it (LEGAL-03), its date and region, the update status that Settings shows
 * too (WALL-16), and the rows More info, Unpin while the image is pinned, Update now, Past week
 * and Settings (WALL-17). It scrolls, so nothing overlaps at large text sizes (A11Y-04).
 *
 * <p>The Rust app decides what it shows; this view reads it again only when the frame's revision
 * changes. Every text is set as plain text: never as HTML, and with no links made clickable
 * (SEC-02). More info shows only while the image has a link and a browser can open it.
 */
final class ImageDetails extends Scroll {
    // The fields of nativeImageDetails, mirrored in src/android.rs: the title, the attribution,
    // the date, the region's tag, the image's file relative to the app's files folder, the
    // image's key, "1" while More info is offered, and "1" while the image is pinned.
    static final int FIELDS = 8;
    private static final int TITLE = 0;
    private static final int ATTRIBUTION = 1;
    private static final int DATE = 2;
    private static final int REGION = 3;
    private static final int PATH = 4;
    private static final int KEY = 5;
    private static final int MORE_INFO = 6;
    private static final int PINNED = 7;
    private static final String OFFERED = "1";

    private static final float TITLE_SP = 24f;
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    private static final float ROW_DP = 56f;
    /** The side margins (SRS 5.2). */
    private static final float MARGIN_DP = 16f;
    private static final float GAP_DP = 8f;
    /** The image is at most 4:3 of the width tall, and at most this share of the screen. */
    private static final float MAX_IMAGE_SHARE = 0.55f;
    /** How faint a row that takes no input is drawn. */
    private static final float DISABLED_ALPHA = 0.38f;

    /** What the user did here. */
    interface Listener {
        /** More info was tapped (WALL-15). */
        void moreInfo();

        /** Update now was tapped (WALL-11). */
        void updateNow();

        /** Unpin was tapped: today's image comes back (WALL-17). */
        void unpin();

        /** Past week was tapped (WALL-17). */
        void pastImages();

        /** Settings was tapped. */
        void settings();
    }

    /** The folder MeeGone's files are in, which the image's path is relative to. */
    private final File files;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final float density;
    private final Photo photo;
    private final ImageView image;
    private final TextView title;
    private final TextView attribution;
    private final TextView dateRegion;
    private final TextView updated;
    private final TextView problem;
    private final TextView moreInfo;
    private final TextView unpin;
    private final TextView updateNow;
    private final TextView pastImages;
    private final TextView settings;
    /** The image as of the last fill: its file and its key. */
    private String path = "";
    private String key = "";
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int accent = -1;

    ImageDetails(Context context, Listener listener) {
        super(context);
        files = context.getFilesDir();
        density = getResources().getDisplayMetrics().density;
        // Touches on it stay there.
        setClickable(true);
        A11y.notAButton(this);
        setFillViewport(true);
        setAccessibilityPaneTitle(getResources().getString(R.string.image_of_the_day));

        LinearLayout content = new LinearLayout(context);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(0, 0, 0, dp(MARGIN_DP));

        photo = new Photo(context);
        image = new ImageView(context);
        image.setScaleType(ImageView.ScaleType.CENTER_CROP);
        photo.addView(image, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));
        content.addView(photo, matchWidth());

        title = text(context, content, TITLE_SP, dp(MARGIN_DP));
        title.setAccessibilityHeading(true);
        // LEGAL-03: in full, never cut short.
        attribution = text(context, content, BODY_SP, dp(GAP_DP));
        dateRegion = text(context, content, BODY_SP, dp(GAP_DP / 2));
        // WALL-16: as Settings says it.
        updated = text(context, content, BODY_SP, dp(MARGIN_DP));
        problem = text(context, content, BODY_SP, dp(GAP_DP / 2));
        problem.setVisibility(GONE);

        LinearLayout actions = new LinearLayout(context);
        actions.setOrientation(LinearLayout.VERTICAL);
        actions.setPadding(0, dp(GAP_DP), 0, 0);
        moreInfo = action(context, actions, R.string.image_more_info);
        moreInfo.setOnClickListener(view -> listener.moreInfo());
        moreInfo.setVisibility(GONE);
        unpin = action(context, actions, R.string.image_unpin);
        unpin.setOnClickListener(view -> listener.unpin());
        unpin.setVisibility(GONE);
        updateNow = action(context, actions, R.string.image_update_now);
        updateNow.setOnClickListener(view -> listener.updateNow());
        pastImages = action(context, actions, R.string.image_past_week);
        pastImages.setOnClickListener(view -> listener.pastImages());
        settings = action(context, actions, R.string.image_open_settings);
        settings.setOnClickListener(view -> listener.settings());
        content.addView(actions, matchWidth());

        addView(content, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
    }

    /** A line of text at the side margins, {@code top} below the one before. */
    private TextView text(Context context, LinearLayout parent, float sp, int top) {
        TextView text = new TextView(context);
        text.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp);
        // SEC-02: what the feed sent is only ever text.
        text.setLinksClickable(false);
        LinearLayout.LayoutParams params = matchWidth();
        int margin = dp(MARGIN_DP);
        params.setMargins(margin, top, margin, 0);
        parent.addView(text, params);
        return text;
    }

    /** A whole row that does something when tapped, as in Settings (A11Y-02). */
    private TextView action(Context context, LinearLayout parent, int label) {
        TextView action = new TextView(context);
        action.setText(label);
        action.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP + 1);
        action.setGravity(Gravity.CENTER_VERTICAL);
        action.setMinHeight(dp(ROW_DP));
        int margin = dp(MARGIN_DP);
        action.setPadding(margin, 0, margin, 0);
        parent.addView(action, matchWidth());
        return action;
    }

    /** Shows the screen from the top, as it opens. */
    void opened() {
        scrollTo(0, 0);
    }

    /** Shows the image and its details, in {@link #FIELDS} fields. */
    void fill(String[] fields) {
        if (fields == null || fields.length < FIELDS) {
            return;
        }
        title.setText(fields[TITLE]);
        attribution.setText(fields[ATTRIBUTION]);
        String line = dateAndRegion(getResources(), fields[DATE], regionName(fields[REGION]));
        dateRegion.setText(line);
        dateRegion.setVisibility(line.isEmpty() ? GONE : VISIBLE);
        moreInfo.setVisibility(OFFERED.equals(fields[MORE_INFO]) ? VISIBLE : GONE);
        unpin.setVisibility(OFFERED.equals(fields[PINNED]) ? VISIBLE : GONE);
        if (!fields[KEY].equals(key)) {
            path = fields[PATH];
            key = fields[KEY];
            decode();
        }
    }

    /**
     * Shows the status line, in {@link ImageSettings#STATUS_FIELDS} fields, as Settings does, or
     * hides it with null (WALL-16).
     */
    void showStatus(String[] status) {
        boolean shown = status != null && status.length >= ImageSettings.STATUS_FIELDS;
        Resources res = getResources();
        updated.setText(shown ? ImageSettings.updatedLine(res, status) : null);
        updated.setVisibility(shown ? VISIBLE : GONE);
        // While pinned, that is what the line says, in place of the checks (WALL-17).
        String line = !shown
                ? null
                : ImageSettings.pinned(status)
                        ? res.getString(R.string.image_pinned)
                        : ImageSettings.problemLine(res, status);
        problem.setText(line);
        problem.setVisibility(line != null ? VISIBLE : GONE);
    }

    /**
     * Update now as the frame gives it, {@code ImageSettings.UPDATE_NOW_*}, or negative while it
     * isn't offered (WALL-11).
     */
    void showUpdateNow(int update) {
        updateNow.setVisibility(update >= 0 ? VISIBLE : GONE);
        boolean running = update == ImageSettings.UPDATE_NOW_RUNNING;
        updateNow.setText(running ? R.string.image_updating : R.string.image_update_now);
        // Unpin is Update now too (WALL-17).
        for (TextView row : new TextView[] {updateNow, unpin}) {
            if (row.isEnabled() == running) {
                row.setEnabled(!running);
                row.setTextColor(running ? dim : accent);
            }
        }
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int surface, int accent) {
        if (foreground == this.foreground && dim == this.dim && surface == this.surface
                && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.surface = surface;
        this.accent = accent;
        setBackgroundColor(surface);
        Themed.scrolling(this, dim);
        title.setTextColor(foreground);
        attribution.setTextColor(dim);
        dateRegion.setTextColor(dim);
        updated.setTextColor(dim);
        problem.setTextColor(foreground);
        for (TextView row : new TextView[] {moreInfo, unpin, updateNow, pastImages, settings}) {
            row.setTextColor(row.isEnabled() ? accent : dim);
            row.setBackground(SettingsRows.ripple(foreground));
        }
    }

    /** "15 November · English (United Kingdom)", or whichever of the two there is. */
    private static String dateAndRegion(Resources res, String date, String region) {
        if (date.isEmpty() || region.isEmpty()) {
            return date.isEmpty() ? region : date;
        }
        return res.getString(R.string.image_date_region, date, region);
    }

    /**
     * The feed region's tag, such as {@code en-GB}, named in the device's language as it is
     * after the date (L10N-04).
     */
    private static String regionName(String tag) {
        return Regions.nameInSentence(tag);
    }

    /** Decodes the image as large as its box is now, behind the scenes. */
    private void decode() {
        String askedPath = path;
        String askedKey = key;
        int width = photo.getWidth();
        int height = photo.getHeight();
        if (width <= 0 || height <= 0) {
            // Not laid out yet: as wide as the screen.
            width = Math.max(1, getResources().getDisplayMetrics().widthPixels);
            height = photo.heightFor(width);
        }
        int boxWidth = width;
        int boxHeight = height;
        ImageCard.DECODER.execute(() -> {
            Bitmap bitmap = ImageCard.decode(new File(files, askedPath), boxWidth, boxHeight);
            main.post(() -> decoded(askedKey, bitmap));
        });
    }

    private void decoded(String askedKey, Bitmap bitmap) {
        if (!askedKey.equals(key)) {
            // Another image came meanwhile; its own decode follows.
            return;
        }
        // A file that is missing or isn't an image leaves no empty band.
        image.setImageBitmap(bitmap);
        photo.setVisibility(bitmap != null ? VISIBLE : GONE);
    }

    private int dp(float value) {
        return Math.round(value * density);
    }

    private static LinearLayout.LayoutParams matchWidth() {
        return new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT);
    }

    /**
     * The image's box: 4:3 of its width tall, and at most {@link #MAX_IMAGE_SHARE} of the screen.
     * The title and the attribution say what it shows, so screen readers skip it. When its size
     * changes, as on rotation, the image is decoded again for it.
     */
    private final class Photo extends FrameLayout {
        Photo(Context context) {
            super(context);
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
        }

        int heightFor(int width) {
            int screen = getResources().getDisplayMetrics().heightPixels;
            return Math.max(1, Math.min(width * 4 / 3, Math.round(screen * MAX_IMAGE_SHARE)));
        }

        @Override
        protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
            int width = MeasureSpec.getSize(widthMeasureSpec);
            super.onMeasure(MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY),
                    MeasureSpec.makeMeasureSpec(heightFor(width), MeasureSpec.EXACTLY));
        }

        @Override
        protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
            super.onSizeChanged(width, height, oldWidth, oldHeight);
            if (!key.isEmpty() && oldWidth > 0 && (width != oldWidth || height != oldHeight)) {
                decode();
            }
        }
    }
}
