package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Insets;
import android.graphics.Rect;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewConfiguration;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.function.IntConsumer;

/**
 * An open folder (FOLD-04, ADR 0015): a black scrim over the Applications grid, and a panel in
 * the surface colour with 12 dp corners and no shadow, centred in the safe area. The panel is
 * four of the grid's cells wide, with the folder's name above its own {@link AppsGrid}, and as
 * tall as its rows, up to what fits; then its grid scrolls. A tap on the name asks to rename the
 * folder (FOLD-03).
 *
 * <p>It is inside the Applications page, so it moves with it, and the home views find what is
 * under a finger in it as for the grid. It grows from the folder's icon as it opens and shrinks
 * back into it as it closes, as far as the frame says (ANIM-15). Only while it is open does it
 * take touches: a tap outside the panel closes it, and goes nowhere else. While it closes,
 * touches reach the grid below.
 */
final class FolderPanel extends FrameLayout {
    private static final float CORNER_DP = 12f;
    private static final float SIDE_DP = 8f;
    private static final float BOTTOM_DP = 12f;
    private static final float NAME_SP = 18f;
    private static final float MIN_TARGET_DP = 48f;
    /** How far the panel stays from the safe area's edges. */
    private static final float MARGIN_DP = 16f;
    private static final float WHOLE = 10_000f;

    private final View scrim;
    private final LinearLayout panel;
    private final GradientDrawable card;
    private final TextView name;
    private final AppsGrid grid;
    /** Where the folder's grid is: the Applications grid, whose cells set the panel's width. */
    private final AppsGrid apps;
    private final IntConsumer closed;
    private final int margin;
    private final int sides;
    private final int touchSlop;
    /** The panel's place, as reported to the Rust app, from the home views' top left. */
    private final Rect bounds = new Rect();
    /** The folder's icon, which the panel grows from, in this view's coordinates. */
    private final Rect from = new Rect();

    /** The folder shown, open or closing, or -1. */
    private int folder = -1;
    private boolean open;
    private float progress;
    private int scrimAlpha;
    private int foreground = -1;
    private int surface = -1;
    /** A touch outside the panel: where it went down, and whether it is still a tap. */
    private float downX;
    private float downY;
    private boolean tapping;
    private View screen;

    /**
     * {@code grid} is the folder's own grid, and {@code apps} the Applications grid. {@code
     * renameRequested} and {@code closed} take the folder's id.
     */
    FolderPanel(Context context, AppsGrid grid, AppsGrid apps, IntConsumer renameRequested,
            IntConsumer closed) {
        super(context);
        this.grid = grid;
        this.apps = apps;
        this.closed = closed;
        float density = getResources().getDisplayMetrics().density;
        margin = Math.round(MARGIN_DP * density);
        sides = Math.round(SIDE_DP * density);
        touchSlop = ViewConfiguration.get(context).getScaledTouchSlop();
        setVisibility(GONE);
        // The scrim covers the whole page, the safe area's padding included.
        setClipToPadding(false);

        scrim = new View(context);
        scrim.setBackgroundColor(Color.BLACK);
        scrim.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        addView(scrim, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));

        // Touches on the panel, between its cells, stay there: none closes it.
        panel = new LinearLayout(context) {
            @Override
            public boolean onTouchEvent(MotionEvent event) {
                return true;
            }
        };
        panel.setOrientation(LinearLayout.VERTICAL);
        panel.setPadding(sides, 0, sides, Math.round(BOTTOM_DP * density));
        card = new GradientDrawable();
        card.setCornerRadius(CORNER_DP * density);
        panel.setBackground(card);
        panel.setClipToOutline(true);

