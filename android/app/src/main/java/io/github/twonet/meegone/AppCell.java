package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.ColorMatrix;
import android.graphics.ColorMatrixColorFilter;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.drawable.Drawable;
import android.os.Build;
import android.os.Bundle;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.MotionEvent;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import java.util.Arrays;
import java.util.Objects;

/**
 * One cell of the Applications grid or of an open folder: an app's icon with its label below
 * (APPS-01, ADR 0010), or a folder's (FOLD-02, ADR 0015). It is a single view that draws both,
 * so a grid of a few hundred apps lays out quickly (PERF-04).
 *
 * <p>The icon is 56 dp, with a placeholder plate in the icons' shape until it loads (APPS-15).
 * The label sits 4 dp below it, at 12 sp in the theme's text colour, on one line and ellipsised
 * (APPS-16). The cell's height follows the font, so it grows with the system font size instead
 * of overlapping, and its 8 dp above and below leave 16 dp between rows. A pressed icon shrinks
 * (ANIM-10). An unavailable app is drawn greyed and at half opacity (APPS-09). A new app has a
 * small dot in the accent colour before its label (APPS-07). As apps come and go, the grid
 * scales, fades and slides the whole cell (ANIM-21). While its icon is dragged, {@link
 * DragShadow} draws it, and the cell is a gap (APPS-11).
 *
 * <p>A folder's icon is a plate in the icons' shape, in the surface colour, with its first four
 * apps in a 2×2 preview whose slots match {@code preview_slot} in the Rust app. Its label is its
 * name, or "Folder" until it has one (FOLD-03). The icon a dragged app is held over grows and
 * pulses, and a folder taking in a dropped app leaves the app's slot empty until it lands; a
 * folder just made shows its first app shrinking into its slot (ANIM-14).
 *
 * <p>In edit mode (APPS-12, ADR 0014) the whole cell is drawn at 92 %, and an app the user can
 * uninstall has a ✕ over its icon's top-start corner, which pops in and out as the frame says
 * (ANIM-12). A tap within the cell's top-start 48 dp square taps the ✕ (A11Y-02, APPS-13); any
 * other tap on an app does nothing, and one on a folder opens it (FOLD-04).
 *
 * <p>Screen readers read its label, "work" for a work-profile app (A11Y-01), and "new" for a
 * new one; a folder is "Folder: ‹name›, N apps". Each cell offers "Open menu", which opens its
 * long-press menu (A11Y-05). In edit mode it also offers "Uninstall ‹label›" for an app's ✕
 * (A11Y-01), and "Move to start", "Move earlier", "Move later" and "Move to end" instead of
 * dragging (A11Y-05). An app in the grid offers "Make folder with ‹next›", or "Move into
 * ‹folder›" when the next cell is a folder, and an app in an open folder "Move out of folder".
 * The view itself isn't long-clickable: {@link HomeViews} times long-presses.
 */
final class AppCell extends View {
    private static final float ICON_DP = 56f;
    private static final float LABEL_GAP_DP = 4f;
    private static final float LABEL_SP = 12f;
    /** Above the icon and below the label: half the 16 dp between rows. */
    private static final float VERTICAL_PADDING_DP = 8f;
    private static final float SIDE_PADDING_DP = 4f;
    private static final float DOT_DP = 6f;
    private static final float DOT_GAP_DP = 4f;
    private static final float DIMMED_ALPHA = 0.5f;
    /** A folder's preview, as {@code PREVIEW_INSET} and {@code PREVIEW_GAP} in the Rust app. */
    private static final int PREVIEW_APPS = 4;
    private static final float PREVIEW_INSET = 0.14f;
    private static final float PREVIEW_GAP = 0.06f;
    private static final int NONE = -1;
    /** The ✕'s centre is this far inside the icon's top-start corner. */
    private static final float BADGE_INSET_DP = 4f;
    private static final ColorMatrixColorFilter GREYED;

    static {
        ColorMatrix grey = new ColorMatrix();
        grey.setSaturation(0f);
        GREYED = new ColorMatrixColorFilter(grey);
    }

