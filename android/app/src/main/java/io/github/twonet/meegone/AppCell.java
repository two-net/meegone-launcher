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
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextPaint;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.View;
import java.util.Objects;

/**
 * One cell of the Applications grid: an app's icon with its label below (APPS-01, ADR 0010).
 * It is a single view that draws both, so a grid of a few hundred apps lays out quickly
 * (PERF-04).
 *
 * <p>The icon is 56 dp, with a placeholder plate until it loads. The label sits 4 dp below it,
 * at 12 sp in the theme's text colour, on one line and ellipsised (APPS-16). The cell's height
 * follows the font, so it grows with the system font size instead of overlapping, and its
 * 8 dp above and below leave 16 dp between rows. A pressed icon shrinks (ANIM-10). An
 * unavailable app is drawn greyed and at half opacity (APPS-09).
 *
 * <p>Screen readers read its label, and "work" for a work-profile app (A11Y-01).
 */
final class AppCell extends View {
    private static final float ICON_DP = 56f;
    private static final float LABEL_GAP_DP = 4f;
    private static final float LABEL_SP = 12f;
    /** Above the icon and below the label: half the 16 dp between rows. */
    private static final float VERTICAL_PADDING_DP = 8f;
    private static final float SIDE_PADDING_DP = 4f;
    private static final float PLATE_CORNER = 0.24f;
    private static final float DIMMED_ALPHA = 0.5f;
    private static final ColorMatrixColorFilter GREYED;

    static {
        ColorMatrix grey = new ColorMatrix();
        grey.setSaturation(0f);
        GREYED = new ColorMatrixColorFilter(grey);
    }

    private final AppsGrid grid;
    private final TextPaint text = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    private final Paint plate = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF plateBounds = new RectF();
    /** The font size only changes with a new configuration, which recreates the views. */
    private final int heightPx;
    private final int iconPx;
    private final int labelGapPx;
    private final int verticalPaddingPx;
    private final int sidePaddingPx;

    /** Its place in the grid's cells. */
    private int index;
    private String label = "";
    /** The app it launches, or null for a folder, which #14 draws. */
    private String packageName;
    private String activity;
    private long profile;
    private boolean available = true;
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

    /**
     * Shows cell {@code index}: an app, or a folder when {@code packageName} is null. Its icon
     * is asked for again.
     */
    void bind(int index, String label, String packageName, String activity, long profile,
            boolean available, boolean work) {
        this.index = index;
        boolean labelChanged = !label.equals(this.label);
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
        boolean app = packageName != null;
        setClickable(app);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        setContentDescription(work
                ? getResources().getString(R.string.app_work, label)
                : label);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            setStateDescription(available
                    ? null
                    : getResources().getString(R.string.app_state_unavailable));
        }
        setAlpha(available ? 1f : DIMMED_ALPHA);
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

    /** The theme's text and placeholder colours, as opaque ARGB. */
    void setColours(int foreground, int dim) {
        text.setColor(foreground);
        // A faint plate in the secondary text colour.
        plate.setColor((dim & 0x00FFFFFF) | 0x33000000);
        invalidate();
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

    @Override
    public void setPressed(boolean pressed) {
        boolean changed = pressed != isPressed();
        super.setPressed(pressed);
        if (changed && packageName != null) {
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
            float corner = iconPx * PLATE_CORNER;
            canvas.drawRoundRect(plateBounds, corner, corner, plate);
        }
        canvas.restore();

        StaticLayout layout = labelLayout();
        if (layout != null) {
            canvas.save();
            canvas.translate(sidePaddingPx, iconTop + iconPx + labelGapPx);
            layout.draw(canvas);
            canvas.restore();
        }
    }

    /** The label on one line, ellipsised to the cell's width (APPS-16). */
    private StaticLayout labelLayout() {
        int width = getWidth() - 2 * sidePaddingPx;
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
