package io.github.twonet.meegone;

import android.animation.ValueAnimator;
import android.content.Context;
import android.graphics.Insets;
import android.graphics.Rect;
import android.graphics.drawable.Drawable;
import android.view.View;
import android.view.ViewGroup;
import android.view.accessibility.AccessibilityEvent;
import android.widget.ScrollView;
import android.widget.Toast;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.function.Consumer;

/**
 * The Applications view: every app in one grid that scrolls vertically, with no pages and no
 * dock (APPS-01 to APPS-03, ADR 0010).
 *
 * <p>The Rust app decides what it shows, as for the rest of the home views (ADR 0006). {@link
 * HomeViews} passes it the frame's Applications slots. When the cells' revision changes, it
 * reads the cells with {@link #nativeAppsCells}. It lays them out in the columns and rows the
 * Rust app gives, the last row empty (APPS-02, HOME-14), and tells the Rust app its width and
 * row height whenever they change. It reports how far it is scrolled, and follows a new
 * position only when it didn't report it itself, as after Home (HOME-07), a restart (REL-05) or
 * a new layout.
 *
 * <p>It scrolls under the system bars, inside 16 dp margins, and stretches at its ends as the
 * platform does (ANIM-23). Scrolling hands the gesture away from the carousel through {@link
 * #requestDisallowInterceptTouchEvent}, as {@link HomeViews} expects. A tap launches the app at
 * once (PERF-06), and a pressed icon shrinks (ANIM-10).
 *
 * <p>{@link HomeViews} times a long-press: on an app's icon, it asks this grid to open the app's
 * menu (APPS-11, ADR 0013). Dragging on picks the icon up, and the frame says how far it has
 * moved from its cell; it lifts above the other cells and is never cut off at the grid's edges.
 *
 * <p>Each cell says where it was in the cells before, so its view, icon and all, moves to its
 * new place instead of another app being drawn there (ADR 0012). As apps come and go, new cells
 * grow and fade in, the views no cell took shrink away at their old places, and the others slide
 * to their new ones, as far as the frame says (ANIM-21). Each slides from where it is drawn when
 * the change comes, so a change that comes while another plays never makes a cell jump.
 *
 * <p>In edit mode (APPS-12, ADR 0014) the cells shrink and the ✕ badges pop in row by row, as
 * the frame says (ANIM-12). A dropped icon moves the cells the same way, and the dragged one
 * stays under the finger, even as auto-scroll moves the grid. A tap on a ✕ asks the system to
 * uninstall the app (APPS-13), and screen readers move icons with actions (A11Y-05).
 */
final class AppsGrid extends ScrollView {
    // The fields of one cell in nativeAppsCells, mirrored in cell_fields in src/android.rs.
    private static final int CELL_FIELDS = 6;
    private static final int LABEL = 0;
    private static final int PACKAGE = 1;
    private static final int ACTIVITY = 2;
    private static final int PROFILE = 3;
    private static final int FLAGS = 4;
    private static final int FROM = 5; // its place in the cells before, or -1 if it just joined
    // A cell's flags.
    private static final int IS_APP = 1;
    private static final int AVAILABLE = 2;
    private static final int WORK = 4;
    private static final int NEW = 8;
    private static final int UNINSTALLABLE = 16; // has a ✕ in edit mode
    private static final float WHOLE = 10_000f; // ten-thousandths

    private static final float MARGIN_DP = 16f;
    private static final int NONE = -1;

    private final Cells cells;
    /** The cells' views, in order. They are the first of the grid's children. */
    private final List<AppCell> shown = new ArrayList<>();
    /**
     * Where each shown cell was drawn when the cells last changed, from the cells' top left, in
     * px; NaN for a cell that just joined, which grows in where it is.
     */
    private float[] fromX = new float[0];
    private float[] fromY = new float[0];
    /** Views of cells that left the grid, shrinking away after the shown ones (ANIM-21). */
    private final List<AppCell> leaving = new ArrayList<>();
    /** Each leaving view's place in the cells before. */
    private int[] leftFrom = new int[0];
    /** Every cell is in its place at full size, so frames at rest change nothing. */
    private boolean settled = true;
    private final AppIcons icons;
    /** Reads the frame again and draws it, after a native call: {@code HomeViews.apply}. */
    private final Runnable redraw;
    /** Tells screen readers of a change, such as a move (A11Y-08). */
    private final Consumer<String> announce;
    private final int margin;
    private final int rowPx;
    private final int[] laidOut = new int[2];

