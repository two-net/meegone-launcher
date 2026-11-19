package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.RippleDrawable;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * Settings → Applications → Hidden apps (APPS-19, SET-02, ADR 0017): the apps the user hid, each
 * with its icon, its label and Unhide, or "No hidden apps".
 *
 * <p>It reads its apps from the Rust app only when the frame's revision changes, and names an app
 * by that revision and its place, so a tap on a list that changed meanwhile does nothing. Icons
 * come from the grids' {@link AppIcons}.
 */
final class HiddenAppsList extends LinearLayout {
    /** Unhide was tapped on the app at {@code index} of the list at {@code revision}. */
    interface Unhide {
        void unhide(int revision, int index);
    }

    // The fields of one app in nativeHiddenApps, mirrored in hidden_app_fields in src/android.rs:
    // its label, package, activity, profile serial number and flags.
    static final int FIELDS = 5;
    private static final int WORK = 1;
    private static final int AVAILABLE = 2;
    private static final float GROUP_SP = 14f;
    private static final float TITLE_SP = 18f;
    private static final float TEXT_SP = 16f;
    private static final float ICON_DP = 40f;
    private static final float ROW_DP = 56f;
    /** The smallest a touch target gets (A11Y-02). */
    private static final float TARGET_DP = 48f;
    private static final float GAP_DP = 16f;
    /** How faint an unavailable app's label is, as its icon in the grid (APPS-09). */
    private static final float UNAVAILABLE_ALPHA = 0.5f;

    private final AppIcons icons;
    private final Unhide unhide;
    private final TextView group;
    private final TextView title;
    private final TextView none;
    private final LinearLayout rows;
    private final int iconSize;
    private final int rowHeight;
    private final int target;
    private final int gap;
    /** The list's revision, and its apps' fields, as last filled. */
    private int revision;
    private String[] fields = new String[0];
    /** The place whose Unhide was tapped, which screen readers move on from, or -1. */
    private int unhid = -1;
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    HiddenAppsList(Context context, AppIcons icons, Unhide unhide) {
        super(context);
        this.icons = icons;
        this.unhide = unhide;
        float density = getResources().getDisplayMetrics().density;
        iconSize = Math.round(ICON_DP * density);
        rowHeight = Math.round(ROW_DP * density);
        target = Math.round(TARGET_DP * density);
        gap = Math.round(GAP_DP * density);
        setOrientation(VERTICAL);

        group = heading(R.string.settings_group_applications, GROUP_SP);
        addView(group, wrap());
        title = heading(R.string.hidden_apps, TITLE_SP);
        LayoutParams titleParams = wrap();
        titleParams.topMargin = gap / 2;
        addView(title, titleParams);

        rows = new LinearLayout(context);
        rows.setOrientation(VERTICAL);
        addView(rows, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));

        none = new TextView(context);
        none.setText(R.string.hidden_apps_none);
        none.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        none.setMinHeight(rowHeight);
        none.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
        addView(none, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
    }

    private TextView heading(int text, float sizeSp) {
        TextView heading = new TextView(getContext());
        heading.setText(text);
        heading.setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp);
        heading.setAccessibilityHeading(true);
        return heading;
    }

    private static LayoutParams wrap() {
        return new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT);
    }

    /** Shows the hidden apps at {@code revision}, in {@link #FIELDS} fields each. */
    void fill(int revision, String[] fields) {
        this.revision = revision;
        this.fields = fields == null ? new String[0] : fields;
        rows.removeAllViews();
        int count = this.fields.length / FIELDS;
        for (int i = 0; i < count; i++) {
            rows.addView(row(i), new LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        }
        none.setVisibility(count == 0 ? VISIBLE : GONE);
        paint();
        if (unhid >= 0) {
            // A screen reader goes on from where the app was: the next one's Unhide, or the
            // line saying there are none.
            View next = count == 0 ? none : rows.getChildAt(Math.min(unhid, count - 1))
                    .findViewWithTag(Unhide.class);
            unhid = -1;
            next.performAccessibilityAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS,
                    null);
        }
    }

    private View row(int index) {
        Context context = getContext();
        String label = field(index, 0);
        int flags = parseInt(field(index, 4));
        boolean work = (flags & WORK) != 0;
        boolean available = (flags & AVAILABLE) != 0;
        String name = work ? getResources().getString(R.string.app_work, label) : label;

        LinearLayout row = new LinearLayout(context);
        row.setOrientation(HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setMinimumHeight(rowHeight);

        ImageView icon = new ImageView(context);
        icon.setScaleType(ImageView.ScaleType.FIT_CENTER);
        icon.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        icon.setImageDrawable(icon(index));
        icon.setAlpha(available ? 1f : UNAVAILABLE_ALPHA);
        row.addView(icon, new LayoutParams(iconSize, iconSize));

        TextView text = new TextView(context);
        text.setText(label);
        text.setContentDescription(name);
        text.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        text.setSingleLine(true);
        text.setEllipsize(TextUtils.TruncateAt.END);
        text.setAlpha(available ? 1f : UNAVAILABLE_ALPHA);
        LayoutParams textParams = new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f);
        textParams.setMarginStart(gap);
        textParams.setMarginEnd(gap);
        row.addView(text, textParams);

        TextView button = new TextView(context);
        button.setTag(Unhide.class);
        button.setText(R.string.hidden_app_unhide);
        button.setContentDescription(getResources().getString(R.string.action_unhide, name));
        button.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        button.setGravity(Gravity.CENTER);
        button.setMinWidth(target);
        button.setMinHeight(target);
        button.setPadding(gap, 0, gap, 0);
        int at = revision;
        button.setOnClickListener(view -> {
            unhid = index;
            unhide.unhide(at, index);
        });
        row.addView(button, wrap());
        return row;
    }

    private String field(int index, int field) {
        String value = fields[index * FIELDS + field];
        return value == null ? "" : value;
    }

    private Drawable icon(int index) {
        return icons.get(parseLong(field(index, 3)), field(index, 1), field(index, 2));
    }

    /** The icons arrived or changed: each row shows its own. */
    void refreshIcons() {
        for (int i = 0; i < rows.getChildCount(); i++) {
            LinearLayout row = (LinearLayout) rows.getChildAt(i);
            ((ImageView) row.getChildAt(0)).setImageDrawable(icon(i));
        }
    }

    /** The theme's colours (THEME-03): text, secondary text, and Unhide in the accent. */
    void colour(int foreground, int dim, int accent) {
        if (foreground == this.foreground && dim == this.dim && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.accent = accent;
        paint();
    }

    private void paint() {
        group.setTextColor(dim);
        title.setTextColor(foreground);
        none.setTextColor(dim);
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        for (int i = 0; i < rows.getChildCount(); i++) {
            LinearLayout row = (LinearLayout) rows.getChildAt(i);
            ((TextView) row.getChildAt(1)).setTextColor(foreground);
            TextView button = (TextView) row.getChildAt(2);
            button.setTextColor(accent);
            button.setBackground(
                    new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
        }
    }

    private static int parseInt(String field) {
        try {
            return Integer.parseInt(field);
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    private static long parseLong(String field) {
        try {
            return Long.parseLong(field);
        } catch (NumberFormatException e) {
            return 0;
        }
    }
}
