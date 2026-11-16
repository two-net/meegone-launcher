package io.github.twonet.meegone;

import android.animation.ValueAnimator;
import android.content.Context;
import android.graphics.Insets;
import android.graphics.Rect;
import android.graphics.drawable.Drawable;
import android.view.View;
import android.view.ViewGroup;
import android.widget.ScrollView;
import android.widget.Toast;

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
 */
final class AppsGrid extends ScrollView {
    // The fields of one cell in nativeAppsCells, mirrored in cell_fields in src/android.rs.
    private static final int CELL_FIELDS = 5;
    private static final int LABEL = 0;
    private static final int PACKAGE = 1;
    private static final int ACTIVITY = 2;
    private static final int PROFILE = 3;
    private static final int FLAGS = 4;
    // A cell's flags.
    private static final int IS_APP = 1;
    private static final int AVAILABLE = 2;
    private static final int WORK = 4;

    private static final float MARGIN_DP = 16f;
    private static final int NONE = -1;

    private final Cells cells;
    private final AppIcons icons;
    /** Reads the frame again and draws it, after a native call: {@code HomeViews.apply}. */
    private final Runnable redraw;
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
    /** The width last reported to the Rust app, or {@link #NONE}. */
    private int reportedWidth = NONE;
    private int pressed = NONE;
    private int foreground;
    private int dim;

    AppsGrid(Context context, Runnable redraw) {
        super(context);
        this.redraw = redraw;
        margin = Math.round(MARGIN_DP * getResources().getDisplayMetrics().density);
        rowPx = AppCell.heightPx(context);
        setClipToPadding(false);
        setOverScrollMode(OVER_SCROLL_ALWAYS);
        setVerticalScrollBarEnabled(false);
        icons = new AppIcons(context, AppCell.iconPx(context), this::refreshIcons);
        cells = new Cells(context);
        addView(cells, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
    }

    /** Keeps the cells inside the safe area, with 16 dp at each side; they scroll under the bars. */
    void setSafeArea(Insets safe) {
        setPadding(safe.left + margin, safe.top, safe.right + margin, safe.bottom);
    }

    /**
     * Draws the frame's Applications slots. {@code pressedCell} is -1 unless a cell of this
     * view is pressed, and {@code pressScale} its icon scale in ten-thousandths.
     */
    void apply(int cellsRevision, int scrollPx, int columns, int rows, int pressedCell,
            int pressScale, int foreground, int dim) {
        if (foreground != this.foreground || dim != this.dim) {
            this.foreground = foreground;
            this.dim = dim;
            for (int i = 0; i < cells.getChildCount(); i++) {
                cell(i).setColours(foreground, dim);
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

    /** Shows the cells in {@code fields}, reusing the cells already there. */
    private void bind(String[] fields) {
        int count = fields == null ? 0 : fields.length / CELL_FIELDS;
        while (cells.getChildCount() > count) {
            cells.removeViewAt(cells.getChildCount() - 1);
        }
        while (cells.getChildCount() < count) {
            AppCell cell = new AppCell(getContext(), this);
            cell.setColours(foreground, dim);
            cells.addView(cell);
        }
        for (int i = 0; i < count; i++) {
            int at = i * CELL_FIELDS;
            int flags = parseInt(fields[at + FLAGS]);
            boolean app = (flags & IS_APP) != 0;
            cell(i).bind(i, orEmpty(fields[at + LABEL]),
                    app ? orEmpty(fields[at + PACKAGE]) : null,
                    orEmpty(fields[at + ACTIVITY]),
                    parseLong(fields[at + PROFILE]),
                    (flags & AVAILABLE) != 0,
                    (flags & WORK) != 0);
        }
        // The new cells start at full size; the next frame says what is pressed.
        pressed = NONE;
        cells.requestLayout();
    }

    /** Scrolls to a position the Rust app set, unless it is the one this grid reported. */
    private void follow(int scrollPx) {
        if (scrollPx == agreedScroll) {
            return;
        }
        agreedScroll = scrollPx;
        if (!isLaidOut() || cells.getHeight() == 0) {
            pendingScroll = scrollPx;
        } else if (scrollPx == 0 && isShown() && ValueAnimator.areAnimatorsEnabled()) {
            // Home scrolls back to the top (HOME-07).
            smoothScrollTo(0, 0);
        } else {
            scrollTo(0, scrollPx);
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
            cell(i).refreshIcon();
        }
    }

    private AppCell cell(int index) {
        return (AppCell) cells.getChildAt(index);
    }

    private static String orEmpty(String text) {
        return text == null ? "" : text;
    }

    private static int parseInt(String text) {
        try {
            return Integer.parseInt(text);
        } catch (NumberFormatException e) {
            return 0;
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

        @Override
        protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
            int width = MeasureSpec.getSize(widthMeasureSpec);
            if (width > 0 && width != reportedWidth) {
                // The Rust app picks the columns for this width (APPS-02). Asked here, the
                // answer lays out in this pass; a new scroll position follows in the next frame.
                reportedWidth = width;
                nativeAppsLayout(
                        width, rowPx, getResources().getDisplayMetrics().density, laidOut);
                columns = Math.max(1, laidOut[0]);
                rows = laidOut[1];
                post(redraw);
            }
            int cellWidth = MeasureSpec.makeMeasureSpec(width / columns, MeasureSpec.EXACTLY);
            int cellHeight = MeasureSpec.makeMeasureSpec(rowPx, MeasureSpec.EXACTLY);
            for (int i = 0; i < getChildCount(); i++) {
                getChildAt(i).measure(cellWidth, cellHeight);
            }
            int rowCount = Math.max(rows, (getChildCount() + columns - 1) / columns + 1);
            setMeasuredDimension(width, rowCount * rowPx);
        }

        @Override
        protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
            int width = right - left;
            boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
            for (int i = 0; i < getChildCount(); i++) {
                int row = i / columns;
                int column = rtl ? columns - 1 - i % columns : i % columns;
                // Spread the leftover pixels, so the cells fill the width exactly.
                int cellLeft = width * column / columns;
                int cellRight = width * (column + 1) / columns;
                View cell = getChildAt(i);
                cell.layout(cellLeft, row * rowPx, cellRight, (row + 1) * rowPx);
            }
        }
    }

    /** The cells as of the last frame, each in {@link #CELL_FIELDS} fields. */
    private native String[] nativeAppsCells();

    /**
     * The grid is {@code widthPx} wide inside its margins, with rows {@code rowHeightPx} apart.
     * Fills {@code grid} with the columns and rows it now has.
     */
    private native void nativeAppsLayout(int widthPx, int rowHeightPx, float density, int[] grid);

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
}