    private boolean loaded;
    private int revision;
    private int columns = 4;
    private int rows = 1;
    /** The scroll position this grid and the Rust app last agreed on. */
    private int agreedScroll;
    /** A position from the Rust app to take once the grid is laid out, or {@link #NONE}. */
    private int pendingScroll = NONE;
    /** The cells' width, and what was last reported to the Rust app; {@link #NONE} for none. */
    private int cellsWidth = NONE;
    private final int[] reported = {NONE, NONE, NONE, NONE};
    private int pressed = NONE;
    /** The picked-up icon's view, or null (APPS-11). */
    private AppCell lifted;
    private int foreground;
    private int dim;
    private int accent;
    private int surface;
    /** Edit mode as the frame says, and every cell's scale (APPS-12, ANIM-12). */
    private boolean editing;
    private float editScale = 1f;
    /** The grid's first visible row, and the ✕ scale of each visible row from it. */
    private int badgeFirstRow;
    private float[] badgeScales = new float[0];

    AppsGrid(Context context, Runnable redraw, Consumer<String> announce) {
        super(context);
        this.redraw = redraw;
        this.announce = announce;
        margin = Math.round(MARGIN_DP * getResources().getDisplayMetrics().density);
        rowPx = AppCell.heightPx(context);
        setClipToPadding(false);
        setOverScrollMode(OVER_SCROLL_ALWAYS);
        setVerticalScrollBarEnabled(false);
        icons = new AppIcons(context, AppCell.iconPx(context), this::refreshIcons);
        cells = new Cells(context);
        // A picked-up icon may be dragged anywhere on the page.
        cells.setClipChildren(false);
        addView(cells, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
    }

    /** Keeps the cells inside the safe area, with 16 dp at each side; they scroll under the bars. */
    void setSafeArea(Insets safe) {
        setPadding(safe.left + margin, safe.top, safe.right + margin, safe.bottom);
        reportAgain();
    }

    @Override
    protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        reportAgain();
    }

    /**
     * Tells the Rust app how the grid is laid out now (APPS-02), if anything changed: the cells'
     * width, the room above and below them, and the grid's height, which the drag's places and
     * auto-scroll need (ADR 0014). Returns whether it did. The answer, the columns and rows,
     * takes effect at once.
     */
    private boolean report(int width) {
        int[] now = {width, getPaddingTop(), getPaddingBottom(), getHeight()};
        if (Arrays.equals(now, reported)) {
            return false;
        }
        System.arraycopy(now, 0, reported, 0, now.length);
        nativeAppsLayout(width, rowPx, getResources().getDisplayMetrics().density, now[1], now[2],
                now[3], laidOut);
        columns = Math.max(1, laidOut[0]);
        rows = laidOut[1];
        return true;
    }

    /** Reports a new padding or height, once the cells' width is known. */
    private void reportAgain() {
        if (cellsWidth > 0 && report(cellsWidth)) {
            cells.requestLayout();
            post(redraw);
        }
    }

    /**
     * Draws the frame's Applications slots. {@code pressedCell} is -1 unless a cell of this
     * view is pressed, and {@code pressScale} its icon scale in ten-thousandths. {@code accent}
     * colours the dot of a new app (APPS-07), and {@code surface} the ✕ badges (APPS-12).
     */
    void apply(int cellsRevision, int scrollPx, int columns, int rows, int pressedCell,
            int pressScale, int foreground, int dim, int accent, int surface) {
        if (foreground != this.foreground || dim != this.dim || accent != this.accent
                || surface != this.surface) {
            this.foreground = foreground;
            this.dim = dim;
            this.accent = accent;
            this.surface = surface;
            for (int i = 0; i < cells.getChildCount(); i++) {
                ((AppCell) cells.getChildAt(i)).setColours(foreground, dim, accent, surface);
            }
        }
        if (!loaded || cellsRevision != revision) {
            loaded = true;
            revision = cellsRevision;
            bind(nativeAppsCells());
        }
        if (columns != this.columns || rows != this.rows) {
            this.columns = Math.max(1, columns);
            this.rows = rows;
            cells.requestLayout();
        }
        follow(scrollPx);
        press(pressedCell, pressScale / 10_000f);
    }

