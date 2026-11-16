package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.ColorMatrix;
import android.graphics.ColorMatrixColorFilter;
import android.graphics.Outline;
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
import android.view.ViewOutlineProvider;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import java.util.Objects;

/**
 * One cell of the Applications grid: an app's icon with its label below (APPS-01, ADR 0010).
 * It is a single view that draws both, so a grid of a few hundred apps lays out quickly
 * (PERF-04).
 *
 * <p>The icon is 56 dp, with a placeholder plate in the icons' shape until it loads (APPS-15).
 * The label sits 4 dp below it, at 12 sp in the theme's text colour, on one line and ellipsised
 * (APPS-16). The cell's height follows the font, so it grows with the system font size instead
 * of overlapping, and its 8 dp above and below leave 16 dp between rows. A pressed icon shrinks
 * (ANIM-10). An unavailable app is drawn greyed and at half opacity (APPS-09). A new app has a
 * small dot in the accent colour before its label (APPS-07). As apps come and go, the grid
 * scales, fades and slides the whole cell (ANIM-21). A picked-up icon lifts above the other
 * cells and follows the finger (APPS-11); the two never undo each other.
 *
 * <p>In edit mode (APPS-12, ADR 0014) the whole cell is drawn at 92 %, and an app the user can
 * uninstall has a ✕ over its icon's top-start corner, which pops in and out as the frame says
 * (ANIM-12). A tap within the cell's top-start 48 dp square taps the ✕ (A11Y-02, APPS-13); any
 * other tap does nothing. A lifted or settling icon takes the lift's scale instead.
 *
 * <p>Screen readers read its label, "work" for a work-profile app (A11Y-01), and "new" for a
 * new one. An app's cell offers "Open menu", which opens its long-press menu (A11Y-05). In edit
 * mode it also offers "Uninstall ‹label›" for its ✕ (A11Y-01), and "Move to start", "Move
 * earlier", "Move later" and "Move to end" instead of dragging (A11Y-05). The view itself isn't
 * long-clickable: {@link HomeViews} times long-presses.
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
    /** How far a picked-up icon rises above the other cells. */
    private static final float LIFT_DP = 8f;
    /** The ✕: a disc this wide, its centre this far inside the icon's top-start corner. */
    private static final float BADGE_DP = 24f;
    private static final float BADGE_INSET_DP = 4f;
    /** Half the length of each stroke of the ✕, and its width. */
    private static final float BADGE_ARM_DP = 4.5f;
    private static final float BADGE_STROKE_DP = 2f;
    /** A tap within this square at the cell's top-start corner taps the ✕ (A11Y-02). */
    private static final float BADGE_TARGET_DP = 48f;
    private static final ColorMatrixColorFilter GREYED;

    static {
        ColorMatrix grey = new ColorMatrix();
        grey.setSaturation(0f);
        GREYED = new ColorMatrixColorFilter(grey);
    }

    private final AppsGrid grid;
    private final TextPaint text = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    private final Paint plate = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint dot = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint badgeFill = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint badgeRing = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint badgeCross = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF plateBounds = new RectF();
    /** The font size only changes with a new configuration, which recreates the views. */
    private final int heightPx;
    private final int iconPx;
    private final int labelGapPx;
    private final int verticalPaddingPx;
    private final int sidePaddingPx;
    private final int dotPx;
    private final int dotGapPx;
    private final int liftPx;
    private final int badgePx;
    private final int badgeInsetPx;
    private final int badgeArmPx;
    private final int badgeTargetPx;

    /** Its place in the grid's cells. */
    private int index;
    private String label = "";
    /** The app it launches, or null for a folder, which #14 draws. */
    private String packageName;
    private String activity;
    private long profile;
    private boolean available = true;
    /** Installed since the grid was set up and not opened yet: it has a dot (APPS-07). */
    private boolean isNew;
    /** The user can uninstall it, so in edit mode it has a ✕ (APPS-12). */
    private boolean uninstallable;
    /** Edit mode is on: only the ✕ acts on a tap (APPS-12). */
    private boolean editing;
    /** The whole cell's scale in edit mode, and its ✕'s (ANIM-12). */
    private float editScale = 1f;
    private float badgeScale;
    /** A finger went down on the ✕ and hasn't left it. */
    private boolean badgeDown;
    /** It is lifted or settling: the lift's scale takes the place of edit mode's. */
    private boolean raised;
    /** Its cell left the grid: it only shrinks away, and can't be tapped (ANIM-21). */
    private boolean leaving;
    /** Its opacity as the grid changes, on top of the dimming of an unavailable app. */
    private float motionAlpha = 1f;
    /** Its scale and distance from its place as the grid changes (ANIM-21). */
    private float motionScale = 1f;
    private float motionDx;
    private float motionDy;
    /** Its scale and distance from its place while it is picked up (APPS-11). */
    private float liftScale = 1f;
    private float liftDx;
    private float liftDy;
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
        liftPx = Math.round(LIFT_DP * density);
        badgePx = Math.round(BADGE_DP * density);
        badgeInsetPx = Math.round(BADGE_INSET_DP * density);
        badgeArmPx = Math.round(BADGE_ARM_DP * density);
        badgeTargetPx = Math.round(BADGE_TARGET_DP * density);
        badgeRing.setStyle(Paint.Style.STROKE);
        badgeRing.setStrokeWidth(Math.max(1f, density));
        badgeCross.setStyle(Paint.Style.STROKE);
        badgeCross.setStrokeWidth(BADGE_STROKE_DP * density);
        badgeCross.setStrokeCap(Paint.Cap.ROUND);
        text.setTextSize(TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_SP, LABEL_SP, getResources().getDisplayMetrics()));
        heightPx = heightPx(context);
        setOnClickListener(view -> grid.tapped(this));
        // A picked-up icon's shadow takes the icons' shape (APPS-15).
        setOutlineProvider(new ViewOutlineProvider() {
            @Override
            public void getOutline(View view, Outline outline) {
                int left = Math.round(view.getWidth() / 2f - iconPx / 2f);
                outline.setRoundRect(left, verticalPaddingPx, left + iconPx,
                        verticalPaddingPx + iconPx, iconPx * AppIcons.CORNER);
            }
        });
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

    /**
     * Shows cell {@code index}: an app, or a folder when {@code packageName} is null. Its icon
     * is asked for again.
     */
    void bind(int index, String label, String packageName, String activity, long profile,
            boolean available, boolean work, boolean isNew, boolean uninstallable) {
        this.index = index;
        boolean labelChanged = !label.equals(this.label) || isNew != this.isNew;
        boolean otherApp = profile != this.profile
                || !Objects.equals(packageName, this.packageName)
                || !Objects.equals(activity, this.activity);
        if (otherApp) {
            icon = null;
        }
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
        boolean app = packageName != null;
        setClickable(app);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        String description = work ? getResources().getString(R.string.app_work, label) : label;
        setContentDescription(isNew
                ? getResources().getString(R.string.app_new, description)
                : description);
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
    }

    int index() {
        return index;
    }

    String label() {
        return label;
    }

    /**
     * The theme's text, placeholder, new-app dot and surface colours, as opaque ARGB. The ✕ is
     * a surface disc with a faint ring and a cross in the text colour.
     */
    void setColours(int foreground, int dim, int accent, int surface) {
        text.setColor(foreground);
        // A faint plate in the secondary text colour.
        plate.setColor((dim & 0x00FFFFFF) | 0x33000000);
        dot.setColor(accent);
        badgeFill.setColor(surface);
        badgeRing.setColor((dim & 0x00FFFFFF) | 0x66000000);
        badgeCross.setColor(foreground);
        invalidate();
    }

    /**
     * Edit mode as the frame says (APPS-12, ANIM-12): whether it is on, the whole cell's scale,
     * and its ✕'s scale, which counts only for an app the user can uninstall.
     */
    void setEdit(boolean editing, float scale, float badge) {
        if (editing != this.editing) {
            this.editing = editing;
            badgeDown = false;
        }
        if (scale != editScale) {
            editScale = scale;
            applyTransform();
        }
        float shown = hasBadge() ? badge : 0f;
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

    /**
     * How the cell is drawn while it is picked up (APPS-11): its scale, and how far it is from
     * its place, in px. {@code lifted} raises it above the other cells, with a shadow.
     */
    void setLift(float dx, float dy, float scale, boolean lifted) {
        liftScale = scale;
        liftDx = dx;
        liftDy = dy;
        raised = lifted;
        applyTransform();
        setTranslationZ(lifted ? liftPx : 0f);
    }

    /**
     * Combines the grid's motion, edit mode's scale and the lift. A lifted or settling icon is
     * drawn where the lift says, from its cell's place, at the lift's scale instead of edit
     * mode's, so the grid's motion never moves it away from the finger.
     */
    private void applyTransform() {
        float scale = motionScale * (raised ? liftScale : editScale);
        setScaleX(scale);
        setScaleY(scale);
        setTranslationX((raised ? 0f : motionDx) + liftDx);
        setTranslationY((raised ? 0f : motionDy) + liftDy);
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

    /** Whether it is an app's cell that can be used: not a folder, and not leaving. */
    private boolean hasMenu() {
        return packageName != null && !leaving;
    }

    /** Whether it has a ✕ in edit mode (APPS-12, APPS-13). */
    private boolean hasBadge() {
        return hasMenu() && uninstallable;
    }

    /** Whether ({@code x}, {@code y}) is in the ✕'s target: the cell's top-start square. */
    private boolean onBadge(float x, float y) {
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        boolean across = rtl ? x >= getWidth() - badgeTargetPx : x >= 0 && x < badgeTargetPx;
        return across && y >= 0 && y < badgeTargetPx;
    }

    // In edit mode only the ✕ acts: a tap on it asks the system to uninstall the app (APPS-13),
    // and any other tap launches nothing (APPS-12). The long-press is still HomeViews'.
    @Override
    public boolean onTouchEvent(MotionEvent event) {
        if (!editing || !hasMenu()) {
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

    // Screen readers open the app's menu with "double-tap and hold", or from their actions
    // (A11Y-05). Only the accessibility node is long-clickable. In edit mode, they uninstall and
    // move the app with actions too (A11Y-01, A11Y-05).
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
        if (changed && packageName != null && !leaving) {
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
        float centreX = getWidth() / 2f;
        int iconTop = verticalPaddingPx;
        canvas.save();
        canvas.scale(iconScale, iconScale, centreX, iconTop + iconPx / 2f);
        int iconLeft = Math.round(centreX - iconPx / 2f);
        if (icon != null) {
            icon.setBounds(iconLeft, iconTop, iconLeft + iconPx, iconTop + iconPx);
            icon.setColorFilter(available ? null : GREYED);
            icon.draw(canvas);
        } else {
            plateBounds.set(iconLeft, iconTop, iconLeft + iconPx, iconTop + iconPx);
            float corner = iconPx * AppIcons.CORNER;
            canvas.drawRoundRect(plateBounds, corner, corner, plate);
        }
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
        if (badgeScale > 0f) {
            drawBadge(canvas, iconLeft, iconTop);
        }
    }

    /**
     * The ✕ over the icon's top-start corner, inside the cell, at its pop-in scale (ANIM-12):
     * a surface disc with a faint ring and a cross in the text colour, mirrored right to left.
     */
    private void drawBadge(Canvas canvas, int iconLeft, int iconTop) {
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        float x = rtl ? iconLeft + iconPx - badgeInsetPx : iconLeft + badgeInsetPx;
        float y = iconTop + badgeInsetPx;
        float radius = badgePx / 2f;
        canvas.save();
        canvas.scale(badgeScale, badgeScale, x, y);
        canvas.drawCircle(x, y, radius, badgeFill);
        canvas.drawCircle(x, y, radius - badgeRing.getStrokeWidth() / 2f, badgeRing);
        canvas.drawLine(x - badgeArmPx, y - badgeArmPx, x + badgeArmPx, y + badgeArmPx,
                badgeCross);
        canvas.drawLine(x - badgeArmPx, y + badgeArmPx, x + badgeArmPx, y - badgeArmPx,
                badgeCross);
        canvas.restore();
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
            labelLayout = StaticLayout.Builder.obtain(label, 0, label.length(), text, width)
                    .setAlignment(Layout.Alignment.ALIGN_CENTER)
                    .setMaxLines(1)
                    .setEllipsize(TextUtils.TruncateAt.END)
                    .setIncludePad(true)
                    .build();
        }
        return labelLayout;
    }
}
