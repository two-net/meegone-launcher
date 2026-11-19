package io.github.twonet.meegone;

import android.annotation.TargetApi;
import android.app.AlertDialog;
import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Insets;
import android.graphics.Matrix;
import android.graphics.Rect;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.os.Build;
import android.os.Bundle;
import android.text.InputFilter;
import android.text.TextUtils;
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
import android.view.WindowManager;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityManager;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;
import java.util.ArrayList;
import java.util.List;

/**
 * The three home views, Events, Applications and Recent, in a wrap-around carousel (HOME-02 to
 * HOME-04), drawn with Android's own views (ADR 0006). They come back from an app with a short
 * scale and fade (ANIM-09, ADR 0007). A long-press on empty space opens the home menu, or enters
 * close mode in Recent (HOME-10, ADR 0008). Two fingers on Recent's cards pinch them between
 * more and bigger ones (REC-05, ADR 0024). A long-press on an app's or a folder's icon opens
 * its menu, and dragging on picks the icon up (APPS-11, ADR 0013), which {@link DragShadow}
 * draws above the pages. A tap on a folder opens it in a {@link FolderPanel} inside the
 * Applications page, with its own {@link AppsGrid}; renaming it asks for the name in a dialog
 * (FOLD-03, FOLD-04, ADR 0015). Pulling down at the top of Applications reveals the search field
 * of a {@link SearchPanel}, whose results are a third {@link AppsGrid} (SRCH-01, ADR 0016).
 * Below the notifications, Events shows the image of the day in an {@link ImageCard}, whose tap
 * opens the image details screen over the home views (EVT-12, ADR 0022).
 *
 * <p>The Rust app owns the gesture, the motion, the view indicator and the menus. This view
 * passes it every touch, frame time and layout change, says when a finger has been held on
 * empty space or on an app's icon, then reads what to draw with {@link #nativeRead} and applies
 * it. It keeps nothing else. The frame slots below must stay in step with {@code frame_slots}
 * in {@code src/android.rs}. Applications is an {@link AppsGrid}, which draws the frame's
 * Applications slots (ADR 0010), as the open folder's grid and search's results draw their own;
 * all three share one {@link AppIcons}, with Events' list and Recent's cards. Events is its
 * header, list and image card (ADR 0019 to ADR 0022), and Recent its {@link RecentList}
 * (ADR 0023).
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
 * opens the home menu. A cell is the one such element whose long-press is timed here.
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
    // As apps come and go (ANIM-21), in ten-thousandths:
    private static final int APPEAR_SCALE = 24; // cells that just joined the grid
    private static final int APPEAR_ALPHA = 25;
    private static final int LEAVE_SCALE = 26; // cells that left it
    private static final int LEAVE_ALPHA = 27;
    private static final int SHIFT = 28; // how far the others are from their old places
    private static final int ACCENT = 29; // ARGB: the new-app dot (APPS-07)
    // An app's long-press menu (APPS-11):
    private static final int APP_MENU = 30; // the cell it is at, -1 to centre it, -2 if closed
    private static final int APP_MENU_REVISION = 31; // changes when its items change
    // The dragged icon (APPS-11, ADR 0015):
    private static final int LIFTED = 32; // its cell, a gap, or -1
    private static final int LIFT_X = 33; // its centre, in px from the top left
    private static final int LIFT_Y = 34;
    private static final int LIFT_SCALE = 35; // ten-thousandths of an icon
    // Edit mode (APPS-12, ANIM-12, ADR 0014).
    private static final int EDITING = 36; // 1 while it is on
    private static final int EDIT_SCALE = 37; // every cell's, in ten-thousandths
    private static final int EDIT_PROGRESS = 38; // 0 to 10 000: Done's opacity
    private static final int BADGE_FIRST_ROW = 39; // the grid's first visible row
    private static final int BADGES = 40; // the ✕ scale of each visible row from it
    private static final int BADGE_ROWS = 12;
    private static final int LIFT_SURFACE = 52; // the dragged icon's cell's surface
    private static final int LIFT_ALPHA = 53; // 0 to 255
    private static final int LIFT_SHOWN = 54; // 1 while it is drawn
    private static final int PRESSED_SURFACE = 55;
    // A dragged app held over an icon, and going into a folder (ANIM-14):
    private static final int HOVER = 56; // the grid cell, or -1
    private static final int HOVER_SCALE = 57; // ten-thousandths
    private static final int MERGE = 58; // the folder's grid cell, or -1
    private static final int MERGE_SLOT = 59; // the preview slot left empty, or -1
    private static final int MERGE_FORMED = 60; // 1 if the folder was just made
    private static final int MERGE_PROGRESS = 61; // 0 to 10 000
    // The open folder (FOLD-04, ANIM-15):
    private static final int FOLDER = 62; // its id, or -1
    private static final int FOLDER_OPEN = 63; // 1 while open, 0 while closing
    private static final int FOLDER_CELL = 64; // its grid cell, or -1
    private static final int FOLDER_PROGRESS = 65; // 0 to 10 000
    private static final int FOLDER_REVISION = 66; // changes when its cells change
    private static final int FOLDER_SCROLL = 67; // px from the top
    private static final int FOLDER_ROWS = 68;
    // Renaming a folder (FOLD-03):
    private static final int RENAMING = 69; // its id, or -1
    private static final int RENAMING_CELL = 70; // its grid cell, or -1
    private static final int APP_MENU_SURFACE = 71;
    private static final int SCRIM = 72; // the open folder's scrim's opacity, 0 to 255
    // Search (SRCH-01 to SRCH-05, ANIM-16, ADR 0016):
    private static final int SEARCH = 73; // the bits below
    private static final int SEARCH_REVEAL = 74; // how much of the field's band shows, in px
    private static final int RESULTS_REVISION = 75; // changes when the results change
    private static final int RESULTS_SCROLL = 76; // px from the top
    private static final int RESULTS_ROWS = 77;
    // Settings → Applications → Hidden apps (APPS-19, ADR 0017):
    private static final int HIDDEN_APPS_OFFERED = 78; // 1 while Applications works
    private static final int HIDDEN_APPS_REVISION = 79; // changes when the list changes
    // The Events header (EVT-01, EVT-02, ADR 0019):
    private static final int EVENTS_HEADER_SHOWN = 80; // 1 while Events works
    private static final int EVENTS_HEADER_REVISION = 81; // changes when its text changes
    // What shows below it (EVT-03, EVT-04, EVT-11, ANIM-20, ADR 0020):
    private static final int EVENTS_LIST = 82; // EventsList.NOTHING, EXPLAINER, EMPTY or LIST
    private static final int EVENTS_LIST_REVISION = 83; // changes when its rows change
    private static final int EVENTS_CHANGE = 84; // 0 to 10 000: rows coming and going
    private static final int DIVIDER = 85; // ARGB: hairlines
    // The Events list's Clear and a notification's menu (ADR 0021).
    private static final int EVENTS_CLEARABLE = 86; // 1 while Clear is offered
    private static final int EVENTS_MENU = 87; // its items as bits 1 << item; 0 if closed
    private static final int EVENTS_MENU_ROW = 88; // the row it opens at, or -1 to centre it
    // The image-of-the-day card, its details and its setting (EVT-12, ANIM-22, ADR 0022):
    private static final int IMAGE_CARD_SHOWN = 89; // 1 while the card shows
    private static final int IMAGE_CARD_REVISION = 90; // changes when what it shows changes
    private static final int IMAGE_CHANGE = 91; // 0 to 10 000: a new thumbnail fading in
    private static final int IMAGE_DETAILS = 92; // 1 while the details screen shows
    private static final int SHOW_IMAGE_CARD = 93; // 1 on, 0 off, -1 not offered
    // Recent (ADR 0023).
    private static final int RECENT = 94; // RecentList.NOTHING, EXPLAINER, EMPTY or CARDS
    private static final int RECENT_REVISION = 95; // changes when its cards change
    private static final int RECENT_COLUMNS = 96; // cards in each row
    // Recent's pinch and its cards' reflow (REC-05, ANIM-17, ADR 0024):
    private static final int RECENT_SCALE = 97; // ten-thousandths; 10 000 at rest
    private static final int RECENT_PIVOT_X = 98; // where the fingers met, in px
    private static final int RECENT_PIVOT_Y = 99;
    private static final int RECENT_REFLOW = 100; // changes when a reflow starts
    private static final int RECENT_REFLOW_PROGRESS = 101; // 0 to 10 000
    private static final int RECENT_ROWS = 102; // rows on one screen: 3, or 2 for bigger cards
    private static final int RECENT_DENSITY_OFFERED = 103; // 1 while screen readers can switch
    // Recent's close mode and its cards leaving (REC-07, REC-08, ANIM-12, ANIM-18, ADR 0025):
    private static final int RECENT_CLOSE = 104; // 1 while close mode is on
    private static final int RECENT_CLOSE_SCALE = 105; // every card's, in ten-thousandths
    private static final int RECENT_CLOSE_PROGRESS = 106; // 0 to 10 000: Clear all's opacity
    private static final int RECENT_EDIT_OFFERED = 107; // 1 while screen readers can enter it
    private static final int RECENT_REMOVALS = 108; // changes when apps were removed
    private static final int RECENT_BADGES = 109; // the ✕ scale of each row, RecentList.ROWS
    private static final int RECENT_LEAVE = 118; // how far each row's cards have gone
    // Settings → Recent → Time window (REC-11, ADR 0026):
    private static final int RECENT_WINDOW = 127; // 12, 24 or 72 hours; -1 not offered
    // Settings → Lock-screen image's daily checks (WALL-04, SRS 7.6, ADR 0029):
    private static final int USE_MOBILE_DATA = 128; // 1 on, 0 off, -1 not offered
    private static final int IMAGE_UNAVAILABLE = 129; // 1 while "Daily image unavailable" shows
    private static final int FRAME_SIZE = 130;
    private static final int SEARCH_OPEN = 1; // the field has the keyboard
    private static final int SEARCH_RESULTS = 2; // the results show instead of the grid
    private static final int SEARCH_NOTHING = 4; // nothing matches
    private static final int SEARCH_WEB = 8; // a web search is offered
    private static final int SEARCH_OFFERED = 32; // screen readers can open search
    /** An open folder's columns, as {@code FOLDER_COLUMNS} in the Rust app. */
    private static final int FOLDER_COLUMNS = 4;
    /** The longest name a folder takes, as {@code MAX_FOLDER_NAME} in the Rust app. */
    private static final int MAX_FOLDER_NAME = 64;

    private static final int VIEWS = 3;
    private static final int EVENTS = 0;
    private static final int APPLICATIONS = 1;
    private static final int RECENT_VIEW = 2;
    private static final int[] NAMES = {
        R.string.view_events, R.string.view_applications, R.string.view_recent
    };
    /** The home menu's items, in the order of {@code MenuItem::ALL}. */
    private static final int[] MENU_ITEMS = {
        R.string.menu_launcher_settings,
        R.string.menu_lock_screen_image,
        R.string.menu_edit_applications
    };
    // A pinch's actions in nativePinch, mirrored in src/android.rs.
    private static final int PINCH_START = 0;
    private static final int PINCH_MOVE = 1;
    private static final int PINCH_END = 2;
    private static final int PINCH_CANCEL = 3;
    /** Passed to {@link #nativeMenu} for a touch outside the menu. */
    private static final int CLOSE_MENU = -1;
    /** The app menu's slot while it is closed. */
    private static final int APP_MENU_CLOSED = -2;
    // The fields of one item in nativeAppMenu, mirrored in menu_item_fields in src/android.rs:
    // its kind, then a shortcut's label.
    private static final int APP_MENU_FIELDS = 2;
    private static final int APP_MENU_SHORTCUT = 3;
    /**
     * The text of each kind of item of a cell's menu: App info, Uninstall, Edit applications, a
     * shortcut's own, Rename folder and Hide.
     */
    private static final int[] APP_MENU_ITEMS = {
        R.string.menu_app_info, R.string.menu_uninstall, R.string.menu_edit_applications, 0,
        R.string.menu_rename_folder, R.string.menu_hide
    };
    /**
     * The text of each item of a notification's menu, by its code in notification_menu_bit in
     * src/android.rs: Dismiss and Notification settings (EVT-08).
     */
    private static final int[] EVENTS_MENU_ITEMS = {
        R.string.menu_dismiss, R.string.menu_notification_settings
    };
    private static final int SETTINGS_AT_LOCK_SCREEN_IMAGE = 2;

    private final int[] frame = new int[FRAME_SIZE];
    private final FrameLayout[] pages = new FrameLayout[VIEWS];
    /** The date and the next alarm at the top of Events (EVT-01, EVT-02). */
    private final EventsHeader eventsHeader;
    /** Below the header: notification access, or the notifications (EVT-03, EVT-04). */
    private final EventsList eventsList;
    /** Below the notifications: the image of the day (EVT-12). */
    private final ImageCard imageCard;
    /** Recent: usage access, or the recently used apps (REC-02, REC-03). */
    private final RecentList recentList;
    private final AppsGrid apps;
    /** The open folder's grid, in its panel (FOLD-04). */
    private final AppsGrid folderApps;
    private final FolderPanel folderPanel;
    /** Search's results (SRCH-02), in its panel with the field. */
    private final AppsGrid results;
    private final SearchPanel searchPanel;
    /** The dragged icon, above the pages (ADR 0015). */
    private final DragShadow shadow;
    /** Every grid's icons. */
    private final AppIcons icons;
    /** Ends edit mode (APPS-12): a flat pill at the bottom of Applications. */
    private final TextView done;
    private final GradientDrawable doneShape;
    private final int doneMargin;
    private final View[] segments = new View[VIEWS];
    /** The view indicator (HOME-09). Screen readers skip it; {@link #status} speaks instead. */
    private final LinearLayout indicator;
    /**
     * Tells screen readers which view shows, e.g. "Applications, 2 of 3" (A11Y-08). It is a
     * polite live region, so changes are announced. It must stay visible to them: a view with
     * no size or no opacity isn't.
     */
    private final TextView status;
    /**
     * Covers the home views while Settings is open. Until #34 draws it, it shows its name and
     * Applications' Hidden apps (APPS-19).
     */
    private final LinearLayout settings;
    private final TextView settingsTitle;
    private final TextView settingsGroup;
    private final HiddenAppsList hiddenApps;
    /** Settings → Recent → Time window (REC-11). */
    private final RecentSettings recentSettings;
    /** Settings → Lock-screen image → "Show card in Events" (EVT-12). */
    private final ImageSettings imageSettings;
    /**
     * Covers the home views while the image details screen is open (WALL-15). Until #30 draws
     * it, it shows its name.
     */
    private final LinearLayout imageDetails;
    private final TextView imageDetailsTitle;
    /**
     * The home menu (HOME-10), a card at the finger, or an app's menu (APPS-11), a card at its
     * icon: over the home views. Its items are made for the menu that opens.
     */
    private final LinearLayout menu;
    private final GradientDrawable menuCard;
    private final List<TextView> menuItems = new ArrayList<>();
    private final List<View> menuDividers = new ArrayList<>();
    /** Where a cell is, for opening its menu there. */
    private final Rect anchor = new Rect();
    /** Where the open folder's icon is, in the Applications page, which its panel grows from. */
    private final Rect folderIcon = new Rect();
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
    /** The two fingers of a pinch, while both are down (ANIM-17). */
    private int pinchFirst = MotionEvent.INVALID_POINTER_ID;
    private int pinchSecond = MotionEvent.INVALID_POINTER_ID;
    /** Where the driving finger went down, for finding what is under it after a long-press. */
    private float downX;
    private float downY;
    /** Where and when the current gesture began, and whether it is still a tap (REC-07). */
    private float tapX;
    private float tapY;
    private long tapTime;
    private boolean tapping;
    private boolean framePosted;
    private int announced = -1;
    /** Edit mode as last drawn, or -1 before the first frame. */
    private int editingShown = -1;
    /** Recent's close mode as last drawn, or -1 before the first frame. */
    private int closingShown = -1;
    /** Recent's removals as last confirmed, or -1 before the first frame (REC-08). */
    private int removalsShown = -1;
    /** Whether screen readers can enter close mode, as last told, or -1 (A11Y-05). */
    private int editRecentOffered = -1;
    /** The confirmation of the last removal from Recent, while it may still show (REC-08). */
    private Toast removedToast;
    /** Done's colours as drawn: the accent behind, and the background's for its text. */
    private int doneAccent;
    private int doneText;
    private int selected = -1;
    private int background;
    private int surface;
    private int foreground;
    private int dim;
    /** The home menu's items as shown, as in {@link #MENU}, or 0. */
    private int menuShown;
    /** The app menu's revision as shown, from 0, or -1 while it is closed. */
    private long appMenuShown = -1;
    /** A notification menu's items as shown, as in {@link #EVENTS_MENU}, or 0. */
    private int eventsMenuShown;
    /** The Events row a notification menu was made for, as in {@link #EVENTS_MENU_ROW}. */
    private int eventsMenuRowShown = -1;
    private int settingsShown;
    /** Hidden apps' revision as shown, as an unsigned int, or -1 before it is first read. */
    private long hiddenAppsShown = -1;
    /** The Events header's revision as shown, as an unsigned int, or -1 before it is first read. */
    private long eventsHeaderShown = -1;
    /** The Events list's revision as shown, as an unsigned int, or -1 before it is first read. */
    private long eventsListShown = -1;
    /** The image card's revision as shown, as an unsigned int, or -1 before it is first read. */
    private long imageCardShown = -1;
    /** Recent's revision as shown, as an unsigned int, or -1 before it is first read. */
    private long recentShown = -1;
    private int imageDetailsShown;
    private int menuOffered = -1;
    /** The density action screen readers were last offered: its rows, or 0 for none. */
    private int densityOffered = -1;
    /** Whether screen readers can open search, as last told, or -1 before the first frame. */
    private int searchOffered = -1;
    /** Whether search was open as last drawn: the field has the keyboard. */
    private boolean searchOpen;
    /** Whether the open folder covers the grid for screen readers, or -1 before the first frame. */
    private int folderCovering = -1;
    /** The folder the rename dialog is for, or -1, and the dialog while it shows (FOLD-03). */
    private int renameShown = -1;
    private AlertDialog renameDialog;
    /** How far content stays from each edge, in px (HOME-11). */
    private Insets safe = Insets.NONE;

    HomeViews(Context context) {
        super(context);
        touchSlop = ViewConfiguration.get(context).getScaledTouchSlop();
        indicatorMargin = dp(12);
        menuMargin = dp(16);
        // Carries the screen-reader actions that switch views and open the menu (A11Y-05).
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);

        icons = new AppIcons(context, AppCell.iconPx(context), this::refreshIcons);
        apps = new AppsGrid(context, AppsGrid.GRID, icons, this::apply, this::announce);
        apps.setScreen(this);
        folderApps = new AppsGrid(context, AppsGrid.FOLDER, icons, this::apply, this::announce);
        folderApps.setScreen(this);
        folderPanel = new FolderPanel(context, folderApps, apps, folder -> {
            nativeRenameRequested(folder);
            apply();
        }, folder -> {
            nativeCloseFolder();
            apply();
        });
        folderPanel.setScreen(this);
        results = new AppsGrid(context, AppsGrid.SEARCH, icons, this::apply, this::announce);
        results.setScreen(this);
        searchPanel = new SearchPanel(context, results, query -> {
            nativeSearchChanged(query);
            apply();
        }, this::submitSearch, () -> {
            String message = nativeSearchWeb();
            apply();
            toast(message);
        });
        searchPanel.setScreen(this);
        eventsHeader = new EventsHeader(context, this::openAlarm);
        eventsList = new EventsList(context, icons, new EventsList.Listener() {
            @Override
            public void allow() {
                allowNotifications();
            }

            @Override
            public void clear() {
                clearNotifications();
            }

            @Override
            public void tapped(int index, Rect onScreen) {
                eventsRowTapped(index, onScreen);
            }

            @Override
            public boolean menuRequested(int index) {
                return eventsMenuRequested(index);
            }
        });
        imageCard = new ImageCard(context, new ImageCard.Listener() {
            @Override
            public void tapped() {
                imageCardTapped();
            }

            @Override
            public void newImageShown() {
                nativeNewImageShown();
                apply();
            }
        });
        recentList = new RecentList(context, icons, new RecentList.Listener() {
            @Override
            public void allow() {
                allowUsage();
            }

            @Override
            public void tapped(int index, Rect onScreen) {
                recentCardTapped(index, onScreen);
            }

            @Override
            public void laidOut(int widthPx) {
                nativeRecentLayout(widthPx);
                apply();
            }

            @Override
            public void badgeTapped(int index) {
                recentBadgeTapped(index);
            }

            @Override
            public void clearAll() {
                clearRecent();
            }
        });
        doneMargin = dp(16);
        done = new TextView(context);
        done.setText(R.string.edit_done);
        done.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        done.setGravity(Gravity.CENTER);
        done.setMinHeight(dp(48));
        done.setMinWidth(dp(120));
        done.setPadding(dp(24), 0, dp(24), 0);
        doneShape = new GradientDrawable();
        doneShape.setCornerRadius(dp(24));
        done.setOnClickListener(view -> apps.done());
        done.setVisibility(GONE);
        for (int i = 0; i < VIEWS; i++) {
            FrameLayout page = new FrameLayout(context);
            if (i == APPLICATIONS) {
                page.addView(apps, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
                // Search's field above the grid, and its results instead of it (SRCH-01).
                page.addView(searchPanel, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
                // Over the grid's last row, which is always empty (HOME-14).
                page.addView(done, new LayoutParams(LayoutParams.WRAP_CONTENT,
                        LayoutParams.WRAP_CONTENT, Gravity.BOTTOM | Gravity.CENTER_HORIZONTAL));
                // An open folder covers both.
                page.addView(folderPanel, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
            } else if (i == EVENTS) {
                // The header, then the notifications, then the image of the day, scrolling
                // together (ADR 0020, ADR 0022).
                LinearLayout content = new LinearLayout(context);
                content.setOrientation(LinearLayout.VERTICAL);
                content.addView(eventsHeader, new LinearLayout.LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                content.addView(eventsList, new LinearLayout.LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                content.addView(imageCard, new LinearLayout.LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                ScrollView scroll = new ScrollView(context);
                scroll.setVerticalScrollBarEnabled(false);
                scroll.setOverScrollMode(OVER_SCROLL_IF_CONTENT_SCROLLS);
                scroll.addView(content, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                page.addView(scroll, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
            } else {
                // Recent's title, then its cards, scrolling together (ADR 0023).
                ScrollView scroll = new ScrollView(context);
                scroll.setVerticalScrollBarEnabled(false);
                scroll.setOverScrollMode(OVER_SCROLL_IF_CONTENT_SCROLLS);
                scroll.addView(recentList, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                page.addView(scroll, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
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

        shadow = new DragShadow(context);
        addView(shadow, new LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT, Gravity.TOP | Gravity.LEFT));

        // One transparent pixel: present for screen readers, invisible on screen.
        status = new TextView(context);
        status.setTextColor(Color.TRANSPARENT);
        status.setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);
        addView(status, new LayoutParams(1, 1, Gravity.TOP | Gravity.START));

        // Below Settings, which can open over it.
        imageDetails = new LinearLayout(context);
        imageDetails.setOrientation(LinearLayout.VERTICAL);
        imageDetails.setGravity(Gravity.TOP | Gravity.CENTER_HORIZONTAL);
        // Touches on it stay there.
        imageDetails.setClickable(true);
        imageDetails.setVisibility(GONE);
        imageDetails.setAccessibilityPaneTitle(
                getResources().getString(R.string.image_of_the_day));
        imageDetailsTitle = new TextView(context);
        imageDetailsTitle.setText(R.string.image_of_the_day);
        imageDetailsTitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, 28);
        imageDetails.addView(imageDetailsTitle, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        addView(imageDetails, new LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));

        settings = new LinearLayout(context);
        settings.setOrientation(LinearLayout.VERTICAL);
        settings.setGravity(Gravity.TOP | Gravity.CENTER_HORIZONTAL);
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
        // Settings → Applications → Hidden apps (APPS-19), which scrolls when it is long.
        hiddenApps = new HiddenAppsList(context, icons, (revision, index) -> {
            String message = nativeUnhide(revision, index);
            apply();
            toast(message);
        });
        hiddenApps.setVisibility(GONE);
        // Settings → Recent → Time window (REC-11), after Applications in SET-02's order.
        recentSettings = new RecentSettings(context, hours -> {
            String message = nativeRecentWindow(hours);
            apply();
            toast(message);
        });
        recentSettings.setVisibility(GONE);
        // Settings → Lock-screen image (EVT-12, ADR 0029), after Recent in SET-02's order.
        imageSettings = new ImageSettings(context, new ImageSettings.Listener() {
            @Override
            public void showCard(boolean on) {
                answered(nativeShowImageCard(on));
            }

            @Override
            public void useMobileData(boolean on) {
                answered(nativeUseMobileData(on));
            }

            @Override
            public void tryAgain() {
                answered(nativeImageTryAgain());
            }

            private void answered(String message) {
                apply();
                toast(message);
            }
        });
        imageSettings.setVisibility(GONE);
        LinearLayout groups = new LinearLayout(context);
        groups.setOrientation(LinearLayout.VERTICAL);
        groups.addView(hiddenApps, new LinearLayout.LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        groups.addView(recentSettings, new LinearLayout.LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        groups.addView(imageSettings, new LinearLayout.LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        ScrollView hiddenAppsScroll = new ScrollView(context);
        hiddenAppsScroll.addView(groups, new ScrollView.LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        LinearLayout.LayoutParams scrollParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f);
        scrollParams.topMargin = dp(24);
        scrollParams.setMarginStart(dp(16));
        scrollParams.setMarginEnd(dp(16));
        settings.addView(hiddenAppsScroll, scrollParams);
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
        // Touches on the card, between its items, stay there: none reaches an icon behind it.
        menu.setClickable(true);
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
        // Recent's cards take the window's proportions, and fit their rows on screen (REC-04,
        // REC-05).
        recentList.window(width, height);
        recentList.viewport(height - safe.top - safe.bottom);
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
        folderPanel.setSafeArea(safe);
        // Only search's results make room for the keyboard (ADR 0016).
        searchPanel.setInsets(safe, keyboardHeight(insets));
        LayoutParams doneParams = (LayoutParams) done.getLayoutParams();
        if (doneParams.bottomMargin != safe.bottom + doneMargin) {
            doneParams.bottomMargin = safe.bottom + doneMargin;
            done.setLayoutParams(doneParams);
        }
        settings.setPadding(safe.left, safe.top, safe.right, safe.bottom);
        imageDetails.setPadding(safe.left, safe.top, safe.right, safe.bottom);
        recentList.viewport(getHeight() - safe.top - safe.bottom);
        LayoutParams params = (LayoutParams) indicator.getLayoutParams();
        if (params.topMargin != safe.top + indicatorMargin) {
            params.topMargin = safe.top + indicatorMargin;
            indicator.setLayoutParams(params);
        }
        if (menu.getVisibility() == VISIBLE) {
            placeMenu();
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
            // With the keyboard up, the bottom inset holds it too: the safe area leaves it out.
            Insets system = insets.getSystemWindowInsets();
            bars = Insets.of(system.left, system.top, system.right,
                    system.bottom - keyboardHeight(insets));
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
     * How tall the keyboard is, in px from the window's bottom, or 0 while it is hidden. Android
     * 10 reports it only as the part of the bottom inset that the bars don't account for.
     */
    @SuppressWarnings("deprecation") // The only way on Android 10.
    private static int keyboardHeight(WindowInsets insets) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return insets.getInsets(WindowInsets.Type.ime()).bottom;
        }
        return Math.max(0, insets.getSystemWindowInsetBottom() - insets.getStableInsetBottom());
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

    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        super.onLayout(changed, left, top, right, bottom);
        // An app's menu stays at its icon, wherever the grid has just laid it out (COMP-03).
        if (menu.getVisibility() == VISIBLE) {
            placeMenu();
        }
    }

    // Every touch reaches the carousel once, here; the content below gets it too until the
    // carousel or a long-press takes the gesture. While a menu is open, a touch outside it
    // closes it and goes nowhere.
    @Override
    public boolean dispatchTouchEvent(MotionEvent event) {
        if (event.getActionMasked() == MotionEvent.ACTION_DOWN) {
            childClaimed = false;
            swallowing = menuOpen() && !isOver(menu, event.getX(), event.getY());
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
            if (pinch(event)) {
                claimed = true;
            }
            apply();
        }
        watchForLongPress(event);
        watchForTap(event);
        return super.dispatchTouchEvent(event);
    }

    /**
     * In Recent's close mode, a tap where nothing takes taps leaves it (REC-07): a finger lifted
     * within the touch slop, before the long-press time, with nothing else having taken the
     * gesture.
     */
    private void watchForTap(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                tapping = true;
                tapX = event.getX();
                tapY = event.getY();
                tapTime = event.getEventTime();
                break;
            case MotionEvent.ACTION_MOVE:
                tapping &= Math.hypot(event.getX() - tapX, event.getY() - tapY) <= touchSlop;
                break;
            case MotionEvent.ACTION_UP:
                boolean tap = tapping && !claimed && !childClaimed
                        && event.getEventTime() - tapTime < ViewConfiguration.getLongPressTimeout();
                tapping = false;
                if (tap && frame[CURRENT] == RECENT_VIEW && frame[RECENT_CLOSE] != 0
                        && touchTarget(this, pages[RECENT_VIEW], tapX, tapY) == null) {
                    nativeRecentEmptyTapped();
                    apply();
                }
                break;
            default:
                tapping = false;
        }
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
     * Passes the first two fingers to the Rust app, which may pinch Recent's cards with them
     * (REC-05, ANIM-17). Returns whether a pinch holds the gesture, so the content below gets no
     * more of it. A third finger changes nothing.
     */
    private boolean pinch(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_POINTER_DOWN:
                if (pinchFirst != MotionEvent.INVALID_POINTER_ID || event.getPointerCount() != 2) {
                    return false;
                }
                pinchFirst = event.getPointerId(0);
                pinchSecond = event.getPointerId(1);
                return sendPinch(PINCH_START, event);
            case MotionEvent.ACTION_MOVE:
                return pinchFirst != MotionEvent.INVALID_POINTER_ID
                        && sendPinch(PINCH_MOVE, event);
            case MotionEvent.ACTION_POINTER_UP: {
                int id = event.getPointerId(event.getActionIndex());
                if (pinchFirst == MotionEvent.INVALID_POINTER_ID
                        || (id != pinchFirst && id != pinchSecond)) {
                    return false;
                }
                boolean holding = sendPinch(PINCH_END, event);
                pinchFirst = MotionEvent.INVALID_POINTER_ID;
                pinchSecond = MotionEvent.INVALID_POINTER_ID;
                return holding;
            }
            case MotionEvent.ACTION_CANCEL:
                if (pinchFirst != MotionEvent.INVALID_POINTER_ID) {
                    nativePinch(PINCH_CANCEL, 0f, 0f, 0f, eventNanos(event));
                }
                pinchFirst = MotionEvent.INVALID_POINTER_ID;
                pinchSecond = MotionEvent.INVALID_POINTER_ID;
                return false;
            case MotionEvent.ACTION_DOWN:
            case MotionEvent.ACTION_UP:
                pinchFirst = MotionEvent.INVALID_POINTER_ID;
                pinchSecond = MotionEvent.INVALID_POINTER_ID;
                return false;
            default:
                return false;
        }
    }

    /** The pinch's two fingers: how far apart they are, and the point halfway between them. */
    private boolean sendPinch(int action, MotionEvent event) {
        int first = event.findPointerIndex(pinchFirst);
        int second = event.findPointerIndex(pinchSecond);
        if (first < 0 || second < 0) {
            return false;
        }
        float x1 = event.getX(first);
        float y1 = event.getY(first);
        float x2 = event.getX(second);
        float y2 = event.getY(second);
        float span = (float) Math.hypot(x2 - x1, y2 - y1);
        return nativePinch(action, span, (x1 + x2) / 2f, (y1 + y2) / 2f, eventNanos(event));
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
                if (!claimed && !menuOpen() && frame[SETTINGS] == 0) {
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
     * or close mode (HOME-10). Anywhere on Recent's cards, it decides whether close mode starts
     * (REC-07). On a cell, of the grid or of the open folder, it decides whether its menu opens
     * (APPS-11).
     * Any other element handles touches itself. If the Rust app takes the long-press, the
     * content gets no more of the gesture, so lifting the finger taps nothing.
     */
    private void longPressed() {
        if (claimed || childClaimed) {
            return;
        }
        View target = touchTarget(this, pages[frame[CURRENT]], downX, downY);
        int row = frame[CURRENT] == EVENTS ? eventsList.indexOfRow(target) : -1;
        boolean onRecentCard =
                frame[CURRENT] == RECENT_VIEW && recentList.indexOfCard(target) >= 0;
        if (target == null || onRecentCard) {
            claimed = nativeLongPress();
        } else if (target instanceof AppCell && frame[CURRENT] == APPLICATIONS) {
            AppCell cell = (AppCell) target;
            claimed = cell.grid().longPressed(cell);
        } else if (row >= 0) {
            // A notification's or a group's menu (EVT-08).
            claimed = nativeEventsRowLongPressed((int) eventsListShown, row);
        } else {
            return;
        }
        apply();
    }

    /**
     * The element inside {@code child}, or {@code child} itself, that handles touches at ({@code
     * x}, {@code y}) in {@code parent}'s coordinates: one that is clickable or long-clickable.
     * Null at any other point, which is empty space (HOME-10).
     */
    private View touchTarget(ViewGroup parent, View child, float x, float y) {
        if (child.getVisibility() != VISIBLE) {
            return null;
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
            return null;
        }
        // The page itself is the empty space.
        if (parent != this && (child.isClickable() || child.isLongClickable())) {
            return child;
        }
        if (child instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) child;
            for (int i = group.getChildCount() - 1; i >= 0; i--) {
                View target = touchTarget(group, group.getChildAt(i), localX, localY);
                if (target != null) {
                    return target;
                }
            }
        }
        return null;
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
        icons.watch();
        // Restarts the frame loop, and sets the system bars now that there is a window.
        background = 0;
        apply();
    }

    @Override
    protected void onDetachedFromWindow() {
        Choreographer.getInstance().removeFrameCallback(this);
        framePosted = false;
        removeCallbacks(longPress);
        icons.stopWatching();
        // The activity is going: its dialog goes with it, and the Rust app keeps the rename
        // open for the next one.
        closeRenameDialog();
        super.onDetachedFromWindow();
    }

    /** The icons arrived or changed: every grid, and the dragged icon, draw them again. */
    private void refreshIcons() {
        apps.refreshIcons();
        folderApps.refreshIcons();
        results.refreshIcons();
        hiddenApps.refreshIcons();
        eventsList.refreshIcons();
        recentList.refreshIcons();
        shadow.invalidate();
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
            settings.setBackgroundColor(surface);
            settingsTitle.setTextColor(foreground);
            imageDetails.setBackgroundColor(surface);
            imageDetailsTitle.setTextColor(foreground);
            settingsGroup.setTextColor(dim);
            menuCard.setColor(surface);
            colourMenu();
        }
        if (coloursChanged || frame[INDICATOR_SELECTED] != selected) {
            selected = frame[INDICATOR_SELECTED];
            for (int i = 0; i < VIEWS; i++) {
                ((GradientDrawable) segments[i].getBackground())
                        .setColor(i == selected ? foreground : dim);
            }
        }
        int current = frame[CURRENT];
        boolean applications = current == APPLICATIONS;
        int pressedSurface = frame[PRESSED_SURFACE];
        apps.apply(frame[APPS_REVISION], frame[APPS_SCROLL], frame[APPS_COLUMNS],
                frame[APPS_ROWS],
                applications && pressedSurface == AppsGrid.GRID ? frame[PRESSED] : -1,
                frame[PRESS_SCALE], foreground, dim, frame[ACCENT], surface);
        folderApps.apply(frame[FOLDER_REVISION], frame[FOLDER_SCROLL], FOLDER_COLUMNS,
                frame[FOLDER_ROWS],
                applications && pressedSurface == AppsGrid.FOLDER ? frame[PRESSED] : -1,
                frame[PRESS_SCALE], foreground, dim, frame[ACCENT], surface);
        apps.move(frame[APPEAR_SCALE], frame[APPEAR_ALPHA], frame[LEAVE_SCALE],
                frame[LEAVE_ALPHA], frame[SHIFT]);
        folderApps.move(frame[APPEAR_SCALE], frame[APPEAR_ALPHA], frame[LEAVE_SCALE],
                frame[LEAVE_ALPHA], frame[SHIFT]);
        applyDrag(applications);
        apps.edit(frame[EDITING] != 0, frame[EDIT_SCALE], frame[BADGE_FIRST_ROW], frame, BADGES,
                BADGE_ROWS);
        folderApps.edit(frame[EDITING] != 0, frame[EDIT_SCALE], 0, frame, BADGES, BADGE_ROWS);
        applyFolder();
        applySearch(applications);
        applyDone();
        if (current != announced) {
            announced = current;
            status.setText(viewPosition(current));
        }
        if (frame[EDITING] != editingShown) {
            // A11Y-08: "Edit mode". Leaving it sets the view's position again, so the next time
            // edit mode starts, the text changes and is read.
            if (editingShown >= 0) {
                status.setText(frame[EDITING] != 0
                        ? getResources().getString(R.string.edit_mode)
                        : viewPosition(current));
            }
            editingShown = frame[EDITING];
        }
        if (frame[RECENT_CLOSE] != closingShown) {
            // A11Y-08: "Editing Recent", read again each time close mode starts, as edit mode.
            if (closingShown >= 0) {
                status.setText(frame[RECENT_CLOSE] != 0
                        ? getResources().getString(R.string.recent_close_mode)
                        : viewPosition(current));
            }
            closingShown = frame[RECENT_CLOSE];
        }
        applyEventsHeader();
        applyEventsList();
        applyImageCard();
        applyRecent();
        applyImageDetails();
        applySettings();
        applyMenu();
        applyRename();
        int offered = (frame[SEARCH] & SEARCH_OFFERED) != 0 ? 1 : 0;
        int density = frame[RECENT_DENSITY_OFFERED] != 0 ? frame[RECENT_ROWS] : 0;
        if (frame[MENU_OFFERED] != menuOffered || offered != searchOffered
                || density != densityOffered || frame[RECENT_EDIT_OFFERED] != editRecentOffered) {
            // Screen readers ask again which actions the home views offer.
            menuOffered = frame[MENU_OFFERED];
            searchOffered = offered;
            densityOffered = density;
            editRecentOffered = frame[RECENT_EDIT_OFFERED];
            sendAccessibilityEvent(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
        }
        if (frame[ANIMATING] != 0 && !framePosted && isAttachedToWindow()) {
            framePosted = true;
            Choreographer.getInstance().postFrameCallback(this);
        }
    }

    /**
     * Draws the dragged icon above the pages, its cell as a gap, the icon it is held over and a
     * folder taking it in (APPS-11, ANIM-13, ANIM-14).
     */
    private void applyDrag(boolean applications) {
        boolean shown = applications && frame[LIFT_SHOWN] != 0;
        int surfaceOfLift = frame[LIFT_SURFACE];
        int lifted = shown ? frame[LIFTED] : -1;
        apps.hide(surfaceOfLift == AppsGrid.GRID ? lifted : -1);
        folderApps.hide(surfaceOfLift == AppsGrid.FOLDER ? lifted : -1);
        AppsGrid from = surfaceOfLift == AppsGrid.FOLDER ? folderApps : apps;
        shadow.show(shown, from.cellAt(lifted), frame[LIFT_X], frame[LIFT_Y], frame[LIFT_SCALE],
                frame[LIFT_ALPHA]);
        apps.hover(applications ? frame[HOVER] : -1, frame[HOVER_SCALE]);
        apps.merge(applications ? frame[MERGE] : -1, frame[MERGE_SLOT],
                frame[MERGE_FORMED] != 0, frame[MERGE_PROGRESS]);
    }

    /**
     * Draws the open or closing folder, growing from its icon (FOLD-04, ANIM-15). While it is
     * open, screen readers read only its panel.
     */
    private void applyFolder() {
        int folder = frame[FOLDER];
        AppCell cell = apps.cellAt(frame[FOLDER_CELL]);
        folderIcon.setEmpty();
        if (cell != null && cell.isLaidOut()) {
            int iconPx = AppCell.iconPx(getContext());
            int iconTop = AppCell.iconTopPx(getContext());
            folderIcon.set(0, 0, cell.getWidth(), cell.getHeight());
            pages[APPLICATIONS].offsetDescendantRectToMyCoords(cell, folderIcon);
            int left = folderIcon.centerX() - iconPx / 2;
            int top = folderIcon.top + iconTop;
            folderIcon.set(left, top, left + iconPx, top + iconPx);
        }
        folderPanel.apply(folder, frame[FOLDER_OPEN] != 0, frame[FOLDER_PROGRESS], folderIcon,
                cell == null ? null : cell.name(), frame[SCRIM], foreground, surface);
        int covering = folder >= 0 && frame[FOLDER_OPEN] != 0 ? 1 : 0;
        if (covering != folderCovering) {
            folderCovering = covering;
            apps.setImportantForAccessibility(covering != 0
                    ? IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                    : IMPORTANT_FOR_ACCESSIBILITY_AUTO);
        }
    }

    /**
     * Draws search (SRCH-01 to SRCH-05, ANIM-16): the field as far as its band is pulled down,
     * with the Applications grid moved down below it, or the results instead of the grid once
     * something is typed. When search opens, the field takes the keyboard, with the query the
     * Rust app kept if this view is new; when it closes, the keyboard goes.
     */
    private void applySearch(boolean applications) {
        int flags = frame[SEARCH];
        boolean open = (flags & SEARCH_OPEN) != 0;
        boolean resultsShown = (flags & SEARCH_RESULTS) != 0;
        int reveal = frame[SEARCH_REVEAL];
        // As wide as the grid, so in its columns.
        results.apply(frame[RESULTS_REVISION], frame[RESULTS_SCROLL], frame[APPS_COLUMNS],
                frame[RESULTS_ROWS],
                applications && frame[PRESSED_SURFACE] == AppsGrid.SEARCH ? frame[PRESSED] : -1,
                frame[PRESS_SCALE], foreground, dim, frame[ACCENT], surface);
        results.move(frame[APPEAR_SCALE], frame[APPEAR_ALPHA], frame[LEAVE_SCALE],
                frame[LEAVE_ALPHA], frame[SHIFT]);
        apps.setTranslationY(resultsShown ? 0 : reveal);
        // Hidden from touches and screen readers while the results take its place, but still
        // laid out, at the same scroll position (SRCH-04).
        apps.setVisibility(resultsShown ? INVISIBLE : VISIBLE);
        searchPanel.apply(open, reveal, resultsShown, (flags & SEARCH_NOTHING) != 0,
                (flags & SEARCH_WEB) != 0, foreground, dim, surface, frame[ACCENT]);
        if (open != searchOpen) {
            searchOpen = open;
            if (open) {
                searchPanel.focus(nativeSearchQuery());
            } else {
                searchPanel.dismiss();
            }
        }
    }

    /**
     * The keyboard's action key: the first result launches from its icon, or the web is
     * searched (SRCH-03, SRCH-05). If the system refuses, its message shows briefly.
     */
    private void submitSearch() {
        Rect icon = searchPanel.firstIcon();
        String message = nativeSearchSubmitted(icon.left, icon.top, icon.right, icon.bottom);
        apply();
        toast(message);
    }

    private void toast(String message) {
        if (message != null) {
            Toast.makeText(getContext(), message, Toast.LENGTH_SHORT).show();
        }
    }

    /**
     * Asks for a folder's new name while the Rust app says it is being renamed (FOLD-03): a
     * dialog with the name as it is, at most {@link #MAX_FOLDER_NAME} characters. OK names it,
     * and Cancel, Back or a touch outside leaves it. In its own window, the dialog has the
     * keyboard and Back to itself. It follows MeeGone's theme, not the system's.
     */
    private void applyRename() {
        int folder = frame[RENAMING];
        if (folder == renameShown) {
            return;
        }
        closeRenameDialog();
        renameShown = folder;
        if (folder < 0 || !isAttachedToWindow()) {
            return;
        }
        AppCell cell = apps.cellAt(frame[RENAMING_CELL]);
        String name = cell == null ? "" : cell.label();
        EditText field = new EditText(getContext());
        field.setSingleLine(true);
        field.setHint(R.string.folder_name);
        field.setFilters(new InputFilter[] {new InputFilter.LengthFilter(MAX_FOLDER_NAME)});
        field.setText(name);
        field.setSelectAllOnFocus(true);
        FrameLayout box = new FrameLayout(getContext());
        box.setPaddingRelative(dp(24), dp(8), dp(24), 0);
        box.addView(field);
        boolean light = Color.luminance(background) > 0.5f;
        AlertDialog dialog = new AlertDialog.Builder(getContext(), light
                        ? android.R.style.Theme_DeviceDefault_Light_Dialog_Alert
                        : android.R.style.Theme_DeviceDefault_Dialog_Alert)
                .setTitle(R.string.menu_rename_folder)
                .setView(box)
                .setPositiveButton(android.R.string.ok, (shown, which) -> {
                    renameDialog = null;
                    nativeFolderRenamed(folder, field.getText().toString());
                    apply();
                })
                .setNegativeButton(android.R.string.cancel, null)
                .create();
        dialog.setOnDismissListener(shown -> {
            // Dismissed by the user, not by OK or by the Rust app.
            if (renameDialog == dialog) {
                renameDialog = null;
                nativeRenameCancelled();
                apply();
            }
        });
        renameDialog = dialog;
        dialog.getWindow().setSoftInputMode(
                WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE);
        dialog.show();
        field.requestFocus();
    }

    /** Closes the rename dialog without telling the Rust app. */
    private void closeRenameDialog() {
        AlertDialog dialog = renameDialog;
        renameDialog = null;
        if (dialog != null) {
            dialog.dismiss();
        }
        renameShown = -1;
    }

    /** The view's name and place for screen readers, e.g. "Applications, 2 of 3" (A11Y-08). */
    private String viewPosition(int view) {
        return getResources().getString(R.string.view_position,
                getResources().getString(NAMES[view]), view + 1, VIEWS);
    }

    /** Tells screen readers of a change, through the polite live region (A11Y-08). */
    private void announce(String text) {
        status.setText(text);
    }

    /**
     * Done fades in and out with edit mode (ANIM-12). It is in the accent colour with the
     * background colour's text, and can be tapped, or found by screen readers, only while edit
     * mode is on.
     */
    private void applyDone() {
        float alpha = frame[EDIT_PROGRESS] / 10_000f;
        done.setVisibility(alpha > 0f ? VISIBLE : GONE);
        done.setAlpha(alpha);
        boolean on = frame[EDITING] != 0;
        done.setEnabled(on);
        // Under an open folder's scrim, screen readers skip it, as they skip the grid.
        boolean covered = frame[FOLDER] >= 0 && frame[FOLDER_OPEN] != 0;
        done.setImportantForAccessibility(on && !covered
                ? IMPORTANT_FOR_ACCESSIBILITY_YES
                : IMPORTANT_FOR_ACCESSIBILITY_NO);
        if (frame[ACCENT] != doneAccent || background != doneText) {
            doneAccent = frame[ACCENT];
            doneText = background;
            doneShape.setColor(doneAccent);
            ColorStateList pressed =
                    ColorStateList.valueOf((background & 0x00FFFFFF) | 0x3D000000);
            done.setBackground(new RippleDrawable(pressed, doneShape, null));
            done.setTextColor(background);
        }
    }

    /**
     * The Events header (EVT-01, EVT-02): drawn only while Events works (ARCH-09), and read again
     * whenever its revision changes, such as at midnight or when an alarm is set.
     */
    private void applyEventsHeader() {
        boolean shown = frame[EVENTS_HEADER_SHOWN] != 0;
        eventsHeader.setVisibility(shown ? VISIBLE : GONE);
        eventsHeader.colour(foreground, dim);
        long revision = frame[EVENTS_HEADER_REVISION] & 0xFFFF_FFFFL;
        if (shown && revision != eventsHeaderShown) {
            eventsHeaderShown = revision;
            eventsHeader.fill(nativeEventsHeader());
        }
    }

    /**
     * What shows below the Events header (EVT-03, EVT-04, EVT-11): nothing while Events doesn't
     * work (ARCH-09), the notification-access card, "No new events" or the app groups. The groups
     * are read again whenever their revision changes, and move as the frame says (ANIM-20).
     */
    private void applyEventsList() {
        int mode = frame[EVENTS_LIST];
        eventsList.show(mode);
        eventsList.colour(foreground, dim, surface, frame[ACCENT], frame[DIVIDER]);
        long revision = frame[EVENTS_LIST_REVISION] & 0xFFFF_FFFFL;
        if (mode != EventsList.NOTHING && revision != eventsListShown) {
            eventsListShown = revision;
            eventsList.fill(nativeEventsList());
        }
        eventsList.move(frame[EVENTS_CHANGE]);
        eventsList.clearable(mode == EventsList.LIST && frame[EVENTS_CLEARABLE] != 0);
    }

    /**
     * The image-of-the-day card below the notifications (EVT-12): shown only while the daily
     * image is on with an image and the card isn't hidden, and read again whenever its revision
     * changes. A new thumbnail fades in as the frame says (ANIM-22).
     */
    private void applyImageCard() {
        boolean shown = frame[IMAGE_CARD_SHOWN] != 0;
        imageCard.setVisibility(shown ? VISIBLE : GONE);
        imageCard.colour(foreground, dim, surface, frame[DIVIDER]);
        long revision = frame[IMAGE_CARD_REVISION] & 0xFFFF_FFFFL;
        if (shown && revision != imageCardShown) {
            imageCardShown = revision;
            imageCard.fill(nativeImageCard());
        }
        imageCard.fade(frame[IMAGE_CHANGE]);
    }

    /** The image card was tapped (EVT-12): the image details screen opens. */
    private void imageCardTapped() {
        // The revision is unsigned in Rust and travels as Java's int.
        String message = nativeImageCardTapped((int) imageCardShown);
        apply();
        toast(message);
    }

    /** Shows the image details screen over the home views while it is open (WALL-15). */
    private void applyImageDetails() {
        if (frame[IMAGE_DETAILS] != imageDetailsShown) {
            imageDetailsShown = frame[IMAGE_DETAILS];
            imageDetails.setVisibility(imageDetailsShown != 0 ? VISIBLE : GONE);
            hidePagesFromScreenReaders();
        }
    }

    /**
     * What Recent shows (REC-02, REC-03, REC-10): nothing below its title while Recent doesn't
     * work (ARCH-09), the usage-access card, "No recent apps" or the cards, in as many columns
     * as the frame says. The cards are read again whenever their revision changes.
     */
    private void applyRecent() {
        int mode = frame[RECENT];
        recentList.show(mode);
        recentList.colour(foreground, dim, surface, frame[ACCENT], frame[DIVIDER]);
        // Where each card is drawn, before the pinch lets go and the columns change (ANIM-17).
        recentList.reflow(frame[RECENT_REFLOW] & 0xFFFF_FFFFL);
        recentList.pinch(frame[RECENT_SCALE] / 10_000f, frame[RECENT_PIVOT_X],
                frame[RECENT_PIVOT_Y], this);
        recentList.columns(frame[RECENT_COLUMNS]);
        recentList.rows(frame[RECENT_ROWS]);
        long revision = frame[RECENT_REVISION] & 0xFFFF_FFFFL;
        if (mode != RecentList.NOTHING && revision != recentShown) {
            recentShown = revision;
            recentList.fill(nativeRecentCards());
        }
        recentList.progress(frame[RECENT_REFLOW_PROGRESS]);
        recentList.close(frame[RECENT_CLOSE] != 0, frame[RECENT_CLOSE_SCALE],
                frame[RECENT_CLOSE_PROGRESS], frame, RECENT_BADGES);
        recentList.leave(frame, RECENT_LEAVE);
        if (frame[RECENT_REMOVALS] != removalsShown) {
            if (removalsShown >= 0) {
                confirmRemoval(recentList.left());
            }
            removalsShown = frame[RECENT_REMOVALS];
        }
    }

    /**
     * Says apps were removed from Recent, never that they were closed (REC-01, REC-08): briefly,
     * "Removed from Recent", replacing a confirmation still showing so quick removals don't
     * queue. With a screen reader, the status says it instead, with the app's name when one went:
     * "Mail removed from Recent" (A11Y-08). It would read the brief message too.
     */
    private void confirmRemoval(String label) {
        AccessibilityManager access = getContext().getSystemService(AccessibilityManager.class);
        if (access != null && access.isTouchExplorationEnabled()) {
            status.setText(label != null
                    ? getResources().getString(R.string.recent_removed_app, label)
                    : getResources().getString(R.string.recent_removed));
            return;
        }
        if (removedToast != null) {
            removedToast.cancel();
        }
        removedToast = Toast.makeText(getContext(), R.string.recent_removed, Toast.LENGTH_SHORT);
        removedToast.show();
    }

    /**
     * In close mode, the ✕ of card {@code index} of Recent's cards last read was tapped, or a
     * screen reader chose "Remove ‹app› from Recent" (REC-08): the card goes from Recent, and
     * the app keeps running.
     */
    private void recentBadgeTapped(int index) {
        // The revision is unsigned in Rust and travels as Java's int.
        String message = nativeRecentBadgeTapped((int) recentShown, index);
        apply();
        toast(message);
    }

    /** In close mode, Clear all was tapped (REC-08): every card goes from Recent. */
    private void clearRecent() {
        String message = nativeRecentClearAll();
        apply();
        toast(message);
    }

    /**
     * A screen reader chose bigger cards, with 2 rows to a screen, or more cards, with 3
     * (A11Y-05). Returns whether Recent now shows them, which the status says (A11Y-08).
     */
    private boolean chooseDensity(int rows) {
        String message = nativeRecentDensity(rows);
        apply();
        toast(message);
        if (frame[RECENT_ROWS] != rows) {
            return false;
        }
        status.setText(rows == 2 ? R.string.action_bigger_cards : R.string.action_more_cards);
        return true;
    }

    /** Allow was tapped on Recent's card (REC-02): the system's usage-access screen opens. */
    private void allowUsage() {
        String message = nativeAllowUsage();
        apply();
        toast(message);
    }

    /**
     * Card {@code index} of Recent's cards last read was tapped, at {@code onScreen} (REC-06):
     * its app comes to the front, growing from the card (ANIM-08).
     */
    private void recentCardTapped(int index, Rect onScreen) {
        // The revision is unsigned in Rust and travels as Java's int.
        String message = nativeRecentCardTapped((int) recentShown, index, onScreen.left,
                onScreen.top, onScreen.right, onScreen.bottom);
        apply();
        toast(message);
    }

    /** Allow was tapped on Events' card (EVT-03): the system's notification-access screen opens. */
    private void allowNotifications() {
        String message = nativeAllowNotifications();
        apply();
        toast(message);
    }

    /** Clear was tapped (EVT-07): every notification that can be cleared goes. */
    private void clearNotifications() {
        String message = nativeClearNotifications();
        apply();
        toast(message);
    }

    /**
     * Row {@code index} of the Events rows last read was tapped, at {@code onScreen} (EVT-06):
     * its notification opens, growing from the row (ANIM-08), or its group expands or collapses.
     */
    private void eventsRowTapped(int index, Rect onScreen) {
        // The revision is unsigned in Rust and travels as Java's int.
        String message = nativeEventsRowTapped((int) eventsListShown, index, onScreen.left,
                onScreen.top, onScreen.right, onScreen.bottom);
        apply();
        toast(message);
    }

    /** A screen reader asks for an Events row's menu (A11Y-05). Returns whether it opened. */
    private boolean eventsMenuRequested(int index) {
        boolean opened = nativeEventsMenuRequested((int) eventsListShown, index);
        apply();
        return opened;
    }

    /** The alarm row was tapped (EVT-02): the app that set the alarm opens. */
    private void openAlarm() {
        String message = nativeOpenAlarm();
        apply();
        toast(message);
    }

    /**
     * Shows Settings over the home views while it is open; until #34, only its name, Hidden
     * apps, Recent's time window and "Show card in Events".
     */
    private void applySettings() {
        if (frame[SETTINGS] != settingsShown) {
            settingsShown = frame[SETTINGS];
            settings.setVisibility(settingsShown != 0 ? VISIBLE : GONE);
            settingsGroup.setVisibility(
                    settingsShown == SETTINGS_AT_LOCK_SCREEN_IMAGE ? VISIBLE : GONE);
            hidePagesFromScreenReaders();
        }
        applyHiddenApps();
        applyRecentSettings();
        applyImageSettings();
    }

    /**
     * Settings → Recent → Time window (REC-11): offered only while Recent works (ARCH-09).
     */
    private void applyRecentSettings() {
        int hours = frame[RECENT_WINDOW];
        recentSettings.setVisibility(hours >= 0 ? VISIBLE : GONE);
        recentSettings.colour(foreground, dim, frame[ACCENT]);
        recentSettings.show(hours);
    }

    /**
     * Settings → Lock-screen image: "Show card in Events" (EVT-12), "Use mobile data" (WALL-04)
     * and "Daily image unavailable" (SRS 7.6), offered only while the lock-screen image works
     * (ARCH-09).
     */
    private void applyImageSettings() {
        int showCard = frame[SHOW_IMAGE_CARD];
        imageSettings.setVisibility(showCard >= 0 ? VISIBLE : GONE);
        imageSettings.colour(foreground, dim, frame[ACCENT]);
        imageSettings.show(
                showCard > 0, frame[USE_MOBILE_DATA] > 0, frame[IMAGE_UNAVAILABLE] != 0);
    }

    /**
     * Settings → Applications → Hidden apps (APPS-19): offered only while Applications works
     * (ARCH-09), and read again whenever its revision changes while Settings shows, such as
     * after an Unhide.
     */
    private void applyHiddenApps() {
        boolean offered = frame[HIDDEN_APPS_OFFERED] != 0;
        hiddenApps.setVisibility(offered ? VISIBLE : GONE);
        hiddenApps.colour(foreground, dim, frame[ACCENT]);
        long revision = frame[HIDDEN_APPS_REVISION] & 0xFFFF_FFFFL;
        if (offered && settingsShown != 0 && revision != hiddenAppsShown) {
            hiddenAppsShown = revision;
            hiddenApps.fill(frame[HIDDEN_APPS_REVISION], nativeHiddenApps());
        }
    }

    /** Whether the home menu, an app's or a notification's menu is open, as of the last frame. */
    private boolean menuOpen() {
        return frame[MENU] != 0 || frame[APP_MENU] != APP_MENU_CLOSED || frame[EVENTS_MENU] != 0;
    }

    /**
     * Shows the home menu with the items it offers (HOME-10), an app's menu with its items
     * (APPS-11), or a notification's (EVT-08), where the Rust app says. Its items are made again
     * whenever they change.
     */
    private void applyMenu() {
        int homeItems = frame[MENU];
        // The revision is unsigned in Rust; any value of it means the menu is open.
        long appMenu = frame[APP_MENU] != APP_MENU_CLOSED
                ? frame[APP_MENU_REVISION] & 0xFFFF_FFFFL
                : -1;
        int eventsItems = frame[EVENTS_MENU];
        int eventsRow = eventsItems != 0 ? frame[EVENTS_MENU_ROW] : -1;
        boolean wasOpen = menuShown != 0 || appMenuShown >= 0 || eventsMenuShown != 0;
        if (homeItems != menuShown || appMenu != appMenuShown || eventsItems != eventsMenuShown
                || eventsRow != eventsMenuRowShown) {
            menuShown = homeItems;
            appMenuShown = appMenu;
            eventsMenuShown = eventsItems;
            eventsMenuRowShown = eventsRow;
            if (homeItems != 0) {
                fillHomeMenu(homeItems);
            } else if (appMenu >= 0) {
                fillAppMenu(frame[APP_MENU_REVISION]);
            } else if (eventsItems != 0) {
                fillEventsMenu(eventsItems, eventsRow);
            }
            hidePagesFromScreenReaders();
            if (!wasOpen && menuOpen() && !menuItems.isEmpty()) {
                // A screen reader moves to the first item (A11Y-05).
                TextView first = menuItems.get(0);
                menu.post(() -> first.performAccessibilityAction(
                        AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null));
            }
        }
        if (menuOpen()) {
            menu.setVisibility(VISIBLE);
            placeMenu();
        } else {
            menu.setVisibility(GONE);
        }
    }

    /** The home menu's items, as bits {@code 1 << item} (HOME-10). */
    private void fillHomeMenu(int items) {
        clearMenu();
        menu.setAccessibilityPaneTitle(getResources().getString(R.string.home_menu));
        for (int i = 0; i < MENU_ITEMS.length; i++) {
            if ((items & (1 << i)) != 0) {
                int index = i;
                addMenuItem(getResources().getText(MENU_ITEMS[i]), view -> {
                    nativeMenu(index);
                    apply();
                });
            }
        }
        colourMenu();
    }

    /**
     * A cell's menu at {@code revision} (APPS-11): App info, Uninstall and Edit applications as
     * an app's offers them, then its shortcuts below a divider, or a folder's Rename folder and
     * Edit applications. Its pane title names the app or the folder.
     */
    private void fillAppMenu(int revision) {
        clearMenu();
        String app = menuGrid().label(frame[APP_MENU]);
        menu.setAccessibilityPaneTitle(app != null
                ? getResources().getString(R.string.app_menu, app)
                : getResources().getString(R.string.action_app_menu));
        String[] fields = nativeAppMenu();
        int count = fields == null ? 0 : fields.length / APP_MENU_FIELDS;
        boolean divided = false;
        for (int i = 0; i < count; i++) {
            int kind = parseKind(fields[i * APP_MENU_FIELDS]);
            CharSequence text;
            if (kind == APP_MENU_SHORTCUT) {
                text = fields[i * APP_MENU_FIELDS + 1];
                if (!divided && i > 0) {
                    addMenuDivider();
                }
                divided = true;
            } else if (kind >= 0 && kind < APP_MENU_ITEMS.length && APP_MENU_ITEMS[kind] != 0) {
                text = getResources().getText(APP_MENU_ITEMS[kind]);
            } else {
                continue;
            }
            int index = i;
            addMenuItem(text == null ? "" : text, view -> chooseAppMenuItem(revision, index));
        }
        colourMenu();
    }

    /**
     * A notification's or a group's menu (EVT-08): Dismiss when something it is for can be
     * cleared, then Notification settings. Its pane title names the app on {@code row}.
     */
    private void fillEventsMenu(int items, int row) {
        clearMenu();
        String app = eventsList.label(row);
        menu.setAccessibilityPaneTitle(app != null
                ? getResources().getString(R.string.app_menu, app)
                : getResources().getString(R.string.notification_menu));
        for (int i = 0; i < EVENTS_MENU_ITEMS.length; i++) {
            if ((items & (1 << i)) != 0) {
                int item = i;
                addMenuItem(getResources().getText(EVENTS_MENU_ITEMS[i]), view -> {
                    String message = nativeEventsMenuChosen(item);
                    apply();
                    toast(message);
                });
            }
        }
        colourMenu();
    }

    private static int parseKind(String field) {
        try {
            return Integer.parseInt(field);
        } catch (NumberFormatException e) {
            return -1;
        }
    }

    /**
     * Item {@code index} of the app menu at {@code revision} was chosen. The system may refuse,
     * such as an App info screen that can't open: its message shows briefly.
     */
    private void chooseAppMenuItem(int revision, int index) {
        String message = nativeAppMenuChosen(revision, index);
        apply();
        if (message != null) {
            Toast.makeText(getContext(), message, Toast.LENGTH_SHORT).show();
        }
    }

    private void clearMenu() {
        menu.removeAllViews();
        menuItems.clear();
        menuDividers.clear();
    }

    /** One item: 16 sp on one line, at least 48 dp tall (A11Y-02) and 200 dp wide. */
    private void addMenuItem(CharSequence text, OnClickListener chosen) {
        TextView item = new TextView(getContext());
        item.setText(text);
        item.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        item.setSingleLine(true);
        item.setEllipsize(TextUtils.TruncateAt.END);
        item.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
        item.setMinHeight(dp(48));
        item.setMinWidth(dp(200));
        item.setPaddingRelative(dp(24), 0, dp(24), 0);
        item.setOnClickListener(chosen);
        menu.addView(item, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        menuItems.add(item);
    }

    /** A thin line between an app menu's own items and its shortcuts. */
    private void addMenuDivider() {
        View divider = new View(getContext());
        divider.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, Math.max(1, dp(1)));
        params.topMargin = dp(8);
        params.bottomMargin = dp(8);
        menu.addView(divider, params);
        menuDividers.add(divider);
    }

    /** The menu's items in the theme's text colour, with a light wash when pressed. */
    private void colourMenu() {
        ColorStateList pressed = ColorStateList.valueOf((foreground & 0x00FFFFFF) | 0x1F000000);
        for (TextView item : menuItems) {
            item.setTextColor(foreground);
            item.setBackground(new RippleDrawable(pressed, null, new ColorDrawable(Color.WHITE)));
        }
        for (View divider : menuDividers) {
            divider.setBackgroundColor((dim & 0x00FFFFFF) | 0x66000000);
        }
    }

    /**
     * Opens the home menu where the finger was held: from there towards the end of the line,
     * and above the finger if there is no room below. An app's menu opens at its icon, and a
     * notification's at its row: from the start, below it, or above it if there is no room
     * below. Without a finger, a cell or a row, or after the layout changed, the menu is
     * centred. Either way it stays inside the safe area (HOME-11).
     */
    private void placeMenu() {
        int left = safe.left + menuMargin;
        int top = safe.top + menuMargin;
        int right = getWidth() - safe.right - menuMargin;
        int bottom = getHeight() - safe.bottom - menuMargin;
        menu.measure(
                MeasureSpec.makeMeasureSpec(Math.max(0, right - left), MeasureSpec.AT_MOST),
                MeasureSpec.makeMeasureSpec(Math.max(0, bottom - top), MeasureSpec.AT_MOST));
        int menuWidth = menu.getMeasuredWidth();
        int menuHeight = menu.getMeasuredHeight();
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        float x;
        float y;
        boolean atItem = frame[MENU] == 0 && (frame[EVENTS_MENU] != 0
                ? eventsRowBounds(frame[EVENTS_MENU_ROW], anchor)
                : appCellBounds(frame[APP_MENU], anchor));
        if (atItem) {
            x = rtl ? anchor.right - menuWidth : anchor.left;
            y = anchor.bottom + menuHeight <= bottom ? anchor.bottom : anchor.top - menuHeight;
        } else if (frame[MENU] != 0 && frame[MENU_X] >= 0 && frame[MENU_Y] >= 0) {
            int anchorX = frame[MENU_X];
            int anchorY = frame[MENU_Y];
            x = rtl ? anchorX - menuWidth : anchorX;
            y = anchorY + menuHeight <= bottom ? anchorY : anchorY - menuHeight;
        } else {
            x = (left + right - menuWidth) / 2f;
            y = (top + bottom - menuHeight) / 2f;
        }
        menu.setTranslationX(within(x, left, right - menuWidth));
        menu.setTranslationY(within(y, top, bottom - menuHeight));
    }

    /** The grid of the cell the menu is open at: the Applications grid's or the folder's. */
    private AppsGrid menuGrid() {
        return frame[APP_MENU_SURFACE] == AppsGrid.FOLDER ? folderApps : apps;
    }

    /**
     * Fills {@code bounds} with where cell {@code index} of the menu's grid is, in this view's
     * coordinates. Returns false if it has no laid-out view.
     */
    private boolean appCellBounds(int index, Rect bounds) {
        View cell = menuGrid().cellAt(index);
        if (cell == null || !cell.isLaidOut()) {
            return false;
        }
        bounds.set(0, 0, cell.getWidth(), cell.getHeight());
        offsetDescendantRectToMyCoords(cell, bounds);
        return true;
    }

    /**
     * Fills {@code bounds} with where row {@code index} of the Events rows last read is, in this
     * view's coordinates. Returns false if it has no laid-out view.
     */
    private boolean eventsRowBounds(int index, Rect bounds) {
        View row = eventsList.rowAt(index);
        if (row == null || !row.isLaidOut()) {
            return false;
        }
        bounds.set(0, 0, row.getWidth(), row.getHeight());
        offsetDescendantRectToMyCoords(row, bounds);
        return true;
    }

    private static float within(float value, float low, float high) {
        return Math.max(low, Math.min(value, high));
    }

    /** While a menu or Settings covers the home views, screen readers skip them. */
    private void hidePagesFromScreenReaders() {
        int importance = menuShown != 0 || appMenuShown >= 0 || eventsMenuShown != 0
                || settingsShown != 0 || imageDetailsShown != 0
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

    // Screen readers switch views, open the home menu, open search, switch Recent between more
    // and bigger cards and enter its close mode with actions instead of gestures (A11Y-05).
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
        if ((frame[SEARCH] & SEARCH_OFFERED) != 0) {
            info.addAction(new AccessibilityAction(
                    R.id.action_search, getResources().getString(R.string.search_action)));
        }
        if (frame[RECENT_DENSITY_OFFERED] != 0) {
            // The other density from the one shown (REC-05).
            boolean bigger = frame[RECENT_ROWS] != 2;
            info.addAction(new AccessibilityAction(
                    bigger ? R.id.action_bigger_cards : R.id.action_more_cards,
                    getResources().getString(
                            bigger ? R.string.action_bigger_cards : R.string.action_more_cards)));
        }
        if (frame[RECENT_EDIT_OFFERED] != 0) {
            // A long-press on Recent's cards enters close mode (REC-07).
            info.addAction(new AccessibilityAction(R.id.action_edit_recent,
                    getResources().getString(R.string.action_edit_recent)));
        }
    }

    @Override
    public boolean performAccessibilityAction(int action, Bundle arguments) {
        if (action == R.id.action_home_menu) {
            boolean opened = nativeOpenMenu();
            apply();
            return opened;
        }
        if (action == R.id.action_search) {
            nativeOpenSearch();
            apply();
            return (frame[SEARCH] & SEARCH_OPEN) != 0;
        }
        if (action == R.id.action_bigger_cards) {
            return chooseDensity(2);
        }
        if (action == R.id.action_more_cards) {
            return chooseDensity(3);
        }
        if (action == R.id.action_edit_recent) {
            nativeRecentEditRequested();
            apply();
            return frame[RECENT_CLOSE] != 0;
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
     * System.nanoTime()} base. Returns whether the carousel, a long-press, the search field's
     * pull or a pinch has taken the gesture.
     */
    private native boolean nativePointer(int action, float x, float y, long timeNanos);

    /**
     * A pinch's two fingers, with one of the {@code PINCH_} actions: how far apart they are and
     * the point halfway between them, in px, at a time in the {@code System.nanoTime()} base
     * (ANIM-17). Returns whether a pinch holds the gesture.
     */
    private native boolean nativePinch(int action, float span, float x, float y,
            long timeNanos);

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

    /** A tap outside the open folder: it closes. */
    private native void nativeCloseFolder();

    /** The user tapped the open folder's name, or chose Rename folder: {@code folder}'s. */
    private native void nativeRenameRequested(int folder);

    /** The user named {@code folder} {@code name}. */
    private native void nativeFolderRenamed(int folder, String name);

    /** The user left the folder's name as it was. */
    private native void nativeRenameCancelled();

    /**
     * An item of the home menu was chosen, 0 to 2, or {@link #CLOSE_MENU} for a touch outside
     * the home menu or an app's menu.
     */
    private native void nativeMenu(int item);

    /** The app menu's items as of the last frame, each in {@link #APP_MENU_FIELDS} fields. */
    private native String[] nativeAppMenu();

    /**
     * Item {@code item} of the app menu at {@code revision} was chosen. Returns null, or a short
     * message to show.
     */
    private native String nativeAppMenuChosen(int revision, int item);

    /** Opens search without a pull, for a screen reader. */
    private native void nativeOpenSearch();

    /** The user typed: the search field now says {@code query}. */
    private native void nativeSearchChanged(String query);

    /**
     * The keyboard's action key, with the first result's icon at this rect on the screen, or an
     * empty one. Returns null, or a short message to show.
     */
    private native String nativeSearchSubmitted(int left, int top, int right, int bottom);

    /** "Search the web for '…'". Returns null, or a short message to show. */
    private native String nativeSearchWeb();

    /** What the search field says while search is open, or null. */
    private native String nativeSearchQuery();

    /**
     * Settings → Applications → Hidden apps as of the last frame, in {@link
     * HiddenAppsList#FIELDS} fields each (APPS-19).
     */
    private native String[] nativeHiddenApps();

    /**
     * Unhide was tapped on app {@code index} of the hidden apps at {@code revision} (APPS-19).
     * Returns null, or a short message when it couldn't be saved.
     */
    private native String nativeUnhide(int revision, int index);

    /**
     * The Events header as of the last frame, in {@link EventsHeader#FIELDS} fields (EVT-01,
     * EVT-02).
     */
    private native String[] nativeEventsHeader();

    /**
     * The alarm row of the Events header was tapped (EVT-02). Returns null, or a short message
     * when nothing could open.
     */
    private native String nativeOpenAlarm();

    /**
     * The rows below the Events header, {@link EventsList#FIELDS} fields each: each app's
     * group, and the expanded group's notifications (EVT-04, EVT-06, ADR 0020, ADR 0021). Empty
     * while Events doesn't work.
     */
    private native String[] nativeEventsList();

    /**
     * Allow was tapped on Events' notification-access card (EVT-03). Returns null, or a short
     * message to show when no system screen could open.
     */
    private native String nativeAllowNotifications();

    /**
     * Recent's cards, {@link RecentList#FIELDS} fields each, newest first (REC-03, REC-04,
     * ADR 0023). Empty while Recent doesn't work.
     */
    private native String[] nativeRecentCards();

    /**
     * Card {@code index} of Recent's cards at {@code revision} was tapped, at this rect on the
     * screen (REC-06). Returns null, or a short message to show when the app couldn't open.
     */
    private native String nativeRecentCardTapped(int revision, int index, int left, int top,
            int right, int bottom);

    /**
     * Allow was tapped on Recent's usage-access card (REC-02). Returns null, or a short message
     * to show when no system screen could open.
     */
    private native String nativeAllowUsage();

    /** Recent's cards were laid out this wide, inside their margins (REC-05). */
    private native void nativeRecentLayout(int widthPx);

    /**
     * A screen reader chose bigger cards, 2, or more cards, 3 (A11Y-05). Returns null, or a
     * short message to show when the choice couldn't be saved.
     */
    private native String nativeRecentDensity(int rows);

    /**
     * Settings → Recent → Time window was set to 12, 24 or 72 hours (REC-11). Returns null, or a
     * short message to show when the choice couldn't be saved.
     */
    private native String nativeRecentWindow(int hours);

    /**
     * In close mode, the ✕ of card {@code index} of Recent's cards at {@code revision} was
     * tapped (REC-08). Returns null, or a short message to show when it couldn't be saved.
     */
    private native String nativeRecentBadgeTapped(int revision, int index);

    /**
     * In close mode, Clear all was tapped (REC-08). Returns null, or a short message to show when
     * it couldn't be saved.
     */
    private native String nativeRecentClearAll();

    /** In close mode, a tap on Recent landed where nothing takes taps, which leaves it (REC-07). */
    private native void nativeRecentEmptyTapped();

    /** A screen reader chose "Edit Recent", which enters close mode (REC-07, A11Y-05). */
    private native void nativeRecentEditRequested();

    /**
     * Row {@code index} of the Events rows at {@code revision} was tapped, at this rect on the
     * screen (EVT-06). Returns null, or a short message to show when nothing could open.
     */
    private native String nativeEventsRowTapped(int revision, int index, int left, int top,
            int right, int bottom);

    /**
     * The finger has been held on row {@code index} of the Events rows at {@code revision} for
     * the system's long-press time (EVT-08). Returns whether the long-press was taken.
     */
    private native boolean nativeEventsRowLongPressed(int revision, int index);

    /**
     * Opens the menu of row {@code index} of the Events rows at {@code revision} for screen
     * readers (A11Y-05). Returns whether it opened.
     */
    private native boolean nativeEventsMenuRequested(int revision, int index);

    /**
     * Item {@code item} of the open notification menu was chosen: 0 Dismiss, 1 Notification
     * settings (EVT-08). Returns null, or a short message to show.
     */
    private native String nativeEventsMenuChosen(int item);

    /** Clear was tapped (EVT-07). Returns null, or a short message to show. */
    private native String nativeClearNotifications();

    /**
     * The image-of-the-day card as of the last frame, in {@link ImageCard#FIELDS} fields
     * (EVT-12). Empty while it doesn't show.
     */
    private native String[] nativeImageCard();

    /**
     * The image card at {@code revision} was tapped (EVT-12): the image details screen opens.
     * Returns null, or a short message to show.
     */
    private native String nativeImageCardTapped(int revision);

    /** A new thumbnail is about to fade in over the card's old one (ANIM-22). */
    private native void nativeNewImageShown();

    /**
     * "Show card in Events" was switched (EVT-12). Returns null, or a short message to show when
     * the choice couldn't be saved.
     */
    private native String nativeShowImageCard(boolean on);

    /**
     * "Use mobile data" was switched (WALL-04). Returns null, or a short message to show when
     * the choice couldn't be saved.
     */
    private native String nativeUseMobileData(boolean on);

    /**
     * Try again was tapped below "Daily image unavailable" (SRS 7.6). Returns null, or a short
     * message to show when it couldn't be done.
     */
    private native String nativeImageTryAgain();

    /** Fills {@code frame} with what to draw now, in the slots above. */
    private native void nativeRead(int[] frame);
}
