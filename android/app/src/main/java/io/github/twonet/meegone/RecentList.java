package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.content.res.Resources;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.ColorMatrix;
import android.graphics.ColorMatrixColorFilter;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;

/**
 * The Recent view (SRS 4.4, ADR 0023): its title, then the card that asks for usage access, "No
 * recent apps", or the recently used apps as cards, newest first, filling rows from the top
 * start (REC-01 to REC-04, REC-10).
 *
 * <p>The Rust app decides what shows and when the cards change; this view reads the cards again
 * only when the frame's revision changes. Each card is a rounded rectangle with the window's
 * proportions, filled with the app's accent, with its icon large in the middle, and below it the
 * app's label and how long ago it was used. It never says an app is running or was closed
 * (REC-01).
 *
 * <p>A tap on a card names it by its place in the cards last read, and passes where it is on the
 * screen, so the app grows from it (REC-06, ANIM-08). Cards aren't long-clickable: a long-press
 * anywhere in Recent is the shell's, for close mode (REC-07, #24).
 *
 * <p>The rows hold as many cards as the frame says, three or two on a phone, and the plates are
 * no taller than lets that many rows fit on one screen below the title (REC-05). This view
 * reports how wide its cards are, and the Rust app picks the columns. While two fingers pinch
 * the cards, they scale about where the fingers met; then they reflow from where they were
 * drawn to their places, as the frame's progress says (ANIM-17, ADR 0024).
 */
final class RecentList extends LinearLayout {
    // What shows, the frame's slot 94, mirrored in src/android.rs.
    static final int NOTHING = 0;
    static final int EXPLAINER = 1;
    static final int EMPTY = 2;
    static final int CARDS = 3;
    // The fields of one card in nativeRecentCards, mirrored in recent_card_fields in
    // src/android.rs: the label; flags; package, activity and profile serial number for its
    // icon; its fill; how long ago it was used, as a unit and a count.
    static final int FIELDS = 8;
    private static final int WORK = 1;
    private static final int UNAVAILABLE = 2;
    private static final int MINUTES = 1;
    private static final int HOURS = 2;
    private static final int DAYS = 3;
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    /** Icon labels: 12–13 sp (SRS 5.2). */
    private static final float LABEL_SP = 13f;
    private static final float AGE_SP = 12f;
    /** The side margins (SRS 5.2), inside the page's safe area. */
    private static final float MARGIN_DP = 16f;
    /** Cards and panels (SRS 5.2). */
    private static final float CORNER_DP = 12f;
    /** Between cards side by side. */
    private static final float GAP_DP = 12f;
    /** Between rows of cards, below the labels. */
    private static final float ROW_GAP_DP = 16f;
    /** The label sits this far below its card, as below an icon (SRS 5.2). */
    private static final float LABEL_GAP_DP = 4f;
    /** The icon: as in the grid, at most half the card's width. */
    private static final float ICON_DP = 56f;
    private static final float ICON_SHARE = 0.5f;
    /** A card's height over its width follows the window's, within these. */
    private static final float MIN_RATIO = 0.6f;
    private static final float MAX_RATIO = 2.2f;
    /** A phone's, until the window's size is known. */
    private static final float PHONE_RATIO = 2400f / 1080f;
    /** The smallest a touch target gets (A11Y-02). */
    private static final float TARGET_DP = 48f;
    /** An unavailable app, greyed and at half opacity as in the grid (APPS-09). */
    private static final float DIMMED_ALPHA = 0.5f;
    private static final ColorMatrixColorFilter GREYED;

    static {
        ColorMatrix grey = new ColorMatrix();
        grey.setSaturation(0f);
        GREYED = new ColorMatrixColorFilter(grey);
    }

    /** What the user does with Recent. */
    interface Listener {
        /** Allow was tapped on the explainer card (REC-02). */
        void allow();

