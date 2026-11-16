package io.github.twonet.meegone;

import android.annotation.TargetApi;
import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Insets;
import android.graphics.Matrix;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.os.Build;
import android.os.Bundle;
import android.util.TypedValue;
import android.view.Choreographer;
import android.view.DisplayCutout;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.RoundedCorner;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * The three home views, Events, Applications and Recent, in a wrap-around carousel (HOME-02 to
 * HOME-04), drawn with Android's own views (ADR 0006). They come back from an app with a short
 * scale and fade (ANIM-09, ADR 0007). A long-press on empty space opens the home menu, or enters
 * close mode in Recent (HOME-10, ADR 0008).
 *
 * <p>The Rust app owns the gesture, the motion, the view indicator and the home menu. This view
 * passes it every touch, frame time and layout change, says when a finger has been held on
 * empty space, then reads what to draw with {@link #nativeRead} and applies it. It keeps nothing
 * else. The frame slots below must stay in step with {@code frame_slots} in {@code
 * src/android.rs}. Applications is an {@link AppsGrid}, which draws the frame's Applications
 * slots (ADR 0010); Events and Recent show their names until #19 and #22.
 *
 * <p>The background fills the window edge to edge, behind the system bars. Everything else stays
 * inside a safe area clear of the bars, display cut-outs and the screen's rounded corners: each
 * page and Settings get it as padding, so their content lays out inside it (HOME-11, ADR 0009).
 * Content that scrolls draws under the bars instead: the Applications grid takes the safe area
 * as its own padding, with {@code clipToPadding} off (ADR 0010).
 *
 * <p>An element inside a view that uses horizontal drags itself keeps them by calling {@link
 * #requestDisallowInterceptTouchEvent} before the carousel takes the gesture (HOME-03). An
 * element with its own long-press is clickable or long-clickable, so a long-press on it never
 * opens the home menu.
 */
final class HomeViews extends FrameLayout implements Choreographer.FrameCallback {
    // The slots of the frame that nativeRead fills.
    private static final int PAGES = 0; // Events, Applications and Recent: offsets in px
    private static final int INDICATOR_ALPHA = 3; // 0 to 255
    private static final int INDICATOR_SELECTED = 4; // 0 to 2
    private static final int CURRENT = 5; // 0 to 2
    private static final int ANIMATING = 6; // 1 while something moves
    private static final int BACKGROUND = 7; // ARGB
    private static final int FOREGROUND = 8; // ARGB
    private static final int DIM = 9; // ARGB
    private static final int CONTENT_SCALE = 10; // ten-thousandths; 10 000 at rest
    private static final int CONTENT_ALPHA = 11; // 0 to 255
    private static final int MENU = 12; // the home menu's items as bits 1 << item; 0 if closed
    private static final int MENU_OFFERED = 13; // 1 when the current view offers the menu
    private static final int MENU_X = 14; // px from the left, or -1 to centre
    private static final int MENU_Y = 15; // px from the top, or -1 to centre
    private static final int SURFACE = 16; // ARGB
    private static final int SETTINGS = 17; // 0 closed, 1 open, 2 at Lock-screen image
    private static final int APPS_REVISION = 18; // changes when the grid's cells change
    private static final int APPS_SCROLL = 19; // px from the top
    private static final int APPS_COLUMNS = 20;
    private static final int APPS_ROWS = 21; // the last one empty
    private static final int PRESSED = 22; // the current view's pressed cell, or -1
    private static final int PRESS_SCALE = 23; // its icon's scale in ten-thousandths
    private static final int FRAME_SIZE = 24;

    private static final int VIEWS = 3;
    private static final int APPLICATIONS = 1;
    private static final int[] NAMES = {
        R.string.view_events, R.string.view_applications, R.string.view_recent
    };
    /** The home menu's items, in the order of {@code MenuItem::ALL}. */
    private static final int[] MENU_ITEMS = {
        R.string.menu_launcher_settings,
        R.string.menu_lock_screen_image,
        R.string.menu_edit_applications
    };
    /** Passed to {@link #nativeMenu} for a touch outside the menu. */
    private static final int CLOSE_MENU = -1;
    private static final int SETTINGS_AT_LOCK_SCREEN_IMAGE = 2;

    private final int[] frame = new int[FRAME_SIZE];
    private final FrameLayout[] pages = new FrameLayout[VIEWS];
    /** Each view's name, until its content arrives; null for Applications, which has its grid. */
    private final TextView[] titles = new TextView[VIEWS];
    private final AppsGrid apps;
    private final View[] segments = new View[VIEWS];
    /** The view indicator (HOME-09). Screen readers skip it; {@link #status} speaks instead. */
    private final LinearLayout indicator;
    /**
     * Tells screen readers which view shows, e.g. "Applications, 2 of 3" (A11Y-08). It is a
     * polite live region, so changes are announced. It must stay visible to them: a view with
     * no size or no opacity isn't.
     */
    private final TextView status;
    /** Covers the home views while Settings is open. Until #34 draws it, it shows its name. */
    private final LinearLayout settings;
    private final TextView settingsTitle;
    private final TextView settingsGroup;
    /** The home menu (HOME-10): a card at the finger, over the home views. */
    private final LinearLayout menu;
    private final GradientDrawable menuCard;
    private final TextView[] menuItems = new TextView[MENU_ITEMS.length];
    private final int indicatorMargin;
    private final int menuMargin;
    private final int touchSlop;
    /** Asks the Rust app about a finger held on empty space (HOME-10). */
    private final Runnable longPress = this::longPressed;
    private final Matrix inverse = new Matrix();
    private final float[] point = new float[2];

    /** Whether the carousel, or a long-press, has taken the current gesture from the content. */
    private boolean claimed;
    /** A child element claimed the current gesture, so the carousel gets none of it. */
    private boolean childClaimed;
    /** The current gesture began outside the open menu: it closed the menu and goes nowhere. */
    private boolean swallowing;
    /** The finger that drives the carousel. */
    private int pointerId = MotionEvent.INVALID_POINTER_ID;
    /** Where the driving finger went down, for finding what is under it after a long-press. */
    private float downX;
    private float downY;
    private boolean framePosted;
    private int announced = -1;
    private int selected = -1;
    private int background;
    private int surface;
    private int foreground;
    private int dim;
    private int menuShown;
    private int settingsShown;
    private int menuOffered = -1;
    /** How far content stays from each edge, in px (HOME-11). */
    private Insets safe = Insets.NONE;

    HomeViews(Context context) {
        super(context);
        touchSlop = ViewConfiguration.get(context).getScaledTouchSlop();
        indicatorMargin = dp(12);
        menuMargin = dp(16);
        // Carries the screen-reader actions that switch views and open the menu (A11Y-05).
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);

        apps = new AppsGrid(context, this::apply);
        for (int i = 0; i < VIEWS; i++) {
            FrameLayout page = new FrameLayout(context);
            if (i == APPLICATIONS) {
                page.addView(apps, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
            } else {
                // Until Events' and Recent's content arrives (#19, #22), each shows its name.
                TextView title = new TextView(context);
                title.setText(NAMES[i]);
                title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 28);
                titles[i] = title;
                page.addView(title, new LayoutParams(
                        LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT, Gravity.CENTER));
            }
            page.setVisibility(INVISIBLE);
            pages[i] = page;
            addView(page, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        }

        // Three segments in the views' order; a right-to-left layout mirrors them (L10N-02).
        indicator = new LinearLayout(context);
        indicator.setOrientation(LinearLayout.HORIZONTAL);
        indicator.setAlpha(0f);
        indicator.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
        for (int i = 0; i < VIEWS; i++) {
            View segment = new View(context);
            GradientDrawable shape = new GradientDrawable();
            shape.setCornerRadius(dp(2));
            segment.setBackground(shape);
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(dp(24), dp(4));
            params.setMarginStart(dp(3));
            params.setMarginEnd(dp(3));
            indicator.addView(segment, params);
            segments[i] = segment;
        }
        LayoutParams params = new LayoutParams(
                LayoutParams.WRAP_CONTENT,
                LayoutParams.WRAP_CONTENT,
                Gravity.TOP | Gravity.CENTER_HORIZONTAL);
        params.topMargin = indicatorMargin;
        addView(indicator, params);

        // One transparent pixel: present for screen readers, invisible on screen.
        status = new TextView(context);
        status.setTextColor(Color.TRANSPARENT);
        status.setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);
        addView(status, new LayoutParams(1, 1, Gravity.TOP | Gravity.START));

        settings = new LinearLayout(context);
        settings.setOrientation(LinearLayout.VERTICAL);
        settings.setGravity(Gravity.CENTER);
        // Touches on Settings stay there.
        settings.setClickable(true);
        settings.setVisibility(GONE);
        settings.setAccessibilityPaneTitle(getResources().getString(R.string.settings_title));
        settingsTitle = new TextView(context);
        settingsTitle.setText(R.string.settings_title);
        settingsTitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, 28);
        settings.addView(settingsTitle, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        settingsGroup = new TextView(context);
        settingsGroup.setText(R.string.menu_lock_screen_image);
        settingsGroup.setTextSize(TypedValue.COMPLEX_UNIT_SP, 18);
        settingsGroup.setVisibility(GONE);
        settings.addView(settingsGroup, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        addView(settings, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));

        menu = new LinearLayout(context);
        menu.setOrientation(LinearLayout.VERTICAL);
        menu.setPadding(0, dp(8), 0, dp(8));
        menuCard = new GradientDrawable();
        menuCard.setCornerRadius(dp(16));
        menu.setBackground(menuCard);
        menu.setClipToOutline(true);
        menu.setElevation(dp(8));
        menu.setVisibility(GONE);
        menu.setAccessibilityPaneTitle(getResources().getString(R.string.home_menu));
        for (int i = 0; i < MENU_ITEMS.length; i++) {
            TextView item = new TextView(context);
            item.setText(MENU_ITEMS[i]);
            item.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
            item.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
            item.setMinHeight(dp(48));
            item.setMinWidth(dp(200));
            item.setPaddingRelative(dp(24), 0, dp(24), 0);
            int index = i;
            item.setOnClickListener(view -> {
                nativeMenu(index);
                apply();
            });
            menu.addView(item, new LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT));
            menuItems[i] = item;
        }
        // Placed from the left whatever the layout direction; placeMenu mirrors it.
        addView(menu, new LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT, Gravity.TOP | Gravity.LEFT));
    }

    /** MeeGone came to the foreground; another activity may have moved the carousel. */
    void onResumed() {
        sendLayout();
        apply();
    }

    /** The Rust app changed what shows without a touch or a frame, such as after Home. */
    void refresh() {
        apply();
    }

    @Override
    protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
        super.onSizeChanged(width, height, oldWidth, oldHeight);
        sendLayout();
        apply();
    }

    @Override
    public void onRtlPropertiesChanged(int layoutDirection) {
        super.onRtlPropertiesChanged(layoutDirection);
        sendLayout();
        apply();
    }

    private void sendLayout() {
        if (getWidth() > 0) {
            nativeLayout(getWidth(), getResources().getDisplayMetrics().density, touchSlop,
                    getLayoutDirection() == LAYOUT_DIRECTION_RTL);
        }
    }

    /**
     * Keeps the pages' content, Settings, the indicator and the menu inside the safe area, as the
     * bars, the cut-out or the window change: on rotation, folding or a new navigation mode
     * (HOME-11, COMP-02, COMP-03).
     */
    @Override
    public WindowInsets onApplyWindowInsets(WindowInsets insets) {
        safe = safeArea(insets);
        for (int i = 0; i < VIEWS; i++) {
            if (i != APPLICATIONS) {
                pages[i].setPadding(safe.left, safe.top, safe.right, safe.bottom);
            }
        }
        // The grid scrolls under the bars, so it takes the safe area as its own padding.
        apps.setSafeArea(safe);
        settings.setPadding(safe.left, safe.top, safe.right, safe.bottom);
        LayoutParams params = (LayoutParams) indicator.getLayoutParams();
        if (params.topMargin != safe.top + indicatorMargin) {
            params.topMargin = safe.top + indicatorMargin;
            indicator.setLayoutParams(params);
        }
        if (menu.getVisibility() == VISIBLE) {
            placeMenu(frame[MENU_X], frame[MENU_Y]);
        }
        return super.onApplyWindowInsets(insets);
    }

    /** The safe area: how far content stays from each edge, in px (HOME-11). */
    @SuppressWarnings("deprecation") // The only way on Android 10.
    private static Insets safeArea(WindowInsets insets) {
        Insets bars;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            bars = insets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
        } else {
            bars = insets.getSystemWindowInsets();
            DisplayCutout cutout = insets.getDisplayCutout();
            if (cutout != null) {
                bars = Insets.max(bars, Insets.of(cutout.getSafeInsetLeft(),
                        cutout.getSafeInsetTop(), cutout.getSafeInsetRight(),
                        cutout.getSafeInsetBottom()));
            }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            return clearOfCorners(insets, bars);
        }
        return bars;
    }

    /**
     * Moves the sides of {@code bars} in where a rounded corner of the screen would cut the safe
     * area's corners: above gesture navigation, or in a window with no bar at an edge.
     */
    @TargetApi(Build.VERSION_CODES.S)
    private static Insets clearOfCorners(WindowInsets insets, Insets bars) {
        int left = Math.max(bars.left, Math.max(
                cornerInset(insets.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT), bars.top),
                cornerInset(insets.getRoundedCorner(RoundedCorner.POSITION_BOTTOM_LEFT),
                        bars.bottom)));
        int right = Math.max(bars.right, Math.max(
                cornerInset(insets.getRoundedCorner(RoundedCorner.POSITION_TOP_RIGHT), bars.top),
                cornerInset(insets.getRoundedCorner(RoundedCorner.POSITION_BOTTOM_RIGHT),
                        bars.bottom)));
        return Insets.of(left, bars.top, right, bars.bottom);
    }

    /**
     * How far in from the side {@code corner}'s curve reaches at {@code across} px from the top
     * or bottom edge: 0 past the curve, its full radius at the edge. It assumes the window
     * reaches the screen's corner; when it doesn't, the answer is larger than needed, never
     * smaller.
     */
    @TargetApi(Build.VERSION_CODES.S)
    private static int cornerInset(RoundedCorner corner, int across) {
        if (corner == null || across >= corner.getRadius()) {
            return 0;
        }
        double radius = corner.getRadius();
        double rise = radius - across;
        return (int) Math.ceil(radius - Math.sqrt(radius * radius - rise * rise));
    }

    // Every touch reaches the carousel once, here; the content below gets it too until the
    // carousel or a long-press takes the gesture. While the home menu is open, a touch outside
    // it closes it and goes nowhere.
    @Override
    public boolean dispatchTouchEvent(MotionEvent event) {
        if (event.getActionMasked() == MotionEvent.ACTION_DOWN) {
            childClaimed = false;
            swallowing = frame[MENU] != 0 && !isOver(menu, event.getX(), event.getY());
            if (swallowing) {
                nativeMenu(CLOSE_MENU);
                apply();
            }
        }
        if (swallowing) {
            return true;
        }
        if (!childClaimed) {
            claimed = forward(event);
            apply();
        }
        watchForLongPress(event);
        return super.dispatchTouchEvent(event);
    }

    @Override
    public boolean onInterceptTouchEvent(MotionEvent event) {
        return claimed;
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        // Keep receiving the gesture, so it can become a drag.
        return true;
    }

    @Override
    public void requestDisallowInterceptTouchEvent(boolean disallow) {
        super.requestDisallowInterceptTouchEvent(disallow);
        if (disallow && !claimed && !childClaimed) {
            childClaimed = true;
            removeCallbacks(longPress);
            nativePointer(MotionEvent.ACTION_CANCEL, 0f, 0f, System.nanoTime());
            apply();
        }
    }

    /** Passes the driving finger to the carousel. Returns whether it has taken the gesture. */
    private boolean forward(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                pointerId = event.getPointerId(0);
                return nativePointer(MotionEvent.ACTION_DOWN, event.getX(0), event.getY(0),
                        eventNanos(event));
            case MotionEvent.ACTION_MOVE: {
                int index = event.findPointerIndex(pointerId);
                if (index < 0) {
                    return claimed;
                }
                // Touches between frames arrive batched; each one counts for the fling.
                for (int h = 0; h < event.getHistorySize(); h++) {
                    nativePointer(MotionEvent.ACTION_MOVE, event.getHistoricalX(index, h),
                            event.getHistoricalY(index, h), historicalNanos(event, h));
                }
                return nativePointer(MotionEvent.ACTION_MOVE, event.getX(index),
                        event.getY(index), eventNanos(event));
            }
            case MotionEvent.ACTION_POINTER_UP:
            case MotionEvent.ACTION_UP: {
                int index = event.getActionIndex();
                if (event.getPointerId(index) != pointerId) {
                    return claimed;
                }
                pointerId = MotionEvent.INVALID_POINTER_ID;
                return nativePointer(MotionEvent.ACTION_UP, event.getX(index), event.getY(index),
                        eventNanos(event));
            }
            case MotionEvent.ACTION_CANCEL:
                pointerId = MotionEvent.INVALID_POINTER_ID;
                return nativePointer(MotionEvent.ACTION_CANCEL, 0f, 0f, eventNanos(event));
            default:
                return claimed;
        }
    }

    /**
     * Times a finger held still, as Android's own views do: the system's long-press time follows
     * the accessibility touch-and-hold delay. Lifting it, a second finger, or the carousel or a
     * child taking the gesture stops the wait. Whether it moved too far is the Rust app's call.
     */
    private void watchForLongPress(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                removeCallbacks(longPress);
                if (!claimed && frame[MENU] == 0 && frame[SETTINGS] == 0) {
                    downX = event.getX();
                    downY = event.getY();
                    postDelayed(longPress, ViewConfiguration.getLongPressTimeout());
                }
                break;
            case MotionEvent.ACTION_MOVE:
                if (claimed || childClaimed) {
                    removeCallbacks(longPress);
                }
                break;
            default:
                removeCallbacks(longPress);
        }
    }

    /**
     * The finger has been held long enough. On empty space, the Rust app decides: the home menu
     * or close mode (HOME-10). If it takes the long-press, the content gets no more of the
     * gesture, so lifting the finger taps nothing.
     */
    private void longPressed() {
        if (claimed || childClaimed || handlesTouch(this, pages[frame[CURRENT]], downX, downY)) {
            return;
        }
        claimed = nativeLongPress();
        apply();
    }

    /**
     * Whether {@code child}, or anything inside it, handles touches itself at ({@code x}, {@code
     * y}) in {@code parent}'s coordinates: an element that is clickable or long-clickable. Any
     * other point is empty space (HOME-10).
     */
    private boolean handlesTouch(ViewGroup parent, View child, float x, float y) {
        if (child.getVisibility() != VISIBLE) {
            return false;
        }
        point[0] = x + parent.getScrollX() - child.getLeft();
        point[1] = y + parent.getScrollY() - child.getTop();
        Matrix matrix = child.getMatrix();
        if (!matrix.isIdentity() && matrix.invert(inverse)) {
            inverse.mapPoints(point);
        }
        float localX = point[0];
        float localY = point[1];
        if (localX < 0 || localY < 0 || localX >= child.getWidth()
                || localY >= child.getHeight()) {
            return false;
        }
        // The page itself is the empty space.
        if (parent != this && (child.isClickable() || child.isLongClickable())) {
            return true;
        }
        if (child instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) child;
            for (int i = group.getChildCount() - 1; i >= 0; i--) {
                if (handlesTouch(group, group.getChildAt(i), localX, localY)) {
                    return true;
                }
            }
        }
        return false;
    }

    /** Whether ({@code x}, {@code y}) in this view's coordinates falls on {@code child}. */
    private static boolean isOver(View child, float x, float y) {
        float left = child.getLeft() + child.getTranslationX();
        float top = child.getTop() + child.getTranslationY();
        return x >= left && x < left + child.getWidth() && y >= top && y < top + child.getHeight();
    }

    @Override
    public void doFrame(long frameTimeNanos) {
        framePosted = false;
        nativeFrame(frameTimeNanos);
        apply();
    }

    @Override
    protected void onAttachedToWindow() {
        super.onAttachedToWindow();
        // Restarts the frame loop, and sets the system bars now that there is a window.
        background = 0;
        apply();
    }

    @Override
    protected void onDetachedFromWindow() {
        Choreographer.getInstance().removeFrameCallback(this);
        framePosted = false;
        removeCallbacks(longPress);
        super.onDetachedFromWindow();
    }

    /** Draws what the Rust app says, and asks for another frame while anything moves. */
    private void apply() {
        nativeRead(frame);
        int width = getWidth();
        // The content comes back from an app scaled down and faded out (ANIM-09); the
        // background stays opaque.
        float scale = frame[CONTENT_SCALE] / 10_000f;
        float alpha = frame[CONTENT_ALPHA] / 255f;
        for (int i = 0; i < VIEWS; i++) {
            int x = frame[PAGES + i];
            pages[i].setTranslationX(x);
            pages[i].setScaleX(scale);
            pages[i].setScaleY(scale);
            pages[i].setAlpha(alpha);
            // A view out of sight is hidden, from screen readers too.
            pages[i].setVisibility(width > 0 && Math.abs(x) < width ? VISIBLE : INVISIBLE);
        }
        indicator.setAlpha(frame[INDICATOR_ALPHA] / 255f);
        if (frame[BACKGROUND] != background) {
            background = frame[BACKGROUND];
            setBackgroundColor(background);
            matchSystemBars(background);
        }
        boolean coloursChanged = frame[FOREGROUND] != foreground || frame[DIM] != dim
                || frame[SURFACE] != surface;
        if (coloursChanged) {
            foreground = frame[FOREGROUND];
            dim = frame[DIM];
            surface = frame[SURFACE];
            for (TextView title : titles) {
                if (title != null) {
                    title.setTextColor(foreground);
                }
            }
            settings.setBackgroundColor(surface);
            settingsTitle.setTextColor(foreground);
            settingsGroup.setTextColor(dim);
            menuCard.setColor(surface);
            // A pressed item shows a light wash of the text colour.
            ColorStateList pressed = ColorStateList.valueOf(
                    (foreground & 0x00FFFFFF) | 0x1F000000);
            for (TextView item : menuItems) {
                item.setTextColor(foreground);
                item.setBackground(
                        new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
            }
        }
        if (coloursChanged || frame[INDICATOR_SELECTED] != selected) {
            selected = frame[INDICATOR_SELECTED];
            for (int i = 0; i < VIEWS; i++) {
                ((GradientDrawable) segments[i].getBackground())
                        .setColor(i == selected ? foreground : dim);
            }
        }
        int current = frame[CURRENT];
        apps.apply(frame[APPS_REVISION], frame[APPS_SCROLL], frame[APPS_COLUMNS],
                frame[APPS_ROWS], current == APPLICATIONS ? frame[PRESSED] : -1,
                frame[PRESS_SCALE], foreground, dim);
        if (current != announced) {
            announced = current;
            status.setText(getResources().getString(R.string.view_position,
                    getResources().getString(NAMES[current]), current + 1, VIEWS));
        }
        applySettings();
        applyMenu();
        if (frame[MENU_OFFERED] != menuOffered) {
            // Screen readers ask again which actions the home views offer.
            menuOffered = frame[MENU_OFFERED];
            sendAccessibilityEvent(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
        }
        if (frame[ANIMATING] != 0 && !framePosted && isAttachedToWindow()) {
            framePosted = true;
            Choreographer.getInstance().postFrameCallback(this);
        }
    }

    /** Shows Settings over the home views while it is open; until #34, only its name. */
    private void applySettings() {
        if (frame[SETTINGS] == settingsShown) {
            return;
        }
        settingsShown = frame[SETTINGS];
        settings.setVisibility(settingsShown != 0 ? VISIBLE : GONE);
        settingsGroup.setVisibility(
                settingsShown == SETTINGS_AT_LOCK_SCREEN_IMAGE ? VISIBLE : GONE);
        hidePagesFromScreenReaders();
    }

    /** Shows the home menu with the items it offers, where the Rust app says (HOME-10). */
    private void applyMenu() {
        int items = frame[MENU];
        if (items != 0) {
            for (int i = 0; i < menuItems.length; i++) {
                menuItems[i].setVisibility((items & (1 << i)) != 0 ? VISIBLE : GONE);
            }
            menu.setVisibility(VISIBLE);
            placeMenu(frame[MENU_X], frame[MENU_Y]);
        } else {
            menu.setVisibility(GONE);
        }
        if (items != menuShown) {
            boolean opened = menuShown == 0;
            menuShown = items;
            hidePagesFromScreenReaders();
            if (opened) {
                // A screen reader moves to the first item (A11Y-05).
                menu.post(() -> {
                    for (TextView item : menuItems) {
                        if (item.getVisibility() == VISIBLE) {
                            item.performAccessibilityAction(
                                    AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null);
                            return;
                        }
                    }
                });
            }
        }
    }

    /**
     * Opens the menu where the finger was held: from there towards the end of the line, and
     * above the finger if there is no room below. Without a finger, or after the layout
     * changed, it is centred. Either way it stays inside the safe area (HOME-11).
     */
    private void placeMenu(int anchorX, int anchorY) {
        int left = safe.left + menuMargin;
        int top = safe.top + menuMargin;
        int right = getWidth() - safe.right - menuMargin;
        int bottom = getHeight() - safe.bottom - menuMargin;
        menu.measure(
                MeasureSpec.makeMeasureSpec(Math.max(0, right - left), MeasureSpec.AT_MOST),
                MeasureSpec.makeMeasureSpec(Math.max(0, bottom - top), MeasureSpec.AT_MOST));
        int menuWidth = menu.getMeasuredWidth();
        int menuHeight = menu.getMeasuredHeight();
        float x;
        float y;
        if (anchorX < 0 || anchorY < 0) {
            x = (left + right - menuWidth) / 2f;
            y = (top + bottom - menuHeight) / 2f;
        } else {
            boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
            x = rtl ? anchorX - menuWidth : anchorX;
            y = anchorY + menuHeight <= bottom ? anchorY : anchorY - menuHeight;
        }
        menu.setTranslationX(within(x, left, right - menuWidth));
        menu.setTranslationY(within(y, top, bottom - menuHeight));
    }

    private static float within(float value, float low, float high) {
        return Math.max(low, Math.min(value, high));
    }

    /** While the menu or Settings covers the home views, screen readers skip them. */
    private void hidePagesFromScreenReaders() {
        int importance = menuShown != 0 || settingsShown != 0
                ? IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                : IMPORTANT_FOR_ACCESSIBILITY_AUTO;
        for (FrameLayout page : pages) {
            page.setImportantForAccessibility(importance);
        }
    }

    /** Dark system-bar icons on a light background, light ones on a dark background. */
    @SuppressWarnings("deprecation") // The flags are the only way on Android 10.
    private void matchSystemBars(int background) {
        boolean light = Color.luminance(background) > 0.5f;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = getWindowInsetsController();
            if (controller != null) {
                int bars = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                        | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                controller.setSystemBarsAppearance(light ? bars : 0, bars);
            }
        } else {
            int flags = SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
            int visibility = getSystemUiVisibility();
            setSystemUiVisibility(light ? visibility | flags : visibility & ~flags);
        }
    }

    // Screen readers switch views and open the home menu with actions instead of gestures
    // (A11Y-05).
    @Override
    public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        info.setScrollable(true);
        info.addAction(AccessibilityAction.ACTION_SCROLL_FORWARD);
        info.addAction(AccessibilityAction.ACTION_SCROLL_BACKWARD);
        info.addAction(AccessibilityAction.ACTION_PAGE_LEFT);
        info.addAction(AccessibilityAction.ACTION_PAGE_RIGHT);
        info.addAction(new AccessibilityAction(
                R.id.action_next_view, getResources().getString(R.string.action_next_view)));
        info.addAction(new AccessibilityAction(R.id.action_previous_view,
                getResources().getString(R.string.action_previous_view)));
        if (frame[MENU_OFFERED] != 0) {
            info.addAction(new AccessibilityAction(
                    R.id.action_home_menu, getResources().getString(R.string.action_home_menu)));
        }
    }

    @Override
    public boolean performAccessibilityAction(int action, Bundle arguments) {
        if (action == R.id.action_home_menu) {
            boolean opened = nativeOpenMenu();
            apply();
            return opened;
        }
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        Boolean next = null;
        if (action == AccessibilityNodeInfo.ACTION_SCROLL_FORWARD
                || action == R.id.action_next_view) {
            next = true;
        } else if (action == AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD
                || action == R.id.action_previous_view) {
            next = false;
        } else if (action == android.R.id.accessibilityActionPageLeft) {
            next = rtl;
        } else if (action == android.R.id.accessibilityActionPageRight) {
            next = !rtl;
        }
        if (next == null) {
            return super.performAccessibilityAction(action, arguments);
        }
        boolean moved = nativeShow(next);
        apply();
        return moved;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private static long eventNanos(MotionEvent event) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            return event.getEventTimeNanos();
        }
        return event.getEventTime() * 1_000_000L;
    }

    private static long historicalNanos(MotionEvent event, int position) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            return event.getHistoricalEventTimeNanos(position);
        }
        return event.getHistoricalEventTime(position) * 1_000_000L;
    }

    /** The home views were laid out: at start, or after a size or direction change. */
    private native void nativeLayout(int widthPx, float density, int touchSlopPx, boolean rtl);

    /**
     * The driving finger, with a {@code MotionEvent} action and a time in the {@code
     * System.nanoTime()} base. Returns whether the carousel or a long-press has taken the
     * gesture.
     */
    private native boolean nativePointer(int action, float x, float y, long timeNanos);

    /** The display is about to draw a frame at this {@code System.nanoTime()}. */
    private native void nativeFrame(long frameTimeNanos);

    /**
     * Shows the next or previous view without a gesture. Returns false if there is none: past
     * an end with wrap-around off.
     */
    private native boolean nativeShow(boolean next);

    /**
     * The driving finger has been held on empty space for the system's long-press time. Returns
     * whether the long-press was taken, so the content gets no more of the gesture.
     */
    private native boolean nativeLongPress();

    /** Opens the home menu without a gesture. Returns false if the current view has none. */
    private native boolean nativeOpenMenu();

    /** An item of the home menu was chosen, 0 to 2, or {@link #CLOSE_MENU}. */
    private native void nativeMenu(int item);

    /** Fills {@code frame} with what to draw now, in the slots above. */
    private native void nativeRead(int[] frame);
}
