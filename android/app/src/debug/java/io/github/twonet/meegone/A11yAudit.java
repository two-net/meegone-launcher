package io.github.twonet.meegone;

import android.graphics.Bitmap;
import android.graphics.Color;
import android.graphics.Matrix;
import android.graphics.Picture;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.os.Handler;
import android.os.Looper;
import android.text.Layout;
import android.text.TextUtils;
import android.util.Log;
import android.view.PixelCopy;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.Window;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.widget.AbsListView;
import android.widget.EditText;
import android.widget.HorizontalScrollView;
import android.widget.ScrollView;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Consumer;

/**
 * Debug builds only: checks one window as it is drawn against #37's rules (ADR 0038), as
 * Google's Accessibility Scanner would, with nothing but the Android SDK.
 *
 * <ul>
 *   <li><b>Names</b> (A11Y-01): every element screen readers can act on has something to say:
 *       its text, description or hint, or the text of what it holds. Every action it offers
 *       has a label.
 *   <li><b>Targets</b> (A11Y-02): every element that takes a tap or a long-press, and every
 *       place a view takes taps of its own ({@link Audited}), is at least 48 × 48 dp on screen.
 *   <li><b>Contrast</b> (A11Y-03): every text on top, in the window's pixels, has at least
 *       4.5:1 against what is behind it, or 3:1 when it is large, unless it is disabled.
 *   <li><b>Scaling</b> (A11Y-04): no text is cut off by its view or an ancestor, no two texts
 *       overlap, and a text cut short is read whole by screen readers.
 *   <li><b>Reach</b> (A11Y-05, A11Y-06, ADR 0039): every action of MeeGone's own is on an element
 *       screen readers can focus, so they can offer it. A keyboard's Tab reaches every element
 *       on top that takes a tap or a long-press. Every stop does something, and stays inside
 *       what is on top. Stops that share a container come in reading order. The focus ring has
 *       3:1 against what is around it ({@link #checkRing}).
 * </ul>
 *
 * <p>Each failure is logged under {@link #TAG} as one line, {@code FAIL <screen> <theme>
 * <rule> <what>}.
 */
final class A11yAudit {
    static final String TAG = "MeeGoneA11y";
    private static final float TARGET_DP = A11y.TARGET_DP;
    private static final double TEXT_RATIO = 4.5;
    private static final double LARGE_RATIO = 3.0;
    /** WCAG's large text: 18 pt, or 14 pt bold, in CSS px, which a dp matches. */
    private static final float LARGE_DP = 24f;
    private static final float LARGE_BOLD_DP = 18.66f;
    /** Overlaps and cuts smaller than this, in px, are rounding. */
    private static final int SLACK_PX = 2;

    private final float density;
    private int checked;
    private int failed;

    A11yAudit(float density) {
        this.density = density;
    }

    int checked() {
        return checked;
    }

    int failed() {
        return failed;
    }

    /**
     * Counts one check of {@code rule} on {@code screen}, such as the views' order right to
     * left (ADR 0040), which fails, saying {@code what}, unless it {@code held}.
     */
    void expect(boolean held, String screen, String theme, String rule, String what) {
        checked++;
        if (!held) {
            fail(screen, theme, rule, what);
        }
    }

    /**
     * Copies {@code window}'s pixels, then checks {@code root}, its content, and calls {@code
     * done} on the main thread. A dialog's window has room for its shadow around it, so a
     * dialog, {@code drawn}, is drawn into the pixels instead of copied.
     */
    void check(Window window, View root, boolean drawn, String screen, String theme,
            Runnable done) {
        check(window, root, drawn, null, screen, theme, done);
    }

