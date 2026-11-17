package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.util.TypedValue;
import android.widget.LinearLayout;
import android.widget.Switch;
import android.widget.TextView;

/**
 * Settings → Lock-screen image (SET-02, ADR 0022), as far as #21 draws it: "Show card in
 * Events", which hides or shows the image-of-the-day card (EVT-12). The change takes effect at
 * once (SET-03). Until #34 draws the rest of Settings, it sits below Hidden apps.
 */
final class ImageSettings extends LinearLayout {
    /** "Show card in Events" was switched. */
    interface Toggle {
        void showCard(boolean on);
    }

    private static final float GROUP_SP = 14f;
    private static final float TEXT_SP = 16f;
    private static final float ROW_DP = 56f;
    private static final float GAP_DP = 16f;

    private final TextView group;
    private final Switch showCard;
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    ImageSettings(Context context, Toggle toggle) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        setOrientation(VERTICAL);
        setPadding(0, Math.round(GAP_DP * density), 0, 0);

        group = new TextView(context);
        group.setText(R.string.menu_lock_screen_image);
        group.setTextSize(TypedValue.COMPLEX_UNIT_SP, GROUP_SP);
        group.setAccessibilityHeading(true);
        addView(group, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));

        // Its own label, so screen readers hear "Show card in Events, switch, on" (A11Y-01).
        showCard = new Switch(context);
        showCard.setText(R.string.show_image_card);
        showCard.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        showCard.setMinHeight(Math.round(ROW_DP * density));
        // A click, unlike a change of state, comes only from the user.
        showCard.setOnClickListener(view -> toggle.showCard(showCard.isChecked()));
        addView(showCard, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
    }

    /** Shows whether the card is on, as the Rust app says. */
    void show(boolean on) {
        if (showCard.isChecked() != on) {
            showCard.setChecked(on);
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
        showCard.setTextColor(foreground);
        int[][] states = {{android.R.attr.state_checked}, {}};
        showCard.setThumbTintList(new ColorStateList(states, new int[] {accent, foreground}));
        showCard.setTrackTintList(new ColorStateList(states, new int[] {accent, dim}));
    }
}
