package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.BitmapRegionDecoder;
import android.graphics.Color;
import android.graphics.Rect;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.io.File;
import java.io.IOException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * The image-of-the-day card, below the notifications in Events (EVT-12, ADR 0022): an "Image of
 * the day" section header, then a flat card in the surface colour with today's lock-screen
 * image as a thumbnail, its title and its attribution, as the source provides it (LEGAL-03). A
 * tap opens the image details screen.
 *
 * <p>The Rust app decides whether the card shows and what it says; this view reads it again only
 * when the frame's revision changes. The thumbnail comes from the image cache's file, which it
 * decodes off the main thread, only as much of it as the thumbnail shows. When a new image
 * arrives over one already shown, it fades in as far as the frame's progress says (ANIM-22).
 * It needs no network and no notification access (EVT-03, REL-06).
 */
final class ImageCard extends LinearLayout {
    // The fields of nativeImageCard, mirrored in src/android.rs: the title, the attribution, the
    // image's file relative to the app's files folder, and the image's key.
    static final int FIELDS = 4;
    private static final int TITLE = 0;
    private static final int ATTRIBUTION = 1;
    private static final int PATH = 2;
    private static final int KEY = 3;
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    private static final float DETAIL_SP = 13f;
    /** The side margins (SRS 5.2), inside the page's safe area. */
    private static final float MARGIN_DP = 16f;
    /** Cards and panels (SRS 5.2). */
    private static final float CORNER_DP = 12f;
    private static final float GAP_DP = 12f;
    /** The thumbnail is 16:9 of the card's width, and never taller than this. */
    private static final float MAX_THUMBNAIL_DP = 240f;
    private static final int TITLE_LINES = 2;
    private static final int OPAQUE = 10_000;

    /**
     * One thread for the whole process: thumbnails, and the image details screen's image, decode
     * behind the scenes.
     */
    static final ExecutorService DECODER = Executors.newSingleThreadExecutor(task -> {
        Thread thread = new Thread(task, "MeeGone image");
        thread.setPriority(Thread.NORM_PRIORITY - 1);
        return thread;
    });

    /** What the user, or the card, does. */
    interface Listener {
        /** The card was tapped (EVT-12). */
        void tapped();

        /** A new thumbnail is about to fade in over the old one (ANIM-22). */
        void newImageShown();
    }

    private final Listener listener;
    /** The folder MeeGone's files are in, which the image's path is relative to. */
    private final File files;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final float density;
    private final SectionHeader section;
    private final LinearLayout card;
    private final GradientDrawable plate = new GradientDrawable();
    private final GradientDrawable mask = new GradientDrawable();
    private final Thumbnail thumbnail;
    /** The thumbnail drawn, and the new one fading in over it. */
    private final ImageView shownImage;
    private final ImageView incomingImage;
    private final TextView title;
    private final TextView attribution;
    /** The image as of the last fill: its file and its key. */
    private String path = "";
    private String key = "";
    /** The key of the image drawn or fading in, or empty while none is. */
    private String drawnKey = "";
    private boolean fading;
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int divider = -1;

