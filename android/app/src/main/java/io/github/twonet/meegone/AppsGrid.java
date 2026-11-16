package io.github.twonet.meegone;

import android.animation.ValueAnimator;
import android.content.Context;
import android.graphics.Insets;
import android.graphics.Rect;
import android.graphics.drawable.Drawable;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
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
 * A grid of cells that scrolls vertically: the Applications view, with every app and folder and
 * no pages and no dock (APPS-01 to APPS-03, ADR 0010), an open folder's apps in its panel
 * (FOLD-04, ADR 0015), or search's results (SRCH-02, ADR 0016). One class draws all three; each
 * is built for its surface, and names it to the Rust app in every call. Search's results have
 * no menus and are never dragged.
 *
 * <p>The Rust app decides what it shows, as for the rest of the home views (ADR 0006). {@link
 * HomeViews} passes it the frame's slots for its surface. When the cells' revision changes, it
 * reads the cells with {@link #nativeAppsCells}. It lays them out in the columns and rows the
 * Rust app gives, the grid's last row empty (APPS-02, HOME-14), and tells the Rust app how it is
 * laid out whenever that changes: its width and row height, the room above and below its cells,
 * its height, where it is on screen and where each icon is in its cell (ADR 0014). It reports how
 * far it is scrolled, and follows a new position only when it didn't report it itself, as after
 * Home (HOME-07), a restart (REL-05), a new layout or auto-scroll.
 *
 * <p>The Applications grid scrolls under the system bars, inside 16 dp margins, and stretches at
 * its ends as the platform does (ANIM-23). Scrolling hands the gesture away from the carousel
 * through {@link #requestDisallowInterceptTouchEvent}, as {@link HomeViews} expects. A tap
 * launches an app at once (PERF-06) or opens a folder (FOLD-04), and a pressed icon shrinks
 * (ANIM-10).
 *
 * <p>{@link HomeViews} times a long-press: on a cell, it asks the cell's grid to open its menu
 * (APPS-11, ADR 0013). A dragged icon is drawn above everything by {@link DragShadow}; its cell
 * here is only hidden, as a gap (ADR 0015). The icon a dragged app is held over grows and pulses,
 * and a folder taking in a dropped app shows it shrinking into its preview (ANIM-14).
 *
 * <p>Each cell says where it was in the cells before, so its view, icon and all, moves to its
 * new place instead of another app being drawn there (ADR 0012); a folder made from an app, or
 * an app a folder turned back into, keeps the view. As cells come, go and move, new cells grow
 * and fade in, the views no cell took shrink away at their old places, and the others slide to
 * their new ones, from where they were drawn, as far as the frame says (ANIM-21, ANIM-13). Only
 * a change of this grid's own plays here.
 *
 * <p>In edit mode (APPS-12, ADR 0014) the cells shrink and the ✕ badges pop in row by row, as
 * the frame says (ANIM-12). A tap on a ✕ asks the system to uninstall the app (APPS-13), and
 * screen readers move icons, make folders and take apps out of them with actions (A11Y-05).
 */
final class AppsGrid extends ScrollView {
    /** The surfaces, as the Rust app numbers them. */
    static final int GRID = 0;
    static final int FOLDER = 1;
    static final int SEARCH = 2;

    // The fields of one cell in nativeAppsCells, mirrored in cell_fields in src/android.rs.
    private static final int CELL_FIELDS = 8;
    private static final int LABEL = 0;
    private static final int PACKAGE = 1;
    private static final int ACTIVITY = 2;
    private static final int PROFILE = 3;
    private static final int FLAGS = 4;
    private static final int FROM = 5; // its place in the cells before, or -1 if it just joined
    private static final int COUNT = 6; // a folder's apps
    private static final int PREVIEW = 7; // a folder's first apps: profile/package/activity lines
    // A cell's flags.
    private static final int IS_APP = 1;
    private static final int AVAILABLE = 2;
    private static final int WORK = 4;
    private static final int NEW = 8;
    private static final int UNINSTALLABLE = 16; // has a ✕ in edit mode
    private static final float WHOLE = 10_000f; // ten-thousandths

    private static final float MARGIN_DP = 16f;
    private static final int NONE = -1;

    private final int surface;
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
    /** A change of this grid's cells is playing: only then do the frame's motion values apply. */
    private boolean changing;
    private final AppIcons icons;
    /** Reads the frame again and draws it, after a native call: {@code HomeViews.apply}. */
    private final Runnable redraw;
    /** Tells screen readers of a change, such as a move (A11Y-08). */
    private final Consumer<String> announce;
    /** The view whose top left the Rust app measures the screen from: {@link HomeViews}. */
    private View screen;
    private final int margin;
    private final int rowPx;
    private final int iconPx;
    private final int iconTopPx;
    private final int[] laidOut = new int[2];
    /**
     * For the open folder's grid: where its panel is, from the screen's top left. For search's
     * results: where the search field's band is (ANIM-16).
     */
    private final Rect panel = new Rect();

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
    private final int[] reported = new int[13];
    private int pressed = NONE;
    /** The dragged icon's cell, hidden as a gap while the shadow draws it, or null. */
    private AppCell lifted;
    /** The cell a dragged app is held over, which grows and pulses, or null (ANIM-14). */
    private AppCell hovered;
    /** The folder taking in a dropped app, or null (ANIM-14). */
    private AppCell merging;
    private int foreground;
    private int dim;
    private int accent;
    private int plate;
    /** Edit mode as the frame says, and every cell's scale (APPS-12, ANIM-12). */
    private boolean editing;
    private float editScale = 1f;
    /** The grid's first visible row, and the ✕ scale of each visible row from it. */
    private int badgeFirstRow;
    private float[] badgeScales = new float[0];

    AppsGrid(Context context, int surface, AppIcons icons, Runnable redraw,
            Consumer<String> announce) {
        super(context);
        this.surface = surface;
        this.icons = icons;
        this.redraw = redraw;
        this.announce = announce;
        Arrays.fill(reported, NONE);
        margin = Math.round(MARGIN_DP * getResources().getDisplayMetrics().density);
        rowPx = AppCell.heightPx(context);
        iconPx = AppCell.iconPx(context);
        iconTopPx = AppCell.iconTopPx(context);
        setClipToPadding(false);
        setOverScrollMode(surface == GRID ? OVER_SCROLL_ALWAYS : OVER_SCROLL_IF_CONTENT_SCROLLS);
        setVerticalScrollBarEnabled(false);
        cells = new Cells(context);
        addView(cells, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
    }

    /** The view the screen's coordinates start from: the home views. */
    void setScreen(View screen) {
        this.screen = screen;
    }

    int surface() {
        return surface;
    }

    /** Keeps the cells inside the safe area, with 16 dp at each side; they scroll under the bars. */
    void setSafeArea(Insets safe) {
        setPadding(safe.left + margin, safe.top, safe.right + margin, safe.bottom);
        reportAgain();
    }

    /** The open folder's panel, or the search field's band, is at {@code bounds} on screen. */
    void setPanel(Rect bounds) {
        if (!bounds.equals(panel)) {
            panel.set(bounds);
            reportAgain();
        }
    }

    /** One cell's width, in px: the open folder's panel is four of them wide. */
    int cellWidth() {
        return cellsWidth > 0 ? cellsWidth / Math.max(1, columns) : 0;
    }

    int rowPx() {
        return rowPx;
    }

    @Override
    protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        reportAgain();
    }

    /**
     * Tells the Rust app how the grid is laid out now (APPS-02), if anything changed: the cells'
     * width, the room above and below them, the grid's height, where it is on screen and where
     * each icon is in its cell, which the drag's places and auto-scroll need (ADR 0014), and, for
     * the open folder, where its panel is (ADR 0015). Returns whether it did. The answer, the
     * columns and rows, takes effect at once.
     */
    private boolean report(int width) {
        int[] at = onScreen();
        int[] now = {width, rowPx, getPaddingTop(), getPaddingBottom(), getHeight(),
                at[0] + getPaddingLeft(), at[1], iconPx, iconTopPx,
                panel.left, panel.top, panel.right, panel.bottom};
        if (Arrays.equals(now, reported)) {
            return false;
        }
        System.arraycopy(now, 0, reported, 0, now.length);
        nativeAppsLayout(surface, getResources().getDisplayMetrics().density, now, laidOut);
        columns = Math.max(1, laidOut[0]);
        rows = laidOut[1];
        return true;
    }

    /**
     * Where this grid's top left is, from the screen's: laid-out places only, without the
     * carousel's or the panel's motion, which are at rest whenever a drag can start.
     */
    private int[] onScreen() {
        return offsetIn(this, screen);
    }

    /**
     * Where {@code view}'s top left is laid out, from {@code screen}'s, an ancestor of it, with
     * every scroll between them but no translation or scale.
     */
    static int[] offsetIn(View view, View screen) {
        int x = 0;
        int y = 0;
        while (view != null && view != screen) {
            x += view.getLeft();
            y += view.getTop();
            ViewParent parent = view.getParent();
            if (!(parent instanceof View)) {
                break;
            }
            view = (View) parent;
            if (view != screen) {
                x -= view.getScrollX();
                y -= view.getScrollY();
            }
        }
        return new int[] {x, y};
    }

    /** Reports a new padding, height or place, once the cells' width is known. */
    private void reportAgain() {
        if (cellsWidth > 0 && report(cellsWidth)) {
            cells.requestLayout();
            post(redraw);
        }
    }

    /**
     * Draws the frame's slots for this grid. {@code pressedCell} is -1 unless a cell of it is
     * pressed, and {@code pressScale} its icon scale in ten-thousandths. {@code accent} colours
     * the dot of a new app (APPS-07), and {@code plate} the ✕ badges (APPS-12) and folders'
     * icons (FOLD-02).
     */
    void apply(int cellsRevision, int scrollPx, int columns, int rows, int pressedCell,
            int pressScale, int foreground, int dim, int accent, int plate) {
        if (foreground != this.foreground || dim != this.dim || accent != this.accent
                || plate != this.plate) {
            this.foreground = foreground;
            this.dim = dim;
            this.accent = accent;
            this.plate = plate;
            for (int i = 0; i < cells.getChildCount(); i++) {
                ((AppCell) cells.getChildAt(i)).setColours(foreground, dim, accent, plate);
            }
        }
        if (!loaded || cellsRevision != revision) {
            loaded = true;
            revision = cellsRevision;
            bind(nativeAppsCells(surface));
        }
        if (columns != this.columns || rows != this.rows) {
            this.columns = Math.max(1, columns);
            this.rows = rows;
            cells.requestLayout();
        }
        follow(scrollPx);
        press(pressedCell, pressScale / WHOLE);
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
            changing = true;
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
                    (flags & UNINSTALLABLE) != 0,
                    parseInt(fields[at + COUNT], 0),
                    preview(fields[at + PREVIEW]));
        }
        // The new cells start at full size; the next frame says what is pressed.
        pressed = NONE;
        cells.requestLayout();
    }

    /** A folder's preview field as its apps' keys: profile, package and activity each. */
    private static String[][] preview(String field) {
        if (field == null || field.isEmpty()) {
            return new String[0][];
        }
        String[] lines = field.split("\n");
        List<String[]> keys = new ArrayList<>();
        for (String line : lines) {
            String[] key = line.split("/", 3);
            if (key.length == 3) {
                keys.add(key);
            }
        }
        return keys.toArray(new String[0][]);
    }

    /**
     * Puts the views in the cells' new order: the view of the cell each was before, or a new one
     * for a cell that just joined. The views no cell took leave after them, except the dragged
     * one, which went into a folder: its shadow shrinks into the preview instead. Each view it
     * keeps slides from where it is drawn now, part-way through an earlier change or not.
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
                cell.setColours(foreground, dim, accent, plate);
                shown.add(cell);
                fromX[i] = Float.NaN;
                fromY[i] = Float.NaN;
            }
        }
        List<AppCell> gone = new ArrayList<>();
        int left = 0;
        leftFrom = new int[before.size()];
        for (int place = 0; place < before.size(); place++) {
            if (taken[place]) {
                continue;
            }
            AppCell cell = before.get(place);
            if (cell == lifted) {
                gone.add(cell);
                lifted = null;
            } else {
                cell.leave();
                leaving.add(cell);
                leftFrom[left++] = place;
            }
        }
        cells.arrange(before);
        for (AppCell cell : gone) {
            cells.removeView(cell);
        }
    }

    private void dropLeaving() {
        for (AppCell cell : leaving) {
            cells.removeView(cell);
        }
        leaving.clear();
        leftFrom = new int[0];
    }

    /**
     * Draws the cells as they come, go and move, from the frame's values in ten-thousandths
     * (ANIM-21, ANIM-13), while a change of this grid's own plays. Cells that just joined take
     * the appearing scale and opacity, and those that left the leaving ones, at their old
     * places. Every other cell is {@code shift} of the way from where it was drawn to its new
     * place. Leaving views go once they have faded out.
     */
    void move(int appearScale, int appearAlpha, int leaveScale, int leaveAlpha, int shift) {
        if (!changing) {
            return;
        }
        boolean atRest = appearScale == WHOLE && appearAlpha == WHOLE && leaveAlpha <= 0
                && shift == WHOLE;
        float still = 1f - shift / WHOLE;
        for (int i = 0; i < shown.size(); i++) {
            AppCell cell = shown.get(i);
            float x = i < fromX.length ? fromX[i] : cells.left(i);
            float y = i < fromY.length ? fromY[i] : cells.top(i);
            if (cell == lifted || atRest) {
                // The dragged icon's gap is where its cell is, at once.
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
        // Once it has played, a change on the other surface replays nothing here.
        changing = !atRest;
    }

    /** Scrolls to a position the Rust app set, unless it is the one this grid reported. */
    private void follow(int scrollPx) {
        if (scrollPx == agreedScroll) {
            return;
        }
        agreedScroll = scrollPx;
        if (!isLaidOut() || cells.getHeight() == 0) {
            pendingScroll = scrollPx;
        } else if (scrollPx == 0 && lifted == null && surface != SEARCH && isShown()
                && ValueAnimator.areAnimatorsEnabled()) {
            // Home scrolls back to the top (HOME-07). A dragged icon's auto-scroll reaching it
            // follows the frame exactly instead, and a new query's results start at their top.
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
     * Leaves cell {@code index} as a gap while {@link DragShadow} draws its icon (ADR 0015), or
     * shows every cell when it is -1.
     */
    void hide(int index) {
        AppCell cell = cellAt(index);
        if (lifted != null && lifted != cell) {
            lifted.setHidden(false);
        }
        lifted = cell;
        if (cell != null) {
            cell.setHidden(true);
        }
    }

    /** Grows cell {@code index}'s icon to {@code scale} ten-thousandths; none at -1 (ANIM-14). */
    void hover(int index, int scale) {
        AppCell cell = cellAt(index);
        if (hovered != null && hovered != cell) {
            hovered.setHoverScale(1f);
        }
        hovered = cell;
        if (cell != null) {
            cell.setHoverScale(scale / WHOLE);
        }
    }

    /**
     * Shows folder {@code index} taking in a dropped app at preview slot {@code slot}, -1 if it
     * has none, {@code progress} ten-thousandths of the way, its first app shrinking into slot 0
     * too if it was just {@code formed} (ANIM-14). -1 shows none.
     */
    void merge(int index, int slot, boolean formed, int progress) {
        AppCell cell = cellAt(index);
        if (merging != null && merging != cell) {
            merging.setMerge(NONE, false, 1f);
        }
        merging = cell;
        if (cell != null) {
            cell.setMerge(slot, formed, progress / WHOLE);
        }
    }

    private void press(int index, float scale) {
        if (pressed != index && pressed >= 0 && pressed < shown.size()) {
            cell(pressed).setIconScale(1f);
        }
        pressed = index;
        if (index >= 0 && index < shown.size()) {
            cell(index).setIconScale(scale);
        }
    }

    @Override
    protected void onScrollChanged(int left, int top, int oldLeft, int oldTop) {
        super.onScrollChanged(left, top, oldLeft, oldTop);
        if (top != agreedScroll) {
            agreedScroll = top;
            nativeAppsScrolled(surface, top);
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
        // Where it is on screen is known only now.
        reportAgain();
    }

    /** A cell was tapped: its app launches from its icon (APPS-10, ANIM-08), or it opens. */
    void tapped(AppCell cell) {
        Rect icon = cell.iconOnScreen();
        String message = nativeAppTapped(surface, revision, cell.index(), icon.left, icon.top,
                icon.right, icon.bottom);
        redraw.run();
        if (message != null) {
            // The system gave no explanation of its own (APPS-09).
            Toast.makeText(getContext(), message, Toast.LENGTH_SHORT).show();
        }
    }

    /**
     * The finger has been held on a cell for the system's long-press time: its menu opens
     * (APPS-11). Returns whether the long-press was taken, so the cell gets no more of the
     * gesture and lifting the finger taps nothing.
     */
    boolean longPressed(AppCell cell) {
        return nativeAppLongPressed(surface, revision, cell.index());
    }

    /**
     * The ✕ of a cell was tapped, or its screen-reader action used (APPS-13): the system asks
     * the user to confirm. If it refuses, its message shows briefly.
     */
    void badgeTapped(AppCell cell) {
        String message = nativeAppBadgeTapped(surface, revision, cell.index());
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
        String label = cell.name();
        int place = nativeAppMoved(surface, revision, cell.index(), to);
        redraw.run();
        if (place < 0) {
            return false;
        }
        announce.accept(getResources().getString(
                R.string.app_moved, label, place + 1, shown.size()));
        return true;
    }

    /**
     * A screen reader put a cell's app into a folder with the next cell (A11Y-05): "Make folder
     * with…" or "Move into…". Returns whether it did.
     */
    boolean grouped(AppCell cell) {
        String label = cell.name();
        int folder = nativeAppGrouped(revision, cell.index());
        redraw.run();
        if (folder < 0) {
            return false;
        }
        announce.accept(getResources().getString(R.string.app_grouped, label));
        return true;
    }

    /** A screen reader took a cell's app out of the open folder (A11Y-05). */
    boolean movedOut(AppCell cell) {
        String label = cell.name();
        int place = nativeAppMovedOut(revision, cell.index());
        redraw.run();
        if (place < 0) {
            return false;
        }
        announce.accept(getResources().getString(R.string.app_moved_out, label));
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

    /** A screen reader asked for a cell's menu (A11Y-05). Returns whether it opened. */
    boolean menuRequested(AppCell cell) {
        boolean opened = nativeAppMenuRequested(surface, revision, cell.index());
        redraw.run();
        return opened;
    }

    /** Cell {@code index}'s view, or null if there is none. */
    AppCell cellAt(int index) {
        return index >= 0 && index < shown.size() ? shown.get(index) : null;
    }

    /** Cell {@code index}'s name as it shows, or null if there is none. */
    String label(int index) {
        AppCell cell = cellAt(index);
        return cell == null ? null : cell.name();
    }

    /** A finger went down on a cell or left it (ANIM-10). */
    void pressChanged(AppCell cell, boolean down) {
        if (down) {
            nativeAppPressed(surface, cell.index(), System.nanoTime());
        } else {
            nativeAppReleased(System.nanoTime());
        }
        redraw.run();
    }

    /** An app's icon, or null until it loads. */
    Drawable icon(long profile, String packageName, String activity) {
        return icons.get(profile, packageName, activity);
    }

    /** The icons arrived or changed: each cell asks for its own again. */
    void refreshIcons() {
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

    static long parseLong(String text) {
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
            // A view no cell took and that isn't leaving is still attached, until it is removed.
            for (AppCell view : before) {
                if (!shown.contains(view) && !leaving.contains(view)) {
                    put(view, index++, children);
                }
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
            // The grid always ends with an empty row (HOME-14); a folder's panel doesn't.
            int extra = surface == GRID ? 1 : 0;
            int rowCount = Math.max(rows, (shown.size() + columns - 1) / columns + extra);
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

    /** The cells of {@code surface} as of the last frame, each in {@link #CELL_FIELDS} fields. */
    private native String[] nativeAppsCells(int surface);

    /**
     * The grid on {@code surface} was laid out, as {@code layout} says, in the order listed in
     * src/android.rs. Fills {@code grid} with the columns and rows it now has.
     */
    private native void nativeAppsLayout(int surface, float density, int[] layout, int[] grid);

    /** The user scrolled the grid on {@code surface} this far from its top. */
    private native void nativeAppsScrolled(int surface, int px);

    /** A finger went down on cell {@code index}, at this {@code System.nanoTime()}. */
    private native void nativeAppPressed(int surface, int index, long timeNanos);

    /** The finger lifted or left the pressed cell, at this {@code System.nanoTime()}. */
    private native void nativeAppReleased(long timeNanos);

    /**
     * Cell {@code index} of the cells at {@code revision} was tapped, with its icon at this
     * rect on the screen. Returns null, or a short message to show.
     */
    private native String nativeAppTapped(int surface, int revision, int index, int left,
            int top, int right, int bottom);

    /**
     * The finger has been held on cell {@code index} of the cells at {@code revision} for the
     * system's long-press time. Returns whether the long-press was taken.
     */
    private native boolean nativeAppLongPressed(int surface, int revision, int index);

    /**
     * Opens the menu of cell {@code index} of the cells at {@code revision} without a gesture.
     * Returns whether it opened.
     */
    private native boolean nativeAppMenuRequested(int surface, int revision, int index);

    /**
     * The ✕ of cell {@code index} of the cells at {@code revision} was tapped. Returns null, or
     * a short message to show.
     */
    private native String nativeAppBadgeTapped(int surface, int revision, int index);

    /**
     * Moves cell {@code index} of the cells at {@code revision} to place {@code to}. Returns its
     * place now, or -1 if nothing moved.
     */
    private native int nativeAppMoved(int surface, int revision, int index, int to);

    /**
     * Puts the app of grid cell {@code index} of the cells at {@code revision} into a folder
     * with the cell after it. Returns the folder's place, or -1 if nothing happened.
     */
    private native int nativeAppGrouped(int revision, int index);

    /**
     * Takes the app of the open folder's cell {@code index} of the cells at {@code revision} out
     * of the folder. Returns its place in the grid, or -1 if nothing moved.
     */
    private native int nativeAppMovedOut(int revision, int index);

    /** Done was tapped: edit mode ends. */
    private native void nativeEditDone();
}