    /**
     * Shows the cells in {@code fields}. A cell that was there before keeps its view, so its
     * icon goes with it; views that no cell takes start leaving (ANIM-21).
     */
    private void bind(String[] fields) {
        int count = fields == null ? 0 : fields.length / CELL_FIELDS;
        int[] places = new int[count];
        boolean moved = count != shown.size();
        for (int i = 0; i < count; i++) {
            places[i] = parseInt(fields[i * CELL_FIELDS + FROM], NONE);
            moved |= places[i] != i;
        }
        // When every cell keeps its place, the cells were only refreshed, as when a new app's
        // accent arrives just after it was installed. A change still playing carries on: each
        // cell keeps where it came from, and views still leaving stay.
        if (moved) {
            // Views still leaving after an earlier change go at once.
            dropLeaving();
            rearrange(places);
        }
        for (int i = 0; i < count; i++) {
            int at = i * CELL_FIELDS;
            int flags = parseInt(fields[at + FLAGS], 0);
            boolean app = (flags & IS_APP) != 0;
            cell(i).bind(i, orEmpty(fields[at + LABEL]),
                    app ? orEmpty(fields[at + PACKAGE]) : null,
                    orEmpty(fields[at + ACTIVITY]),
                    parseLong(fields[at + PROFILE]),
                    (flags & AVAILABLE) != 0,
                    (flags & WORK) != 0,
                    (flags & NEW) != 0,
                    (flags & UNINSTALLABLE) != 0);
        }
        // The new cells start at full size; the next frame says what is pressed.
        pressed = NONE;
        settled = false;
        cells.requestLayout();
    }

    /**
     * Puts the views in the cells' new order: the view of the cell each was before, or a new one
     * for a cell that just joined. The views no cell took leave after them. Each view it keeps
     * slides from where it is drawn now, part-way through an earlier change or not.
     */
    private void rearrange(int[] places) {
        List<AppCell> before = new ArrayList<>(shown);
        boolean[] taken = new boolean[before.size()];
        shown.clear();
        fromX = new float[places.length];
        fromY = new float[places.length];
        for (int i = 0; i < places.length; i++) {
            int place = places[i];
            if (place >= 0 && place < before.size() && !taken[place]) {
                taken[place] = true;
                AppCell cell = before.get(place);
                shown.add(cell);
                boolean drawn = cell.isLaidOut();
                fromX[i] = drawn ? cell.getLeft() + cell.motionX() : Float.NaN;
                fromY[i] = drawn ? cell.getTop() + cell.motionY() : Float.NaN;
            } else {
                AppCell cell = new AppCell(getContext(), this);
                cell.setColours(foreground, dim, accent, surface);
                shown.add(cell);
                fromX[i] = Float.NaN;
                fromY[i] = Float.NaN;
            }
        }
        int left = 0;
        leftFrom = new int[before.size()];
        for (int place = 0; place < before.size(); place++) {
            if (!taken[place]) {
                AppCell cell = before.get(place);
                cell.leave();
                leaving.add(cell);
                leftFrom[left++] = place;
            }
        }
        cells.arrange(before);
    }

    private void dropLeaving() {
        for (AppCell cell : leaving) {
            cells.removeView(cell);
        }
        leaving.clear();
        leftFrom = new int[0];
    }

    /**
     * Draws the cells as apps come, go and move, from the frame's values in ten-thousandths
     * (ANIM-21, ANIM-13). Cells that just joined take the appearing scale and opacity, and those
     * that left the leaving ones, at their old places. Every other cell is {@code shift} of the
     * way from where it was drawn to its new place. Leaving views go once they have faded out.
     */
    void move(int appearScale, int appearAlpha, int leaveScale, int leaveAlpha, int shift) {
        boolean atRest = appearScale == WHOLE && appearAlpha == WHOLE && leaveAlpha <= 0
                && shift == WHOLE;
        if (settled && atRest) {
            return;
        }
        float still = 1f - shift / WHOLE;
        for (int i = 0; i < shown.size(); i++) {
            AppCell cell = shown.get(i);
            float x = i < fromX.length ? fromX[i] : cells.left(i);
            float y = i < fromY.length ? fromY[i] : cells.top(i);
            if (cell == lifted) {
                // The dragged icon follows the finger, wherever its cell moves (ANIM-13).
                cell.setMotion(1f, 1f, 0f, 0f);
            } else if (Float.isNaN(x) || Float.isNaN(y)) {
                cell.setMotion(appearScale / WHOLE, appearAlpha / WHOLE, 0f, 0f);
            } else {
                cell.setMotion(1f, 1f, still * (x - cells.left(i)), still * (y - cells.top(i)));
            }
        }
        if (leaveAlpha <= 0) {
            dropLeaving();
        } else {
            for (AppCell cell : leaving) {
                cell.setMotion(leaveScale / WHOLE, leaveAlpha / WHOLE, 0f, 0f);
            }
        }
        settled = atRest;
    }

