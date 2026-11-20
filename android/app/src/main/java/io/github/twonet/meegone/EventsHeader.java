package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.Typeface;
import android.text.Layout;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * The top of Events (EVT-01, EVT-02, ADR 0019): today's weekday and date in large, light type
 * and, while an alarm is set, a row with its time that opens the app that set it.
 *
 * <p>The Rust app writes the text in the device's language and 12- or 24-hour setting (L10N-03),
 * and this view reads it again only when the frame's revision changes. It needs no notification
 * access (EVT-03).
 */
final class EventsHeader extends LinearLayout {
    // The fields of nativeEventsHeader, mirrored in src/android.rs: the weekday, the day and
    // month, and when the next alarm rings, or empty.
    static final int FIELDS = 3;
    /** The Events date (SRS 5.2); sp, so it scales with the system font size. */
    private static final float DATE_SP = 40f;
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    /** Light, for large numerals and headers (SRS 5.2). */
    private static final int LIGHT = 300;
    /** The side margins (SRS 5.2), inside the page's safe area. */
    private static final float MARGIN_DP = 16f;
    private static final float TOP_DP = 8f;

    private final TextView weekday;
    private final TextView dayMonth;
    private final TextView alarm;
    private int foreground = -1;
    private int dim = -1;

    EventsHeader(Context context, Runnable openAlarm) {
        super(context);
        float density = getResources().getDisplayMetrics().density;
        int margin = Math.round(MARGIN_DP * density);
        setOrientation(VERTICAL);
        setPadding(margin, Math.round(TOP_DP * density), margin, 0);

        // Screen readers read the date as one heading, e.g. "Tuesday, 17 November".
        LinearLayout date = new LinearLayout(context);
        date.setOrientation(VERTICAL);
        date.setScreenReaderFocusable(true);
        date.setAccessibilityHeading(true);
        weekday = large(context);
        dayMonth = large(context);
        date.addView(weekday, matchWidth());
        date.addView(dayMonth, matchWidth());
        addView(date, matchWidth());

        alarm = new TextView(context);
        alarm.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        alarm.setMinHeight(Math.round(A11y.TARGET_DP * density));
        alarm.setMinWidth(Math.round(A11y.TARGET_DP * density));
        alarm.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
        alarm.setOnClickListener(view -> openAlarm.run());
        alarm.setAccessibilityDelegate(new AccessibilityDelegate() {
            @Override
            public void onInitializeAccessibilityNodeInfo(View host, AccessibilityNodeInfo info) {
                super.onInitializeAccessibilityNodeInfo(host, info);
                info.addAction(new AccessibilityAction(AccessibilityNodeInfo.ACTION_CLICK,
                        getResources().getString(R.string.action_open_alarm)));
            }
        });
        alarm.setVisibility(GONE);
        addView(alarm, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
    }

    /**
     * A line of the date. At large font sizes it wraps, hyphenating a long word, rather than
     * overlapping or being cut short (A11Y-04).
     */
    private static TextView large(Context context) {
        TextView text = new TextView(context);
        text.setTextSize(TypedValue.COMPLEX_UNIT_SP, DATE_SP);
        text.setTypeface(Typeface.create(Typeface.DEFAULT, LIGHT, false));
        text.setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NORMAL);
        return text;
    }

    private static LayoutParams matchWidth() {
        return new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
    }

    /** Shows the header's {@link #FIELDS} fields; an empty one leaves its line out. */
    void fill(String[] fields) {
        boolean whole = fields != null && fields.length == FIELDS;
        show(weekday, whole ? fields[0] : "");
        show(dayMonth, whole ? fields[1] : "");
        String time = whole ? fields[2] : "";
        show(alarm, time.isEmpty() ? "" : getResources().getString(R.string.events_alarm, time));
    }

    private static void show(TextView line, String text) {
        line.setText(text);
        line.setVisibility(text.isEmpty() ? GONE : VISIBLE);
    }

    /** The theme's colours (THEME-03): the date in the text colour, the alarm in the secondary. */
    void colour(int foreground, int dim) {
        if (foreground == this.foreground && dim == this.dim) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        weekday.setTextColor(foreground);
        dayMonth.setTextColor(foreground);
        alarm.setTextColor(dim);
        alarm.setBackground(SettingsRows.ripple(foreground));
    }
}
