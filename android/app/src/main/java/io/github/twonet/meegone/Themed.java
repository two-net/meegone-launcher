package io.github.twonet.meegone;

import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.view.View;
import android.widget.AbsListView;
import android.widget.ScrollView;
import android.widget.TextView;

/**
 * The parts of Android's own widgets that would otherwise take their colours from the device's
 * light or dark theme, drawn in MeeGone's instead (THEME-03, ADR 0033). The activity's theme
 * follows the device as it was when the activity was made, and MeeGone's can differ from it, so
 * every colour MeeGone shows is set from the frame's tokens.
 */
final class Themed {
    private static final int CURSOR_DP = 2;
    private static final int THUMB_DP = 4;

    private Themed() {}

    /**
     * A text field's cursor, selection handles and selection highlight, in the accent: the
     * search field (SRCH-01) and the folder rename dialog's (FOLD-03).
     */
    static void textField(TextView field, int accent) {
        float density = field.getResources().getDisplayMetrics().density;
        GradientDrawable cursor = new GradientDrawable();
        cursor.setColor(accent);
        cursor.setSize(Math.round(CURSOR_DP * density), 0);
        field.setTextCursorDrawable(cursor);
        field.setHighlightColor((accent & 0x00FFFFFF) | 0x66000000);
        Drawable handle = tinted(field.getTextSelectHandle(), accent);
        if (handle != null) {
            field.setTextSelectHandle(handle);
        }
        Drawable left = tinted(field.getTextSelectHandleLeft(), accent);
        if (left != null) {
            field.setTextSelectHandleLeft(left);
        }
        Drawable right = tinted(field.getTextSelectHandleRight(), accent);
        if (right != null) {
            field.setTextSelectHandleRight(right);
        }
    }

    /** A copy of a selection handle in the accent, or null for none. */
    private static Drawable tinted(Drawable handle, int accent) {
        if (handle == null) {
            return null;
        }
        // Mutated, so the system's shared handle keeps its own colour elsewhere.
        Drawable own = handle.mutate();
        own.setTint(accent);
        return own;
    }

    /**
     * What a scrolling view draws of its own: the glow at its ends on Android 10 and 11 (later
     * versions stretch, without a colour), and the scrollbar's thumb where it shows one, in the
     * secondary colour.
     */
    static void scrolling(ScrollView scroll, int dim) {
        scroll.setEdgeEffectColor(dim);
        scroll.setVerticalScrollbarThumbDrawable(thumb(scroll, dim));
    }

    /** The same for a list, such as the region dialog's (ADR 0038). */
    static void scrolling(AbsListView list, int dim) {
        list.setEdgeEffectColor(dim);
        list.setVerticalScrollbarThumbDrawable(thumb(list, dim));
    }

    private static Drawable thumb(View view, int dim) {
        float density = view.getResources().getDisplayMetrics().density;
        GradientDrawable thumb = new GradientDrawable();
        thumb.setColor(dim);
        thumb.setCornerRadius(THUMB_DP * density / 2);
        thumb.setSize(Math.round(THUMB_DP * density), 0);
        return thumb;
    }
}