    /** Scrolls to a position the Rust app set, unless it is the one this grid reported. */
    private void follow(int scrollPx) {
        if (scrollPx == agreedScroll) {
            return;
        }
        agreedScroll = scrollPx;
        if (!isLaidOut() || cells.getHeight() == 0) {
            pendingScroll = scrollPx;
        } else if (scrollPx == 0 && lifted == null && isShown()
                && ValueAnimator.areAnimatorsEnabled()) {
            // Home scrolls back to the top (HOME-07). A dragged icon's auto-scroll reaching it
            // follows the frame exactly instead.
            smoothScrollTo(0, 0);
        } else {
            scrollTo(0, scrollPx);
        }
    }

    /**
     * Draws edit mode (APPS-12, ANIM-12): whether it is on, every cell's scale in ten-thousandths,
     * and the ✕ scale of each visible row from {@code firstRow}, in {@code frame} from {@code
     * badgesAt} on, {@code badgeRows} of them. Rows further down take the last one. When it
     * turns on or off, screen readers read the cells' actions again.
     */
    void edit(boolean on, int scale, int firstRow, int[] frame, int badgesAt, int badgeRows) {
        boolean changed = on != editing;
        editing = on;
        editScale = scale / WHOLE;
        badgeFirstRow = firstRow;
        if (badgeScales.length != badgeRows) {
            badgeScales = new float[badgeRows];
        }
        for (int i = 0; i < badgeRows; i++) {
            badgeScales[i] = frame[badgesAt + i] / WHOLE;
        }
        for (int i = 0; i < shown.size(); i++) {
            AppCell cell = shown.get(i);
            cell.setEdit(editing, editScale, badgeScale(i));
            if (changed) {
                cell.sendAccessibilityEvent(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
            }
        }
    }

    /** The ✕ scale of cell {@code index}, from its row's place among the visible ones. */
    private float badgeScale(int index) {
        if (badgeScales.length == 0) {
            return 0f;
        }
        int rank = index / columns - badgeFirstRow;
        return badgeScales[Math.max(0, Math.min(rank, badgeScales.length - 1))];
    }

    /**
     * Draws the picked-up icon: cell {@code index}, {@code dx} and {@code dy} px from its place,
     * at {@code scale} ten-thousandths (APPS-11). It stays raised until it has settled into its
     * cell (ANIM-13). -1 puts every icon back in its cell.
     */
    void lift(int index, int dx, int dy, int scale) {
        AppCell cell = index >= 0 && index < shown.size() ? shown.get(index) : null;
        if (lifted != null && lifted != cell) {
            lifted.setLift(0f, 0f, 1f, false);
        }
        lifted = cell;
        if (cell != null) {
            cell.setLift(dx, dy, scale / WHOLE, true);
        }
    }

    private void press(int index, float scale) {
        if (pressed != index && pressed >= 0 && pressed < cells.getChildCount()) {
            cell(pressed).setIconScale(1f);
        }
        pressed = index;
        if (index >= 0 && index < cells.getChildCount()) {
            cell(index).setIconScale(scale);
        }
    }

    @Override
    protected void onScrollChanged(int left, int top, int oldLeft, int oldTop) {
        super.onScrollChanged(left, top, oldLeft, oldTop);
        if (top != agreedScroll) {
            agreedScroll = top;
            nativeAppsScrolled(top);
        }
    }

    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        super.onLayout(changed, left, top, right, bottom);
        if (pendingScroll != NONE) {
            int target = pendingScroll;
            pendingScroll = NONE;
            scrollTo(0, target);
        }
    }

