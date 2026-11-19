package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.content.res.Resources;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.RippleDrawable;
import android.os.Handler;
import android.os.Looper;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * The past week's images (WALL-17, ADR 0032). It covers the home views in the surface colour,
 * over the image details screen or Settings, which open it: today's image and the six before it,
 * each as a card with its preview, its title, its attribution as the source provides it
 * (LEGAL-03) and its date, and Pin, or Unpin for the pinned one. An image the source doesn't offer
 * as a wallpaper has no preview, and says so instead (LEGAL-04). It scrolls, so nothing overlaps
 * at large text sizes (A11Y-04).
 *
 * <p>The Rust app decides what it shows; this view reads it again only when the frame's revision
 * changes. Every text is set as plain text: never as HTML, and with no links made clickable
 * (SEC-02). Each preview is a JPEG the Rust app checked, decoded on {@link ImageCard}'s thread no
 * larger than it shows, and kept by its image's id until the screen hides.
 */
final class PastImages extends ScrollView {
    // The fields of nativePastImages, mirrored in src/android.rs: two for the screen, "1" while
    // the week is being read and the last problem, then IMAGE_FIELDS for each image.
    private static final int HEADER_FIELDS = 2;
    private static final int LOADING = 0;
    private static final int PROBLEM = 1;
    private static final int IMAGE_FIELDS = 7;
    private static final int ID = 0;
    private static final int TITLE = 1;
    private static final int ATTRIBUTION = 2;
    private static final int DATE = 3;
    private static final int CURRENT = 4;
    private static final int ACTION = 5;
    private static final int PREVIEW = 6;
    private static final String SET = "1";
    // What an image offers, as nativePastImages names it.
    private static final String PIN = "pin";
    private static final String PIN_LATER = "later";
    private static final String PINNING = "pinning";
    private static final String UNPIN = "unpin";
    private static final String UNPINNING = "unpinning";
    private static final String NOT_OFFERED = "not-offered";

    private static final float TITLE_SP = 24f;
    private static final float IMAGE_TITLE_SP = 18f;
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    private static final float ROW_DP = 56f;
    /** The smallest a touch target gets (A11Y-02). */
    private static final float TARGET_DP = 48f;
    /** The side margins (SRS 5.2). */
    private static final float MARGIN_DP = 16f;
    private static final float GAP_DP = 8f;
    /** A preview is 5:3, the feed's, and at most this tall. */
    private static final float MAX_PREVIEW_DP = 240f;
    private static final int MEDIUM = 500;

    /** What the user did here. */
    interface Listener {
        /** The preview of the image at {@code index} as read at {@code revision}, or null. */
        byte[] preview(int revision, int index);

        /** Pin was tapped on the image at {@code index} as read at {@code revision}. */
        void pin(int revision, int index);

        /** Unpin was tapped: today's image comes back (WALL-11). */
        void unpin();

        /** Try again was tapped, after a problem. */
        void tryAgain();
    }

    private final Listener listener;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final float density;
    private final TextView title;
    private final TextView loading;
    private final LinearLayout problemRow;
    private final TextView problem;
    private final TextView tryAgain;
    private final LinearLayout cards;
    /** The previews decoded so far, by their image's id, until the screen hides. */
    private final Map<String, Bitmap> previews = new HashMap<>();
    /** The ids whose previews are being decoded. */
    private final Set<String> decoding = new HashSet<>();
    /** The cards as of the last fill, in order. */
    private final List<Card> shown = new ArrayList<>();
    private int revision = -1;
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int accent = -1;