    /**
     * The same, with {@code layers}, the views a keyboard's focus may reach now, or null for
     * all of {@code root}.
     */
    void check(Window window, View root, boolean drawn, List<View> layers, String screen,
            String theme, Runnable done) {
        if (root.getWidth() <= 0 || root.getHeight() <= 0) {
            fail(screen, theme, "WINDOW", "has no size");
            done.run();
            return;
        }
        if (drawn) {
            // Recorded and drawn by the GPU, as the icons are hardware bitmaps (ADR 0041), then
            // copied where its pixels can be read.
            Picture recorded = new Picture();
            root.draw(recorded.beginRecording(root.getWidth(), root.getHeight()));
            recorded.endRecording();
            Bitmap drawnByGpu = Bitmap.createBitmap(recorded);
            Bitmap pixels = drawnByGpu.copy(Bitmap.Config.ARGB_8888, false);
            drawnByGpu.recycle();
            checkDrawn(root, pixels, screen, theme, layers);
            checkFocus(root, layers, screen, theme);
            pixels.recycle();
            done.run();
            return;
        }
        Bitmap pixels = Bitmap.createBitmap(root.getWidth(), root.getHeight(),
                Bitmap.Config.ARGB_8888);
        Handler main = new Handler(Looper.getMainLooper());
        PixelCopy.request(window, pixels, result -> {
            if (result == PixelCopy.SUCCESS) {
                checkDrawn(root, pixels, screen, theme, layers);
                checkFocus(root, layers, screen, theme);
            } else {
                fail(screen, theme, "WINDOW", "its pixels couldn't be copied (" + result + ")");
            }
            pixels.recycle();
            done.run();
        }, main);
    }

    private void checkDrawn(View root, Bitmap pixels, String screen, String theme,
            List<View> layers) {
        List<View> views = new ArrayList<>();
        collect(root, views);
        List<Drawn> texts = new ArrayList<>();
        for (View view : views) {
            boolean exposed = exposed(view);
            if (exposed) {
                checkNode(view, screen, theme);
            }
            if (view instanceof Audited) {
                checkAudited((Audited) view, root, pixels, screen, theme, texts);
            }
            if (view instanceof TextView) {
                // A text under an open pane is read once it is on top again.
                boolean onTop = layers == null || within(view, layers);
                Drawn text = checkText((TextView) view, root, pixels, screen, theme, onTop);
                if (text != null) {
                    texts.add(text);
                }
            }
        }
        // No two texts on top overlap (A11Y-04).
        for (int i = 0; i < texts.size(); i++) {
            for (int j = i + 1; j < texts.size(); j++) {
                Drawn a = texts.get(i);
                Drawn b = texts.get(j);
                if (related(a.view, b.view)) {
                    continue;
                }
                Rect both = new Rect();
                if (both.setIntersect(a.bounds, b.bounds) && both.width() > SLACK_PX
                        && both.height() > SLACK_PX) {
                    checked++;
                    fail(screen, theme, "OVERLAP", a.what + " and " + b.what + " at " + dp(both));
                }
            }
        }
    }

