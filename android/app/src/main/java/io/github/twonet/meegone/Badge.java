package io.github.twonet.meegone;

import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Rect;

/**
 * The ✕ that edit mode puts on an app's icon (ADR 0014) and close mode on a Recent card (ADR
 * 0025): a disc in the surface colour with a faint ring and a cross in the text colour, at its
 * pop-in scale (ANIM-12). Each view puts it at the top-start corner of what it marks, mirrored
 * right to left. A tap within the 48 dp square at the top-start corner of the view it is on taps
 * it, 48 dp on screen whatever the view's scale (A11Y-02).
 */
final class Badge {
    /** A disc this wide. */
    private static final float SIZE_DP = 24f;
    /** Half the length of each stroke of the ✕, and its width. */
    private static final float ARM_DP = 4.5f;
    private static final float STROKE_DP = 2f;

    private final Paint fill = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint ring = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint cross = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final int sizePx;
    private final int armPx;
    private final int targetPx;

    Badge(float density) {
        sizePx = Math.round(SIZE_DP * density);
        armPx = Math.round(ARM_DP * density);
        // A tap within this square at the view's top-start corner taps the ✕ (A11Y-02).
        targetPx = Math.round(A11y.TARGET_DP * density);
        ring.setStyle(Paint.Style.STROKE);
        ring.setStrokeWidth(Math.max(1f, density));
        cross.setStyle(Paint.Style.STROKE);
        cross.setStrokeWidth(STROKE_DP * density);
        cross.setStrokeCap(Paint.Cap.ROUND);
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int surface) {
        fill.setColor(surface);
        ring.setColor((dim & 0x00FFFFFF) | 0x66000000);
        cross.setColor(foreground);
    }

    /** Draws it at {@code scale}, centred at ({@code x}, {@code y}). */
    void draw(Canvas canvas, float x, float y, float scale) {
        if (scale <= 0f) {
            return;
        }
        float radius = sizePx / 2f;
        canvas.save();
        canvas.scale(scale, scale, x, y);
        canvas.drawCircle(x, y, radius, fill);
        canvas.drawCircle(x, y, radius - ring.getStrokeWidth() / 2f, ring);
        canvas.drawLine(x - armPx, y - armPx, x + armPx, y + armPx, cross);
        canvas.drawLine(x - armPx, y + armPx, x + armPx, y - armPx, cross);
        canvas.restore();
    }

    /**
     * Whether ({@code x}, {@code y}), in a view {@code width} px wide drawn at {@code scale}, is
     * in the ✕'s target: the view's top-start square.
     */
    boolean hit(float x, float y, int width, boolean rtl, float scale) {
        return target(width, rtl, scale).contains(Math.round(x), Math.round(y));
    }

    /**
     * The ✕'s target in a view {@code width} px wide drawn at {@code scale}: the view's top-start
     * square, in its own coordinates, as big as {@link A11y#TARGET_DP} on screen.
     */
    Rect target(int width, boolean rtl, float scale) {
        int size = Math.round(targetPx / Math.max(scale, 0.01f));
        return rtl ? new Rect(width - size, 0, width, size) : new Rect(0, 0, size, size);
    }
}
