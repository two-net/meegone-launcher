package io.github.twonet.meegone;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Rect;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.List;

/**
 * What Events shows below its header (EVT-03, EVT-04, EVT-09, EVT-11, ADR 0020): the card that
 * asks for notification access, "No new events", or a "Notifications" section with one row per
 * app, newest first.
 *
 * <p>The Rust app decides what shows and when the rows change; this view reads the rows again
 * only when the frame's revision changes. Each row says where it was in the rows last read, so a
 * row that stays keeps its view, a new one slides in, and one that went collapses where it was,
 * as far as the frame's progress says (ANIM-20).
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
    /** Body text (SRS 5.2). */
    private static final float BODY_SP = 15f;
    /** Section headers: 13 sp, in capitals and letter-spaced (SRS 5.2). */
    private static final float SECTION_SP = 13f;
    private static final float SECTION_SPACING = 0.1f;
    /** Medium, for the app's name. */
    private static final int MEDIUM = 500;
    /** The side margins (SRS 5.2), inside the page's safe area. */
    private static final float MARGIN_DP = 16f;
    /** Cards and panels (SRS 5.2). */
    private static final float CORNER_DP = 12f;
    private static final float ICON_DP = 40f;
    private static final float MARKER_DP = 4f;
    private static final float GAP_DP = 12f;
    /** The smallest a touch target gets (A11Y-02). */
    private static final float TARGET_DP = 48f;

    /** Allow was tapped on the explainer card. */
    interface Allow {
        void allow();
    }

    private final AppIcons icons;
    private final float density;
    private final LinearLayout explainer;
    private final GradientDrawable card;
    private final TextView sentence;
    private final TextView allow;
    private final LinearLayout section;
    private final TextView sectionTitle;
    private final View hairline;
    private final TextView empty;
    private final Rows rows;
    /** The rows as of the last fill, in order, without those going. */
    private final List<Row> shown = new ArrayList<>();
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int accent = -1;
    private int divider = -1;

    EventsList(Context context, AppIcons icons, Allow onAllow) {
        super(context);
        this.icons = icons;
        density = getResources().getDisplayMetrics().density;
        int margin = dp(MARGIN_DP);
        setOrientation(VERTICAL);
        setPadding(margin, dp(GAP_DP), margin, dp(MARGIN_DP));

        // EVT-03, ONB-02: one plain sentence, and a button to the system's screen.
        explainer = new LinearLayout(context);
        explainer.setOrientation(VERTICAL);
        explainer.setPadding(margin, margin, margin, dp(4));
        card = new GradientDrawable();
        card.setCornerRadius(dp(CORNER_DP));
        explainer.setBackground(card);
        sentence = new TextView(context);
        sentence.setText(R.string.events_access_explainer);
        sentence.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        explainer.addView(sentence, matchWidth());
        allow = new TextView(context);
        allow.setText(R.string.events_access_allow);
        allow.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        allow.setTypeface(Typeface.create(Typeface.DEFAULT, MEDIUM, false));
        allow.setGravity(Gravity.CENTER);
        allow.setMinHeight(dp(TARGET_DP));
        allow.setMinWidth(dp(TARGET_DP));
        allow.setPadding(dp(GAP_DP), 0, dp(GAP_DP), 0);
        allow.setOnClickListener(view -> onAllow.allow());
        LayoutParams allowParams = wrap();
        allowParams.gravity = Gravity.END;
        allowParams.topMargin = dp(4);
        explainer.addView(allow, allowParams);
        addView(explainer, matchWidth());

        // The section header, with a hairline to the right edge (SRS 5.2).
        section = new LinearLayout(context);
        section.setOrientation(HORIZONTAL);
        section.setGravity(Gravity.CENTER_VERTICAL);
        section.setMinimumHeight(dp(TARGET_DP));
        sectionTitle = new TextView(context);
        sectionTitle.setText(R.string.events_notifications);
        sectionTitle.setAllCaps(true);
        sectionTitle.setLetterSpacing(SECTION_SPACING);
        sectionTitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, SECTION_SP);
        sectionTitle.setAccessibilityHeading(true);
        section.addView(sectionTitle, wrap());
        hairline = new View(context);
        hairline.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        LayoutParams hairlineParams =
                new LayoutParams(0, Math.max(1, Math.round(density)), 1f);
        hairlineParams.setMarginStart(dp(GAP_DP));
        section.addView(hairline, hairlineParams);
        addView(section, matchWidth());

        rows = new Rows(context);
        addView(rows, matchWidth());

        // EVT-11: a calm line, where the groups would be.
        empty = new TextView(context);
        empty.setText(R.string.events_empty);
        empty.setTextSize(TypedValue.COMPLEX_UNIT_SP, BODY_SP);
        empty.setGravity(Gravity.CENTER);
        empty.setMinHeight(dp(TARGET_DP));
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
            // Screen readers no longer reach it (A11Y-01).
            going.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
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
        card.setColor(surface);
        sentence.setTextColor(foreground);
        allow.setTextColor(accent);
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        allow.setBackground(new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
        sectionTitle.setTextColor(dim);
        hairline.setBackgroundColor(divider);
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
     * notification's title and text in at most two lines. Screen readers read it as one item.
     */
    private final class Row extends LinearLayout {
        static final int STAYING = 0;
        static final int COMING = 1;
        static final int GOING = 2;

        private final GradientDrawable marker = new GradientDrawable();
        private final ImageView image;
        private final TextView name;
        private final TextView count;
        private final TextView message;
        int role = STAYING;
        /** How much of its height it takes, from 0 to 1 (ANIM-20). */
        float share = 1f;
        private long profile;
        private String packageName = "";
        private String activity = "";
        private String title = "";
        private String text = "";

        Row(Context context) {
            super(context);
            setOrientation(HORIZONTAL);
            setPadding(0, dp(GAP_DP), 0, dp(GAP_DP));
            setScreenReaderFocusable(true);

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
            LinearLayout top = new LinearLayout(context);
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
            String label = field(fields, at + 1);
            boolean work = (parseInt(field(fields, at + 2), 0) & WORK) != 0;
            packageName = field(fields, at + 3);
            activity = field(fields, at + 4);
            profile = parseLong(field(fields, at + 5));
            marker.setColor(parseInt(field(fields, at + 6), Color.GRAY));
            int notifications = Math.max(1, parseInt(field(fields, at + 7), 1));
            title = field(fields, at + 8);
            text = field(fields, at + 9);

            name.setText(label);
            count.setText(Integer.toString(notifications));
            String said = said();
            message.setText(said);
            message.setVisibility(said.isEmpty() ? GONE : VISIBLE);
            String app = work ? getResources().getString(R.string.app_work, label) : label;
            setContentDescription(getResources().getQuantityString(
                    R.plurals.events_group_description, notifications, app, notifications,
                    said));
        }

        /** The title and the text, either alone if the other is empty. */
        private String said() {
            if (title.isEmpty() || text.isEmpty()) {
                return title.isEmpty() ? text : title;
            }
            return getResources().getString(R.string.events_title_text, title, text);
        }

        void icon(AppIcons icons) {
            Drawable icon = icons.get(profile, packageName, activity);
            image.setImageDrawable(icon);
        }

        void colour(int foreground, int dim) {
            name.setTextColor(foreground);
            count.setTextColor(dim);
            message.setTextColor(dim);
        }
    }

    private static String field(String[] fields, int at) {
        String value = at < fields.length ? fields[at] : null;
        return value == null ? "" : value;
    }
}
