package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.drawable.GradientDrawable;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * The banner that offers to make MeeGone the default home app while it isn't, at most once a
 * day (ONB-04, ADR 0005, ADR 0036): a flat card in the surface colour at the bottom of the home
 * views, with Dismiss and Make default. The Rust app decides when it shows: never over setup,
 * Settings, a menu, an open folder, search, edit mode or close mode.
 */
final class HomeBannerView extends LinearLayout {
    /** What the user did here. */
    interface Listener {
        void makeDefault();

        void dismiss();
    }

    private final GradientDrawable card = new GradientDrawable();
    private final TextView text;
    private final TextView dismiss;
    private final TextView makeDefault;
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int accent = -1;

    HomeBannerView(Context context, Listener listener) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        int gap = Math.round(SettingsRows.GAP_DP * density);
        setOrientation(VERTICAL);
        setPadding(gap, gap, gap / 2, 0);
        card.setCornerRadius(12 * density);
        setBackground(card);
        // Touches on the card stay there: none reaches the views behind it.
        setClickable(true);
        text = new TextView(context);
        text.setText(R.string.home_banner_text);
        text.setTextSize(TypedValue.COMPLEX_UNIT_SP, SettingsRows.TEXT_SP);
        addView(text, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        LinearLayout buttons = new LinearLayout(context);
        buttons.setOrientation(HORIZONTAL);
        buttons.setGravity(Gravity.END | Gravity.CENTER_VERTICAL);
        dismiss = new TextView(context);
        SettingsRows.button(dismiss, R.string.home_banner_dismiss);
        dismiss.setOnClickListener(view -> listener.dismiss());
        makeDefault = new TextView(context);
        SettingsRows.button(makeDefault, R.string.access_make_default);
        makeDefault.setOnClickListener(view -> listener.makeDefault());
        buttons.addView(dismiss, new LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        buttons.addView(makeDefault, new LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        addView(buttons, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
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
        SettingsRows.colourButton(dismiss, foreground, dim, accent);
        SettingsRows.colourButton(makeDefault, foreground, dim, accent);
    }
}
