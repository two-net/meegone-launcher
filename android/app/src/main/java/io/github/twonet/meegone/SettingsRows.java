package io.github.twonet.meegone;

import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.RippleDrawable;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.Switch;
import android.widget.TextView;

/**
 * The rows every group of Settings is made of, so the groups look and read alike (SET-02,
 * ADR 0035): a group's heading, a switch, an action, a note, a message with a button at its end,
 * and a title with its value below. Each is added to the end of a vertical group, and coloured
 * with the theme by the methods at the end (THEME-03).
 */
final class SettingsRows {
    static final float GROUP_SP = 14f;
    static final float TEXT_SP = 16f;
    static final float NOTE_SP = 14f;
    static final float ROW_DP = 56f;
    static final float GAP_DP = 16f;
    static final int MEDIUM = 500;
    /** How faint a control that takes no input is drawn. */
    static final float DISABLED_ALPHA = 0.38f;

    private SettingsRows() {}

    /** Space above a group, between it and the one before. */
    static void startGroup(LinearLayout group) {
        group.setOrientation(LinearLayout.VERTICAL);
        group.setPadding(0, px(group, GAP_DP), 0, 0);
    }

    /** A group's heading, such as "Home", which screen readers can jump to. */
    static TextView heading(LinearLayout group, int text) {
        TextView heading = new TextView(group.getContext());
        heading.setText(text);
        heading.setTextSize(TypedValue.COMPLEX_UNIT_SP, GROUP_SP);
        heading.setAccessibilityHeading(true);
        group.addView(heading, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        return heading;
    }

    /**
     * An option that is on or off, its own label, so screen readers hear "Wrap-around, switch,
     * on" (A11Y-01).
     */
    static Switch switchRow(LinearLayout group, int label) {
        Switch row = new Switch(group.getContext());
        row.setText(label);
        row.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        row.setMinHeight(px(group, ROW_DP));
        group.addView(row, fill());
        return row;
    }

    /** A line of smaller text below the row it explains, hidden until it applies. */
    static TextView note(LinearLayout group) {
        TextView note = new TextView(group.getContext());
        note.setTextSize(TypedValue.COMPLEX_UNIT_SP, NOTE_SP);
        note.setVisibility(LinearLayout.GONE);
        group.addView(note, fill());
        return note;
    }

    /** A paragraph of plain text, such as the privacy notice. */
    static TextView paragraph(LinearLayout group, int text) {
        TextView paragraph = new TextView(group.getContext());
        paragraph.setText(text);
        paragraph.setTextSize(TypedValue.COMPLEX_UNIT_SP, NOTE_SP);
        int gap = px(group, GAP_DP);
        paragraph.setPadding(0, gap / 2, 0, gap / 2);
        group.addView(paragraph, fill());
        return paragraph;
    }

    /** A whole row that does something when tapped, such as "Update now", hidden until offered. */
    static TextView action(LinearLayout group, int label) {
        TextView action = new TextView(group.getContext());
        action.setText(label);
        action.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        action.setGravity(Gravity.CENTER_VERTICAL);
        action.setMinHeight(px(group, ROW_DP));
        action.setVisibility(LinearLayout.GONE);
        group.addView(action, fill());
        return action;
    }

    /** A message with a button at its end, such as Try again, hidden until it applies. */
    static LinearLayout withAction(LinearLayout group, TextView text, TextView button, int label) {
        LinearLayout row = new LinearLayout(group.getContext());
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setMinimumHeight(px(group, ROW_DP));
        row.setVisibility(LinearLayout.GONE);
        text.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        row.addView(text, new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f));
        button(button, label);
        row.addView(button, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        group.addView(row, fill());
        return row;
    }

    /** A text button of at least 48 dp each way (A11Y-02), drawn in the accent. */
    static void button(TextView button, int label) {
        if (label != 0) {
            button.setText(label);
        }
        button.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        button.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
        button.setGravity(Gravity.CENTER);
        int target = A11y.targetPx(button);
        button.setMinHeight(target);
        button.setMinWidth(target);
        int gap = px(button, GAP_DP);
        button.setPadding(gap, 0, gap, 0);
    }

    /**
     * A row with a title and its value below, such as "Region" and "Automatic". Tapped, it does
     * what {@code tapped} says; otherwise screen readers read it as one piece of text.
     */
    static LinearLayout titled(LinearLayout group, TextView title, TextView value, int label,
            Runnable tapped) {
        LinearLayout row = new LinearLayout(group.getContext());
        row.setOrientation(LinearLayout.VERTICAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setMinimumHeight(px(group, ROW_DP));
        int gap = px(group, GAP_DP);
        row.setPadding(0, gap / 2, 0, gap / 2);
        title.setText(label);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        value.setTextSize(TypedValue.COMPLEX_UNIT_SP, NOTE_SP);
        row.addView(title, fill());
        row.addView(value, fill());
        if (tapped != null) {
            row.setOnClickListener(view -> tapped.run());
        } else {
            // Read as one by screen readers, but no stop for a keyboard, where it does nothing
            // (A11Y-06).
            row.setScreenReaderFocusable(true);
        }
        group.addView(row, fill());
        return row;
    }

    /** The press ripple in the theme's foreground, not the device theme's (THEME-03). */
    static RippleDrawable ripple(int foreground) {
        return new RippleDrawable(pressed(foreground), null, new ColorDrawable(Color.WHITE));
    }

    /**
     * The colour of every press: the theme's foreground at 12 %, {@code PRESSED_PERCENT} in the
     * Rust app's tokens. It shows only under the finger, so it isn't held to a contrast ratio
     * (A11Y-03, ADR 0038). Focus alone draws no wash, which would stay under a keyboard's focus
     * for as long as it rests there: the focus ring shows it instead (A11Y-06, ADR 0039).
     */
    static ColorStateList pressed(int foreground) {
        int press = (foreground & 0x00FFFFFF) | 0x1F000000;
        int[][] states = {
            {android.R.attr.state_pressed}, {android.R.attr.state_focused}, {},
        };
        return new ColorStateList(states, new int[] {press, foreground & 0x00FFFFFF, press});
    }

    /** A switch row in the theme's colours (THEME-03). */
    static void colourSwitch(Switch row, int foreground, int dim, int accent) {
        int[][] states = {{android.R.attr.state_checked}, {}};
        row.setTextColor(foreground);
        row.setBackground(ripple(foreground));
        row.setThumbTintList(new ColorStateList(states, new int[] {accent, foreground}));
        row.setTrackTintList(new ColorStateList(states, new int[] {accent, dim}));
    }

    /** A button or an action in the accent, or dim while it takes no input (THEME-03). */
    static void colourButton(TextView button, int foreground, int dim, int accent) {
        button.setTextColor(button.isEnabled() ? accent : dim);
        button.setBackground(ripple(foreground));
    }

    private static LinearLayout.LayoutParams fill() {
        return new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT);
    }

    private static int px(android.view.View view, float dp) {
        return Math.round(dp * view.getResources().getDisplayMetrics().density);
    }
}
