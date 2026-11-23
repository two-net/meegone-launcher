package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.drawable.GradientDrawable;
import android.util.TypedValue;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * A card that says a part of MeeGone stopped working after a bug, while the rest goes on
 * working, with Restart MeeGone (ARCH-09, ADR 0043): a flat card in the surface colour, as the
 * home-app banner is. {@link HomeViews} owns every one and shows it where the Rust app says, so
 * none of the failed part's own code runs to show it.
 */
final class StoppedCard extends LinearLayout {
    private final GradientDrawable card = new GradientDrawable();
    private final TextView text;
    private final TextView restart;
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int accent = -1;

    /** A card saying {@code says}, whose button calls {@code restartNow}. */
    StoppedCard(Context context, int says, Runnable restartNow) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        int gap = Math.round(SettingsRows.GAP_DP * density);
        setOrientation(VERTICAL);
        // The button's row ends half a gap from the edge, on the right or, right to left, on
        // the left (L10N-02).
        setPaddingRelative(gap, gap, gap / 2, 0);
        card.setCornerRadius(12 * density);
        setBackground(card);
        // Touches on the card stay there: none reaches what is behind it.
        setClickable(true);
        A11y.notAButton(this);
        text = new TextView(context);
        text.setText(says);
        text.setTextSize(TypedValue.COMPLEX_UNIT_SP, SettingsRows.TEXT_SP);
        // Said when it appears on the view showing; pages out of sight are silent (A11Y-08).
        text.setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);
        addView(text, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        ButtonRow buttons = new ButtonRow(context);
        restart = new TextView(context);
        SettingsRows.button(restart, R.string.stopped_restart);
        restart.setOnClickListener(view -> restartNow.run());
        buttons.addView(restart);
        addView(buttons, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        setVisibility(GONE);
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int surface, int accent) {
        if (foreground == this.foreground && dim == this.dim && surface == this.surface
                && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.surface = surface;
        this.accent = accent;
        card.setColor(surface);
        text.setTextColor(foreground);
        SettingsRows.colourButton(restart, foreground, dim, accent);
    }

    /** Shows it or hides it. Returns whether that changed. */
    boolean show(boolean shown) {
        int visibility = shown ? VISIBLE : GONE;
        if (getVisibility() == visibility) {
            return false;
        }
        setVisibility(visibility);
        return true;
    }
}