    @Override
    protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        icons.watch();
    }

    @Override
    protected void onDetachedFromWindow() {
        icons.stopWatching();
        super.onDetachedFromWindow();
    }

    /** A cell was tapped: its app launches from its icon (APPS-10, ANIM-08). */
    void tapped(AppCell cell) {
        Rect icon = cell.iconOnScreen();
        String message = nativeAppTapped(
                revision, cell.index(), icon.left, icon.top, icon.right, icon.bottom);
        redraw.run();
        if (message != null) {
            // The system gave no explanation of its own (APPS-09).
            Toast.makeText(getContext(), message, Toast.LENGTH_SHORT).show();
        }
    }

    /**
     * The finger has been held on an app's cell for the system's long-press time: its menu opens
     * (APPS-11). Returns whether the long-press was taken, so the cell gets no more of the
     * gesture and lifting the finger launches nothing.
     */
    boolean longPressed(AppCell cell) {
        return nativeAppLongPressed(revision, cell.index());
    }

    /**
     * The ✕ of a cell was tapped, or its screen-reader action used (APPS-13): the system asks
     * the user to confirm. If it refuses, its message shows briefly.
     */
    void badgeTapped(AppCell cell) {
        String message = nativeAppBadgeTapped(revision, cell.index());
        redraw.run();
        if (message != null) {
            Toast.makeText(getContext(), message, Toast.LENGTH_SHORT).show();
        }
    }

    /**
     * A screen reader moved a cell to place {@code to} (A11Y-05). Its new place is announced, as
     * "Mail, place 3 of 40" (A11Y-08). Returns whether it moved.
     */
    boolean moveTo(AppCell cell, int to) {
        String label = cell.label();
        int place = nativeAppMoved(revision, cell.index(), to);
        redraw.run();
        if (place < 0) {
            return false;
        }
        announce.accept(getResources().getString(
                R.string.app_moved, label, place + 1, shown.size()));
        return true;
    }

    /** Done: edit mode ends (APPS-12). */
    void done() {
        nativeEditDone();
        redraw.run();
    }

    /** How many cells the grid shows. */
    int cellCount() {
        return shown.size();
    }

    /** A screen reader asked for an app's menu (A11Y-05). Returns whether it opened. */
    boolean menuRequested(AppCell cell) {
        boolean opened = nativeAppMenuRequested(revision, cell.index());
        redraw.run();
        return opened;
    }

    /** Cell {@code index}'s view, or null if there is none. */
    View cellView(int index) {
        return index >= 0 && index < shown.size() ? shown.get(index) : null;
    }

    /** Cell {@code index}'s label, or null if there is none. */
    String label(int index) {
        return index >= 0 && index < shown.size() ? shown.get(index).label() : null;
    }

    /** A finger went down on a cell or left it (ANIM-10). */
    void pressChanged(AppCell cell, boolean down) {
        if (down) {
            nativeAppPressed(cell.index(), System.nanoTime());
        } else {
            nativeAppReleased(System.nanoTime());
        }
        redraw.run();
    }

    /** An app's icon, or null until it loads. */
    Drawable icon(long profile, String packageName, String activity) {
        return icons.get(profile, packageName, activity);
    }

    private void refreshIcons() {
        for (int i = 0; i < cells.getChildCount(); i++) {
            ((AppCell) cells.getChildAt(i)).refreshIcon();
        }
    }

    private AppCell cell(int index) {
        return shown.get(index);
    }

    private static String orEmpty(String text) {
        return text == null ? "" : text;
    }

    private static int parseInt(String text, int otherwise) {
        try {
            return Integer.parseInt(text);
        } catch (NumberFormatException e) {
            return otherwise;
        }
    }

    private static long parseLong(String text) {
        try {
            return Long.parseLong(text);
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    /** The cells, in rows of equal height; a right-to-left layout mirrors the columns. */
    private final class Cells extends ViewGroup {
        Cells(Context context) {
            super(context);
        }

        /**
         * Makes the children the shown views, in order, then the leaving ones. Views that were
         * children, {@code before}, are moved without leaving the window, so they keep their
         * icons and state.
         */
        void arrange(List<AppCell> before) {
            Set<View> children = new HashSet<>(before);
            detachAllViewsFromParent();
            int index = 0;
            for (AppCell view : shown) {
                put(view, index++, children);
            }
            for (AppCell view : leaving) {
                put(view, index++, children);
            }
            requestLayout();
            invalidate();
        }

        private void put(View view, int index, Set<View> children) {
            if (children.contains(view)) {
                attachViewToParent(view, index, view.getLayoutParams());
            } else {
                addViewInLayout(view, index, generateDefaultLayoutParams(), true);
            }
        }

        /** The column cell {@code index} is drawn in, from the left: mirrored right to left. */
        private int column(int index) {
            int column = index % columns;
            return getLayoutDirection() == LAYOUT_DIRECTION_RTL ? columns - 1 - column : column;
        }

        /** Where cell {@code index} starts, from this view's left, in px. */
        int left(int index) {
            // Spread the leftover pixels, so the cells fill the width exactly.
            return getWidth() * column(index) / columns;
        }

        /** Where cell {@code index} starts, from this view's top, in px. */
        int top(int index) {
            return index / columns * rowPx;
        }

        @Override
        protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
            int width = MeasureSpec.getSize(widthMeasureSpec);
            if (width > 0) {
                cellsWidth = width;
                // The Rust app picks the columns for this width (APPS-02). Asked here, the
                // answer lays out in this pass; a new scroll position follows in the next frame.
                if (report(width)) {
                    post(redraw);
                }
            }
            int cellWidth = MeasureSpec.makeMeasureSpec(width / columns, MeasureSpec.EXACTLY);
            int cellHeight = MeasureSpec.makeMeasureSpec(rowPx, MeasureSpec.EXACTLY);
            for (int i = 0; i < getChildCount(); i++) {
                getChildAt(i).measure(cellWidth, cellHeight);
            }
            int rowCount = Math.max(rows, (shown.size() + columns - 1) / columns + 1);
            setMeasuredDimension(width, rowCount * rowPx);
        }

        @Override
        protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
            for (int i = 0; i < shown.size(); i++) {
                place(shown.get(i), i, right - left);
            }
            // Leaving views stay where they were until they have faded out (ANIM-21).
            for (int i = 0; i < leaving.size(); i++) {
                place(leaving.get(i), leftFrom[i], right - left);
            }
        }

        private void place(View cell, int index, int width) {
            int column = column(index);
            cell.layout(width * column / columns, top(index), width * (column + 1) / columns,
                    top(index) + rowPx);
        }
    }

    /** The cells as of the last frame, each in {@link #CELL_FIELDS} fields. */
    private native String[] nativeAppsCells();

    /**
     * The grid is {@code widthPx} wide inside its margins, with rows {@code rowHeightPx} apart,
     * {@code topPx} and {@code bottomPx} of room above and below its cells, and {@code heightPx}
     * tall. Fills {@code grid} with the columns and rows it now has.
     */
    private native void nativeAppsLayout(int widthPx, int rowHeightPx, float density, int topPx,
            int bottomPx, int heightPx, int[] grid);

    /** The user scrolled the grid this far from its top. */
    private native void nativeAppsScrolled(int px);

    /** A finger went down on cell {@code index}, at this {@code System.nanoTime()}. */
    private native void nativeAppPressed(int index, long timeNanos);

    /** The finger lifted or left the pressed cell, at this {@code System.nanoTime()}. */
    private native void nativeAppReleased(long timeNanos);

    /**
     * Cell {@code index} of the cells at {@code revision} was tapped, with its icon at this
     * rect on the screen. Returns null, or a short message to show.
     */
    private native String nativeAppTapped(
            int revision, int index, int left, int top, int right, int bottom);

    /**
     * The finger has been held on cell {@code index} of the cells at {@code revision} for the
     * system's long-press time. Returns whether the long-press was taken.
     */
    private native boolean nativeAppLongPressed(int revision, int index);

    /**
     * Opens the menu of cell {@code index} of the cells at {@code revision} without a gesture.
     * Returns whether it opened.
     */
    private native boolean nativeAppMenuRequested(int revision, int index);

    /**
     * The ✕ of cell {@code index} of the cells at {@code revision} was tapped. Returns null, or
     * a short message to show.
     */
    private native String nativeAppBadgeTapped(int revision, int index);

    /**
     * Moves cell {@code index} of the cells at {@code revision} to place {@code to}. Returns its
     * place now, or -1 if nothing moved.
     */
    private native int nativeAppMoved(int revision, int index, int to);

    /** Done was tapped: edit mode ends. */
    private native void nativeEditDone();
}