    private final AppsGrid grid;
    private final TextPaint text = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    private final Paint plate = new Paint(Paint.ANTI_ALIAS_FLAG);
    /** A folder's plate, in the surface colour with a faint ring (FOLD-02). */
    private final Paint folderFill = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint folderRing = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint dot = new Paint(Paint.ANTI_ALIAS_FLAG);
    /** The ✕ over the icon in edit mode (ANIM-12). */
    private final Badge badge;
    private final int badgeInsetPx;
    private final RectF plateBounds = new RectF();
    /** The font size only changes with a new configuration, which recreates the views. */
    private final int heightPx;
    private final int iconPx;
    private final int labelGapPx;
    private final int verticalPaddingPx;
    private final int sidePaddingPx;
    private final int dotPx;
    private final int dotGapPx;

    /** Its place in the grid's cells. */
    private int index;
    private String label = "";
    /** The app it launches, or null for a folder. */
    private String packageName;
    private String activity;
    private long profile;
    private boolean available = true;
    /** Installed since the grid was set up and not opened yet: it has a dot (APPS-07). */
    private boolean isNew;
    /** The user can uninstall it, so in edit mode it has a ✕ (APPS-12). */
    private boolean uninstallable;
    /** A folder's apps, and the keys of its first ones: profile, package and activity. */
    private int count;
    private String[][] preview = new String[0][];
    private final Drawable[] previewIcons = new Drawable[PREVIEW_APPS];
    /** Its icon is dragged: {@link DragShadow} draws it, and the cell is a gap. */
    private boolean hidden;
    /** Its icon's scale while a dragged app is held over it (ANIM-14). */
    private float hoverScale = 1f;
    /**
     * A folder taking in a dropped app (ANIM-14): the preview slot left empty until it lands,
     * or -1, whether the folder was just made, and how far the app has gone in.
     */
    private int mergeSlot = NONE;
    private boolean mergeFormed;
    private float mergeProgress = 1f;
    /** Edit mode is on: only the ✕ acts on a tap (APPS-12). */
    private boolean editing;
    /** The whole cell's scale in edit mode, and its ✕'s (ANIM-12). */
    private float editScale = 1f;
    private float badgeScale;
    /** A finger went down on the ✕ and hasn't left it. */
    private boolean badgeDown;
    /** Its cell left the grid: it only shrinks away, and can't be tapped (ANIM-21). */
    private boolean leaving;
    /** Its opacity as the grid changes, on top of the dimming of an unavailable app. */
    private float motionAlpha = 1f;
    /** Its scale and distance from its place as the grid changes (ANIM-21). */
    private float motionScale = 1f;
    private float motionDx;
    private float motionDy;
    private Drawable icon;
    private StaticLayout labelLayout;
    private float iconScale = 1f;

