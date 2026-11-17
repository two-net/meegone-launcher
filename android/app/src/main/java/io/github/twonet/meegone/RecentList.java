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
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.os.Bundle;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.accessibility.AccessibilityEvent;
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
 * screen, so the app grows from it (REC-06, ANIM-08). Cards aren't long-clickable: {@link
 * HomeViews} times a long-press anywhere on Recent, which enters close mode (REC-07).
 *
 * <p>The rows hold as many cards as the frame says, three or two on a phone, and the plates are
 * no taller than lets that many rows fit on one screen below the title (REC-05). This view
 * reports how wide its cards are, and the Rust app picks the columns. While two fingers pinch
 * the cards, they scale about where the fingers met; then they reflow from where they were
 * drawn to their places, as the frame's progress says (ANIM-17, ADR 0024).
 *
 * <p>In close mode (REC-07, ADR 0025) every card is drawn at 92 % with a ✕ over its plate's
 * top-start corner, popping in row by row as the frame says (ANIM-12), and Clear all shows at the
 * end of the title. A tap within a card's top-start 48 dp square taps its ✕ (A11Y-02); any other
 * tap on a card does nothing. Screen readers get "Remove ‹app› from Recent" on each card instead
 * (A11Y-01). Each card read says where it was in the cards read before: a card no card keeps has
 * gone, and shrinks and fades where it was, as the frame says for its row, while the others
 * close the gap (ANIM-18).
 */
final class RecentList extends LinearLayout {
    // What shows, the frame's slot 94, mirrored in src/android.rs.
    static final int NOTHING = 0;
    static final int EXPLAINER = 1;
    static final int EMPTY = 2;
    static final int CARDS = 3;
    // The fields of one card in nativeRecentCards, mirrored in recent_card_fields in
    // src/android.rs: the label; flags; package, activity and profile serial number for its
    // icon; its fill; how long ago it was used, as a unit and a count; where it was in the cards
    // read before, or -1.
    static final int FIELDS = 9;
    /** Rows of cards with a ✕ scale and a leaving progress of their own, as RECENT_ROWS. */
    static final int ROWS = 9;
    private static final int WORK = 1;
    private static final int UNAVAILABLE = 2;
    private static final int MINUTES = 1;
    private static final int HOURS = 2;
    private static final int DAYS = 3;
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    private static final int MEDIUM = 500;
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
    /** The ✕'s centre is this far inside the plate's top-start corner, so it stays on the plate. */
    private static final float BADGE_INSET_DP = 16f;
    /** A card that leaves shrinks to this as it fades (ANIM-18), as a cell that leaves does. */
    private static final float LEAVE_SCALE = 0.5f;
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

        /**
         * In close mode, the ✕ of card {@code index} of the cards last read was tapped, or a
         * screen reader chose "Remove ‹app› from Recent" (REC-08).
         */
        void badgeTapped(int index);