    /** Every view that is shown, from {@code view} down, in drawing order. */
    private static void collect(View view, List<View> out) {
        if (view.getVisibility() != View.VISIBLE || view.getAlpha() <= 0f) {
            return;
        }
        out.add(view);
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                collect(group.getChildAt(i), out);
            }
        }
    }

    /** Whether screen readers see {@code view}: important, and not hidden by an ancestor. */
    private static boolean exposed(View view) {
        if (!view.isImportantForAccessibility()) {
            return false;
        }
        for (ViewParent parent = view.getParent(); parent instanceof View;
                parent = parent.getParent()) {
            if (((View) parent).getImportantForAccessibility()
                    == View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS) {
                return false;
            }
        }
        return true;
    }

    // ---- Names (A11Y-01) and targets (A11Y-02).

    private void checkNode(View view, String screen, String theme) {
        AccessibilityNodeInfo info = view.createAccessibilityNodeInfo();
        if (!info.isVisibleToUser()) {
            return;
        }
        boolean custom = false;
        for (AccessibilityAction action : info.getActionList()) {
            if (isCustom(action.getId())) {
                custom = true;
                checked++;
                if (TextUtils.isEmpty(action.getLabel())) {
                    fail(screen, theme, "ACTION_LABEL", describe(view));
                }
            }
        }
        boolean taps = info.isClickable() || info.isLongClickable();
        boolean focusable = info.isFocusable() || info.isScreenReaderFocusable();
        if (custom) {
            // Screen readers offer the actions of the element they focus, and never focus a
            // container that only holds others (A11Y-05).
            checked++;
            if (!focusable && !taps) {
                fail(screen, theme, "ACTION_REACH", describe(view)
                        + " offers actions no screen reader can focus");
            }
        }
        boolean acts = taps || info.isEditable() || (custom && focusable);
        if (acts) {
            checked++;
            if (!speaks(info) && !childrenSpeak(view)) {
                fail(screen, theme, "NAME", describe(view));
            }
        }
        if (taps) {
            checked++;
            float width = view.getWidth() * scaleX(view) / density;
            float height = view.getHeight() * scaleY(view) / density;
            if (width + 0.5f < TARGET_DP || height + 0.5f < TARGET_DP) {
                fail(screen, theme, "TARGET", describe(view) + " is "
                        + Math.round(width) + " × " + Math.round(height) + " dp");
            }
        }
    }

    // ---- A keyboard's reach (A11Y-06, ADR 0039).

    /** Stops in one container closer than this, in px, are on one line. */
    private static final int LINE_SLACK_PX = 4;

    /**
     * Walks Tab's stops in {@code root}, in Android's own order, without moving the focus, and
     * checks them against what is on top: {@code layers}, or all of {@code root}.
     */
    private void checkFocus(View root, List<View> layers, String screen, String theme) {
        ArrayList<View> stops = new ArrayList<>();
        root.addFocusables(stops, View.FOCUS_FORWARD, View.FOCUSABLES_ALL);
        List<View> allowed = layers != null ? layers : java.util.Collections.singletonList(root);
        for (View stop : stops) {
            checked++;
            if (!within(stop, allowed) || !exposed(stop)) {
                fail(screen, theme, "FOCUS_TRAP", describe(stop)
                        + " is a stop under what is on top");
                continue;
            }
            checked++;
            AccessibilityNodeInfo info = stop.createAccessibilityNodeInfo();
            boolean does = info.isClickable() || info.isLongClickable() || info.isEditable()
                    || info.isScrollable() || stop instanceof ViewNode || hasOwnAction(info);
            if (!does) {
                fail(screen, theme, "FOCUS_EMPTY", describe(stop) + " is a stop that does nothing");
            }
        }
        // Every element on top that a finger can tap or hold, a keyboard reaches.
        List<View> shown = new ArrayList<>();
        collect(root, shown);
        for (View view : shown) {
            if (!view.isEnabled() || !exposed(view) || !within(view, allowed)) {
                continue;
            }
            AccessibilityNodeInfo info = view.createAccessibilityNodeInfo();
            if (!info.isVisibleToUser() || !(info.isClickable() || info.isLongClickable())) {
                continue;
            }
            checked++;
            // A list's rows are reached through the list, which moves among them itself.
            if (!stops.contains(view) && !inListThatIsAStop(view, stops)) {
                fail(screen, theme, "FOCUS_REACH", describe(view) + " can't be reached by Tab");
            }
        }
        // Stops side by side in one container come in reading order.
        boolean rtl = root.getLayoutDirection() == View.LAYOUT_DIRECTION_RTL;
        for (int i = 1; i < stops.size(); i++) {
            View a = stops.get(i - 1);
            View b = stops.get(i);
            if (a.getParent() != b.getParent()) {
                continue;
            }
            Rect first = inWindow(a, new Rect(0, 0, a.getWidth(), a.getHeight()), root);
            Rect then = inWindow(b, new Rect(0, 0, b.getWidth(), b.getHeight()), root);
            checked++;
            boolean sameLine = Math.abs(then.top - first.top) <= LINE_SLACK_PX;
            boolean back = sameLine
                    ? (rtl ? then.left > first.left : then.left < first.left)
                    : then.top < first.top;
            if (back) {
                fail(screen, theme, "FOCUS_ORDER", describe(b) + " comes after " + describe(a));
            }
        }
    }

    private static boolean hasOwnAction(AccessibilityNodeInfo info) {
        for (AccessibilityAction action : info.getActionList()) {
            if (isCustom(action.getId())) {
                return true;
            }
        }
        return false;
    }

    private static boolean inListThatIsAStop(View view, List<View> stops) {
        for (ViewParent parent = view.getParent(); parent instanceof View;
                parent = parent.getParent()) {
            if (parent instanceof AbsListView && stops.contains((View) parent)) {
                return true;
            }
        }
        return false;
    }

    private static boolean within(View view, List<View> layers) {
        for (View layer : layers) {
            for (View at = view; at != null;
                    at = at.getParent() instanceof View ? (View) at.getParent() : null) {
                if (at == layer) {
                    return true;
                }
            }
        }
        return false;
    }

    /**
     * Checks the focus ring drawn at {@code ring}, in {@code root}'s coordinates: at the middle
     * of each of its sides on screen, the ring has 3:1 against what is just outside it, the
     * focused element's surroundings as they are without focus (A11Y-06, WCAG 2.4.13).
     */
    void checkRing(Window window, View root, Rect ring, String screen, String theme,
            Runnable done) {
        checked++;
        if (ring.isEmpty()) {
            fail(screen, theme, "FOCUS_RING", "nothing shows where the focus is");
            done.run();
            return;
        }
        Bitmap pixels = Bitmap.createBitmap(root.getWidth(), root.getHeight(),
                Bitmap.Config.ARGB_8888);
        PixelCopy.request(window, pixels, result -> {
            if (result != PixelCopy.SUCCESS) {
                fail(screen, theme, "WINDOW", "its pixels couldn't be copied (" + result + ")");
            } else {
                int out = Math.max(2, Math.round(2.5f * density));
                int[][] sides = {
                    {ring.centerX(), ring.top, 0, -out}, {ring.centerX(), ring.bottom, 0, out},
                    {ring.left, ring.centerY(), -out, 0}, {ring.right, ring.centerY(), out, 0},
                };
                for (int[] side : sides) {
                    int x = side[0];
                    int y = side[1];
                    int outsideX = x + side[2];
                    int outsideY = y + side[3];
                    if (!inside(pixels, x, y) || !inside(pixels, outsideX, outsideY)) {
                        continue;
                    }
                    checked++;
                    int drawnColour = pixels.getPixel(x, y) | 0xFF000000;
                    int around = pixels.getPixel(outsideX, outsideY) | 0xFF000000;
                    double ratio = contrast(drawnColour, around);
                    if (ratio < LARGE_RATIO) {
                        fail(screen, theme, "FOCUS_RING", "the ring is "
                                + String.format("%.2f", ratio) + ":1 (" + hex(drawnColour)
                                + " beside " + hex(around) + ") at " + Math.round(x / density)
                                + "," + Math.round(y / density) + " dp");
                    }
                }
            }
            pixels.recycle();
            done.run();
        }, new Handler(Looper.getMainLooper()));
    }

    private static boolean inside(Bitmap pixels, int x, int y) {
        return x >= 0 && y >= 0 && x < pixels.getWidth() && y < pixels.getHeight();
    }

    /** An action of MeeGone's own, named by one of its resources, rather than a standard one. */
    private static boolean isCustom(int id) {
        return (id >>> 24) == 0x7f;
    }

    private static boolean speaks(AccessibilityNodeInfo info) {
        return !TextUtils.isEmpty(info.getContentDescription())
                || !TextUtils.isEmpty(info.getText()) || !TextUtils.isEmpty(info.getHintText());
    }

    /**
     * Whether what {@code view} holds gives it something to say, as screen readers put it
     * together: the text of each child they see that doesn't act on its own.
     */
    private static boolean childrenSpeak(View view) {
        if (!(view instanceof ViewGroup)) {
            return false;
        }
        ViewGroup group = (ViewGroup) view;
        for (int i = 0; i < group.getChildCount(); i++) {
            View child = group.getChildAt(i);
            if (child.getVisibility() != View.VISIBLE || child.getImportantForAccessibility()
                    == View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS) {
                continue;
            }
            if (child.isClickable() || child.isLongClickable()) {
                continue;
            }
            if (!TextUtils.isEmpty(child.getContentDescription())) {
                return true;
            }
            if (child instanceof TextView && !TextUtils.isEmpty(((TextView) child).getText())) {
                return true;
            }
            if (childrenSpeak(child)) {
                return true;
            }
        }
        return false;
    }

    private void checkAudited(Audited view, View root, Bitmap pixels, String screen,
            String theme, List<Drawn> texts) {
        View self = (View) view;
        List<Rect> targets = new ArrayList<>();
        view.auditTargets(targets);
        for (Rect target : targets) {
            checked++;
            float width = target.width() * scaleX(self) / density;
            float height = target.height() * scaleY(self) / density;
            if (width + 0.5f < TARGET_DP || height + 0.5f < TARGET_DP) {
                fail(screen, theme, "TARGET", "a target in " + describe(self) + " is "
                        + Math.round(width) + " × " + Math.round(height) + " dp");
            }
        }
        List<Rect> drawn = new ArrayList<>();
        view.auditTexts(drawn);
        for (Rect text : drawn) {
            Rect bounds = inWindow(self, text, root);
            if (!visible(self, bounds)) {
                continue;
            }
            Rect seen = new Rect();
            self.getGlobalVisibleRect(seen);
            bounds.intersect(seen);
            if (covered(self, bounds, root)) {
                continue;
            }
            checked++;
            int[] colours = dominant(pixels, bounds);
            if (colours == null) {
                continue;
            }
            double ratio = contrast(colours[0], colours[1]);
            if (ratio < TEXT_RATIO) {
                fail(screen, theme, "CONTRAST", "text in " + describe(self) + " is "
                        + String.format("%.2f", ratio) + ":1 ("
                        + hex(colours[1]) + " on " + hex(colours[0]) + ")");
            }
            texts.add(new Drawn(self, bounds, "text in " + describe(self)));
        }
    }

    // ---- Contrast (A11Y-03) and scaling (A11Y-04) of a text view.

    private Drawn checkText(TextView view, View root, Bitmap pixels, String screen,
            String theme, boolean onTop) {
        Layout layout = view.getLayout();
        CharSequence text = view.getText();
        boolean hint = TextUtils.isEmpty(text) && view instanceof EditText
                && !TextUtils.isEmpty(view.getHint());
        if (layout == null || (TextUtils.isEmpty(text) && !hint)) {
            return null;
        }
        int colour = hint ? view.getCurrentHintTextColor() : view.getCurrentTextColor();
        if (Color.alpha(colour) == 0) {
            // Drawn for screen readers only, such as the status line.
            return null;
        }
        String what = "\"" + ellipsise(hint ? view.getHint() : text) + "\" ("
                + view.getClass().getSimpleName() + ")";
        Rect own = hint
                ? new Rect(view.getCompoundPaddingLeft(), view.getCompoundPaddingTop(),
                        view.getWidth() - view.getCompoundPaddingRight(),
                        view.getHeight() - view.getCompoundPaddingBottom())
                : textBounds(view, layout);
        Rect bounds = inWindow(view, own, root);
        if (!visible(view, bounds)) {
            return null;
        }
        // Only what shows: a scrolling view's text goes on past its edge.
        Rect seen = new Rect();
        view.getGlobalVisibleRect(seen);
        bounds.intersect(seen);
        if (covered(view, bounds, root)) {
            return null;
        }

        // Cut off by the view itself, or by an ancestor that doesn't scroll.
        int lines = layout.getLineCount();
        int maxLines = view.getMaxLines();
        int shownLines = maxLines > 0 ? Math.min(lines, maxLines) : lines;
        if (!hint) {
            checked++;
            int room = view.getHeight() - view.getCompoundPaddingTop()
                    - view.getCompoundPaddingBottom();
            int needed = layout.getLineTop(shownLines);
            if (needed > room + SLACK_PX && !view.canScrollVertically(1)
                    && !view.canScrollVertically(-1)) {
                fail(screen, theme, "CLIPPED", what + " needs " + Math.round(needed / density)
                        + " dp of " + Math.round(room / density));
            }
        }
        Rect clip = clippedBy(view, root);
        if (clip != null) {
            fail(screen, theme, "CLIPPED", what + " is cut off at " + dp(clip));
        }

        // Cut short: screen readers still read it whole.
        boolean shortened = shownLines < lines;
        for (int line = 0; line < shownLines; line++) {
            shortened |= layout.getEllipsisCount(line) > 0;
        }
        if (shortened && !hint) {
            checked++;
            if (onTop && !readWhole(view, text.toString())) {
                fail(screen, theme, "TRUNCATED", what + " isn't read whole by screen readers");
            }
        }

        // Contrast with what is drawn behind it, unless it takes no input.
        if (view.isEnabled()) {
            checked++;
            int behind = background(pixels, bounds);
            int drawn = over(withAlpha(colour, effectiveAlpha(view)), behind);
            double ratio = contrast(drawn, behind);
            double need = large(view) ? LARGE_RATIO : TEXT_RATIO;
            if (ratio < need) {
                fail(screen, theme, "CONTRAST", what + " is " + String.format("%.2f", ratio)
                        + ":1, needs " + need + " (" + hex(drawn) + " on " + hex(behind) + ")");
            }
        }
        return new Drawn(view, bounds, what);
    }

    /** Where the text of {@code view} is drawn, in its own coordinates. */
    private static Rect textBounds(TextView view, Layout layout) {
        int left = view.getTotalPaddingLeft() - view.getScrollX();
        int top = view.getTotalPaddingTop() - view.getScrollY();
        Rect bounds = new Rect();
        Rect line = new Rect();
        for (int i = 0; i < layout.getLineCount(); i++) {
            line.set(left + (int) Math.floor(layout.getLineLeft(i)), top + layout.getLineTop(i),
                    left + (int) Math.ceil(layout.getLineRight(i)), top + layout.getLineBottom(i));
            if (line.width() > 0) {
                bounds.union(line);
            }
        }
        return bounds;
    }

    /**
     * Whether a text cut short reaches screen readers whole: through its own node, which holds
     * the whole text, or, when it is hidden from them, through the description of an ancestor
     * they read instead, which says it in words, such as "5 minutes ago" for "5 min ago".
     */
    private static boolean readWhole(TextView view, String text) {
        if (exposed(view)) {
            return true;
        }
        for (ViewParent parent = view.getParent(); parent instanceof View;
                parent = parent.getParent()) {
            View ancestor = (View) parent;
            if (exposed(ancestor) && !TextUtils.isEmpty(ancestor.getContentDescription())) {
                return true;
            }
        }
        return false;
    }

    /**
     * The part of {@code view} an ancestor that doesn't scroll cuts off, in the window's dp, or
     * null if none does. Scrolling views show the rest as they scroll.
     */
    private Rect clippedBy(View view, View root) {
        Rect full = inWindow(view, new Rect(0, 0, view.getWidth(), view.getHeight()), root);
        for (ViewParent parent = view.getParent(); parent instanceof ViewGroup && parent != root;
                parent = parent.getParent()) {
            ViewGroup ancestor = (ViewGroup) parent;
            if (scrolls(ancestor)) {
                return null;
            }
            if (!ancestor.getClipChildren()) {
                continue;
            }
            Rect own = inWindow(ancestor,
                    new Rect(0, 0, ancestor.getWidth(), ancestor.getHeight()), root);
            if (!own.contains(full) && full.width() > 0 && full.height() > 0) {
                Rect cut = new Rect(full);
                if (!cut.intersect(own)) {
                    return full;
                }
                if (full.width() - cut.width() > SLACK_PX
                        || full.height() - cut.height() > SLACK_PX) {
                    return full;
                }
            }
        }
        return null;
    }

    private static boolean scrolls(View view) {
        return view instanceof ScrollView || view instanceof HorizontalScrollView
                || view instanceof AbsListView || view instanceof HomeViews;
    }

    private static boolean large(TextView view) {
        float sizeDp = view.getTextSize() / view.getResources().getDisplayMetrics().density;
        Typeface face = view.getTypeface();
        boolean bold = face != null && (face.isBold() || face.getWeight() >= 600);
        return sizeDp >= LARGE_DP || (bold && sizeDp >= LARGE_BOLD_DP);
    }

    // ---- Where views are, and what covers them.

    /**
     * {@code rect}, in {@code view}'s coordinates, in the window's, where its pixels are: every
     * scroll, translation and scale on the way included.
     */
    private static Rect inWindow(View view, Rect rect, View root) {
        Matrix matrix = new Matrix();
        view.transformMatrixToGlobal(matrix);
        RectF mapped = new RectF(rect);
        matrix.mapRect(mapped);
        return new Rect(Math.round(mapped.left), Math.round(mapped.top),
                Math.round(mapped.right), Math.round(mapped.bottom));
    }

    /** Whether any of {@code bounds} is on screen within {@code view}'s visible part. */
    private static boolean visible(View view, Rect bounds) {
        Rect seen = new Rect();
        return !bounds.isEmpty() && view.getGlobalVisibleRect(seen) && Rect.intersects(seen,
                bounds);
    }

    /**
     * Whether something drawn after {@code view} with a background of its own covers the middle
     * of {@code drawn}, which {@code view} draws, in the window's coordinates: Settings, a menu,
     * a scrim, Done over the grid. Screen readers skip what is behind those, and its contrast
     * isn't what anyone reads.
     */
    private static boolean covered(View view, Rect drawn, View root) {
        int x = drawn.centerX();
        int y = drawn.centerY();
        View child = view;
        for (ViewParent parent = view.getParent(); parent instanceof ViewGroup;
                parent = parent.getParent()) {
            ViewGroup group = (ViewGroup) parent;
            for (int i = group.indexOfChild(child) + 1; i < group.getChildCount(); i++) {
                View later = group.getChildAt(i);
                if (later.getVisibility() == View.VISIBLE && later.getAlpha() > 0f
                        && later.getBackground() != null && later.getWidth() > 0) {
                    Rect over = inWindow(later,
                            new Rect(0, 0, later.getWidth(), later.getHeight()), root);
                    if (over.contains(x, y)) {
                        return true;
                    }
                }
            }
            if (group == root) {
                break;
            }
            child = group;
        }
        return false;
    }

    private static boolean related(View a, View b) {
        for (ViewParent p = a.getParent(); p instanceof View; p = p.getParent()) {
            if (p == b) {
                return true;
            }
        }
        for (ViewParent p = b.getParent(); p instanceof View; p = p.getParent()) {
            if (p == a) {
                return true;
            }
        }
        return a == b;
    }

    private static float scaleX(View view) {
        float scale = view.getScaleX();
        for (ViewParent p = view.getParent(); p instanceof View; p = p.getParent()) {
            scale *= ((View) p).getScaleX();
        }
        return scale;
    }

    private static float scaleY(View view) {
        float scale = view.getScaleY();
        for (ViewParent p = view.getParent(); p instanceof View; p = p.getParent()) {
            scale *= ((View) p).getScaleY();
        }
        return scale;
    }

    private static float effectiveAlpha(View view) {
        float alpha = view.getAlpha();
        for (ViewParent p = view.getParent(); p instanceof View; p = p.getParent()) {
            alpha *= ((View) p).getAlpha();
        }
        return alpha;
    }

    // ---- Colours, as WCAG 2.2 measures them (R8), and as crates/core/data/src/colour.rs does.

    /** The colour most of {@code bounds} is drawn in: what is behind a text. */
    private static int background(Bitmap pixels, Rect bounds) {
        int[] colours = dominant(pixels, bounds);
        return colours == null ? Color.BLACK : colours[0];
    }

    /**
     * The colour most of {@code bounds} is drawn in, then, of the colours that cover at least a
     * little of the rest, the one with the most contrast against it: a text's own colour,
     * rather than its blended edges. Null if {@code bounds} is off the window.
     */
    private static int[] dominant(Bitmap pixels, Rect bounds) {
        Rect area = new Rect(bounds);
        if (!area.intersect(0, 0, pixels.getWidth(), pixels.getHeight()) || area.isEmpty()) {
            return null;
        }
        int[] drawn = new int[area.width() * area.height()];
        pixels.getPixels(drawn, 0, area.width(), area.left, area.top, area.width(),
                area.height());
        Map<Integer, Integer> counts = new HashMap<>();
        int most = 0;
        int mostCount = 0;
        for (int pixel : drawn) {
            int colour = pixel | 0xFF000000;
            int count = counts.merge(colour, 1, Integer::sum);
            if (count > mostCount) {
                most = colour;
                mostCount = count;
            }
        }
        int rest = drawn.length - mostCount;
        int floor = Math.max(3, rest / 50);
        int text = most;
        double best = 1;
        for (Map.Entry<Integer, Integer> entry : counts.entrySet()) {
            if (entry.getValue() >= floor && entry.getKey() != most) {
                double ratio = contrast(entry.getKey(), most);
                if (ratio > best) {
                    best = ratio;
                    text = entry.getKey();
                }
            }
        }
        return new int[] {most, text};
    }

    private static int withAlpha(int colour, float alpha) {
        int a = Math.round(Color.alpha(colour) * Math.max(0f, Math.min(1f, alpha)));
        return (colour & 0x00FFFFFF) | (a << 24);
    }

    /** {@code top}, with its alpha, drawn over the opaque {@code below}. */
    private static int over(int top, int below) {
        float a = Color.alpha(top) / 255f;
        return Color.rgb(
                Math.round(Color.red(top) * a + Color.red(below) * (1 - a)),
                Math.round(Color.green(top) * a + Color.green(below) * (1 - a)),
                Math.round(Color.blue(top) * a + Color.blue(below) * (1 - a)));
    }

    static double contrast(int a, int b) {
        double la = luminance(a);
        double lb = luminance(b);
        return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
    }

    private static double luminance(int colour) {
        return 0.2126 * channel(Color.red(colour)) + 0.7152 * channel(Color.green(colour))
                + 0.0722 * channel(Color.blue(colour));
    }

    private static double channel(int value) {
        double c = value / 255.0;
        return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
    }

    // ---- Reporting.

    private void fail(String screen, String theme, String rule, String what) {
        failed++;
        Log.w(TAG, "FAIL " + screen + " " + theme + " " + rule + " " + what);
    }

    private String describe(View view) {
        CharSequence said = view.getContentDescription();
        if (TextUtils.isEmpty(said) && view instanceof TextView) {
            said = ((TextView) view).getText();
        }
        int[] at = new int[2];
        view.getLocationOnScreen(at);
        return view.getClass().getSimpleName()
                + (TextUtils.isEmpty(said) ? "" : " \"" + ellipsise(said) + "\"")
                + " at " + Math.round(at[0] / density) + "," + Math.round(at[1] / density)
                + " dp";
    }

    private String dp(Rect rect) {
        return Math.round(rect.left / density) + "," + Math.round(rect.top / density) + " "
                + Math.round(rect.width() / density) + "×" + Math.round(rect.height() / density)
                + " dp";
    }

    private static String ellipsise(CharSequence text) {
        String plain = text.toString().replace('\n', ' ');
        return plain.length() > 40 ? plain.substring(0, 39) + "…" : plain;
    }

    private static String hex(int colour) {
        return String.format("#%06X", colour & 0xFFFFFF);
    }

    /** A text on top, where it is drawn in the window, and how to name it. */
    private static final class Drawn {
        final View view;
        final Rect bounds;
        final String what;

        Drawn(View view, Rect bounds, String what) {
            this.view = view;
            this.bounds = bounds;
            this.what = what;
        }
    }

    /** For the tour: runs {@code step} after {@code delayMs} on the main thread. */
    static void later(long delayMs, Runnable step) {
        new Handler(Looper.getMainLooper()).postDelayed(step, delayMs);
    }

    /** For the tour: every view under {@code root} for which {@code test} holds. */
    static void each(View root, java.util.function.Predicate<View> test, Consumer<View> found) {
        if (test.test(root)) {
            found.accept(root);
        }
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int i = 0; i < group.getChildCount(); i++) {
                each(group.getChildAt(i), test, found);
            }
        }
    }
}