    AppCell(Context context, AppsGrid grid) {
        super(context);
        this.grid = grid;
        float density = getResources().getDisplayMetrics().density;
        iconPx = Math.round(ICON_DP * density);
        labelGapPx = Math.round(LABEL_GAP_DP * density);
        verticalPaddingPx = Math.round(VERTICAL_PADDING_DP * density);
        sidePaddingPx = Math.round(SIDE_PADDING_DP * density);
        dotPx = Math.round(DOT_DP * density);
        dotGapPx = Math.round(DOT_GAP_DP * density);
        badge = new Badge(density);
        badgeInsetPx = Math.round(BADGE_INSET_DP * density);
        folderRing.setStyle(Paint.Style.STROKE);
        folderRing.setStrokeWidth(Math.max(1f, density));
        text.setTextSize(TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_SP, LABEL_SP, getResources().getDisplayMetrics()));
        heightPx = heightPx(context);
        setOnClickListener(view -> grid.tapped(this));
    }

    /** The height of every cell: the same for every label, so rows line up. */
    static int heightPx(Context context) {
        float density = context.getResources().getDisplayMetrics().density;
        TextPaint text = new TextPaint();
        text.setTextSize(TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_SP, LABEL_SP, context.getResources().getDisplayMetrics()));
        Paint.FontMetricsInt font = text.getFontMetricsInt();
        return Math.round((2 * VERTICAL_PADDING_DP + ICON_DP + LABEL_GAP_DP) * density)
                + font.bottom - font.top;
    }

    /** The icon size, in px, that {@link AppIcons} draws icons at. */
    static int iconPx(Context context) {
        return Math.round(ICON_DP * context.getResources().getDisplayMetrics().density);
    }

    /** How far below the cell's top its icon starts, in px. */
    static int iconTopPx(Context context) {
        return Math.round(
                VERTICAL_PADDING_DP * context.getResources().getDisplayMetrics().density);
    }

    /**
     * Shows cell {@code index}: an app, or a folder of {@code count} apps when {@code
     * packageName} is null, whose first apps {@code preview} names. Its icons are asked for
     * again.
     */
    void bind(int index, String label, String packageName, String activity, long profile,
            boolean available, boolean work, boolean isNew, boolean uninstallable, int count,
            String[][] preview) {
        this.index = index;
        boolean labelChanged = !label.equals(this.label) || isNew != this.isNew
                || (packageName == null) != (this.packageName == null);
        boolean otherApp = profile != this.profile
                || !Objects.equals(packageName, this.packageName)
                || !Objects.equals(activity, this.activity);
        if (otherApp) {
            icon = null;
        }
        if (!Arrays.deepEquals(preview, this.preview)) {
            // Each slot keeps its icon only while it shows the same app.
            for (int i = 0; i < PREVIEW_APPS; i++) {
                boolean same = i < preview.length && i < this.preview.length
                        && Arrays.equals(preview[i], this.preview[i]);
                if (!same) {
                    previewIcons[i] = null;
                }
            }
            this.preview = preview;
            invalidate();
        }
        this.count = count;
        this.label = label;
        this.packageName = packageName;
        this.activity = activity;
        this.profile = profile;
        this.available = available;
        this.isNew = isNew;
        if (uninstallable != this.uninstallable) {
            this.uninstallable = uninstallable;
            invalidate();
        }
        setClickable(true);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        if (isFolder()) {
            setContentDescription(getResources().getQuantityString(
                    R.plurals.folder_description, count, name(), count));
        } else {
            String description =
                    work ? getResources().getString(R.string.app_work, label) : label;
            setContentDescription(isNew
                    ? getResources().getString(R.string.app_new, description)
                    : description);
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            setStateDescription(available
                    ? null
                    : getResources().getString(R.string.app_state_unavailable));
        }
        applyAlpha();
        if (labelChanged) {
            labelLayout = null;
        }
        setIconScale(1f);
        refreshIcon();
        invalidate();
    }

    /**
     * Asks for the icon again, as when icons arrive or change. The icon shown stays until its
     * replacement arrives, so an updated app doesn't flash its placeholder.
     */
    void refreshIcon() {
        Drawable found = packageName == null ? null : grid.icon(profile, packageName, activity);
        if (found != null && found != icon) {
            icon = found;
            invalidate();
        }
        for (int i = 0; i < Math.min(PREVIEW_APPS, preview.length); i++) {
            String[] key = preview[i];
            Drawable slot = grid.icon(AppsGrid.parseLong(key[0]), key[1], key[2]);
            if (slot != null && slot != previewIcons[i]) {
                previewIcons[i] = slot;
                invalidate();
            }
        }
    }

    int index() {
        return index;
    }

    AppsGrid grid() {
        return grid;
    }

    /** Its label as it is stored: a folder's is "" until the user names it (FOLD-03). */
    String label() {
        return label;
    }

    /** Its name as it shows: its label, or "Folder" for a folder with none (FOLD-03). */
    String name() {
        return isFolder() && label.isEmpty() ? getResources().getString(R.string.folder) : label;
    }

    boolean isFolder() {
        return packageName == null;
    }

    /**
     * The theme's text, placeholder, new-app dot and surface colours, as opaque ARGB. The ✕ is
     * a surface disc with a faint ring and a cross in the text colour.
     */
    void setColours(int foreground, int dim, int accent, int surface) {
        text.setColor(foreground);
        folderFill.setColor(surface);
        folderRing.setColor((dim & 0x00FFFFFF) | 0x40000000);
        // A faint plate in the secondary text colour.
        plate.setColor((dim & 0x00FFFFFF) | 0x33000000);
        dot.setColor(accent);
        badge.colour(foreground, dim, surface);
        invalidate();
    }

    /**
     * Edit mode as the frame says (APPS-12, ANIM-12): whether it is on, the whole cell's scale,
     * and its ✕'s scale, which counts only for an app the user can uninstall.
     */
    void setEdit(boolean editing, float scale, float popped) {
        if (editing != this.editing) {
            this.editing = editing;
            badgeDown = false;
        }
        if (scale != editScale) {
            editScale = scale;
            applyTransform();
        }
        float shown = hasBadge() ? popped : 0f;
        if (shown != badgeScale) {
            badgeScale = shown;
            invalidate();
        }
    }

    /** Where the grid's motion draws it now, from its laid-out place, in px (ANIM-21). */
    float motionX() {
        return motionDx;
    }

    float motionY() {
        return motionDy;
    }

    /**
     * How the cell is drawn as the grid changes (ANIM-21): its scale and opacity, and how far it
     * is from its place, in px.
     */
    void setMotion(float scale, float alpha, float dx, float dy) {
        motionScale = scale;
        motionDx = dx;
        motionDy = dy;
        applyTransform();
        if (alpha != motionAlpha) {
            motionAlpha = alpha;
            applyAlpha();
        }
    }

    /** Leaves the cell as a gap while {@link DragShadow} draws its icon (ADR 0015). */
    void setHidden(boolean hidden) {
        if (hidden != this.hidden) {
            this.hidden = hidden;
            invalidate();
        }
    }

    /** Its icon's scale while a dragged app is held over it (ANIM-14). */
    void setHoverScale(float scale) {
        if (scale != hoverScale) {
            hoverScale = scale;
            invalidate();
        }
    }

    /**
     * A folder taking in a dropped app (ANIM-14): preview slot {@code slot} stays empty until
     * {@code progress} reaches 1, or none at -1, and if it was just {@code formed}, its first
     * app shrinks into slot 0 from the whole icon as the plate fades in.
     */
    void setMerge(int slot, boolean formed, float progress) {
        if (slot != mergeSlot || formed != mergeFormed || progress != mergeProgress) {
            mergeSlot = slot;
            mergeFormed = formed;
            mergeProgress = progress;
            invalidate();
        }
    }

    /** Combines the grid's motion and edit mode's scale. */
    private void applyTransform() {
        float scale = motionScale * editScale;
        setScaleX(scale);
        setScaleY(scale);
        setTranslationX(motionDx);
        setTranslationY(motionDy);
    }

    /** Its cell left the grid: it shrinks away where it was, and can't be used (ANIM-21). */
    void leave() {
        leaving = true;
        setClickable(false);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
    }

    private void applyAlpha() {
        setAlpha((available ? 1f : DIMMED_ALPHA) * motionAlpha);
    }

    /** The icon's scale while it is pressed (ANIM-10). */
    void setIconScale(float scale) {
        if (scale != iconScale) {
            iconScale = scale;
            invalidate();
        }
    }

    /** Where the icon is on the screen, which a launch grows from (ANIM-08). */
    Rect iconOnScreen() {
        int[] at = new int[2];
        getLocationOnScreen(at);
        int left = at[0] + (getWidth() - iconPx) / 2;
        int top = at[1] + verticalPaddingPx;
        return new Rect(left, top, left + iconPx, top + iconPx);
    }

    /**
     * Whether it is a cell that can be used, with a menu: not leaving, and not one of search's
     * results, which have none (ADR 0016).
     */
    private boolean hasMenu() {
        return !leaving && grid.surface() != AppsGrid.SEARCH;
    }

    /** Whether it has a ✕ in edit mode (APPS-12, APPS-13): an app the user can uninstall. */
    private boolean hasBadge() {
        return hasMenu() && !isFolder() && uninstallable;
    }

    /** Whether ({@code x}, {@code y}) is in the ✕'s target: the cell's top-start square. */
    private boolean onBadge(float x, float y) {
        return badge.hit(x, y, getWidth(), getLayoutDirection() == LAYOUT_DIRECTION_RTL);
    }

    // In edit mode only the ✕ acts on an app: a tap on it asks the system to uninstall the app
    // (APPS-13), and any other tap launches nothing (APPS-12). A folder still opens. The
    // long-press is still HomeViews'.
    @Override
    public boolean onTouchEvent(MotionEvent event) {
        if (!editing || !hasMenu() || isFolder()) {
            return super.onTouchEvent(event);
        }
        float x = event.getX();
        float y = event.getY();
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                badgeDown = hasBadge() && badgeScale > 0f && onBadge(x, y);
                break;
            case MotionEvent.ACTION_MOVE:
                badgeDown &= onBadge(x, y);
                break;
            case MotionEvent.ACTION_UP:
                if (badgeDown && onBadge(x, y)) {
                    grid.badgeTapped(this);
                }
                badgeDown = false;
                break;
            default:
                badgeDown = false;
        }
        return true;
    }

    // Screen readers open the cell's menu with "double-tap and hold", or from their actions
    // (A11Y-05). Only the accessibility node is long-clickable. In edit mode, they uninstall and
    // move it, and put apps into folders and take them out, with actions too (A11Y-01, A11Y-05).
    @Override
    public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        if (!hasMenu()) {
            return;
        }
        info.setLongClickable(true);
        info.addAction(new AccessibilityAction(AccessibilityNodeInfo.ACTION_LONG_CLICK,
                getResources().getString(R.string.action_app_menu)));
        if (!editing) {
            return;
        }
        if (hasBadge()) {
            info.addAction(new AccessibilityAction(R.id.action_uninstall,
                    getResources().getString(R.string.action_uninstall, label)));
        }
        if (index > 0) {
            addAction(info, R.id.action_move_first, R.string.action_move_first);
            addAction(info, R.id.action_move_earlier, R.string.action_move_earlier);
        }
        if (index < grid.cellCount() - 1) {
            addAction(info, R.id.action_move_later, R.string.action_move_later);
            addAction(info, R.id.action_move_last, R.string.action_move_last);
        }
        if (isFolder()) {
            return;
        }
        AppCell next = grid.cellAt(index + 1);
        if (grid.surface() == AppsGrid.GRID && next != null) {
            int text = next.isFolder() ? R.string.action_move_into : R.string.action_make_folder;
            info.addAction(new AccessibilityAction(
                    R.id.action_group, getResources().getString(text, next.name())));
        } else if (grid.surface() == AppsGrid.FOLDER) {
            addAction(info, R.id.action_move_out, R.string.action_move_out);
        }
    }

    private void addAction(AccessibilityNodeInfo info, int id, int text) {
        info.addAction(new AccessibilityAction(id, getResources().getString(text)));
    }

    @Override
    public boolean performAccessibilityAction(int action, Bundle arguments) {
        if (action == AccessibilityNodeInfo.ACTION_LONG_CLICK && hasMenu()) {
            return grid.menuRequested(this);
        }
        if (editing && hasMenu()) {
            if (action == R.id.action_uninstall && hasBadge()) {
                grid.badgeTapped(this);
                return true;
            }
            if (action == R.id.action_group && !isFolder()) {
                return grid.grouped(this);
            }
            if (action == R.id.action_move_out && !isFolder()) {
                return grid.movedOut(this);
            }
            int to = moveTo(action);
            if (to >= 0) {
                return grid.moveTo(this, to);
            }
        }
        return super.performAccessibilityAction(action, arguments);
    }

    /** The place a screen-reader move action takes it to, or -1 for any other action. */
    private int moveTo(int action) {
        if (action == R.id.action_move_first) {
            return 0;
        } else if (action == R.id.action_move_earlier) {
            return Math.max(0, index - 1);
        } else if (action == R.id.action_move_later) {
            return index + 1;
        } else if (action == R.id.action_move_last) {
            return Math.max(0, grid.cellCount() - 1);
        }
        return -1;
    }

    @Override
    public void setPressed(boolean pressed) {
        boolean changed = pressed != isPressed();
        super.setPressed(pressed);
        if (changed && !leaving) {
            grid.pressChanged(this, pressed);
        }
    }

    @Override
    public boolean hasOverlappingRendering() {
        // The icon and label never overlap, so dimming needs no offscreen layer.
        return false;
    }

    @Override
    protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        setMeasuredDimension(MeasureSpec.getSize(widthMeasureSpec), heightPx);
    }

    @Override
    protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        labelLayout = null;
    }

    @Override
    protected void onDraw(Canvas canvas) {
        if (hidden) {
            return;
        }
        float centreX = getWidth() / 2f;
        int iconTop = verticalPaddingPx;
        canvas.save();
        float scale = iconScale * hoverScale;
        canvas.scale(scale, scale, centreX, iconTop + iconPx / 2f);
        int iconLeft = Math.round(centreX - iconPx / 2f);
        drawIcon(canvas, iconLeft, iconTop, iconPx);
        canvas.restore();

        StaticLayout layout = labelLayout();
        if (layout != null) {
            canvas.save();
            canvas.translate(sidePaddingPx + dotRoom(), iconTop + iconPx + labelGapPx);
            layout.draw(canvas);
            if (isNew) {
                drawDot(canvas, layout);
            }
            canvas.restore();
        }
        // The ✕ over the icon's top-start corner, inside the cell (ANIM-12).
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        badge.draw(canvas, rtl ? iconLeft + iconPx - badgeInsetPx : iconLeft + badgeInsetPx,
                iconTop + badgeInsetPx, badgeScale);
    }

    /**
     * Draws its icon in the square at ({@code left}, {@code top}), {@code size} px wide: the
     * app's, or a folder's plate and preview. {@link DragShadow} draws a dragged icon with it.
     */
    void drawIcon(Canvas canvas, float left, float top, float size) {
        float corner = size * AppIcons.CORNER;
        plateBounds.set(left, top, left + size, top + size);
        if (isFolder()) {
            drawFolder(canvas, corner, size);
        } else if (icon != null) {
            icon.setBounds(Math.round(left), Math.round(top), Math.round(left + size),
                    Math.round(top + size));
            icon.setColorFilter(available ? null : GREYED);
            icon.draw(canvas);
        } else {
            canvas.drawRoundRect(plateBounds, corner, corner, plate);
        }
    }

    /**
     * A folder's icon in {@link #plateBounds} (FOLD-02): the plate, then its first apps in the
     * preview's slots, in reading order and mirrored right to left. A slot an app is going into
     * stays empty until it lands, and the first app of a folder just made shrinks into its slot
     * from the whole icon as the plate fades in (ANIM-14).
     */
    private void drawFolder(Canvas canvas, float corner, float size) {
        float left = plateBounds.left;
        float top = plateBounds.top;
        boolean forming = mergeFormed && mergeProgress < 1f;
        int fillAlpha = folderFill.getAlpha();
        int ringAlpha = folderRing.getAlpha();
        if (forming) {
            folderFill.setAlpha(Math.round(fillAlpha * mergeProgress));
            folderRing.setAlpha(Math.round(ringAlpha * mergeProgress));
        }
        canvas.drawRoundRect(plateBounds, corner, corner, folderFill);
        float ring = folderRing.getStrokeWidth() / 2f;
        canvas.drawRoundRect(left + ring, top + ring, left + size - ring, top + size - ring,
                corner, corner, folderRing);
        folderFill.setAlpha(fillAlpha);
        folderRing.setAlpha(ringAlpha);
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        float slotSize = (1f - 2f * PREVIEW_INSET - PREVIEW_GAP) / 2f;
        for (int i = 0; i < Math.min(PREVIEW_APPS, preview.length); i++) {
            if (i == mergeSlot && mergeProgress < 1f) {
                continue;
            }
            int column = rtl ? 1 - i % 2 : i % 2;
            int row = i / 2;
            float x = left + size * (PREVIEW_INSET + column * (slotSize + PREVIEW_GAP));
            float y = top + size * (PREVIEW_INSET + row * (slotSize + PREVIEW_GAP));
            float width = size * slotSize;
            if (forming && i == 0) {
                // From the whole icon into the slot.
                float t = mergeProgress;
                x = left + (x - left) * t;
                y = top + (y - top) * t;
                width = size + (width - size) * t;
            }
            Drawable slot = previewIcons[i];
            if (slot != null) {
                slot.setBounds(Math.round(x), Math.round(y), Math.round(x + width),
                        Math.round(y + width));
                slot.setColorFilter(null);
                slot.draw(canvas);
            } else {
                float slotCorner = width * AppIcons.CORNER;
                canvas.drawRoundRect(x, y, x + width, y + width, slotCorner, slotCorner, plate);
            }
        }
    }

    /** The room kept at each side of a new app's label, so the label and its dot stay centred. */
    private int dotRoom() {
        return isNew ? dotPx + dotGapPx : 0;
    }

    /** The new-app dot, before the label's first letter, mid-height (APPS-07). */
    private void drawDot(Canvas canvas, StaticLayout layout) {
        float radius = dotPx / 2f;
        float x = getLayoutDirection() == LAYOUT_DIRECTION_RTL
                ? layout.getLineRight(0) + dotGapPx + radius
                : layout.getLineLeft(0) - dotGapPx - radius;
        float y = (layout.getLineTop(0) + layout.getLineBottom(0)) / 2f;
        canvas.drawCircle(x, y, radius, dot);
    }

    /** The label on one line, ellipsised to the cell's width (APPS-16). */
    private StaticLayout labelLayout() {
        int width = getWidth() - 2 * (sidePaddingPx + dotRoom());
        if (labelLayout == null && width > 0) {
            String shown = name();
            labelLayout = StaticLayout.Builder.obtain(shown, 0, shown.length(), text, width)
                    .setAlignment(Layout.Alignment.ALIGN_CENTER)
                    .setMaxLines(1)
                    .setEllipsize(TextUtils.TruncateAt.END)
                    .setIncludePad(true)
                    .build();
        }
        return labelLayout;
    }
}
