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
 * "Notifications" adds Clear after the hairline (EVT-07), and Recent Clear all (REC-07); "Image
 * of the day" has nothing more (EVT-12). A button at the end keeps its width, and the title
 * wraps into what is left, so a large font size never pushes the button off the screen
 * (A11Y-04).
 */
final class SectionHeader extends LinearLayout {
    /** Section headers: 13 sp, in capitals and letter-spaced (SRS 5.2). */
    private static final float TITLE_SP = 13f;
    private static final float SPACING = 0.1f;
    private static final float GAP_DP = 12f;

    private final TextView title;
    private final View hairline;

    SectionHeader(Context context, int text) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        setOrientation(HORIZONTAL);
        setGravity(Gravity.CENTER_VERTICAL);
        setMinimumHeight(Math.round(A11y.TARGET_DP * density));
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

    @Override
    protected void onMeasure(int widthMeasureSpec, int heightMeasureSpec) {
        if (MeasureSpec.getMode(widthMeasureSpec) != MeasureSpec.UNSPECIFIED) {
            int room = MeasureSpec.getSize(widthMeasureSpec) - getPaddingLeft()
                    - getPaddingRight();
            int taken = 0;
            for (int i = 0; i < getChildCount(); i++) {
                View child = getChildAt(i);
                if (child == title || child == hairline || child.getVisibility() == GONE) {
                    continue;
                }
                LayoutParams params = (LayoutParams) child.getLayoutParams();
                child.measure(MeasureSpec.makeMeasureSpec(room, MeasureSpec.AT_MOST),
                        MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED));
                taken += child.getMeasuredWidth() + params.getMarginStart()
                        + params.getMarginEnd();
            }
            int most = Math.max(0, room - taken);
            if (title.getMaxWidth() != most) {
                title.setMaxWidth(most);
            }
        }
        super.onMeasure(widthMeasureSpec, heightMeasureSpec);
    }

    /** The theme's colours (THEME-03): the title's, and the hairline's. */
    void colour(int dim, int divider) {
        title.setTextColor(dim);
        hairline.setBackgroundColor(divider);
    }
}
