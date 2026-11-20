package io.github.twonet.meegone;

import android.content.Context;
import android.view.View;
import android.view.ViewGroup;

/**
 * Buttons side by side at the end of a row, such as Not now and Make default, while they fit,
 * and otherwise one above another at the end, in the same order, so a large font size never
 * squeezes or cuts one of them (A11Y-04, ADR 0038). Screen readers go through them in the same
 * order either way.
 */
final class ButtonRow extends ViewGroup {
    /** Whether the buttons are stacked, as last measured. */
    private boolean stacked;

    ButtonRow(Context context) {
        super(context);
    }

    @Override
    protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        int width = MeasureSpec.getSize(widthMeasureSpec);
        boolean bounded = MeasureSpec.getMode(widthMeasureSpec) != MeasureSpec.UNSPECIFIED;
        int room = Math.max(0, width - getPaddingLeft() - getPaddingRight());
        int each = bounded
                ? MeasureSpec.makeMeasureSpec(room, MeasureSpec.AT_MOST)
                : MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED);
        int unbounded = MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED);
        int across = 0;
        int widest = 0;
        int tallest = 0;
        int stack = 0;
        for (int i = 0; i < getChildCount(); i++) {
            View child = getChildAt(i);
            if (child.getVisibility() == GONE) {
                continue;
            }
            child.measure(each, unbounded);
            across += child.getMeasuredWidth();
            widest = Math.max(widest, child.getMeasuredWidth());
            tallest = Math.max(tallest, child.getMeasuredHeight());
            stack += child.getMeasuredHeight();
        }
        stacked = bounded && across > room;
        int contentWidth = stacked ? widest : across;
        int contentHeight = stacked ? stack : tallest;
        setMeasuredDimension(
                resolveSize(contentWidth + getPaddingLeft() + getPaddingRight(),
                        widthMeasureSpec),
                resolveSize(contentHeight + getPaddingTop() + getPaddingBottom(),
                        heightMeasureSpec));
    }

    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        int start = rtl ? getPaddingLeft() : right - left - getPaddingRight();
        int height = bottom - top - getPaddingTop() - getPaddingBottom();
        int across = 0;
        for (int i = 0; i < getChildCount(); i++) {
            View child = getChildAt(i);
            if (child.getVisibility() != GONE) {
                across += child.getMeasuredWidth();
            }
        }
        // Side by side, the row starts this far from the end edge.
        int x = rtl ? start + across : start - across;
        int y = getPaddingTop();
        for (int i = 0; i < getChildCount(); i++) {
            View child = getChildAt(i);
            if (child.getVisibility() == GONE) {
                continue;
            }
            int w = child.getMeasuredWidth();
            int h = child.getMeasuredHeight();
            if (stacked) {
                // Each at the end edge, one below another.
                int childLeft = rtl ? start : start - w;
                child.layout(childLeft, y, childLeft + w, y + h);
                y += h;
            } else {
                int childTop = getPaddingTop() + (height - h) / 2;
                int childLeft = rtl ? x - w : x;
                child.layout(childLeft, childTop, childLeft + w, childTop + h);
                x = rtl ? x - w : x + w;
            }
        }
    }
}