        /** In close mode, Clear all was tapped (REC-08). */
        void clearAll();
    }

    private final AppIcons icons;
    private final Listener listener;
    private final float density;
    private final SectionHeader title;
    /** At the end of the title in close mode: removes every card (REC-07, REC-08). */
    private final TextView clearAll;
    private final AccessCard explainer;
    private final TextView empty;
    private final Grid grid;
    private final List<Card> shown = new ArrayList<>();
    /** Cards that went, shrinking and fading where they were until their row has gone. */
    private final List<Card> leaving = new ArrayList<>();
    /** The label of the one card that went with the cards last read, or null (A11Y-08). */
    private String left;
    private int mode = NOTHING;
    private int columns = 3;
    /** The columns as of the cards last read: the rows that cards leave from. */
    private int columnsRead = 3;
    /** How many rows of cards one screen holds (REC-05). */
    private int rows = 3;
    /** How tall the page's content is, inside the safe area, in px; 0 until it is known. */
    private int viewport;
    private float ratio = PHONE_RATIO;
    /** Close mode is on: only a card's ✕ acts on a tap (REC-07). */
    private boolean closing;
    /** Every card's scale in close mode, 0.92, and 1 out of it (ANIM-12). */
    private float closeScale = 1f;
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

        // REC-01: the view's name, also a heading for screen readers, with Clear all at its end
        // in close mode (REC-07).
        title = new SectionHeader(context, R.string.view_recent);
        clearAll = new TextView(context);
        clearAll.setText(R.string.recent_clear_all);
        clearAll.setContentDescription(
                getResources().getString(R.string.recent_clear_all_description));
        clearAll.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        clearAll.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
        clearAll.setGravity(Gravity.CENTER);
        clearAll.setMinHeight(dp(TARGET_DP));
        clearAll.setMinWidth(dp(TARGET_DP));
        clearAll.setPadding(dp(GAP_DP), 0, dp(GAP_DP), 0);
        clearAll.setOnClickListener(view -> {
            if (closing) {
                listener.clearAll();
            }
        });
        clearAll.setVisibility(GONE);
        LayoutParams clearParams =
                new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT);
        clearParams.setMarginStart(dp(4));
        title.addView(clearAll, clearParams);
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
        // A11Y-08: read when it appears, such as after Clear all.
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
     * doesn't work (REC-01, ARCH-09). Cards that went stay until they have shrunk away, and "No
     * recent apps" shows after them (ANIM-18).
     */
    void show(int mode) {
        this.mode = mode;
        visibility();
    }

    private void visibility() {
        boolean going = !leaving.isEmpty();
        explainer.setVisibility(mode == EXPLAINER ? VISIBLE : GONE);
        empty.setVisibility(mode == EMPTY && !going ? VISIBLE : GONE);
        grid.setVisibility(mode == CARDS || going ? VISIBLE : GONE);
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
     * is drawn now to its place, so this records where that is. Call it before the scale, the
     * columns and the cards change.
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

    /**
     * Close mode as the frame says (REC-07, ANIM-12): whether it is on, every card's scale, how
     * far into it the cards are, which Clear all fades with, and the ✕ scale of each row, from
     * {@code frame[badgesAt]}. All in ten-thousandths.
     */
    void close(boolean on, int scale, int into, int[] frame, int badgesAt) {
        closing = on;
        closeScale = scale / 10_000f;
        for (int i = 0; i < shown.size(); i++) {
            int row = Math.min(i / columns, ROWS - 1);
            shown.get(i).close(on, closeScale, frame[badgesAt + row] / 10_000f);
        }
        float alpha = Math.max(0f, Math.min(1f, into / 10_000f));
        clearAll.setAlpha(alpha);
        clearAll.setVisibility(alpha > 0f && mode == CARDS ? VISIBLE : GONE);
        clearAll.setEnabled(on);
        clearAll.setImportantForAccessibility(on
                ? IMPORTANT_FOR_ACCESSIBILITY_YES
                : IMPORTANT_FOR_ACCESSIBILITY_NO);
    }

    /**
     * How far the cards that went from each row of the cards read before have gone, from
     * {@code frame[at]}, in ten-thousandths (ANIM-18). A card whose row has gone all the way is
     * dropped.
     */
    void leave(int[] frame, int at) {
        boolean dropped = false;
        for (int i = leaving.size() - 1; i >= 0; i--) {
            Card card = leaving.get(i);
            float gone = frame[at + Math.min(card.row, ROWS - 1)] / 10_000f;
            if (gone >= 1f) {
                leaving.remove(i);
                grid.removeView(card);
                dropped = true;
            } else {
                card.leave(Math.max(0f, gone), closeScale);
            }
        }
        if (dropped) {
            visibility();
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

    /**
     * The label of the card that went with the cards last read, if exactly one did, so screen
     * readers hear "Mail removed from Recent" (A11Y-08); otherwise null.
     */
    String left() {
        return left;
    }

    /**
     * Shows the cards in {@code fields}, {@link #FIELDS} each. A card keeps the view it had in
     * the cards read before, so it moves from where it is drawn; a card the host had that no card
     * keeps has gone, and shrinks away where it was (ANIM-18).
     */
    void fill(String[] fields) {
        String[] all = fields == null ? new String[0] : fields;
        int count = all.length / FIELDS;
        Card[] kept = new Card[count];
        boolean[] stays = new boolean[shown.size()];
        for (int i = 0; i < count; i++) {
            int from = parseInt(field(all, i * FIELDS + 8), -1);
            if (from >= 0 && from < shown.size() && !stays[from]) {
                stays[from] = true;
                kept[i] = shown.get(from);
            }
        }
        left = null;
        int went = 0;
        for (int i = 0; i < shown.size(); i++) {
            if (!stays[i]) {
                Card card = shown.get(i);
                card.startLeaving(i / columnsRead);
                leaving.add(card);
                left = card.name;
                went++;
            }
        }
        if (went != 1) {
            left = null;
        }
        shown.clear();
        for (int i = 0; i < count; i++) {
            Card card = kept[i] != null ? kept[i] : new Card(getContext());
            shown.add(card);
            // Children in reading order, so screen readers go through the cards in it.
            if (grid.indexOfChild(card) != i) {
                grid.removeView(card);
                grid.addView(card, i);
            }
            card.bind(all, i * FIELDS);
            card.colour(foreground, dim, surface);
            card.icon(icons);
            card.close(closing, closeScale, closing ? 1f : 0f);
        }
        columnsRead = columns;
        grid.requestLayout();
        visibility();
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
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        clearAll.setTextColor(accent);
        clearAll.setBackground(
                new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
        explainer.colour(foreground, surface, accent);
        empty.setTextColor(dim);
        for (Card card : shown) {
            card.colour(foreground, dim, surface);
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
     * the window's proportions make it. Cards that went keep their place until they have gone,
     * and the grid stays tall enough to show them.
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
            for (int i = 0; i < shown.size(); i++) {
                Card card = shown.get(i);
                card.size(plate, icon);
                card.measure(childWidth, childHeight);
                rowHeight = Math.max(rowHeight, card.getMeasuredHeight());
                if (i % columns == columns - 1 || i == shown.size() - 1) {
                    height += rowHeight + (height > 0 ? dp(ROW_GAP_DP) : 0);
                    rowHeight = 0;
                }
            }
            for (Card card : leaving) {
                height = Math.max(height, card.getBottom());
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
            if (viewport <= 0 || shown.isEmpty()) {
                return byWindow;
            }
            // The label and the age under a plate, measured on a card with no plate.
            Card sample = shown.get(0);
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
            for (int i = 0; i < shown.size(); i++) {
                Card card = shown.get(i);
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
                // A card in a reflow starts where it was drawn (ANIM-17, ANIM-18).
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
     *
     * <p>The card itself moves and scales as the cards reflow (ANIM-17); its body takes close
     * mode's scale and shrinks and fades as it leaves (ANIM-12, ANIM-18), so neither undoes the
     * other.
     */
    private final class Card extends FrameLayout {
        private final GradientDrawable fill = new GradientDrawable();
        private final GradientDrawable mask = new GradientDrawable();
        private final Body body;
        private final FrameLayout plate;
        private final IconView image;
        private final TextView label;
        private final TextView age;
        private final Badge badge;
        private final int badgeInsetPx;
        private String name = "";
        private String packageName = "";
        private String activity = "";
        private long profile;
        private boolean available = true;
        /** Close mode is on: only the ✕ acts on a tap (REC-07). */
        private boolean closing;
        private float badgeScale;
        /** A finger went down on the ✕ and hasn't left it. */
        private boolean badgeDown;
        /** The row of the cards read before that it left from, or -1 while it shows. */
        int row = -1;

        Card(Context context) {
            super(context);
            setClickable(true);
            setFocusable(true);
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
            setOnClickListener(view -> tapped());
            setClipChildren(false);
            badge = new Badge(density);
            badgeInsetPx = dp(BADGE_INSET_DP);

            body = new Body(context);
            body.setOrientation(VERTICAL);
            addView(body, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

            fill.setCornerRadius(dp(CORNER_DP));
            mask.setCornerRadius(dp(CORNER_DP));
            mask.setColor(Color.WHITE);
            plate = new FrameLayout(context);
            plate.setBackground(fill);
            plate.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
            image = new IconView(context);
            plate.addView(image, new FrameLayout.LayoutParams(
                    dp(ICON_DP), dp(ICON_DP), Gravity.CENTER));
            body.addView(plate, new LinearLayout.LayoutParams(LayoutParams.MATCH_PARENT, 0));

            label = text(context, LABEL_SP);
            LinearLayout.LayoutParams labelParams = new LinearLayout.LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
            labelParams.topMargin = dp(LABEL_GAP_DP);
            body.addView(label, labelParams);
            age = text(context, AGE_SP);
            body.addView(age, new LinearLayout.LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
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
            LinearLayout.LayoutParams plateParams =
                    (LinearLayout.LayoutParams) plate.getLayoutParams();
            plateParams.height = plateHeight;
            FrameLayout.LayoutParams iconParams =
                    (FrameLayout.LayoutParams) image.getLayoutParams();
            iconParams.width = iconSize;
            iconParams.height = iconSize;
        }

        void bind(String[] fields, int at) {
            Resources res = getResources();
            name = field(fields, at);
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

        void colour(int foreground, int dim, int surface) {
            label.setTextColor(foreground);
            age.setTextColor(dim);
            badge.colour(foreground, dim, surface);
            // The card's press shows on its plate, which takes it and its hotspot as a child.
            ColorStateList pressed =
                    ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
            plate.setForeground(new RippleDrawable(pressed, null, mask));
            body.invalidate();
        }

        /** Close mode as the frame says: whether it is on, the card's scale and its ✕'s. */
        void close(boolean on, float scale, float popped) {
            if (on != closing) {
                closing = on;
                badgeDown = false;
                // The long-press that started it leaves no press showing on the plate.
                cancelPendingInputEvents();
                setPressed(false);
                // Screen readers ask again what the card offers (A11Y-01).
                sendAccessibilityEvent(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
            }
            scaleBody(scale);
            if (popped != badgeScale) {
                badgeScale = popped;
                body.invalidate();
            }
        }

        /** It went from Recent: it takes no more touches, and screen readers no longer see it. */
        void startLeaving(int row) {
            this.row = Math.max(0, row);
            closing = false;
            badgeDown = false;
            setClickable(false);
            setFocusable(false);
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
        }

        /** It shrinks and fades as it goes, from 0 to 1 (ANIM-18). */
        void leave(float gone, float scale) {
            scaleBody(scale * (1f - (1f - LEAVE_SCALE) * gone));
            body.setAlpha(1f - gone);
        }

        /** About the body's centre: its pivot is left at the default, which follows its size. */
        private void scaleBody(float scale) {
            body.setScaleX(scale);
            body.setScaleY(scale);
        }

        private void tapped() {
            int index = shown.indexOf(this);
            if (index < 0 || closing) {
                return;
            }
            // The app grows from the card's plate (ANIM-08).
            int[] at = new int[2];
            plate.getLocationOnScreen(at);
            listener.tapped(index, new Rect(at[0], at[1], at[0] + plate.getWidth(),
                    at[1] + plate.getHeight()));
        }

        private boolean onBadge(float x, float y) {
            return badge.hit(x, y, getWidth(), getLayoutDirection() == LAYOUT_DIRECTION_RTL);
        }

        // In close mode only the ✕ acts on a card: a tap on it removes the card from Recent
        // (REC-08), and any other tap does nothing (REC-07). The long-press is still HomeViews'.
        @Override
        public boolean onTouchEvent(MotionEvent event) {
            if (!closing) {
                return super.onTouchEvent(event);
            }
            setPressed(false);
            float x = event.getX();
            float y = event.getY();
            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN:
                    badgeDown = badgeScale > 0f && onBadge(x, y);
                    break;
                case MotionEvent.ACTION_MOVE:
                    badgeDown &= onBadge(x, y);
                    break;
                case MotionEvent.ACTION_UP:
                    if (badgeDown && onBadge(x, y)) {
                        int index = shown.indexOf(this);
                        if (index >= 0) {
                            listener.badgeTapped(index);
                        }
                    }
                    badgeDown = false;
                    break;
                default:
                    badgeDown = false;
            }
            return true;
        }

        // A tap opens the app (REC-06, A11Y-05). In close mode it does nothing, and the card
        // offers "Remove ‹app› from Recent", the ✕'s name (A11Y-01).
        @Override
        public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
            super.onInitializeAccessibilityNodeInfo(info);
            if (!closing) {
                info.addAction(new AccessibilityAction(AccessibilityNodeInfo.ACTION_CLICK,
                        getResources().getString(R.string.action_open_app)));
                return;
            }
            info.setClickable(false);
            info.removeAction(AccessibilityAction.ACTION_CLICK);
            info.addAction(new AccessibilityAction(R.id.action_remove_from_recent,
                    getResources().getString(R.string.action_remove_from_recent, name)));
        }

        @Override
        public boolean performAccessibilityAction(int action, Bundle arguments) {
            if (action == R.id.action_remove_from_recent && closing) {
                int index = shown.indexOf(this);
                if (index >= 0) {
                    listener.badgeTapped(index);
                    return true;
                }
                return false;
            }
            if (action == AccessibilityNodeInfo.ACTION_CLICK && closing) {
                return false;
            }
            return super.performAccessibilityAction(action, arguments);
        }

        /** The plate, its label and age, and the ✕ over the plate's top-start corner. */
        private final class Body extends LinearLayout {
            Body(Context context) {
                super(context);
                setWillNotDraw(false);
                setClipChildren(false);
            }

            @Override
            protected void dispatchDraw(Canvas canvas) {
                super.dispatchDraw(canvas);
                boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
                float x = rtl ? plate.getRight() - badgeInsetPx : plate.getLeft() + badgeInsetPx;
                badge.draw(canvas, x, plate.getTop() + badgeInsetPx, badgeScale);
            }
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