    ImageCard(Context context, Listener listener) {
        super(context);
        this.listener = listener;
        files = context.getFilesDir();
        density = getResources().getDisplayMetrics().density;
        int margin = dp(MARGIN_DP);
        setOrientation(VERTICAL);
        setPadding(margin, 0, margin, margin);

        section = new SectionHeader(context, R.string.image_of_the_day);
        addView(section, matchWidth());

        // SRS 5.2: flat, 12 dp corners, the surface colour; the image keeps to the corners.
        card = new LinearLayout(context);
        card.setOrientation(VERTICAL);
        plate.setCornerRadius(dp(CORNER_DP));
        mask.setCornerRadius(dp(CORNER_DP));
        mask.setColor(Color.WHITE);
        card.setBackground(plate);
        card.setClipToOutline(true);
        card.setOnClickListener(view -> listener.tapped());
        card.setAccessibilityDelegate(new AccessibilityDelegate() {
            @Override
            public void onInitializeAccessibilityNodeInfo(View host,
                    AccessibilityNodeInfo info) {
                super.onInitializeAccessibilityNodeInfo(host, info);
                info.addAction(new AccessibilityAction(AccessibilityNodeInfo.ACTION_CLICK,
                        getResources().getString(R.string.action_image_details)));
            }
        });

        thumbnail = new Thumbnail(context, dp(MAX_THUMBNAIL_DP));
        shownImage = image(context);
        incomingImage = image(context);
        incomingImage.setAlpha(0f);
        thumbnail.addView(shownImage, fill());
        thumbnail.addView(incomingImage, fill());
        card.addView(thumbnail, matchWidth());

        title = new TextView(context);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        title.setMaxLines(TITLE_LINES);
        title.setEllipsize(TextUtils.TruncateAt.END);
        LayoutParams titleParams = matchWidth();
        titleParams.setMargins(margin, dp(GAP_DP), margin, 0);
        card.addView(title, titleParams);
        // LEGAL-03: in full, never cut short.
        attribution = new TextView(context);
        attribution.setTextSize(TypedValue.COMPLEX_UNIT_SP, DETAIL_SP);
        LayoutParams attributionParams = matchWidth();
        attributionParams.setMargins(margin, dp(4), margin, margin);
        card.addView(attribution, attributionParams);
        addView(card, matchWidth());

        setVisibility(GONE);
    }

    private static ImageView image(Context context) {
        ImageView image = new ImageView(context);
        image.setScaleType(ImageView.ScaleType.CENTER_CROP);
        // The title and the attribution say what it shows.
        image.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        return image;
    }

    /** Shows the card at its title, attribution and image, in {@link #FIELDS} fields. */
    void fill(String[] fields) {
        if (fields == null || fields.length < FIELDS) {
            return;
        }
        title.setText(fields[TITLE]);
        attribution.setText(fields[ATTRIBUTION]);
        if (!fields[KEY].equals(key)) {
            path = fields[PATH];
            key = fields[KEY];
            decode();
        }
    }

    /**
     * How far the new thumbnail has faded in, from 0 to 10 000 (ANIM-22). At 10 000 it is the
     * one drawn.
     */
    void fade(int progress) {
        if (!fading) {
            return;
        }
        if (progress >= OPAQUE) {
            shownImage.setImageDrawable(incomingImage.getDrawable());
            incomingImage.setImageDrawable(null);
            incomingImage.setAlpha(0f);
            fading = false;
        } else {
            incomingImage.setAlpha(Math.max(0, progress) / (float) OPAQUE);
        }
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int surface, int divider) {
        if (foreground == this.foreground && dim == this.dim && surface == this.surface
                && divider == this.divider) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.surface = surface;
        this.divider = divider;
        section.colour(dim, divider);
        plate.setColor(surface);
        title.setTextColor(foreground);
        attribution.setTextColor(dim);
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        card.setForeground(new RippleDrawable(pressed, null, mask));
    }

    /** Decodes the image as large as the thumbnail is now, behind the scenes. */
    private void decode() {
        String askedPath = path;
        String askedKey = key;
        int width = thumbnail.getWidth();
        int height = thumbnail.getHeight();
        if (width <= 0 || height <= 0) {
            // Not laid out yet: as wide as the screen, less the side margins.
            width = Math.max(1, getResources().getDisplayMetrics().widthPixels - 2 * dp(MARGIN_DP));
            height = thumbnail.heightFor(width);
        }
        int boxWidth = width;
        int boxHeight = height;
        DECODER.execute(() -> {
            Bitmap bitmap = decode(new File(files, askedPath), boxWidth, boxHeight);
            main.post(() -> decoded(askedKey, bitmap));
        });
    }

    private void decoded(String askedKey, Bitmap bitmap) {
        if (!askedKey.equals(key)) {
            // Another image came meanwhile; its own decode follows.
            return;
        }
        if (bitmap == null) {
            // The file is missing or isn't an image: the plate shows, without a stale image.
            shownImage.setImageDrawable(null);
            incomingImage.setImageDrawable(null);
            fading = false;
            drawnKey = "";
            return;
        }
        if (!drawnKey.isEmpty() && !drawnKey.equals(askedKey)
                && shownImage.getDrawable() != null) {
            // ANIM-22: a new image over the one shown fades in, as the Rust app times it.
            incomingImage.setImageBitmap(bitmap);
            incomingImage.setAlpha(0f);
            fading = true;
            drawnKey = askedKey;
            listener.newImageShown();
        } else {
            // The first image, or the same one at a new size, shows at once.
            shownImage.setImageBitmap(bitmap);
            drawnKey = askedKey;
        }
    }

