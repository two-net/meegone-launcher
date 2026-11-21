package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.Resources;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * Settings → Access (SET-02, ADR 0035): the default home app (HOME-01), notification access, for
 * Events (EVT-03), and usage access, for Recent (REC-02). Each shows its status and a shortcut
 * to its system screen; where the device has no such screen, the written instructions the Rust
 * app gives show below the item instead (SRS 8.1). The status is read again whenever MeeGone
 * comes back from the system's screens.
 */
final class AccessSettings extends LinearLayout {
    /** A shortcut was tapped: the item at {@code item}, numbered as the Rust app numbers them. */
    interface Shortcut {
        void open(int item);
    }

    /** The items, in the order the Rust app numbers them. */
    static final int DEFAULT_HOME = 0;
    static final int NOTIFICATIONS = 1;
    static final int USAGE = 2;
    private static final int ITEMS = 3;
    private static final int[] TITLES = {
        R.string.access_default_home, R.string.access_notifications, R.string.access_usage,
    };
    /** Each item's status while granted, then while not. */
    private static final int[][] STATUSES = {
        {R.string.access_default_home_on, R.string.access_default_home_off},
        {R.string.access_notifications_on, R.string.access_notifications_off},
        {R.string.access_usage_on, R.string.access_usage_off},
    };
    /** Each item's button while granted, then while not. */
    private static final int[][] BUTTONS = {
        {R.string.access_change, R.string.access_make_default},
        {R.string.access_change, R.string.access_allow},
        {R.string.access_change, R.string.access_allow},
    };

    private final TextView group;
    private final TextView[] titles = new TextView[ITEMS];
    private final TextView[] statuses = new TextView[ITEMS];
    private final TextView[] buttons = new TextView[ITEMS];
    private final TextView[] instructions = new TextView[ITEMS];
    /** What each item shows: 1 granted, 0 not, -1 before the first frame. */
    private final int[] shown = {-1, -1, -1};
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    AccessSettings(Context context, Shortcut shortcut) {
        super(context);
        SettingsRows.startGroup(this);
        group = SettingsRows.heading(this, R.string.settings_group_access);
        float density = getResources().getDisplayMetrics().density;
        int rowHeight = Math.round(SettingsRows.ROW_DP * density);
        int gap = Math.round(SettingsRows.GAP_DP * density);
        for (int item = 0; item < ITEMS; item++) {
            LinearLayout row = new LinearLayout(context);
            row.setOrientation(HORIZONTAL);
            row.setGravity(Gravity.CENTER_VERTICAL);
            row.setMinimumHeight(rowHeight);
            row.setPadding(0, gap / 2, 0, gap / 2);
            LinearLayout text = new LinearLayout(context);
            text.setOrientation(VERTICAL);
            // Read as one: "Notification access, Not allowed. Events can't list…" (A11Y-01). Not
            // a keyboard's stop, where it would do nothing (A11Y-06).
            text.setScreenReaderFocusable(true);
            titles[item] = new TextView(context);
            titles[item].setText(TITLES[item]);
            titles[item].setTextSize(TypedValue.COMPLEX_UNIT_SP, SettingsRows.TEXT_SP);
            statuses[item] = new TextView(context);
            statuses[item].setTextSize(TypedValue.COMPLEX_UNIT_SP, SettingsRows.NOTE_SP);
            text.addView(titles[item], new LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
            text.addView(statuses[item], new LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
            row.addView(text, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f));
            buttons[item] = new TextView(context);
            SettingsRows.button(buttons[item], 0);
            int tapped = item;
            buttons[item].setOnClickListener(view -> shortcut.open(tapped));
            row.addView(buttons[item], new LayoutParams(
                    LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
            addView(row, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
            instructions[item] = SettingsRows.note(this);
        }
    }

    /**
     * Shows each item's status as the Rust app says, and {@code text}, the written instructions,
     * below the item at {@code instructed}, or none with -1.
     */
    void show(boolean defaultHome, boolean notifications, boolean usage, int instructed,
            String text) {
        boolean[] granted = {defaultHome, notifications, usage};
        Resources res = getResources();
        for (int item = 0; item < ITEMS; item++) {
            int state = granted[item] ? 1 : 0;
            if (shown[item] != state) {
                shown[item] = state;
                int which = granted[item] ? 0 : 1;
                statuses[item].setText(STATUSES[item][which]);
                String button = res.getString(BUTTONS[item][which]);
                buttons[item].setText(button);
                // "Notification access: Allow" (A11Y-01).
                buttons[item].setContentDescription(res.getString(
                        R.string.access_button_description, res.getString(TITLES[item]), button));
            }
            boolean instructedHere = item == instructed && text != null;
            if (instructedHere && !text.contentEquals(instructions[item].getText())) {
                instructions[item].setText(text);
            }
            instructions[item].setVisibility(instructedHere ? VISIBLE : GONE);
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
        for (int item = 0; item < ITEMS; item++) {
            titles[item].setTextColor(foreground);
            statuses[item].setTextColor(dim);
            instructions[item].setTextColor(foreground);
            SettingsRows.colourButton(buttons[item], foreground, dim, accent);
        }
    }
}
