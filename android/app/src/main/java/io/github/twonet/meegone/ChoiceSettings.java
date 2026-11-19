package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.RippleDrawable;
import android.util.TypedValue;
import android.widget.LinearLayout;
import android.widget.RadioButton;
import android.widget.RadioGroup;
import android.widget.TextView;

/**
 * A Settings item that is one of a few choices (SET-02), under its group's heading: Appearance →
 * Theme, System, Light or Dark (THEME-01, ADR 0033), and Recent → Time window, 12 hours, 24
 * hours or 3 days (REC-11, ADR 0026). Inside a group with other items, such as Home → "Home
 * button opens" (HOME-06, ADR 0035), it has no heading of its own. A choice takes effect at once
 * (SET-03).
 */
final class ChoiceSettings extends LinearLayout {
    /** A choice was made: one of the values the item was made with. */
    interface Choice {
        void chosen(int value);
    }

    private static final float GROUP_SP = 14f;
    private static final float TITLE_SP = 18f;
    private static final float TEXT_SP = 16f;
    private static final float ROW_DP = 56f;
    private static final float GAP_DP = 16f;

    private final int[] values;
    /** The group's heading, or null inside a group that has its own. */
    private final TextView group;
    private final TextView title;
    private final RadioGroup choices;
    private final RadioButton[] buttons;
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    /**
     * An item headed {@code group}, or 0 for none, and {@code title}, offering {@code values}
     * with the matching {@code labels}, as the Rust app numbers them.
     */
    ChoiceSettings(Context context, int group, int title, int[] values, int[] labels,
            Choice choice) {
        super(context);
        this.values = values.clone();
        float density = getResources().getDisplayMetrics().density;
        int gap = Math.round(GAP_DP * density);
        setOrientation(VERTICAL);
        if (group != 0) {
            setPadding(0, gap, 0, 0);
            this.group = heading(group, GROUP_SP);
            addView(this.group, wrap());
        } else {
            this.group = null;
        }
        this.title = heading(title, TITLE_SP);
        this.title.setId(generateViewId());
        LayoutParams titleParams = wrap();
        titleParams.topMargin = gap / 2;
        addView(this.title, titleParams);

        // One radio button per choice, so screen readers hear "24 hours, radio button, checked,
        // 2 of 3" under the "Time window" heading (A11Y-01).
        choices = new RadioGroup(context);
        choices.setId(generateViewId());
        choices.setOrientation(VERTICAL);
        this.title.setLabelFor(choices.getId());
        buttons = new RadioButton[values.length];
        for (int i = 0; i < values.length; i++) {
            RadioButton button = new RadioButton(context);
            button.setId(generateViewId());
            button.setText(labels[i]);
            button.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
            button.setMinHeight(Math.round(ROW_DP * density));
            int value = values[i];
            // A click, unlike a change of state, comes only from the user.
            button.setOnClickListener(view -> choice.chosen(value));
            buttons[i] = button;
            choices.addView(button, new RadioGroup.LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        }
        addView(choices, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
    }

    private TextView heading(int text, float sizeSp) {
        TextView heading = new TextView(getContext());
        heading.setText(text);
        heading.setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp);
        heading.setAccessibilityHeading(true);
        return heading;
    }

    private static LayoutParams wrap() {
        return new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT);
    }

    /**
     * Checks the value the Rust app says is chosen. A tap checks its button at once, so this
     * also puts the check back when the choice couldn't be saved.
     */
    void show(int value) {
        int checked = -1;
        for (int i = 0; i < values.length; i++) {
            if (values[i] == value) {
                checked = buttons[i].getId();
            }
        }
        if (choices.getCheckedRadioButtonId() != checked) {
            if (checked == -1) {
                choices.clearCheck();
            } else {
                choices.check(checked);
            }
        }
    }

    /**
     * The theme's colours (THEME-03), down to the press ripple, which would otherwise come from
     * the device's theme rather than MeeGone's.
     */
    void colour(int foreground, int dim, int accent) {
        if (foreground == this.foreground && dim == this.dim && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.accent = accent;
        if (group != null) {
            group.setTextColor(dim);
        }
        title.setTextColor(foreground);
        int[][] states = {{android.R.attr.state_checked}, {}};
        ColorStateList tint = new ColorStateList(states, new int[] {accent, dim});
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        for (RadioButton button : buttons) {
            button.setTextColor(foreground);
            button.setButtonTintList(tint);
            button.setBackground(
                    new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
        }
    }
}
