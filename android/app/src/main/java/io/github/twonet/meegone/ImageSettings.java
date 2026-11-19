package io.github.twonet.meegone;

import android.content.Context;
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
 * Settings → Lock-screen image (SET-02, ADR 0022, ADR 0029), as far as #21 and #28 draw it:
 *
 * <ul>
 *   <li>"Show card in Events", which hides or shows the image-of-the-day card (EVT-12);
 *   <li>"Use mobile data", which lets the daily image download over mobile data (WALL-04);
 *   <li>after three days of failed checks, "Daily image unavailable" with Try again (SRS 7.6).
 * </ul>
 *
 * Changes take effect at once (SET-03). Until #34 draws the rest of Settings, it sits below
 * Hidden apps.
 */
final class ImageSettings extends LinearLayout {
    /** What the user did here. */
    interface Listener {
        /** "Show card in Events" was switched. */
        void showCard(boolean on);

        /** "Use mobile data" was switched. */
        void useMobileData(boolean on);

        /** Try again was tapped. */
        void tryAgain();
    }

    private static final float GROUP_SP = 14f;
    private static final float TEXT_SP = 16f;
    private static final float ROW_DP = 56f;
    private static final float GAP_DP = 16f;
    /** The smallest a touch target gets (A11Y-02). */
    private static final float TARGET_DP = 48f;
    private static final int MEDIUM = 500;

    private final TextView group;
    private final Switch showCard;
    private final Switch useMobileData;
    private final LinearLayout unavailable;
    private final TextView unavailableText;
    private final TextView tryAgain;
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    ImageSettings(Context context, Listener listener) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        setOrientation(VERTICAL);
        setPadding(0, Math.round(GAP_DP * density), 0, 0);

        group = new TextView(context);
        group.setText(R.string.menu_lock_screen_image);
        group.setTextSize(TypedValue.COMPLEX_UNIT_SP, GROUP_SP);
        group.setAccessibilityHeading(true);
        addView(group, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));

        // "Daily image unavailable" comes first, where it is seen, and only while it applies.
        unavailable = new LinearLayout(context);
        unavailable.setOrientation(HORIZONTAL);
        unavailable.setGravity(Gravity.CENTER_VERTICAL);
        unavailable.setMinimumHeight(Math.round(ROW_DP * density));
        unavailable.setVisibility(GONE);
        unavailableText = new TextView(context);
        unavailableText.setText(R.string.image_unavailable);
        unavailableText.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        unavailable.addView(unavailableText, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f));
        tryAgain = new TextView(context);
        tryAgain.setText(R.string.image_try_again);
        tryAgain.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        tryAgain.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
        tryAgain.setGravity(Gravity.CENTER);
        int target = Math.round(TARGET_DP * density);
        tryAgain.setMinHeight(target);
        tryAgain.setMinWidth(target);
        int gap = Math.round(GAP_DP * density);
        tryAgain.setPadding(gap, 0, gap, 0);
        tryAgain.setOnClickListener(view -> listener.tryAgain());
        unavailable.addView(tryAgain, new LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(unavailable, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        // Each its own label, so screen readers hear "Show card in Events, switch, on" (A11Y-01).
        // A click, unlike a change of state, comes only from the user.
        showCard = row(context, R.string.show_image_card, density);
        showCard.setOnClickListener(view -> listener.showCard(showCard.isChecked()));
        useMobileData = row(context, R.string.use_mobile_data, density);
        useMobileData.setOnClickListener(
                view -> listener.useMobileData(useMobileData.isChecked()));
    }

    private Switch row(Context context, int label, float density) {
        Switch row = new Switch(context);
        row.setText(label);
        row.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        row.setMinHeight(Math.round(ROW_DP * density));
        addView(row, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        return row;
    }

    /** Shows the choices, and whether the daily image is unavailable, as the Rust app says. */
    void show(boolean showCardOn, boolean useMobileDataOn, boolean imageUnavailable) {
        if (showCard.isChecked() != showCardOn) {
            showCard.setChecked(showCardOn);
        }
        if (useMobileData.isChecked() != useMobileDataOn) {
            useMobileData.setChecked(useMobileDataOn);
        }
        unavailable.setVisibility(imageUnavailable ? VISIBLE : GONE);
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
        unavailableText.setTextColor(foreground);
        tryAgain.setTextColor(accent);
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        tryAgain.setBackground(new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
        int[][] states = {{android.R.attr.state_checked}, {}};
        for (Switch row : new Switch[] {showCard, useMobileData}) {
            row.setTextColor(foreground);
            row.setThumbTintList(new ColorStateList(states, new int[] {accent, foreground}));
            row.setTrackTintList(new ColorStateList(states, new int[] {accent, dim}));
        }
    }
}