        name = new TextView(context);
        name.setTextSize(TypedValue.COMPLEX_UNIT_SP, NAME_SP);
        name.setGravity(Gravity.CENTER);
        name.setSingleLine(true);
        name.setEllipsize(TextUtils.TruncateAt.END);
        name.setMinHeight(Math.round(MIN_TARGET_DP * density));
        name.setPadding(sides, 0, sides, 0);
        name.setOnClickListener(view -> {
            if (folder >= 0) {
                renameRequested.accept(folder);
            }
        });
        panel.addView(name, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        panel.addView(grid, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        addView(panel, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
    }

    /** The view the screen's coordinates start from: the home views. */
    void setScreen(View screen) {
        this.screen = screen;
    }

    /** Keeps the panel inside the safe area (HOME-11). */
    void setSafeArea(Insets safe) {
        setPadding(safe.left, safe.top, safe.right, safe.bottom);
    }

    /**
     * Draws the frame's folder: {@code folder}, or -1 for none, {@code open} or closing, {@code
     * progress} ten-thousandths of the way from its icon, {@code icon}, which may be empty, and
     * its {@code title}. {@code scrim} is the scrim's full opacity, 0 to 255.
     */
    void apply(int folder, boolean open, int progress, Rect icon, String title, int scrim,
            int foreground, int surface) {
        if (foreground != this.foreground || surface != this.surface) {
            this.foreground = foreground;
            this.surface = surface;
            card.setColor(surface);
            name.setTextColor(foreground);
            ColorStateList pressed =
                    ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
            name.setBackground(new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
        }
        if (folder < 0) {
            this.folder = -1;
            this.open = false;
            tapping = false;
            setVisibility(GONE);
            return;
        }
        boolean opened = folder != this.folder;
        this.folder = folder;
        this.open = open;
        this.progress = progress / WHOLE;
        scrimAlpha = scrim;
        if (!icon.isEmpty()) {
            from.set(icon);
        }
        if (title != null && !title.contentEquals(name.getText())) {
            name.setText(title);
            panel.setAccessibilityPaneTitle(
                    getResources().getString(R.string.folder_pane, title));
        }
        if (opened || getVisibility() != VISIBLE) {
            setVisibility(VISIBLE);
            requestLayout();
        }
        grow();
    }

    /** Places the panel {@link #progress} of the way from the folder's icon to its own place. */
    private void grow() {
        scrim.setAlpha(progress * scrimAlpha / 255f);
        int width = panel.getWidth();
        int height = panel.getHeight();
        if (width == 0 || height == 0 || from.isEmpty()) {
            panel.setAlpha(progress);
            return;
        }
        float still = 1f - progress;
        float scale = from.width() / (float) width;
        panel.setPivotX(width / 2f);
        panel.setPivotY(height / 2f);
        panel.setScaleX(scale + (1f - scale) * progress);
        panel.setScaleY(scale + (1f - scale) * progress);
        panel.setTranslationX(still * (from.exactCenterX() - (panel.getLeft() + width / 2f)));
        panel.setTranslationY(still * (from.exactCenterY() - (panel.getTop() + height / 2f)));
        panel.setAlpha(progress);
    }

    @Override
    protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        int width = MeasureSpec.getSize(widthMeasureSpec);
        int height = MeasureSpec.getSize(heightMeasureSpec);
        setMeasuredDimension(width, height);
        scrim.measure(MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY),
                MeasureSpec.makeMeasureSpec(height, MeasureSpec.EXACTLY));
        int room = Math.max(0, width - getPaddingLeft() - getPaddingRight() - 2 * margin);
        int cell = apps.cellWidth();
        // Four of the grid's cells, so its icons and the folder's line up (FOLD-04).
        int cells = cell > 0 ? Math.min(4 * cell, room - 2 * sides) : room - 2 * sides;
        int tall = Math.max(0, height - getPaddingTop() - getPaddingBottom() - 2 * margin);
        panel.measure(MeasureSpec.makeMeasureSpec(Math.max(0, cells) + 2 * sides,
                        MeasureSpec.EXACTLY),
                MeasureSpec.makeMeasureSpec(tall, MeasureSpec.AT_MOST));
    }

    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        scrim.layout(0, 0, right - left, bottom - top);
        int width = panel.getMeasuredWidth();
        int height = panel.getMeasuredHeight();
        int areaLeft = getPaddingLeft();
        int areaTop = getPaddingTop();
        int areaWidth = right - left - getPaddingLeft() - getPaddingRight();
        int areaHeight = bottom - top - getPaddingTop() - getPaddingBottom();
        int x = areaLeft + (areaWidth - width) / 2;
        int y = areaTop + (areaHeight - height) / 2;
        panel.layout(x, y, x + width, y + height);
        // The Rust app takes an icon dragged out of this rect to the grid (FOLD-05).
        int[] at = AppsGrid.offsetIn(panel, screen);
        bounds.set(at[0], at[1], at[0] + width, at[1] + height);
        grid.setPanel(bounds);
        grow();
    }

    // Only an open folder takes touches; while it closes, they reach the grid below.
    @Override
    public boolean dispatchTouchEvent(MotionEvent event) {
        if (!open || getVisibility() != VISIBLE) {
            tapping = false;
            return false;
        }
        return super.dispatchTouchEvent(event);
    }

    // A tap outside the panel closes the folder (FOLD-04). Touches on the panel never get here.
    @Override
    public boolean onTouchEvent(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                downX = event.getX();
                downY = event.getY();
                tapping = true;
                break;
            case MotionEvent.ACTION_MOVE:
                tapping &= Math.hypot(event.getX() - downX, event.getY() - downY) <= touchSlop;
                break;
            case MotionEvent.ACTION_UP:
                if (tapping && folder >= 0) {
                    closed.accept(folder);
                }
                tapping = false;
                break;
            default:
                tapping = false;
        }
        return true;
    }
}