    PastImages(Context context, Listener listener) {
        super(context);
        this.listener = listener;
        density = getResources().getDisplayMetrics().density;
        // Touches on it stay there.
        setClickable(true);
        setFillViewport(true);
        setAccessibilityPaneTitle(getResources().getString(R.string.image_past_week));

        LinearLayout content = new LinearLayout(context);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(0, dp(MARGIN_DP), 0, dp(MARGIN_DP));

        title = text(context, content, TITLE_SP, 0);
        title.setText(R.string.image_past_week);
        title.setAccessibilityHeading(true);
        loading = text(context, content, BODY_SP, dp(GAP_DP));
        loading.setText(R.string.image_past_loading);
        loading.setVisibility(GONE);

        problem = new TextView(context);
        problem.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        tryAgain = new TextView(context);
        tryAgain.setText(R.string.image_try_again);
        tryAgain.setOnClickListener(view -> listener.tryAgain());
        problemRow = withButton(context, problem, tryAgain);
        problemRow.setVisibility(GONE);
        content.addView(problemRow, matchWidth());

        cards = new LinearLayout(context);
        cards.setOrientation(LinearLayout.VERTICAL);
        content.addView(cards, matchWidth());

        addView(content, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
    }

    /** Shows the screen from the top, as it opens. */
    void opened() {
        scrollTo(0, 0);
    }

    /** Lets the decoded previews go once the screen hides (PERF-07). */
    void closed() {
        previews.clear();
        cards.removeAllViews();
        shown.clear();
        revision = -1;
    }

    /** Shows the past week as read at {@code revision}. */
    void fill(int revision, String[] fields) {
        if (fields == null || fields.length < HEADER_FIELDS) {
            return;
        }
        this.revision = revision;
        Resources res = getResources();
        int count = (fields.length - HEADER_FIELDS) / IMAGE_FIELDS;
        boolean busy = SET.equals(fields[LOADING]);
        // Loading shows only while there is nothing yet to look at.
        loading.setVisibility(busy && count == 0 ? VISIBLE : GONE);
        String why = fields[PROBLEM];
        problem.setText(why.isEmpty() ? null : res.getString(R.string.image_last_problem, why));
        problemRow.setVisibility(why.isEmpty() ? GONE : VISIBLE);
        tryAgain.setEnabled(!busy);
        tryAgain.setTextColor(busy ? dim : accent);

        cards.removeAllViews();
        shown.clear();
        for (int index = 0; index < count; index++) {
            int at = HEADER_FIELDS + index * IMAGE_FIELDS;
            String[] image = new String[IMAGE_FIELDS];
            System.arraycopy(fields, at, image, 0, IMAGE_FIELDS);
            Card card = new Card(getContext(), index, image);
            shown.add(card);
            cards.addView(card, matchWidth());
            if (SET.equals(image[PREVIEW])) {
                Bitmap preview = previews.get(image[ID]);
                if (preview != null) {
                    card.showPreview(preview);
                } else {
                    decode(index, image[ID]);
                }
            }
        }
    }

    /** Decodes the preview of the image at {@code index}, as large as its box, behind the scenes. */
    private void decode(int index, String id) {
        if (decoding.contains(id)) {
            return;
        }
        byte[] jpeg = listener.preview(revision, index);
        if (jpeg == null) {
            return;
        }
        decoding.add(id);
        int width = cards.getWidth();
        if (width <= 0) {
            // Not laid out yet: as wide as the screen.
            width = Math.max(1, getResources().getDisplayMetrics().widthPixels);
        }
        int boxWidth = width;
        int boxHeight = previewHeight(width);
        ImageCard.DECODER.execute(() -> {
            Bitmap bitmap = ImageCard.decode(jpeg, boxWidth, boxHeight);
            main.post(() -> decoded(id, bitmap));
        });
    }

    private void decoded(String id, Bitmap bitmap) {
        decoding.remove(id);
        if (bitmap == null || revision < 0) {
            // Unreadable, or the screen hid meanwhile.
            return;
        }
        previews.put(id, bitmap);
        for (Card card : shown) {
            if (card.id.equals(id)) {
                card.showPreview(bitmap);
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
        title.setTextColor(foreground);
        loading.setTextColor(dim);
        problem.setTextColor(foreground);
        tryAgain.setTextColor(tryAgain.isEnabled() ? accent : dim);
        tryAgain.setBackground(ripple());
        for (Card card : shown) {
            card.colour();
        }
    }

    private RippleDrawable ripple() {
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        return new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE));
    }

    /** A preview's height for {@code width}: 5:3, and at most {@link #MAX_PREVIEW_DP}. */
    private int previewHeight(int width) {
        return Math.max(1, Math.min(width * 3 / 5, dp(MAX_PREVIEW_DP)));
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

    /** A message with a button at its end, such as Try again. */
    private LinearLayout withButton(Context context, TextView text, TextView button) {
        LinearLayout row = new LinearLayout(context);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setMinimumHeight(dp(ROW_DP));
        int margin = dp(MARGIN_DP);
        row.setPadding(margin, 0, 0, 0);
        row.addView(text, new LinearLayout.LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f));
        button.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP + 1);
        button.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
        button.setGravity(Gravity.CENTER);
        button.setMinHeight(dp(TARGET_DP));
        button.setMinWidth(dp(TARGET_DP));
        button.setPadding(margin, 0, margin, 0);
        row.addView(button, new LinearLayout.LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        return row;
    }

    private int dp(float value) {
        return Math.round(value * density);
    }

    private static LinearLayout.LayoutParams matchWidth() {
        return new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT);
    }

