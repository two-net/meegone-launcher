package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Outline;
import android.view.View;
import android.view.ViewOutlineProvider;

/**
 * The dragged icon (APPS-11, ADR 0015): drawn above the pages, so it can go anywhere on the
 * screen, out of an open folder's panel and over the grid. It is one icon's size and draws the
 * icon of the cell it was picked up from, which stays as a gap, with the platform's shadow in
 * the icons' shape (APPS-15).
 *
 * <p>The Rust app says where its centre is, from the home views' top left, its scale and its
 * opacity, as it follows the finger, settles into its place (ANIM-13) or shrinks into a folder
 * (ANIM-14). It keeps the last cell it was given, so it still draws an app that has just gone
 * into a folder, until the frame says it has landed.
 */
final class DragShadow extends View {
    private static final float ELEVATION_DP = 8f;

    private final int iconPx;
    /** The cell whose icon it draws, or null. */
    private AppCell cell;

    DragShadow(Context context) {
        super(context);
        iconPx = AppCell.iconPx(context);
        setElevation(ELEVATION_DP * getResources().getDisplayMetrics().density);
        setOutlineProvider(new ViewOutlineProvider() {
            @Override
            public void getOutline(View view, Outline outline) {
                outline.setRoundRect(0, 0, iconPx, iconPx, iconPx * AppIcons.CORNER);
            }
        });
        // Touches go through it to the views below; screen readers never see it.
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        setVisibility(GONE);
    }

    /** One icon's size, wherever it is placed. */
    @Override
    protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        setMeasuredDimension(iconPx, iconPx);
    }

    /**
     * Draws {@code from}'s icon, or the last cell's when it is null, with its centre at ({@code
     * x}, {@code y}) px, at {@code scale} ten-thousandths and {@code alpha} 0 to 255; {@code
     * shown} false hides it.
     */
    void show(boolean shown, AppCell from, int x, int y, int scale, int alpha) {
        if (from != null && from != cell) {
            cell = from;
            invalidate();
        }
        if (!shown || cell == null) {
            cell = shown ? cell : null;
            setVisibility(GONE);
            return;
        }
        setVisibility(VISIBLE);
        setTranslationX(x - iconPx / 2f);
        setTranslationY(y - iconPx / 2f);
        setScaleX(scale / 10_000f);
        setScaleY(scale / 10_000f);
        setAlpha(alpha / 255f);
        invalidate();
    }

    @Override
    protected void onDraw(Canvas canvas) {
        if (cell != null) {
            cell.drawIcon(canvas, 0f, 0f, iconPx);
        }
    }
}
