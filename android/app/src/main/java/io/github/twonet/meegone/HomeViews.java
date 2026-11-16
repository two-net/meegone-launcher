package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.Color;
import android.graphics.drawable.GradientDrawable;
import android.os.Build;
import android.os.Bundle;
import android.util.TypedValue;
import android.view.Choreographer;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * The three home views, Events, Applications and Recent, in a wrap-around carousel (HOME-02 to
 * HOME-04), drawn with Android's own views (ADR 0006). They come back from an app with a short
 * scale and fade (ANIM-09, ADR 0007).
 *
 * <p>The Rust app owns the gesture, the motion and the view indicator. This view passes it every
 * touch, frame time and layout change, then reads what to draw with {@link #nativeRead} and
 * applies it. It keeps nothing else. The frame slots below must stay in step with {@code
 * frame_slots} in {@code src/android.rs}.
 *
 * <p>An element inside a view that uses horizontal drags itself keeps them by calling {@link
 * #requestDisallowInterceptTouchEvent} before the carousel takes the gesture (HOME-03).
 */
final class HomeViews extends FrameLayout implements Choreographer.FrameCallback {
    // The slots of the frame that nativeRead fills.
    private static final int PAGES = 0; // Events, Applications and Recent: offsets in px
    private static final int INDICATOR_ALPHA = 3; // 0 to 255
    private static final int INDICATOR_SELECTED = 4; // 0 to 2
    private static final int CURRENT = 5; // 0 to 2
    private static final int ANIMATING = 6; // 1 while something moves
    private static final int BACKGROUND = 7; // ARGB
    private static final int FOREGROUND = 8; // ARGB
    private static final int DIM = 9; // ARGB
    private static final int CONTENT_SCALE = 10; // ten-thousandths; 10 000 at rest
    private static final int CONTENT_ALPHA = 11; // 0 to 255
    private static final int FRAME_SIZE = 12;

    private static final int VIEWS = 3;
    private static final int[] NAMES = {
        R.string.view_events, R.string.view_applications, R.string.view_recent
    };

    private final int[] frame = new int[FRAME_SIZE];
    private final FrameLayout[] pages = new FrameLayout[VIEWS];
    private final TextView[] titles = new TextView[VIEWS];
    private final View[] segments = new View[VIEWS];
    /** The view indicator (HOME-09). Screen readers skip it; {@link #status} speaks instead. */
    private final LinearLayout indicator;
    /**
     * Tells screen readers which view shows, e.g. "Applications, 2 of 3" (A11Y-08). It is a
     * polite live region, so changes are announced. It must stay visible to them: a view with
     * no size or no opacity isn't.
     */
    private final TextView status;
    private final int indicatorMargin;
    private final int touchSlop;

    /** Whether the carousel has taken the current gesture from the content. */
    private boolean claimed;
    /** A child element claimed the current gesture, so the carousel gets none of it. */
    private boolean childClaimed;
    /** The finger that drives the carousel. */
    private int pointerId = MotionEvent.INVALID_POINTER_ID;
    private boolean framePosted;
    private int announced = -1;
    private int selected = -1;
    private int background;
    private int foreground;
    private int dim;

    HomeViews(Context context) {
        super(context);
        touchSlop = ViewConfiguration.get(context).getScaledTouchSlop();
        indicatorMargin = dp(12);
        // Carries the screen-reader actions that switch views (A11Y-05).
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);

        for (int i = 0; i < VIEWS; i++) {
            // Until each view's content arrives (#9, #19, #22), it shows its name.
            TextView title = new TextView(context);
            title.setText(NAMES[i]);
            title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 28);
            titles[i] = title;
            FrameLayout page = new FrameLayout(context);
            page.addView(title, new LayoutParams(
                    LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT, Gravity.CENTER));
            page.setVisibility(INVISIBLE);
            pages[i] = page;
            addView(page, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        }

        // Three segments in the views' order; a right-to-left layout mirrors them (L10N-02).
        indicator = new LinearLayout(context);
        indicator.setOrientation(LinearLayout.HORIZONTAL);
        indicator.setAlpha(0f);
        indicator.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
        for (int i = 0; i < VIEWS; i++) {
            View segment = new View(context);
            GradientDrawable shape = new GradientDrawable();
            shape.setCornerRadius(dp(2));
            segment.setBackground(shape);
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(dp(24), dp(4));
            params.setMarginStart(dp(3));
            params.setMarginEnd(dp(3));
            indicator.addView(segment, params);
            segments[i] = segment;
        }
        LayoutParams params = new LayoutParams(
                LayoutParams.WRAP_CONTENT,
                LayoutParams.WRAP_CONTENT,
                Gravity.TOP | Gravity.CENTER_HORIZONTAL);
        params.topMargin = indicatorMargin;
        addView(indicator, params);

        // One transparent pixel: present for screen readers, invisible on screen.
        status = new TextView(context);
        status.setTextColor(Color.TRANSPARENT);
        status.setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);
        addView(status, new LayoutParams(1, 1, Gravity.TOP | Gravity.START));
    }

    /** MeeGone came to the foreground; another activity may have moved the carousel. */
    void onResumed() {
        sendLayout();
        apply();
    }

    /** The Rust app changed what shows without a touch or a frame, such as after Home. */
    void refresh() {
        apply();
    }

    @Override
    protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        sendLayout();
        apply();
    }

    @Override
    public void onRtlPropertiesChanged(int layoutDirection) {
        super.onRtlPropertiesChanged(layoutDirection);
        sendLayout();
        apply();
    }

    private void sendLayout() {
        if (getWidth() > 0) {
            nativeLayout(getWidth(), getResources().getDisplayMetrics().density, touchSlop,
                    getLayoutDirection() == LAYOUT_DIRECTION_RTL);
        }
    }

    /** Keeps the indicator below the status bar and any display cut-out. */
    @Override
    @SuppressWarnings("deprecation") // The only way on Android 10.
    public WindowInsets onApplyWindowInsets(WindowInsets insets) {
        int top;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            top = insets.getInsets(
                    WindowInsets.Type.statusBars() | WindowInsets.Type.displayCutout()).top;
        } else {
            top = insets.getSystemWindowInsetTop();
        }
        LayoutParams params = (LayoutParams) indicator.getLayoutParams();
        if (params.topMargin != top + indicatorMargin) {
            params.topMargin = top + indicatorMargin;
            indicator.setLayoutParams(params);
        }
        return super.onApplyWindowInsets(insets);
    }

    // Every touch reaches the carousel once, here; the content below gets it too until the
    // carousel takes the gesture.
    @Override
    public boolean dispatchTouchEvent(MotionEvent event) {
        if (event.getActionMasked() == MotionEvent.ACTION_DOWN) {
            childClaimed = false;
        }
        if (!childClaimed) {
            claimed = forward(event);
            apply();
        }
        return super.dispatchTouchEvent(event);
    }

    @Override
    public boolean onInterceptTouchEvent(MotionEvent event) {
        return claimed;
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        // Keep receiving the gesture, so it can become a drag.
        return true;
    }

    @Override
    public void requestDisallowInterceptTouchEvent(boolean disallow) {
        super.requestDisallowInterceptTouchEvent(disallow);
        if (disallow && !claimed && !childClaimed) {
            childClaimed = true;
            nativePointer(MotionEvent.ACTION_CANCEL, 0f, 0f, System.nanoTime());
            apply();
        }
    }

    /** Passes the driving finger to the carousel. Returns whether it has taken the gesture. */
    private boolean forward(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                pointerId = event.getPointerId(0);
                return nativePointer(MotionEvent.ACTION_DOWN, event.getX(0), event.getY(0),
                        eventNanos(event));
            case MotionEvent.ACTION_MOVE: {
                int index = event.findPointerIndex(pointerId);
                if (index < 0) {
                    return claimed;
                }
                // Touches between frames arrive batched; each one counts for the fling.
                for (int h = 0; h < event.getHistorySize(); h++) {
                    nativePointer(MotionEvent.ACTION_MOVE, event.getHistoricalX(index, h),
                            event.getHistoricalY(index, h), historicalNanos(event, h));
                }
                return nativePointer(MotionEvent.ACTION_MOVE, event.getX(index),
                        event.getY(index), eventNanos(event));
            }
            case MotionEvent.ACTION_POINTER_UP:
            case MotionEvent.ACTION_UP: {
                int index = event.getActionIndex();
                if (event.getPointerId(index) != pointerId) {
                    return claimed;
                }
                pointerId = MotionEvent.INVALID_POINTER_ID;
                return nativePointer(MotionEvent.ACTION_UP, event.getX(index), event.getY(index),
                        eventNanos(event));
            }
            case MotionEvent.ACTION_CANCEL:
                pointerId = MotionEvent.INVALID_POINTER_ID;
                return nativePointer(MotionEvent.ACTION_CANCEL, 0f, 0f, eventNanos(event));
            default:
                return claimed;
        }
    }

    @Override
    public void doFrame(long frameTimeNanos) {
        framePosted = false;
        nativeFrame(frameTimeNanos);
        apply();
    }

    @Override
    protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        // Restarts the frame loop, and sets the system bars now that there is a window.
        background = 0;
        apply();
    }

    @Override
    protected void onDetachedFromWindow() {
        Choreographer.getInstance().removeFrameCallback(this);
        framePosted = false;
        super.onDetachedFromWindow();
    }

    /** Draws what the Rust app says, and asks for another frame while anything moves. */
    private void apply() {
        nativeRead(frame);
        int width = getWidth();
        // The content comes back from an app scaled down and faded out (ANIM-09); the
        // background stays opaque.
        float scale = frame[CONTENT_SCALE] / 10_000f;
        float alpha = frame[CONTENT_ALPHA] / 255f;
        for (int i = 0; i < VIEWS; i++) {
            int x = frame[PAGES + i];
            pages[i].setTranslationX(x);
            pages[i].setScaleX(scale);
            pages[i].setScaleY(scale);
            pages[i].setAlpha(alpha);
            // A view out of sight is hidden, from screen readers too.
            pages[i].setVisibility(width > 0 && Math.abs(x) < width ? VISIBLE : INVISIBLE);
        }
        indicator.setAlpha(frame[INDICATOR_ALPHA] / 255f);
        if (frame[BACKGROUND] != background) {
            background = frame[BACKGROUND];
            setBackgroundColor(background);
            matchSystemBars(background);
        }
        boolean coloursChanged = frame[FOREGROUND] != foreground || frame[DIM] != dim;
        if (coloursChanged) {
            foreground = frame[FOREGROUND];
            dim = frame[DIM];
            for (TextView title : titles) {
                title.setTextColor(foreground);
            }
        }
        if (coloursChanged || frame[INDICATOR_SELECTED] != selected) {
            selected = frame[INDICATOR_SELECTED];
            for (int i = 0; i < VIEWS; i++) {
                ((GradientDrawable) segments[i].getBackground())
                        .setColor(i == selected ? foreground : dim);
            }
        }
        int current = frame[CURRENT];
        if (current != announced) {
            announced = current;
            status.setText(getResources().getString(R.string.view_position,
                    getResources().getString(NAMES[current]), current + 1, VIEWS));
        }
        if (frame[ANIMATING] != 0 && !framePosted && isAttachedToWindow()) {
            framePosted = true;
            Choreographer.getInstance().postFrameCallback(this);
        }
    }

    /** Dark system-bar icons on a light background, light ones on a dark background. */
    @SuppressWarnings("deprecation") // The flags are the only way on Android 10.
    private void matchSystemBars(int background) {
        boolean light = Color.luminance(background) > 0.5f;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = getWindowInsetsController();
            if (controller != null) {
                int bars = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                        | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                controller.setSystemBarsAppearance(light ? bars : 0, bars);
            }
        } else {
            int flags = SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
            int visibility = getSystemUiVisibility();
            setSystemUiVisibility(light ? visibility | flags : visibility & ~flags);
        }
    }

    // Screen readers switch views with actions instead of gestures (A11Y-05).
    @Override
    public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        info.setScrollable(true);
        info.addAction(AccessibilityAction.ACTION_SCROLL_FORWARD);
        info.addAction(AccessibilityAction.ACTION_SCROLL_BACKWARD);
        info.addAction(AccessibilityAction.ACTION_PAGE_LEFT);
        info.addAction(AccessibilityAction.ACTION_PAGE_RIGHT);
        info.addAction(new AccessibilityAction(
                R.id.action_next_view, getResources().getString(R.string.action_next_view)));
        info.addAction(new AccessibilityAction(R.id.action_previous_view,
                getResources().getString(R.string.action_previous_view)));
    }

    @Override
    public boolean performAccessibilityAction(int action, Bundle arguments) {
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        Boolean next = null;
        if (action == AccessibilityNodeInfo.ACTION_SCROLL_FORWARD
                || action == R.id.action_next_view) {
            next = true;
        } else if (action == AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD
                || action == R.id.action_previous_view) {
            next = false;
        } else if (action == android.R.id.accessibilityActionPageLeft) {
            next = rtl;
        } else if (action == android.R.id.accessibilityActionPageRight) {
            next = !rtl;
        }
        if (next == null) {
            return super.performAccessibilityAction(action, arguments);
        }
        boolean moved = nativeShow(next);
        apply();
        return moved;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private static long eventNanos(MotionEvent event) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            return event.getEventTimeNanos();
        }
        return event.getEventTime() * 1_000_000L;
    }

    private static long historicalNanos(MotionEvent event, int position) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            return event.getHistoricalEventTimeNanos(position);
        }
        return event.getHistoricalEventTime(position) * 1_000_000L;
    }

    /** The home views were laid out: at start, or after a size or direction change. */
    private native void nativeLayout(int widthPx, float density, int touchSlopPx, boolean rtl);

    /**
     * The driving finger, with a {@code MotionEvent} action and a time in the {@code
     * System.nanoTime()} base. Returns whether the carousel has taken the gesture.
     */
    private native boolean nativePointer(int action, float x, float y, long timeNanos);

    /** The display is about to draw a frame at this {@code System.nanoTime()}. */
    private native void nativeFrame(long frameTimeNanos);

    /**
     * Shows the next or previous view without a gesture. Returns false if there is none: past
     * an end with wrap-around off.
     */
    private native boolean nativeShow(boolean next);

    /** Fills {@code frame} with what to draw now, in the slots above. */
    private native void nativeRead(int[] frame);
}