    /**
     * The middle of the image in {@code width} × {@code height}'s proportions, as the thumbnail
     * crops it, decoded no larger than needed: the image's file may be as large as the lock
     * screen. Null if it can't be read.
     */
    @SuppressWarnings("deprecation") // newInstance(String) arrived in Android 12.
    static Bitmap decode(File file, int width, int height) {
        try {
            return decode(Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
                    ? BitmapRegionDecoder.newInstance(file.getPath())
                    : BitmapRegionDecoder.newInstance(file.getPath(), false), width, height);
        } catch (IOException | RuntimeException e) {
            return null;
        }
    }

    /**
     * As {@link #decode(File, int, int)}, from a JPEG in memory, such as a past image's preview
     * (WALL-17). The Rust app checked its header, so decoding it stays bounded (SEC-02).
     */
    @SuppressWarnings("deprecation") // newInstance(byte[], int, int) arrived in Android 12.
    static Bitmap decode(byte[] jpeg, int width, int height) {
        try {
            return decode(Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
                    ? BitmapRegionDecoder.newInstance(jpeg, 0, jpeg.length)
                    : BitmapRegionDecoder.newInstance(jpeg, 0, jpeg.length, false), width, height);
        } catch (IOException | RuntimeException e) {
            return null;
        }
    }

    /** The middle of what {@code decoder} reads, in a box's proportions, then lets it go. */
    private static Bitmap decode(BitmapRegionDecoder decoder, int width, int height) {
        if (decoder == null) {
            return null;
        }
        try {
            Rect band = centreCrop(decoder.getWidth(), decoder.getHeight(), width, height);
            if (band.isEmpty()) {
                return null;
            }
            BitmapFactory.Options options = new BitmapFactory.Options();
            options.inSampleSize = sampleSize(band.width(), width);
            return decoder.decodeRegion(band, options);
        } finally {
            decoder.recycle();
        }
    }

    /** The middle of an image, in a box's proportions (centre crop). */
    static Rect centreCrop(int imageWidth, int imageHeight, int boxWidth, int boxHeight) {
        if (imageWidth <= 0 || imageHeight <= 0 || boxWidth <= 0 || boxHeight <= 0) {
            return new Rect();
        }
        if ((long) imageWidth * boxHeight > (long) imageHeight * boxWidth) {
            // Wider than the box: all of its height, the middle of its width.
            int width = (int) ((long) imageHeight * boxWidth / boxHeight);
            int left = (imageWidth - width) / 2;
            return new Rect(left, 0, left + width, imageHeight);
        }
        int height = (int) ((long) imageWidth * boxHeight / boxWidth);
        int top = (imageHeight - height) / 2;
        return new Rect(0, top, imageWidth, top + height);
    }

    /** The largest power of two that keeps {@code width} at least {@code needed} wide. */
    static int sampleSize(int width, int needed) {
        int size = 1;
        while (width / (size * 2) >= needed) {
            size *= 2;
        }
        return size;
    }

    private int dp(float value) {
        return Math.round(value * density);
    }

    private static LayoutParams matchWidth() {
        return new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
    }

    private static FrameLayout.LayoutParams fill() {
        return new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT);
    }

    /**
     * The thumbnail's box: 16:9 of its width, at most {@link #MAX_THUMBNAIL_DP} tall. When its
     * size changes, as on rotation, the image is decoded again for it.
     */
    private final class Thumbnail extends FrameLayout {
        private final int maxHeight;

        Thumbnail(Context context, int maxHeight) {
            super(context);
            this.maxHeight = maxHeight;
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
        }

        int heightFor(int width) {
            return Math.min(width * 9 / 16, maxHeight);
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
