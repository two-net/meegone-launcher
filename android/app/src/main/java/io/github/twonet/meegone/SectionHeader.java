package io.github.twonet.meegone;

import android.content.Context;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * A section header (SRS 5.2): its title in 13 sp capitals, letter-spaced, in the secondary
 * colour, then a hairline to the end. Screen readers hear the title as a heading. Events'
 * "Notifications" adds Clear after the hairline (EVT-07); "Image of the day" has nothing more
 * (EVT-12).
 */
final class SectionHeader extends LinearLayout {
    /** Section headers: 13 sp, in capitals and letter-spaced (SRS 5.2). */
    private static final float TITLE_SP = 13f;
    private static final float SPACING = 0.1f;
    private static final float GAP_DP = 12f;
    /** The smallest a touch target gets (A11Y-02), so a button at its end fits. */
    private static final float TARGET_DP = 48f;

    private final TextView title;
    private final View hairline;

    SectionHeader(Context context, int text) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        setOrientation(HORIZONTAL);
        setGravity(Gravity.CENTER_VERTICAL);
        setMinimumHeight(Math.round(TARGET_DP * density));
        title = new TextView(context);
        title.setText(text);
        title.setAllCaps(true);
        title.setLetterSpacing(SPACING);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, TITLE_SP);
        title.setAccessibilityHeading(true);
        addView(title, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        hairline = new View(context);
        hairline.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        LayoutParams hairlineParams =
                new LayoutParams(0, Math.max(1, Math.round(density)), 1f);
        hairlineParams.setMarginStart(Math.round(GAP_DP * density));
        addView(hairline, hairlineParams);
    }

    /** The theme's colours (THEME-03): the title's, and the hairline's. */
    void colour(int dim, int divider) {
        title.setTextColor(dim);
        hairline.setBackgroundColor(divider);
    }
}