    /**
     * One of the week's images: its preview across the top, its title, attribution and date,
     * then what it offers as a whole row.
     */
    private final class Card extends LinearLayout {
        final String id;
        private final String action;
        private final FrameLayout box;
        private final ImageView preview;
        private final TextView name;
        private final TextView attribution;
        private final TextView date;
        private final TextView row;

        Card(Context context, int index, String[] image) {
            super(context);
            setOrientation(VERTICAL);
            setPadding(0, dp(MARGIN_DP), 0, 0);
            id = image[ID];
            action = image[ACTION];
            Resources res = getResources();

            box = new FrameLayout(context) {
                @Override
                protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
                    int width = MeasureSpec.getSize(widthMeasureSpec);
                    super.onMeasure(MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY),
                            MeasureSpec.makeMeasureSpec(previewHeight(width), MeasureSpec.EXACTLY));
                }
            };
            // The title says what it shows, so screen readers skip it.
            box.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
            preview = new ImageView(context);
            preview.setScaleType(ImageView.ScaleType.CENTER_CROP);
            box.addView(preview, new FrameLayout.LayoutParams(
                    FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));
            // Until its preview is decoded, or if it has none, it takes no room.
            box.setVisibility(GONE);
            addView(box, matchWidth());

            name = text(context, this, IMAGE_TITLE_SP, dp(GAP_DP));
            name.setText(image[TITLE]);
            // LEGAL-03: in full, never cut short.
            attribution = text(context, this, BODY_SP, dp(GAP_DP / 2));
            attribution.setText(image[ATTRIBUTION]);
            date = text(context, this, BODY_SP, dp(GAP_DP / 2));
            String when = image[DATE];
            if (SET.equals(image[CURRENT])) {
                String here = res.getString(R.string.image_on_lock_screen);
                when = when.isEmpty() ? here : res.getString(R.string.image_date_region, when, here);
            }
            date.setText(when);
            date.setVisibility(when.isEmpty() ? GONE : VISIBLE);

            row = new TextView(context);
            row.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP + 1);
            row.setGravity(Gravity.CENTER_VERTICAL);
            row.setMinHeight(dp(ROW_DP));
            int margin = dp(MARGIN_DP);
            row.setPadding(margin, 0, margin, 0);
            switch (action) {
                case PIN:
                case PIN_LATER:
                    row.setText(R.string.image_pin);
                    // A11Y-01: "Pin Step into the great outdoors", not just "Pin".
                    row.setContentDescription(res.getString(R.string.image_pin_named, image[TITLE]));
                    row.setOnClickListener(view -> listener.pin(revision, index));
                    break;
                case PINNING:
                    row.setText(R.string.image_pinning);
                    break;
                case UNPIN:
                    row.setText(R.string.image_unpin);
                    row.setContentDescription(
                            res.getString(R.string.image_unpin_named, image[TITLE]));
                    row.setOnClickListener(view -> listener.unpin());
                    break;
                case UNPINNING:
                    row.setText(R.string.image_unpinning);
                    break;
                case NOT_OFFERED:
                default:
                    // Not a button: it only says why there is none (LEGAL-04).
                    row.setText(R.string.image_not_offered);
                    row.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
                    break;
            }
            row.setEnabled(PIN.equals(action) || UNPIN.equals(action));
            addView(row, matchWidth());
            colour();
        }

        void showPreview(Bitmap bitmap) {
            preview.setImageBitmap(bitmap);
            box.setVisibility(VISIBLE);
        }

        void colour() {
            name.setTextColor(foreground);
            attribution.setTextColor(dim);
            date.setTextColor(dim);
            boolean button = PIN.equals(action) || UNPIN.equals(action);
            row.setTextColor(button ? accent : dim);
            row.setBackground(button ? ripple() : null);
            if (row.isClickable() != button) {
                row.setClickable(button);
            }
        }
    }
}
