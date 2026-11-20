package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.Rect;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.List;

/**
 * What Events shows below its header (EVT-03, EVT-04, EVT-09, EVT-11, ADR 0020): the card that
 * asks for notification access, "No new events", or a "Notifications" section with one row per
 * app, newest first, and Clear (EVT-07). Below the expanded group's row, each of its
 * notifications has a row of its own (EVT-06, ADR 0021).
 *
 * <p>The Rust app decides what shows and when the rows change; this view reads the rows again
 * only when the frame's revision changes. Each row says where it was in the rows last read, so a
 * row that stays keeps its view, a new one slides in, and one that went collapses where it was,
 * as far as the frame's progress says (ANIM-20).
 *
 * <p>A tap on a row opens its notification or expands its group, and a long-press, which {@link
 * HomeViews} times, opens its menu (EVT-08). Rows are named by their place in the rows last
 * read. Screen readers get the long-press as "Open menu", and a group's expanding as its own
 * actions (A11Y-05).
 *
 * <p>Rows hold what each newest notification says only to show it; nothing here logs it
 * (EVT-13).
 */
final class EventsList extends LinearLayout {
    // What shows below the header, the frame's slot 82, mirrored in src/android.rs.
    static final int NOTHING = 0;
    static final int EXPLAINER = 1;
    static final int EMPTY = 2;
    static final int LIST = 3;
    // The fields of one row in nativeEventsList, mirrored in events_row_fields in
    // src/android.rs: where it was, or -1; the app's name; flags; package, activity and profile
    // serial number for its icon; the marker's colour; the count; the title and the text.
    static final int FIELDS = 10;
    private static final int WORK = 1;
    /** One notification of the expanded group, below its group's row. */
    private static final int NOTIFICATION = 2;
    /** A group's row while its notifications show below it. */
    private static final int EXPANDED = 4;
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    /** Medium, for the app's name. */
    private static final int MEDIUM = 500;
    /** The side margins (SRS 5.2), inside the page's safe area. */
    private static final float MARGIN_DP = 16f;
    private static final float ICON_DP = 40f;
    private static final float MARKER_DP = 4f;
    private static final float GAP_DP = 12f;

    /** What the user does with the list. */
    interface Listener {
        /** Allow was tapped on the explainer card (EVT-03). */
        void allow();

        /** Clear was tapped on the section header (EVT-07). */
        void clear();

        /** Row {@code index} of the rows last read was tapped, at {@code onScreen} (EVT-06). */
        void tapped(int index, Rect onScreen);

        /**
         * A screen reader asks for row {@code index}'s menu (A11Y-05). Returns whether it
         * opened.
         */
        boolean menuRequested(int index);
    }

    private final AppIcons icons;
    private final Listener listener;
    private final float density;
    private final AccessCard explainer;
    private final SectionHeader section;
    private final TextView clear;
    private final TextView empty;
    private final Rows rows;
    /** The rows as of the last fill, in order, without those going. */
    private final List<Row> shown = new ArrayList<>();
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int accent = -1;
    private int divider = -1;

    EventsList(Context context, AppIcons icons, Listener listener) {
        super(context);
        this.icons = icons;
        this.listener = listener;
        density = getResources().getDisplayMetrics().density;
        int margin = dp(MARGIN_DP);
        setOrientation(VERTICAL);
        setPadding(margin, dp(GAP_DP), margin, dp(MARGIN_DP));

        // EVT-03, ONB-02: one plain sentence, and a button to the system's screen.
        explainer = new AccessCard(context, R.string.events_access_explainer,
                R.string.events_access_allow, listener::allow);
        addView(explainer, matchWidth());

        // The section header, with a hairline towards the end and Clear there (SRS 5.2, EVT-07).
        section = new SectionHeader(context, R.string.events_notifications);
        clear = new TextView(context);
        clear.setText(R.string.events_clear);
        clear.setContentDescription(getResources().getString(R.string.events_clear_description));
        clear.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        clear.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
        clear.setGravity(Gravity.CENTER);
        clear.setMinHeight(dp(A11y.TARGET_DP));
        clear.setMinWidth(dp(A11y.TARGET_DP));
        clear.setPadding(dp(GAP_DP), 0, dp(GAP_DP), 0);
        clear.setOnClickListener(view -> listener.clear());
        clear.setVisibility(GONE);
        LayoutParams clearParams = wrap();
        clearParams.setMarginStart(dp(4));
        section.addView(clear, clearParams);
        addView(section, matchWidth());

        rows = new Rows(context);
        addView(rows, matchWidth());

        // EVT-11: a calm line, where the groups would be.
        empty = new TextView(context);
        empty.setText(R.string.events_empty);
        empty.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        empty.setGravity(Gravity.CENTER);
        empty.setMinHeight(dp(A11y.TARGET_DP));
        // A11Y-08: read when it appears, such as after Clear.
        empty.setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);
        LayoutParams emptyParams = matchWidth();
        emptyParams.topMargin = dp(MARGIN_DP);
        addView(empty, emptyParams);