        /** Card {@code index} of the cards last read was tapped, at {@code onScreen} (REC-06). */
        void tapped(int index, Rect onScreen);

        /** The cards were laid out {@code widthPx} wide, inside the margins (REC-05). */
        void laidOut(int widthPx);
    }

    private final AppIcons icons;
    private final Listener listener;
    private final float density;
    private final SectionHeader title;
    private final AccessCard explainer;
    private final TextView empty;
    private final Grid grid;
    private final List<Card> shown = new ArrayList<>();
    private int columns = 3;
    /** How many rows of cards one screen holds (REC-05). */
    private int rows = 3;
    /** How tall the page's content is, inside the safe area, in px; 0 until it is known. */
    private int viewport;
    private float ratio = PHONE_RATIO;
    /** The reflow last started, as the frame's slot 100, or -1 before the first frame. */
    private long reflowShown = -1;
    /** How far the reflow has come, from 0 to 1. */
    private float progress = 1f;
    /** Where each card was drawn as the reflow started, in the grid's coordinates. */
    private final Map<Card, RectF> drawn = new IdentityHashMap<>();
    /** Each card's offset and scale at the start of the reflow, once laid out in its place. */
    private final Map<Card, float[]> starts = new IdentityHashMap<>();
    private final float[] point = new float[2];
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int accent = -1;
    private int divider = -1;

    RecentList(Context context, AppIcons icons, Listener listener) {
        super(context);
        this.icons = icons;
        this.listener = listener;
        density = getResources().getDisplayMetrics().density;
        int margin = dp(MARGIN_DP);
        setOrientation(VERTICAL);
        setPadding(margin, dp(GAP_DP), margin, dp(MARGIN_DP));
        // Pinched cards may grow past the margins (ANIM-17).
        setClipToPadding(false);
        setClipChildren(false);

        // REC-01: the view's name, also a heading for screen readers.
        title = new SectionHeader(context, R.string.view_recent);
        addView(title, matchWidth());

        // REC-02, ONB-02: one plain sentence, and a button to the system's screen.
        explainer = new AccessCard(context, R.string.recent_access_explainer,
                R.string.recent_access_allow, listener::allow);
        addView(explainer, matchWidth());

        // REC-10: a calm line, where the cards would be.
        empty = new TextView(context);
        empty.setText(R.string.recent_empty);
        empty.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        empty.setGravity(Gravity.CENTER);
        empty.setMinHeight(dp(TARGET_DP));
        // A11Y-08: read when it appears.
        empty.setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);
        LayoutParams emptyParams = matchWidth();
        emptyParams.topMargin = dp(MARGIN_DP);
        addView(empty, emptyParams);

        grid = new Grid(context);
        LayoutParams gridParams = matchWidth();
        gridParams.topMargin = dp(GAP_DP);
        addView(grid, gridParams);

