package io.github.twonet.meegone;

import android.graphics.Canvas;
import android.graphics.ColorFilter;
import android.graphics.Paint;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.drawable.Drawable;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.ViewTreeObserver;
import android.widget.ScrollView;

/**
 * The focus indicator (A11Y-06, ADR 0039): a ring in the accent around whatever has input focus,
 * drawn over everything in its root, which is the home views or a dialog's window. It shows only
 * out of touch mode, as Android's own focus does, so a keyboard, a D-pad or a switch sees where
 * it is and a finger never does.
 *
 * <p>Android's default highlight takes the device theme's colours and is too faint to see, so it
 * is turned off for each view that takes focus. A ripple's own faint wash stays. The ring
 * follows the view through its transforms, such as edit mode's 92 %, the menu card's place and a
 * view moving to another page, and through scrolling.
 *
 * <p>The ring is 2 dp wide, 2 dp outside the view, in the accent. Inside it is a 1 dp halo in
 * the background colour, so the ring keeps 3:1 against whatever it crosses. {@code PAIRINGS}
 * in the Rust app's tokens holds the accent to that against the background and the surface, in
 * both themes. The ring stays inside what holds the view: the nearest scrolling column or card
 * that clips it, such as the menu's, or else the root. So it never crosses onto what is behind a
 * card, and a view as big as the screen, such as a view's own element, is ringed just inside
 * the screen's edge.
 */
final class FocusRing extends Drawable implements ViewTreeObserver.OnGlobalFocusChangeListener,
        ViewTreeObserver.OnPreDrawListener, ViewTreeObserver.OnTouchModeChangeListener {
    private static final float WIDTH_DP = 2f;
    private static final float GAP_DP = 2f;
    private static final float HALO_DP = 1f;
    private static final float CORNER_DP = 8f;
    /** How far inside a column or card that holds the view the ring stays, past its halo. */
    private static final float INSIDE_DP = 4f;

    private final ViewGroup root;
    private final Paint ring = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint halo = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final float density;
    /** Where the ring is drawn, in the root's coordinates, or empty for nowhere. */
    private final RectF drawn = new RectF();
    private final RectF next = new RectF();
    private final RectF inner = new RectF();
    private final RectF holder = new RectF();

    private FocusRing(ViewGroup root) {
        this.root = root;
        density = root.getResources().getDisplayMetrics().density;
        ring.setStyle(Paint.Style.STROKE);
        ring.setStrokeWidth(WIDTH_DP * density);
        halo.setStyle(Paint.Style.STROKE);
        halo.setStrokeWidth(HALO_DP * density);
    }

    /** Draws the ring over {@code root} from now on, once {@link #colour} gives its colours. */
    static FocusRing attach(ViewGroup root) {
        FocusRing focus = new FocusRing(root);
        root.getOverlay().add(focus);
        ViewTreeObserver observer = root.getViewTreeObserver();
        observer.addOnGlobalFocusChangeListener(focus);
        observer.addOnPreDrawListener(focus);
        observer.addOnTouchModeChangeListener(focus);
        return focus;
    }

    /** The ring's colours, from the theme's tokens. */
    void colour(int accent, int background) {
        if (ring.getColor() != accent || halo.getColor() != background) {
            ring.setColor(accent);
            halo.setColor(background);
            invalidateSelf();
        }
    }

    @Override
    public void onGlobalFocusChanged(View oldFocus, View newFocus) {
        if (newFocus != null) {
            newFocus.setDefaultFocusHighlightEnabled(false);
        }
        follow();
    }

    @Override
    public void onTouchModeChanged(boolean inTouchMode) {
        follow();
    }

    @Override
    public boolean onPreDraw() {
        follow();
        return true;
    }

    /** Moves the ring to where the focused view is drawn now, if that changed. */
    private void follow() {
        View focused = root.isInTouchMode() ? null : root.findFocus();
        if (focused == null || !focused.isShown() || !bounds(focused, next)) {
            next.setEmpty();
        } else {
            float out = (GAP_DP + WIDTH_DP / 2) * density;
            next.inset(-out, -out);
            View holds = holder(focused);
            float edge;
            if (holds != null && bounds(holds, holder)) {
                edge = INSIDE_DP * density;
            } else {
                holder.set(0, 0, root.getWidth(), root.getHeight());
                edge = WIDTH_DP * density / 2;
            }
            holder.inset(edge, edge);
            if (!next.intersect(holder)) {
                next.setEmpty();
            }
        }
        setBounds(0, 0, root.getWidth(), root.getHeight());
        if (!next.equals(drawn)) {
            drawn.set(next);
            invalidateSelf();
        }
    }

    /**
     * The nearest view around {@code view} that clips what it holds to a column or a card: a
     * scroll view, or a view clipped to its outline. Null for none below the root.
     */
    private View holder(View view) {
        for (ViewParent parent = view.getParent(); parent instanceof View && parent != root;
                parent = parent.getParent()) {
            View around = (View) parent;
            if (around instanceof ScrollView || around.getClipToOutline()) {
                return around;
            }
        }
        return null;
    }

    /**
     * Sets {@code out} to where {@code view} is drawn in the root, through every transform and
     * scroll between them. Returns false if it isn't in the root.
     */
    private boolean bounds(View view, RectF out) {
        out.set(0, 0, view.getWidth(), view.getHeight());
        View at = view;
        while (at != root) {
            at.getMatrix().mapRect(out);
            out.offset(at.getLeft(), at.getTop());
            ViewParent parent = at.getParent();
            if (!(parent instanceof View)) {
                return false;
            }
            at = (View) parent;
            out.offset(-at.getScrollX(), -at.getScrollY());
        }
        return true;
    }

    /** Where the ring is drawn, in the root's coordinates, for the debug build's audit. */
    void drawnAt(Rect out) {
        drawn.roundOut(out);
    }

    @Override
    public void draw(Canvas canvas) {
        if (drawn.isEmpty()) {
            return;
        }
        float corner = CORNER_DP * density;
        canvas.drawRoundRect(drawn, corner, corner, ring);
        float in = (WIDTH_DP + HALO_DP) * density / 2;
        inner.set(drawn);
        inner.inset(in, in);
        canvas.drawRoundRect(inner, Math.max(0, corner - in), Math.max(0, corner - in), halo);
    }

    @Override
    public void setAlpha(int alpha) {}

    @Override
    public void setColorFilter(ColorFilter colorFilter) {}

    @Override
    public int getOpacity() {
        return PixelFormat.TRANSLUCENT;
    }
}
