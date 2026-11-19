package io.github.twonet.meegone;

import android.content.Context;
import android.widget.LinearLayout;
import android.widget.Switch;
import android.widget.TextView;

/**
 * Settings → Home (SET-02, ADR 0035): "Home button opens", the last view or always Applications
 * (HOME-06), and Wrap-around, which joins Recent and Events in the carousel (HOME-04). A change
 * takes effect at once (SET-03).
 */
final class HomeSettings extends LinearLayout {
    /** What the user did here. */
    interface Listener {
        /** "Home button opens" was set to {@code target}, as the Rust app numbers it. */
        void homeTarget(int target);

        /** Wrap-around was switched. */
        void wrapAround(boolean on);
    }

    /** "Home button opens": the last view, or always Applications, as the Rust app numbers them. */
    private static final int[] TARGETS = {0, 1};
    private static final int[] TARGET_LABELS = {
        R.string.home_target_last_view, R.string.home_target_applications,
    };

    private final TextView group;
    private final ChoiceSettings homeTarget;
    private final Switch wrapAround;
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    HomeSettings(Context context, Listener listener) {
        super(context);
        SettingsRows.startGroup(this);
        group = SettingsRows.heading(this, R.string.settings_group_home);
        homeTarget = new ChoiceSettings(context, 0, R.string.home_target, TARGETS, TARGET_LABELS,
                listener::homeTarget);
        addView(homeTarget, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        // A click, unlike a change of state, comes only from the user.
        wrapAround = SettingsRows.switchRow(this, R.string.wrap_around);
        wrapAround.setOnClickListener(view -> listener.wrapAround(wrapAround.isChecked()));
    }

    /**
     * Shows the choices the Rust app says. A tap changes the control at once, so this also puts
     * it back when the change couldn't be saved.
     */
    void show(int target, boolean wrapAroundOn) {
        homeTarget.show(target);
        if (wrapAround.isChecked() != wrapAroundOn) {
            wrapAround.setChecked(wrapAroundOn);
        }
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int accent) {
        homeTarget.colour(foreground, dim, accent);
        if (foreground == this.foreground && dim == this.dim && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.accent = accent;
        group.setTextColor(dim);
        SettingsRows.colourSwitch(wrapAround, foreground, dim, accent);
    }
}