        show(NOTHING);
    }

    /**
     * Shows what the frame's slot 94 says. The title stays, so Recent is named even while it
     * doesn't work (REC-01, ARCH-09).
     */
    void show(int mode) {
        explainer.setVisibility(mode == EXPLAINER ? VISIBLE : GONE);
        empty.setVisibility(mode == EMPTY ? VISIBLE : GONE);
        grid.setVisibility(mode == CARDS ? VISIBLE : GONE);
    }

    /** How many cards each row holds, as the frame's slot 96 says (REC-05). */
    void columns(int columns) {
        int next = Math.max(1, columns);
        if (next != this.columns) {
            this.columns = next;
            grid.requestLayout();
        }
    }

    /** How many rows of cards one screen holds, as the frame's slot 102 says (REC-05). */
    void rows(int rows) {
        int next = Math.max(1, rows);
        if (next != this.rows) {
            this.rows = next;
            grid.requestLayout();
        }
    }

    /** How tall the page's content is, inside the safe area, in px (REC-05). */
    void viewport(int height) {
        int next = Math.max(0, height);
        if (next != viewport) {
            viewport = next;
            grid.requestLayout();
        }
    }

    /**
     * The cards' scale while two fingers pinch them, about ({@code x}, {@code y}) in {@code
     * ancestor}'s coordinates, where the fingers met (ANIM-17).
     */
    void pinch(float scale, float x, float y, View ancestor) {
        if (scale == 1f && grid.getScaleX() == 1f) {
            return;
        }
        point[0] = x;
        point[1] = y;
        toGrid(ancestor);
        grid.setPivotX(point[0]);
        grid.setPivotY(point[1]);
        grid.setScaleX(scale);
        grid.setScaleY(scale);
    }

    /**
     * The frame's reflow, slot 100: when it changes, each card is about to move from where it
     * is drawn now to its place, so this records where that is. Call it before the scale and the
     * columns change.
     */
    void reflow(long reflow) {
        if (reflow == reflowShown) {
            return;
        }
        boolean first = reflowShown < 0;
        reflowShown = reflow;
        if (first) {
            return;
        }
        drawn.clear();
        starts.clear();
        float scale = grid.getScaleX();
        float pivotX = grid.getPivotX();
        float pivotY = grid.getPivotY();
        for (Card card : shown) {
            float left = card.getLeft() + card.getTranslationX();
            float top = card.getTop() + card.getTranslationY();
            float width = card.getWidth() * card.getScaleX();
            float height = card.getHeight() * card.getScaleY();
            left = pivotX + (left - pivotX) * scale;
            top = pivotY + (top - pivotY) * scale;
            drawn.put(card, new RectF(left, top, left + width * scale, top + height * scale));
        }
        // The cards are laid out again, even if their columns stay, so each finds its start.
        grid.requestLayout();
    }

    /** How far the reflow has come, from 0 to 10 000, as the frame's slot 101 says (ANIM-17). */
    void progress(int progress) {
        float next = Math.max(0f, Math.min(1f, progress / 10_000f));
        if (next != this.progress) {
            this.progress = next;
            place();
        }
    }

    /** Moves each card from where it was drawn towards its place, as far as the reflow says. */
    private void place() {
        boolean done = progress >= 1f;
        for (Card card : shown) {
            float[] start = done ? null : starts.get(card);
            float rest = 1f - progress;
            card.setPivotX(0f);
            card.setPivotY(0f);
            card.setTranslationX(start == null ? 0f : start[0] * rest);
            card.setTranslationY(start == null ? 0f : start[1] * rest);
            float scale = start == null ? 1f : start[2] + (1f - start[2]) * progress;
            card.setScaleX(scale);
            card.setScaleY(scale);
        }
        if (done) {
            drawn.clear();
            starts.clear();
        }
    }

    /** {@link #point}, in {@code ancestor}'s coordinates, in the grid's, by layout and scroll. */
    private void toGrid(View ancestor) {
        View view = grid;
        while (view != ancestor) {
            point[0] -= view.getLeft();
            point[1] -= view.getTop();
            ViewParent parent = view.getParent();
            if (!(parent instanceof View)) {
                return;
            }
            view = (View) parent;
            point[0] += view.getScrollX();
            point[1] += view.getScrollY();
        }
    }

    /** The window's size, whose proportions each card takes (REC-04). */
    void window(int width, int height) {
        if (width <= 0 || height <= 0) {
            return;
        }
        float next = Math.min(MAX_RATIO, Math.max(MIN_RATIO, height / (float) width));
        if (next != ratio) {
            ratio = next;
            grid.requestLayout();
        }
    }

    /** Where {@code view} is in the cards last read, if it is one of them; otherwise -1. */
    int indexOfCard(View view) {
        return view instanceof Card ? shown.indexOf(view) : -1;
    }

    /** Shows the cards in {@code fields}, {@link #FIELDS} each, reusing the views it has. */
    void fill(String[] fields) {
        String[] all = fields == null ? new String[0] : fields;
        int count = all.length / FIELDS;
        while (shown.size() > count) {
            grid.removeView(shown.remove(shown.size() - 1));
        }
        while (shown.size() < count) {
            Card card = new Card(getContext());
            shown.add(card);
            grid.addView(card);
        }
        for (int i = 0; i < count; i++) {
            Card card = shown.get(i);
            card.bind(all, i * FIELDS);
            card.colour(foreground, dim);
            card.icon(icons);
        }
    }

    /** The icons arrived or changed: each card shows its own. */
    void refreshIcons() {
        for (Card card : shown) {
            card.icon(icons);
        }
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int surface, int accent, int divider) {
        if (foreground == this.foreground && dim == this.dim && surface == this.surface
                && accent == this.accent && divider == this.divider) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.surface = surface;
        this.accent = accent;
        this.divider = divider;
        title.colour(dim, divider);
        explainer.colour(foreground, surface, accent);
        empty.setTextColor(dim);
        for (Card card : shown) {
            card.colour(foreground, dim);
        }
    }

    private int dp(float value) {
        return Math.round(value * density);
    }

    private static LayoutParams matchWidth() {
        return new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
    }

    private static int parseInt(String field, int otherwise) {
        try {
            return Integer.parseInt(field);
        } catch (NumberFormatException e) {
            return otherwise;
        }
    }

    private static long parseLong(String field) {
        try {
            return Long.parseLong(field);
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    private static String field(String[] fields, int at) {
        String value = at < fields.length ? fields[at] : null;
        return value == null ? "" : value;
    }

    /**
     * The cards in rows of {@link #columns}, filling each from the start, so a right-to-left
     * layout mirrors them (L10N-02). Each card is as wide as a column, and its plate as tall as
     * the window's proportions make it.
     */
    private final class Grid extends ViewGroup {
        Grid(Context context) {
            super(context);
        }

        @Override
        protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
            super.onSizeChanged(width, height, oldWidth, oldHeight);
            if (width != oldWidth && width > 0) {
                // Not during layout: the columns it picks lay the cards out again.
                post(() -> listener.laidOut(getWidth()));
            }
        }

        @Override
        protected void onMeasure(int widthSpec, int heightSpec) {
            int width = MeasureSpec.getSize(widthSpec);
            int gap = dp(GAP_DP);
            int cellWidth = Math.max(0, (width - (columns - 1) * gap) / columns);
            int icon = Math.min(dp(ICON_DP), Math.round(cellWidth * ICON_SHARE));
            int childWidth = MeasureSpec.makeMeasureSpec(cellWidth, MeasureSpec.EXACTLY);
            int childHeight = MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED);
            int plate = plateHeight(cellWidth, icon, childWidth, childHeight);
            int height = 0;
            int rowHeight = 0;
            for (int i = 0; i < getChildCount(); i++) {
                Card card = (Card) getChildAt(i);
                card.size(plate, icon);
                card.measure(childWidth, childHeight);
                rowHeight = Math.max(rowHeight, card.getMeasuredHeight());
                if (i % columns == columns - 1 || i == getChildCount() - 1) {
                    height += rowHeight + (height > 0 ? dp(ROW_GAP_DP) : 0);
                    rowHeight = 0;
                }
            }
            setMeasuredDimension(width, resolveSize(height, heightSpec));
        }

        /**
         * A plate's height: the window's proportions, but no taller than lets {@link #rows} rows
         * fit on one screen below the title, with their labels at the current font size (REC-05),
         * and never squatter than {@link #MIN_RATIO}.
         */
        private int plateHeight(int cellWidth, int icon, int childWidth, int childHeight) {
            int byWindow = Math.round(cellWidth * ratio);
            if (viewport <= 0 || getChildCount() == 0) {
                return byWindow;
            }
            // The label and the age under a plate, measured on a card with no plate.
            Card sample = (Card) getChildAt(0);
            sample.size(0, icon);
            sample.measure(childWidth, childHeight);
            int caption = sample.getMeasuredHeight();
            int above = RecentList.this.getPaddingTop() + title.getMeasuredHeight()
                    + ((MarginLayoutParams) getLayoutParams()).topMargin;
            int room = viewport - above - RecentList.this.getPaddingBottom()
                    - (rows - 1) * dp(ROW_GAP_DP);
            int fit = room / rows - caption;
            return Math.max(Math.round(cellWidth * MIN_RATIO), Math.min(byWindow, fit));
        }

        @Override
        protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
            int width = right - left;
            int gap = dp(GAP_DP);
            boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
            int y = 0;
            int rowHeight = 0;
            for (int i = 0; i < getChildCount(); i++) {
                Card card = (Card) getChildAt(i);
                int column = i % columns;
                int cardWidth = card.getMeasuredWidth();
                int x = column * (cardWidth + gap);
                if (rtl) {
                    x = width - x - cardWidth;
                }
                card.layout(x, y, x + cardWidth, y + card.getMeasuredHeight());
                rowHeight = Math.max(rowHeight, card.getMeasuredHeight());
                if (column == columns - 1) {
                    y += rowHeight + dp(ROW_GAP_DP);
                    rowHeight = 0;
                }
                // A card in a reflow starts where it was drawn (ANIM-17).
                RectF from = drawn.remove(card);
                if (from != null && cardWidth > 0) {
                    starts.put(card, new float[] {
                        from.left - x, from.top - card.getTop(), from.width() / cardWidth
                    });
                }
            }
            place();
        }
    }

    /**
     * One recently used app (REC-04): a plate in its accent with its icon in the middle, then
     * its label and how long ago it was used. Screen readers read it as one item, "Mail, 5
     * minutes ago", with ", work" after a work app's name (A11Y-01).
     */
    private final class Card extends LinearLayout {
        private final GradientDrawable fill = new GradientDrawable();
        private final GradientDrawable mask = new GradientDrawable();
        private final FrameLayout plate;
        private final IconView image;
        private final TextView label;
        private final TextView age;
        private String packageName = "";
        private String activity = "";
        private long profile;
        private boolean available = true;

        Card(Context context) {
            super(context);
            setOrientation(VERTICAL);
            setClickable(true);
            setFocusable(true);
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
            setOnClickListener(view -> tapped());

            fill.setCornerRadius(dp(CORNER_DP));
            mask.setCornerRadius(dp(CORNER_DP));
            mask.setColor(Color.WHITE);
            plate = new FrameLayout(context);
            plate.setBackground(fill);
            plate.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
            image = new IconView(context);
            plate.addView(image, new FrameLayout.LayoutParams(
                    dp(ICON_DP), dp(ICON_DP), Gravity.CENTER));
            addView(plate, new LayoutParams(LayoutParams.MATCH_PARENT, 0));

            label = text(context, LABEL_SP);
            LayoutParams labelParams =
                    new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
            labelParams.topMargin = dp(LABEL_GAP_DP);
            addView(label, labelParams);
            age = text(context, AGE_SP);
            addView(age, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        }

        private TextView text(Context context, float sp) {
            TextView view = new TextView(context);
            view.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp);
            view.setGravity(Gravity.CENTER_HORIZONTAL);
            view.setSingleLine(true);
            view.setEllipsize(TextUtils.TruncateAt.END);
            view.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
            return view;
        }

        /** The plate's height and the icon's size, both in px, as the grid lays it out. */
        void size(int plateHeight, int iconSize) {
            LayoutParams plateParams = (LayoutParams) plate.getLayoutParams();
            plateParams.height = plateHeight;
            FrameLayout.LayoutParams iconParams =
                    (FrameLayout.LayoutParams) image.getLayoutParams();
            iconParams.width = iconSize;
            iconParams.height = iconSize;
        }

        void bind(String[] fields, int at) {
            Resources res = getResources();
            String name = field(fields, at);
            int flags = parseInt(field(fields, at + 1), 0);
            packageName = field(fields, at + 2);
            activity = field(fields, at + 3);
            profile = parseLong(field(fields, at + 4));
            fill.setColor(parseInt(field(fields, at + 5), Color.GRAY));
            int unit = parseInt(field(fields, at + 6), 0);
            int count = parseInt(field(fields, at + 7), 0);
            available = (flags & UNAVAILABLE) == 0;

            label.setText(name);
            age.setText(ago(res, unit, count, false));
            String named = (flags & WORK) != 0 ? res.getString(R.string.app_work, name) : name;
            String spoken = res.getString(R.string.recent_card_description, named,
                    ago(res, unit, count, true));
            setContentDescription(available
                    ? spoken
                    : res.getString(R.string.recent_card_description, spoken,
                            res.getString(R.string.app_state_unavailable)));
            plate.setAlpha(available ? 1f : DIMMED_ALPHA);
        }

        void icon(AppIcons icons) {
            image.show(icons.get(profile, packageName, activity), !available);
        }

        void colour(int foreground, int dim) {
            label.setTextColor(foreground);
            age.setTextColor(dim);
            // The card's press shows on its plate, which takes it and its hotspot as a child.
            ColorStateList pressed =
                    ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
            plate.setForeground(new RippleDrawable(pressed, null, mask));
        }

        private void tapped() {
            int index = shown.indexOf(this);
            if (index < 0) {
                return;
            }
            // The app grows from the card's plate (ANIM-08).
            int[] at = new int[2];
            plate.getLocationOnScreen(at);
            listener.tapped(index, new Rect(at[0], at[1], at[0] + plate.getWidth(),
                    at[1] + plate.getHeight()));
        }

        // A tap opens the app (REC-06, A11Y-05).
        @Override
        public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
            super.onInitializeAccessibilityNodeInfo(info);
            info.addAction(new AccessibilityAction(AccessibilityNodeInfo.ACTION_CLICK,
                    getResources().getString(R.string.action_open_app)));
        }
    }

    /**
     * An app's icon at this view's size. {@link AppIcons} gives every view the same drawable, and
     * the grid's cells set its bounds and filter as they draw it, so this sets them at each draw
     * too, as {@link AppCell} does: an {@code ImageView} would draw it where a cell last put it.
     * An unavailable app's icon is greyed (APPS-09).
     */
    private static final class IconView extends View {
        private Drawable icon;
        private boolean greyed;

        IconView(Context context) {
            super(context);
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        }

        void show(Drawable icon, boolean greyed) {
            if (icon != this.icon || greyed != this.greyed) {
                this.icon = icon;
                this.greyed = greyed;
                invalidate();
            }
        }

        @Override
        protected void onDraw(Canvas canvas) {
            if (icon == null) {
                return;
            }
            icon.setBounds(0, 0, getWidth(), getHeight());
            icon.setColorFilter(greyed ? GREYED : null);
            icon.draw(canvas);
        }
    }

    /** How long ago an app was used, in short or, for screen readers, in words (REC-04). */
    private static String ago(Resources res, int unit, int count, boolean spoken) {
        int plural;
        switch (unit) {
            case MINUTES:
                plural = spoken ? R.plurals.recent_minutes_ago_spoken : R.plurals.recent_minutes_ago;
                break;
            case HOURS:
                plural = spoken ? R.plurals.recent_hours_ago_spoken : R.plurals.recent_hours_ago;
                break;
            case DAYS:
                plural = spoken ? R.plurals.recent_days_ago_spoken : R.plurals.recent_days_ago;
                break;
            default:
                return res.getString(R.string.recent_just_now);
        }
        return res.getQuantityString(plural, count, count);
    }
}
