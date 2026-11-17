package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.util.TypedValue;
import android.widget.LinearLayout;
import android.widget.RadioButton;
import android.widget.RadioGroup;
import android.widget.TextView;

/**
 * Settings → Recent → Time window (REC-11, SET-02, ADR 0026): how far back Recent looks, 12
 * hours, 24 hours or 3 days. The change takes effect at once (SET-03). Until #34 draws the rest
 * of Settings, it sits between Hidden apps and the lock-screen image's group.
 */
final class RecentSettings extends LinearLayout {
    /** A time window was chosen, in hours. */
    interface Choice {
        void window(int hours);
    }

    /** The choices, in hours, as {@code RecentWindow::hours} in the Rust app gives them. */
    private static final int[] HOURS = {12, 24, 72};
    private static final int[] LABELS = {
        R.string.recent_window_12_hours, R.string.recent_window_24_hours,
        R.string.recent_window_3_days,
    };
    private static final float GROUP_SP = 14f;
    private static final float TITLE_SP = 18f;
    private static final float TEXT_SP = 16f;
    private static final float ROW_DP = 56f;
    private static final float GAP_DP = 16f;

    private final TextView group;
    private final TextView title;
    private final RadioGroup choices;
    private final RadioButton[] buttons = new RadioButton[HOURS.length];
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    RecentSettings(Context context, Choice choice) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        int gap = Math.round(GAP_DP * density);
        setOrientation(VERTICAL);
        setPadding(0, gap, 0, 0);

        group = heading(R.string.settings_group_recent, GROUP_SP);
        addView(group, wrap());
        title = heading(R.string.recent_time_window, TITLE_SP);
        title.setId(generateViewId());
        LayoutParams titleParams = wrap();
        titleParams.topMargin = gap / 2;
        addView(title, titleParams);

        // One radio button per choice, so screen readers hear "24 hours, radio button, checked,
        // 2 of 3" under the "Time window" heading (A11Y-01).
        choices = new RadioGroup(context);
        choices.setId(generateViewId());
        choices.setOrientation(VERTICAL);
        title.setLabelFor(choices.getId());
        for (int i = 0; i < HOURS.length; i++) {
            RadioButton button = new RadioButton(context);
            button.setId(generateViewId());
            button.setText(LABELS[i]);
            button.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
            button.setMinHeight(Math.round(ROW_DP * density));
            int hours = HOURS[i];
            // A click, unlike a change of state, comes only from the user.
            button.setOnClickListener(view -> choice.window(hours));
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
     * Checks the window the Rust app says is chosen, in hours. A tap checks its button at once,
     * so this also puts the check back when the choice couldn't be saved.
     */
    void show(int hours) {
        int checked = -1;
        for (int i = 0; i < HOURS.length; i++) {
            if (HOURS[i] == hours) {
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

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int accent) {
        if (foreground == this.foreground && dim == this.dim && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.accent = accent;
        group.setTextColor(dim);
        title.setTextColor(foreground);
        int[][] states = {{android.R.attr.state_checked}, {}};
        ColorStateList tint = new ColorStateList(states, new int[] {accent, dim});
        for (RadioButton button : buttons) {
            button.setTextColor(foreground);
            button.setButtonTintList(tint);
        }
    }
}