        show(NOTHING);
    }

    /** Shows what the frame's slot 82 says shows. */
    void show(int mode) {
        setVisibility(mode == NOTHING ? GONE : VISIBLE);
        explainer.setVisibility(mode == EXPLAINER ? VISIBLE : GONE);
        section.setVisibility(mode == EMPTY || mode == LIST ? VISIBLE : GONE);
        // A group that went still collapses as the empty state shows.
        rows.setVisibility(mode == EMPTY || mode == LIST ? VISIBLE : GONE);
        empty.setVisibility(mode == EMPTY ? VISIBLE : GONE);
    }

    /** Offers Clear while something shown can be cleared, as the frame's slot 86 says. */
    void clearable(boolean offered) {
        clear.setVisibility(offered ? VISIBLE : GONE);
    }

    /** Row {@code index} of the rows last read, or null. */
    View rowAt(int index) {
        return index >= 0 && index < shown.size() ? shown.get(index) : null;
    }

    /**
     * The app's name on row {@code index} of the rows last read, as screen readers read it, with
     * "work" for a work-profile app (A11Y-01), or null.
     */
    String label(int index) {
        return index >= 0 && index < shown.size() ? shown.get(index).spoken : null;
    }

    /** Where {@code view} is in the rows last read, if it is one of them; otherwise -1. */
    int indexOfRow(View view) {
        return view instanceof Row ? shown.indexOf(view) : -1;
    }

    /**
     * Shows the rows in {@code fields}, {@link #FIELDS} each. A row that says where it was keeps
     * that view; the others are new and come in. Rows of the last fill that none keeps go, in
     * their place. Rows still going from an earlier change are dropped, as its motion starts
     * again.
     */
    void fill(String[] fields) {
        String[] all = fields == null ? new String[0] : fields;
        int count = all.length / FIELDS;
        for (int i = rows.getChildCount() - 1; i >= 0; i--) {
            if (((Row) rows.getChildAt(i)).role == Row.GOING) {
                rows.removeViewAt(i);
            }
        }
        Row[] before = shown.toArray(new Row[0]);
        boolean[] kept = new boolean[before.length];
        List<Row> next = new ArrayList<>();
        for (int i = 0; i < count; i++) {
            int from = parseInt(all[i * FIELDS], -1);
            Row row;
            if (from >= 0 && from < before.length && !kept[from]) {
                kept[from] = true;
                row = before[from];
                row.role = Row.STAYING;
            } else {
                row = new Row(getContext());
                row.role = Row.COMING;
                row.share = 0f;
            }
            row.bind(all, i * FIELDS);
            next.add(row);
        }
        // In order: the rows that stay keep theirs, so each one that goes stays among them where
        // it was, and new ones come in where they are.
        List<Row> order = new ArrayList<>();
        int was = 0;
        for (Row row : next) {
            if (row.role == Row.STAYING) {
                int place = indexOf(before, row);
                for (; was < place; was++) {
                    goingAt(before, kept, was, order);
                }
                was = place + 1;
            }
            order.add(row);
        }
        for (; was < before.length; was++) {
            goingAt(before, kept, was, order);
        }
        rows.removeAllViews();
        for (Row row : order) {
            rows.addView(row);
            row.icon(icons);
        }
        shown.clear();
        shown.addAll(next);
        paint();
    }

    private static void goingAt(Row[] before, boolean[] kept, int place, List<Row> order) {
        if (!kept[place]) {
            Row going = before[place];
            going.role = Row.GOING;
            // Screen readers and touches no longer reach it (A11Y-01).
            going.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
            going.setClickable(false);
            order.add(going);
        }
    }

    private static int indexOf(Row[] rows, Row row) {
        for (int i = 0; i < rows.length; i++) {
            if (rows[i] == row) {
                return i;
            }
        }
        return -1;
    }

    /**
     * Draws the rows {@code progress} of the way through their change, from 0 to 10 000
     * (ANIM-20): a new row slides down into its full height and fades in, a row that went
     * collapses and fades out, and is dropped at the end.
     */
    void move(int progress) {
        float at = Math.min(1f, Math.max(0f, progress / 10_000f));
        boolean changed = false;
        for (int i = rows.getChildCount() - 1; i >= 0; i--) {
            Row row = (Row) rows.getChildAt(i);
            float share;
            if (row.role == Row.COMING) {
                share = at;
            } else if (row.role == Row.GOING) {
                share = 1f - at;
            } else {
                share = 1f;
            }
            if (at >= 1f) {
                if (row.role == Row.GOING) {
                    rows.removeViewAt(i);
                    changed = true;
                    continue;
                }
                row.role = Row.STAYING;
            }
            if (row.share != share) {
                row.share = share;
                changed = true;
            }
        }
        if (changed) {
            rows.requestLayout();
        }
    }

    /** The icons arrived or changed: each row shows its own. */
    void refreshIcons() {
        for (int i = 0; i < rows.getChildCount(); i++) {
            ((Row) rows.getChildAt(i)).icon(icons);
        }
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int surface, int accent, int divider) {
        if (foreground == this.foreground && dim == this.dim && surface == this.surface
                && accent == this.accent && divider == this.divider) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.surface = surface;
        this.accent = accent;
        this.divider = divider;
        paint();
    }

    private void paint() {
        explainer.colour(foreground, surface, accent);
        section.colour(dim, divider);
        clear.setTextColor(accent);
        clear.setBackground(SettingsRows.ripple(foreground));
        empty.setTextColor(dim);
        for (int i = 0; i < rows.getChildCount(); i++) {
            ((Row) rows.getChildAt(i)).colour(foreground, dim);
        }
    }

    private int dp(float value) {
        return Math.round(value * density);
    }

    private static LayoutParams matchWidth() {
        return new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
    }

    private static LayoutParams wrap() {
        return new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT);
    }

    private static int parseInt(String field, int otherwise) {
        try {
            return Integer.parseInt(field);
        } catch (NumberFormatException e) {
            return otherwise;
        }
    }

    private static long parseLong(String field) {
        try {
            return Long.parseLong(field);
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    /**
     * The rows, one under another, each taking its {@link Row#share} of its height. A row
     * coming in shows its bottom first, so it slides down from above; a row going is cut off
     * from below, so it collapses.
     */
    private static final class Rows extends ViewGroup {
        private final Rect clip = new Rect();

        Rows(Context context) {
            super(context);
        }

        @Override
        protected void onMeasure(int widthSpec, int heightSpec) {
            int width = MeasureSpec.getSize(widthSpec);
            int childWidth = MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY);
            int childHeight = MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED);
            int height = 0;
            for (int i = 0; i < getChildCount(); i++) {
                Row row = (Row) getChildAt(i);
                row.measure(childWidth, childHeight);
                height += slot(row);
            }
            setMeasuredDimension(width, resolveSize(height, heightSpec));
        }

        @Override
        protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
            int width = right - left;
            int y = 0;
            for (int i = 0; i < getChildCount(); i++) {
                Row row = (Row) getChildAt(i);
                int full = row.getMeasuredHeight();
                int slot = slot(row);
                row.layout(0, y, width, y + full);
                if (slot >= full) {
                    row.setTranslationY(0);
                    row.setClipBounds(null);
                } else if (row.role == Row.GOING) {
                    row.setTranslationY(0);
                    clip.set(0, 0, width, slot);
                    row.setClipBounds(clip);
                } else {
                    row.setTranslationY(slot - full);
                    clip.set(0, full - slot, width, full);
                    row.setClipBounds(clip);
                }
                row.setAlpha(row.share);
                y += slot;
            }
        }

        private static int slot(Row row) {
            return Math.round(row.getMeasuredHeight() * row.share);
        }
    }

    /**
     * One app's group (EVT-04): its accent marker (EVT-09), icon, name and count, and its newest
     * notification's title and text in at most two lines, or nothing more while it is expanded.
     * Or one notification of the expanded group, below it (EVT-06): the marker, and its title
     * and text where the group's name is. Screen readers read either as one item.
     */
    private final class Row extends LinearLayout {
        static final int STAYING = 0;
        static final int COMING = 1;
        static final int GOING = 2;

        private final GradientDrawable marker = new GradientDrawable();
        private final ImageView image;
        private final LinearLayout top;
        private final TextView name;
        private final TextView count;
        private final TextView message;
        int role = STAYING;
        /** How much of its height it takes, from 0 to 1 (ANIM-20). */
        float share = 1f;
        String label = "";
        /** The app's name as screen readers read it (A11Y-01). */
        String spoken = "";
        private long profile;
        private String packageName = "";
        private String activity = "";
        private String title = "";
        private String text = "";
        private boolean notification;
        private boolean expanded;
        private int notifications = 1;

        Row(Context context) {
            super(context);
            setOrientation(HORIZONTAL);
            setPadding(0, dp(GAP_DP), 0, dp(GAP_DP));
            setMinimumHeight(dp(A11y.TARGET_DP));
            setScreenReaderFocusable(true);
            // Not long-clickable: HomeViews times the long-press, so Android starts no second
            // timer (ADR 0013).
            setOnClickListener(view -> tapped());

            View bar = new View(context);
            marker.setCornerRadius(dp(MARKER_DP) / 2f);
            bar.setBackground(marker);
            bar.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
            addView(bar, new LayoutParams(dp(MARKER_DP), LayoutParams.MATCH_PARENT));

            image = new ImageView(context);
            image.setScaleType(ImageView.ScaleType.FIT_CENTER);
            image.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
            LayoutParams imageParams = new LayoutParams(dp(ICON_DP), dp(ICON_DP));
            imageParams.setMarginStart(dp(GAP_DP));
            addView(image, imageParams);

            LinearLayout texts = new LinearLayout(context);
            texts.setOrientation(VERTICAL);
            texts.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
            top = new LinearLayout(context);
            top.setOrientation(HORIZONTAL);
            name = body(context);
            name.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
            name.setSingleLine(true);
            name.setEllipsize(TextUtils.TruncateAt.END);
            top.addView(name, new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f));
            count = body(context);
            LayoutParams countParams = wrap();
            countParams.setMarginStart(dp(GAP_DP));
            top.addView(count, countParams);
            texts.addView(top, matchWidth());
            message = body(context);
            message.setMaxLines(2);
            message.setEllipsize(TextUtils.TruncateAt.END);
            texts.addView(message, matchWidth());
            LayoutParams textsParams = new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f);
            textsParams.setMarginStart(dp(MARGIN_DP));
            textsParams.gravity = Gravity.CENTER_VERTICAL;
            addView(texts, textsParams);
        }

        private TextView body(Context context) {
            TextView line = new TextView(context);
            line.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
            return line;
        }

        /** Takes on the row whose fields start at {@code at}. */
        void bind(String[] fields, int at) {
            label = field(fields, at + 1);
            int flags = parseInt(field(fields, at + 2), 0);
            boolean work = (flags & WORK) != 0;
            notification = (flags & NOTIFICATION) != 0;
            expanded = (flags & EXPANDED) != 0;
            packageName = field(fields, at + 3);
            activity = field(fields, at + 4);
            profile = parseLong(field(fields, at + 5));
            // The Rust app always sends the marker's colour; without one, there is no marker.
            marker.setColor(parseInt(field(fields, at + 6), 0));
            notifications = Math.max(1, parseInt(field(fields, at + 7), 1));
            title = field(fields, at + 8);
            text = field(fields, at + 9);

            // A notification's row keeps the icon's room, so its text lines up with the name.
            image.setVisibility(notification ? INVISIBLE : VISIBLE);
            top.setVisibility(notification ? GONE : VISIBLE);
            int padding = dp(notification ? GAP_DP / 2 : GAP_DP);
            setPadding(0, padding, 0, padding);
            name.setText(label);
            count.setText(Integer.toString(notifications));
            String said = said();
            message.setText(said);
            message.setVisibility(said.isEmpty() ? GONE : VISIBLE);
            String app = A11y.appName(getResources(), label, work);
            spoken = app;
            String description;
            if (notification) {
                description = said.isEmpty() ? app : said;
            } else if (expanded) {
                description = getResources().getQuantityString(
                        R.plurals.events_group_expanded_description, notifications, app,
                        notifications);
            } else {
                description = getResources().getQuantityString(
                        R.plurals.events_group_description, notifications, app, notifications,
                        said);
            }
            A11y.describe(this, description, expandable()
                    ? getResources().getString(
                            expanded ? R.string.events_expanded : R.string.events_collapsed)
                    : null);
            setClickable(role != GOING);
        }

        /** A group of several, which a tap expands or collapses rather than opens. */
        private boolean expandable() {
            return !notification && notifications > 1;
        }

        /** The title and the text, either alone if the other is empty. */
        private String said() {
            if (title.isEmpty() || text.isEmpty()) {
                return title.isEmpty() ? text : title;
            }
            return getResources().getString(R.string.events_title_text, title, text);
        }

        private void tapped() {
            int index = shown.indexOf(this);
            if (index < 0 || role == GOING) {
                return;
            }
            int[] at = new int[2];
            getLocationOnScreen(at);
            listener.tapped(index, new Rect(at[0], at[1], at[0] + getWidth(),
                    at[1] + getHeight()));
        }

        void icon(AppIcons icons) {
            Drawable icon = icons.get(profile, packageName, activity);
            image.setImageDrawable(icon);
        }

        void colour(int foreground, int dim) {
            name.setTextColor(foreground);
            count.setTextColor(dim);
            // A notification's own row reads as the group's newest does.
            message.setTextColor(notification ? foreground : dim);
            setBackground(SettingsRows.ripple(foreground));
        }

        // A tap opens or expands, and the long-press's menu is an action (A11Y-05).
        @Override
        public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
            super.onInitializeAccessibilityNodeInfo(info);
            if (role == GOING) {
                return;
            }
            int click = expandable()
                    ? (expanded ? R.string.action_collapse : R.string.action_expand)
                    : R.string.action_open_notification;
            info.addAction(new AccessibilityAction(AccessibilityNodeInfo.ACTION_CLICK,
                    getResources().getString(click)));
            if (expandable()) {
                info.addAction(expanded
                        ? AccessibilityAction.ACTION_COLLAPSE
                        : AccessibilityAction.ACTION_EXPAND);
            }
            info.setLongClickable(true);
            info.addAction(new AccessibilityAction(AccessibilityNodeInfo.ACTION_LONG_CLICK,
                    getResources().getString(R.string.action_app_menu)));
        }

        @Override
        public boolean performAccessibilityAction(int action, Bundle arguments) {
            if (role != GOING) {
                if (action == AccessibilityNodeInfo.ACTION_LONG_CLICK) {
                    int index = shown.indexOf(this);
                    return index >= 0 && listener.menuRequested(index);
                }
                if ((action == AccessibilityNodeInfo.ACTION_EXPAND && expandable() && !expanded)
                        || (action == AccessibilityNodeInfo.ACTION_COLLAPSE && expanded)) {
                    tapped();
                    return true;
                }
            }
            return super.performAccessibilityAction(action, arguments);
        }
    }

    private static String field(String[] fields, int at) {
        String value = at < fields.length ? fields[at] : null;
        return value == null ? "" : value;
    }
}
