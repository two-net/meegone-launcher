package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * The card that asks for special access (ONB-02): one plain sentence saying what MeeGone reads
 * and why, and Allow, a text button in the accent colour that opens the system's access screen.
 * It is flat, with 12 dp corners, in the surface colour (SRS 5.2). Events asks for notification
 * access with it (EVT-03, ADR 0020), and Recent for usage access (REC-02, ADR 0023).
 */
final class AccessCard extends LinearLayout {
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    /** Medium, for the button. */
    private static final int MEDIUM = 500;
    private static final float PADDING_DP = 16f;
    /** Cards and panels (SRS 5.2). */
    private static final float CORNER_DP = 12f;
    private static final float GAP_DP = 12f;

    private final GradientDrawable card = new GradientDrawable();
    private final TextView sentence;
    private final TextView allow;

    /** A card saying {@code explainer}, whose {@code button} runs {@code allowed}. */
    AccessCard(Context context, int explainer, int button, Runnable allowed) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        int padding = Math.round(PADDING_DP * density);
        setOrientation(VERTICAL);
        setPadding(padding, padding, padding, Math.round(4 * density));
        card.setCornerRadius(CORNER_DP * density);
        setBackground(card);
        sentence = new TextView(context);
        sentence.setText(explainer);
        sentence.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        addView(sentence, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        allow = new TextView(context);
        allow.setText(button);
        allow.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        allow.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
        allow.setGravity(Gravity.CENTER);
        int target = Math.round(A11y.TARGET_DP * density);
        allow.setMinHeight(target);
        allow.setMinWidth(target);
        int gap = Math.round(GAP_DP * density);
        allow.setPadding(gap, 0, gap, 0);
        allow.setOnClickListener(view -> allowed.run());
        LayoutParams allowParams =
                new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT);
        allowParams.gravity = Gravity.END;
        allowParams.topMargin = Math.round(4 * density);
        addView(allow, allowParams);
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int surface, int accent) {
        card.setColor(surface);
        sentence.setTextColor(foreground);
        allow.setTextColor(accent);
        allow.setBackground(SettingsRows.ripple(foreground));
    }
}
