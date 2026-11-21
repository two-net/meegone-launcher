package io.github.twonet.meegone;

import android.animation.ValueAnimator;
import android.content.Context;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.accessibility.AccessibilityNodeInfo;
import android.widget.ScrollView;

/**
 * Every column MeeGone scrolls: the Applications grid, Events, Recent, Settings, the menu card,
 * the image screens, setup and the licences (ADR 0039).
 *
 * <ul>
 *   <li>A focused element is kept clear of the padding. The grid and Recent scroll under the
 *       system bars, and Android's own scroll to a focused child would leave it under them
 *       (A11Y-06).
 *   <li>With animations removed, Android's own scrolls are instant: a screen reader's page, a
 *       keyboard's arrows and pages, and scrolling to what took focus (A11Y-07, ANIM-05).
 *       Android runs those for 250 ms whatever the animation speed.
 *   <li>The scroll bars then fade at once.
 *   <li>Below Android 12, over-scroll is off, as the glow there ignores the speed. From
 *       Android 12, the stretch follows the setting itself.
 * </ul>
 *
 * <p>It follows the speed whenever it scrolls, so a change made over the home takes effect at
 * once.
 */
class Scroll extends ScrollView {
    /** The over-scroll mode the view asked for, kept while animations are removed. */
    private int overScroll = OVER_SCROLL_IF_CONTENT_SCROLLS;

    Scroll(Context context) {
        super(context);
        followAnimations();
    }

    /**
     * Whether animations play: the system's animator duration scale isn't 0, which Developer
     * options and Accessibility's "Remove animations" both set (ANIM-05).
     */
    static boolean animating() {
        return ValueAnimator.areAnimatorsEnabled();
    }

    private void followAnimations() {
        boolean on = animating();
        setSmoothScrollingEnabled(on);
        setScrollBarFadeDuration(on ? ViewConfiguration.getScrollBarFadeDuration() : 0);
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
            super.setOverScrollMode(on ? overScroll : OVER_SCROLL_NEVER);
        }
    }

    @Override
    public void setOverScrollMode(int mode) {
        // Called by View's own constructor too, before this class's fields are set.
        overScroll = mode;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S || animating()) {
            super.setOverScrollMode(mode);
        } else {
            super.setOverScrollMode(OVER_SCROLL_NEVER);
        }
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        followAnimations();
        return super.dispatchKeyEvent(event);
    }

    @Override
    protected void onScrollChanged(int left, int top, int oldLeft, int oldTop) {
        followAnimations();
        super.onScrollChanged(left, top, oldLeft, oldTop);
    }

    @Override
    public boolean requestChildRectangleOnScreen(View child, Rect rectangle, boolean immediate) {
        return super.requestChildRectangleOnScreen(child, rectangle, immediate || !animating());
    }

    /**
     * How far to scroll so {@code rect}, in the content's coordinates, shows between the
     * padding's edges, or as much of it as fits from its top. Android's own leaves it anywhere
     * in the view, padding included.
     */
    @Override
    protected int computeScrollDeltaToGetChildRectOnScreen(Rect rect) {
        if (getChildCount() == 0) {
            return 0;
        }
        int scroll = getScrollY();
        int top = scroll + getPaddingTop();
        int bottom = scroll + getHeight() - getPaddingBottom();
        int room = bottom - top;
        int delta = 0;
        if (rect.bottom > bottom && rect.top > top) {
            delta = rect.height() > room ? rect.top - top : rect.bottom - bottom;
        } else if (rect.top < top && rect.bottom < bottom) {
            delta = rect.height() > room ? rect.bottom - bottom : rect.top - top;
        }
        int range = Math.max(0, getChildAt(0).getHeight() - room);
        return Math.max(0, Math.min(scroll + delta, range)) - scroll;
    }

    // A screen reader's scroll is a page at once with animations removed. Otherwise Android's
    // own runs.
    @Override
    public boolean performAccessibilityAction(int action, Bundle arguments) {
        followAnimations();
        int direction = 0;
        if (action == AccessibilityNodeInfo.ACTION_SCROLL_FORWARD
                || action == android.R.id.accessibilityActionScrollDown) {
            direction = 1;
        } else if (action == AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD
                || action == android.R.id.accessibilityActionScrollUp) {
            direction = -1;
        }
        if (direction == 0 || animating() || !isEnabled() || getChildCount() == 0) {
            return super.performAccessibilityAction(action, arguments);
        }
        int room = getHeight() - getPaddingTop() - getPaddingBottom();
        int range = Math.max(0, getChildAt(0).getHeight() - room);
        int target = Math.max(0, Math.min(getScrollY() + direction * room, range));
        if (target == getScrollY()) {
            return false;
        }
        scrollTo(getScrollX(), target);
        return true;
    }
}
