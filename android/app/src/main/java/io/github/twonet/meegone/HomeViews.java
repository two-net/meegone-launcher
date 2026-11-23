package io.github.twonet.meegone;

import android.annotation.TargetApi;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.ComponentCallbacks2;
import android.content.Context;
import android.content.res.Resources;
import android.graphics.Color;
import android.graphics.Insets;
import android.graphics.Matrix;
import android.graphics.Rect;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.os.Build;
import android.os.Bundle;
import android.os.Looper;
import android.os.MessageQueue;
import android.text.InputFilter;
import android.text.TextUtils;
import android.util.Log;
import android.util.TypedValue;
import android.view.Choreographer;
import android.view.DisplayCutout;
import android.view.FocusFinder;
import android.view.Gravity;
import android.view.KeyCharacterMap;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.RoundedCorner;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.ViewGroup;
import android.view.ViewTreeObserver;
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
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
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
 * Below the notifications, Events shows the image of the day in an {@link ImageCard}, whose tap,
 * like the home menu's Lock-screen image, opens the {@link ImageDetails} screen over the home
 * views (EVT-12, WALL-15, ADR 0022, ADR 0031). From there, or from Settings, the {@link
 * PastImages} screen opens over them, to pin one of the past week's images (WALL-17,
 * ADR 0032).
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
    // Settings → Lock-screen image's wallpaper (WALL-02, WALL-11 to WALL-14, ADR 0030):
    private static final int DAILY_IMAGE = 130; // ImageSettings.DAILY_IMAGE_*; -1 not offered
    private static final int UPDATE_NOW = 131; // ImageSettings.UPDATE_NOW_*; -1 not offered
    private static final int IMAGE_PAUSED = 132; // 1 while someone else changed the lock screen
    private static final int IMAGE_MAY_NOT_SHOW = 133; // 1 while the device didn't keep it
    private static final int IMAGE_OFF_ACTIONS = 134; // 1 while the reset is offered
    // The image details screen and the daily image's status line (WALL-15, WALL-16, ADR 0031):
    private static final int IMAGE_DETAILS_REVISION = 135; // changes when it changes; -1 closed
    private static final int IMAGE_STATUS_REVISION = 136; // changes when it changes; -1 none
    // The past week's images (WALL-17, ADR 0032):
    private static final int PAST_IMAGES = 137; // 1 while the screen shows
    private static final int PAST_IMAGES_REVISION = 138; // changes when it changes; -1 closed
    // The theme (THEME-01, THEME-06, ANIM-19, ADR 0033):
    private static final int THEME_CHANGE = 139; // how far the new theme has faded in, to 10 000
    private static final int THEME = 140; // a value of THEMES; -1 not offered
    private static final int RECENT_LEAVE_SCALE = 141; // a leaving card's end, ten-thousandths
    // Settings → Home (HOME-04, HOME-06), Access (SET-02), Region (WALL-05) and Reset launcher
    // (SET-06, ADR 0035):
    private static final int HOME_TARGET = 142; // 0 the last view, 1 Applications; -1 not offered
    private static final int WRAP_AROUND = 143; // 1 on, 0 off, -1 not offered
    private static final int DEFAULT_HOME = 144; // 1 MeeGone is, 0 it isn't; -1 not offered
    private static final int NOTIFICATION_ACCESS = 145; // 1 granted, 0 not
    private static final int USAGE_ACCESS = 146; // 1 granted, 0 not
    private static final int ACCESS_INSTRUCTIONS = 147; // an AccessSettings item, or -1
    private static final int REGION = 148; // 0 Automatic, else its place in nativeRegions; or -1
    private static final int RESET_OFFERED = 149; // 1 while Reset launcher is offered
    // First-run setup and the home-app banner (ONB-01, ONB-02, ONB-04, ADR 0036):
    private static final int SETUP_STEP = 150; // a SetupScreens step; -1 while none shows
    private static final int SETUP_PUT_OFF = 151; // SetupScreens.*_PUT_OFF bits
    private static final int SETUP_IMAGE_SWITCH = 152; // SetupScreens.SWITCH_*; -1 Continue alone
    private static final int SETUP_IMAGE_REVISION = 153; // changes when it changes; -1 none
    private static final int HOME_BANNER = 154; // 1 while the banner shows
    // Parts that stopped working (ARCH-09, ADR 0043):
    private static final int FAILED = 155; // bits at each FEATURE_*: the parts drawn no more
    private static final int STOPPED = 156; // STOPPED_* bits: where a stopped card shows
    private static final int FRAME_SIZE = 157;
    /** The parts of MeeGone that fail on their own, as the Rust app's {@code Feature} numbers them. */
    private static final int FEATURE_APPLICATIONS = 0;
    private static final int FEATURE_EVENTS = 1;
    private static final int FEATURE_RECENT = 2;
    private static final int FEATURE_IMAGE = 3;
    private static final int FEATURE_SETTINGS = 4;
    /** Where a card says a part stopped working, as the frame's slot 156 has them. */
    private static final int STOPPED_APPLICATIONS = 1;
    private static final int STOPPED_EVENTS = 2;
    private static final int STOPPED_RECENT = 4;
    private static final int STOPPED_IMAGE_IN_EVENTS = 8;
    private static final int STOPPED_IMAGE_IN_SETTINGS = 16;
    private static final int STOPPED_SETTINGS = 32;
    private static final String TAG = "MeeGone";
    /** The licences of the third-party code MeeGone includes, in About (LEGAL-06). */
    private static final int[] LICENCES = {
        R.raw.licence_mit, R.raw.licence_apache_2_0, R.raw.licence_unicode_3_0,
    };
    /** Settings → Appearance → Theme: System, Light and Dark, as the Rust app numbers them. */
    private static final int[] THEMES = {0, 1, 2};
    private static final int[] THEME_LABELS = {
        R.string.theme_system, R.string.theme_light, R.string.theme_dark,
    };
    /** Settings → Recent → Time window, in hours, as {@code RecentWindow::hours} gives them. */
    private static final int[] RECENT_HOURS = {12, 24, 72};
    private static final int SEARCH_OPEN = 1; // the field has the keyboard
    private static final int SEARCH_RESULTS = 2; // the results show instead of the grid
    private static final int SEARCH_NOTHING = 4; // nothing matches
    private static final int SEARCH_WEB = 8; // a web search is offered
    private static final int SEARCH_OFFERED = 32; // screen readers can open search
    /** An open folder's columns, as {@code FOLDER_COLUMNS} in the Rust app. */
    private static final int FOLDER_COLUMNS = 4;
    /** A zero-width space: added to an announcement that repeats the last, so it is read. */
    private static final String UNSPOKEN = "\u200B";
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
    // its kind, then a name, then whether that is a work app's.
    private static final int APP_MENU_FIELDS = 3;
    private static final int APP_MENU_SHORTCUT = 3;
    // Edit mode's moves (ADR 0039): to the start, earlier, later and to the end, then into a
    // folder with the next app, into the next folder, and out of the open folder.
    private static final int APP_MENU_MOVE_FIRST = 6;
    private static final int APP_MENU_MOVE_LAST = 9;
    private static final int APP_MENU_MAKE_FOLDER = 10;
    private static final int APP_MENU_MOVE_INTO = 11;
    private static final int APP_MENU_MOVE_OUT = 12;
    /**
     * The text of each kind of item of a cell's menu: App info, Uninstall, Edit applications, a
     * shortcut's own, Rename folder, Hide, then edit mode's moves, the same as screen readers'
     * actions. The two that name the next cell are made from its name.
     */
    private static final int[] APP_MENU_ITEMS = {
        R.string.menu_app_info, R.string.menu_uninstall, R.string.menu_edit_applications, 0,
        R.string.menu_rename_folder, R.string.menu_hide, R.string.action_move_first,
        R.string.action_move_earlier, R.string.action_move_later, R.string.action_move_last,
        R.string.action_make_folder, R.string.action_move_into, R.string.action_move_out
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
    /** Each view's own element, first in its page (A11Y-05, A11Y-06, ADR 0039). */
    private final ViewNode[] nodes = new ViewNode[VIEWS];
    /** The focus indicator over everything here (A11Y-06). */
    private final FocusRing ring;
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
    /**
     * Whether the system was told this home is fully drawn: Applications with every icon in
     * sight (PERF-02, ADR 0041).
     */
    private boolean fullyDrawn;
    /** Whether {@link #idle} waits for the main thread to have nothing to do. */
    private boolean idleAsked;
    /**
     * Once nothing moves and the main thread has nothing else to do, the Rust app reads again
     * the view that came into sight. Reading it while the row moves would cost a frame its
     * deadline (PERF-03, ADR 0041).
     */
    private final MessageQueue.IdleHandler idle = () -> {
        idleAsked = false;
        if (isAttachedToWindow() && nativeIdle()) {
            apply();
        }
        return false;
    };
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
     * Covers the home views while Settings is open: its name, then its groups in SET-02's order,
     * in one scrolling column (ADR 0035).
     */
    private final LinearLayout settings;
    private final TextView settingsTitle;
    private final ScrollView settingsScroll;
    /** Settings → Appearance → Theme (THEME-01). */
    private final ChoiceSettings themeSettings;
    /** Settings → Home: "Home button opens" and Wrap-around (HOME-04, HOME-06). */
    private final HomeSettings homeSettings;
    /** Settings → Applications → Hidden apps (APPS-19). */
    private final HiddenAppsList hiddenApps;
    /** Settings → Recent → Time window (REC-11). */
    private final ChoiceSettings recentSettings;
    /** Settings → Lock-screen image (EVT-12, WALL-02 to WALL-17). */
    private final ImageSettings imageSettings;
    /** Settings → Access: the default home app, notification and usage access (SET-02). */
    private final AccessSettings accessSettings;
    /** Settings → About, with Reset launcher (SET-05, SET-06). */
    private final AboutSettings aboutSettings;
    /** Covers the home views while the image details screen is open (WALL-15). */
    private final ImageDetails imageDetails;
    private final PastImages pastImages;
    /** Covers everything but the menu until first-run setup is finished (ONB-01). */
    private final SetupScreens setup;
    /** Offers to make MeeGone the default home app, at the bottom of the home views (ONB-04). */
    private final HomeBannerView banner;
    /** The Events and Recent pages' and Settings' scrolling, which the theme colours. */
    private final List<ScrollView> scrolls = new ArrayList<>();
    /** The whole screen cross-fading to a new theme (ANIM-19). */
    private final ThemeFade themeFade;
    /**
     * The home menu (HOME-10), a card at the finger, or an app's menu (APPS-11), a card at its
     * icon: over the home views. Its items are made for the menu that opens.
     */
    private final LinearLayout menu;
    /** The menu's items, which scroll inside the card when they don't all fit (A11Y-04). */
    private final LinearLayout menuList;
    private final ScrollView menuScroll;
    /** The cell or Events row whose menu is open, which takes screen readers' focus back. */
    private View menuOpener;
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
    /** The view is changing at a view element's or a key's request, which reads it itself. */
    private boolean quietSwitch;
    /** What Back does: the activity's, which tells the Rust app (HOME-08). */
    private Runnable back = () -> {};
    /** The topmost of what covers the home views, as in {@link #topLayer}, as last drawn. */
    private int layerShown = -1;
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
    private int donePress;
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
    /**
     * The regions Settings offers, as BCP 47 tags: the one Automatic stands for, then the feed's,
     * read each time Settings opens (WALL-05).
     */
    private String[] regions = new String[0];
    /** The region shown, as in {@link #REGION}, or -2 to show it again. */
    private int regionShown = -2;
    /** The Access item whose instructions show, as in {@link #ACCESS_INSTRUCTIONS}, and them. */
    private int instructionsShown = -1;
    private String instructions;
    /** The dialog over Settings: the regions, the licences or Reset launcher's question. */
    private AlertDialog settingsDialog;
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
    private int pastImagesShown;
    /** Setup's step as shown, or -1 for none; and its image screen's revision last read. */
    private int setupShown = -1;
    private int setupImageRead = -1;
    private int bannerShown = -1;
    /** The FEATURE_* bits of the parts this view no longer draws (ARCH-09, ADR 0043). */
    private int failed;
    /** The frame's STOPPED bits as last shown. */
    private int stoppedShown = -1;
    /**
     * Cards that say a part stopped working, with Restart MeeGone (ADR 0043): Applications' at
     * the bottom of the home views; Events' and Recent's on their pages; the lock-screen
     * image's in Events, where its card was, and in Settings; and Settings' at its top.
     */
    private final StoppedCard applicationsStopped;
    private final StoppedCard eventsStopped;
    private final StoppedCard recentStopped;
    private final StoppedCard imageInEventsStopped;
    private final StoppedCard imageInSettingsStopped;
    private final StoppedCard settingsStopped;
    private final StoppedCard[] stoppedCards;
    /** The revisions of the image details, the status line and the past week last read, or -1. */
    private int imageDetailsRead = -1;
    private int imageStatusRead = -1;
    private int pastImagesRead = -1;
    private int menuOffered = -1;
    /** The density action screen readers were last offered: its rows, or 0 for none. */
    private int densityOffered = -1;
    /** Whether screen readers can open search, as last told, or -1 before the first frame. */
    private int searchOffered = -1;
    /** Whether search was open as last drawn: the field has the keyboard. */
    private boolean searchOpen;
    /** Whether the open folder covers the grid for screen readers, or -1 before the first frame. */
    private int folderCovering = -1;
    /** The open folder's cell in the grid, which screen readers' focus goes back to. */
    private int folderCellShown = -1;
    /** The folder the rename dialog is for, or -1, and the dialog while it shows (FOLD-03). */
    private int renameShown = -1;
    private AlertDialog renameDialog;
    /** The rename dialog's field, and whether the dialog is in a light theme. */
    private EditText renameField;
    private boolean renameLight;
    /** How far content stays from each edge, in px (HOME-11). */
    private Insets safe = Insets.NONE;

    HomeViews(Context context) {
        super(context);
        touchSlop = ViewConfiguration.get(context).getScaledTouchSlop();
        indicatorMargin = dp(12);
        menuMargin = dp(16);
        // Carries the screen-reader actions that switch views and open the menu (A11Y-05).
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);

        themeFade = new ThemeFade(this);
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
            DebugHooks.typed(this);
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

            @Override
            public boolean editRequested() {
                // A long-press on a card, without a finger (A11Y-05).
                nativeRecentEditRequested();
                apply();
                return frame[RECENT_CLOSE] != 0;
            }
        });
        doneMargin = dp(16);
        done = new TextView(context);
        done.setText(R.string.edit_done);
        done.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        done.setGravity(Gravity.CENTER);
        done.setMinHeight(A11y.targetPx(this));
        done.setMinWidth(dp(120));
        done.setPadding(dp(24), 0, dp(24), 0);
        doneShape = new GradientDrawable();
        doneShape.setCornerRadius(dp(24));
        done.setOnClickListener(view -> apps.done());
        done.setVisibility(GONE);
        // Restart MeeGone, on any card that says a part stopped working (ADR 0043).
        Runnable restart = () -> Restart.now((Activity) getContext());
        applicationsStopped = new StoppedCard(context, R.string.stopped_applications, restart);
        eventsStopped = new StoppedCard(context, R.string.stopped_events, restart);
        recentStopped = new StoppedCard(context, R.string.stopped_recent, restart);
        imageInEventsStopped = new StoppedCard(context, R.string.stopped_lock_screen_image, restart);
        imageInSettingsStopped =
                new StoppedCard(context, R.string.stopped_lock_screen_image, restart);
        settingsStopped = new StoppedCard(context, R.string.stopped_settings, restart);
        stoppedCards = new StoppedCard[] {
            applicationsStopped, eventsStopped, recentStopped, imageInEventsStopped,
            imageInSettingsStopped, settingsStopped,
        };
        for (int i = 0; i < VIEWS; i++) {
            FrameLayout page = new FrameLayout(context);
            // The view's own element, under its content and first in its order (ADR 0039).
            nodes[i] = new ViewNode(context, i, viewNodes);
            nodes[i].setContentDescription(viewPosition(i));
            page.addView(nodes[i], new LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
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
                // In place of the header and the notifications while Events doesn't work.
                content.addView(eventsStopped, cardParams());
                content.addView(eventsHeader, new LinearLayout.LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                content.addView(eventsList, new LinearLayout.LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                content.addView(imageCard, new LinearLayout.LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                content.addView(imageInEventsStopped, cardParams());
                ScrollView scroll = new Scroll(context);
                scroll.setVerticalScrollBarEnabled(false);
                scroll.setOverScrollMode(OVER_SCROLL_IF_CONTENT_SCROLLS);
                scroll.addView(content, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                scrolls.add(scroll);
                page.addView(scroll, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
            } else {
                // Recent's title, then its cards, scrolling together (ADR 0023).
                ScrollView scroll = new Scroll(context);
                scroll.setVerticalScrollBarEnabled(false);
                scroll.setOverScrollMode(OVER_SCROLL_IF_CONTENT_SCROLLS);
                scroll.addView(recentList, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
                scrolls.add(scroll);
                page.addView(scroll, new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
                // In place of Recent's title and cards while Recent doesn't work.
                LayoutParams stoppedParams = new LayoutParams(
                        LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT, Gravity.TOP);
                stoppedParams.setMargins(dp(16), dp(24), dp(16), 0);
                page.addView(recentStopped, stoppedParams);
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

        // At the bottom of the home views, below every screen that covers them (ONB-04).
        banner = new HomeBannerView(context, new HomeBannerView.Listener() {
            @Override
            public void makeDefault() {
                String message = nativeHomeBannerMakeDefault();
                apply();
                toast(message);
            }

            @Override
            public void dismiss() {
                String message = nativeHomeBannerDismissed();
                apply();
                toast(message);
            }
        });
        banner.setVisibility(GONE);
        addView(banner, new LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT, Gravity.BOTTOM));
        // In the banner's place, while Applications doesn't work (ADR 0043).
        addView(applicationsStopped, new LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT, Gravity.BOTTOM));

        // Below Settings, which can open over it (ADR 0031).
        imageDetails = new ImageDetails(context, new ImageDetails.Listener() {
            @Override
            public void moreInfo() {
                String message = nativeImageMoreInfo();
                apply();
                toast(message);
            }

            @Override
            public void updateNow() {
                String message = nativeImageUpdateNow();
                apply();
                toast(message);
            }

            @Override
            public void unpin() {
                String message = nativeUnpinImage();
                apply();
                toast(message);
            }

            @Override
            public void pastImages() {
                nativeOpenPastImages();
                apply();
            }

            @Override
            public void settings() {
                nativeImageDetailsSettings();
                apply();
            }
        });
        imageDetails.setVisibility(GONE);
        addView(imageDetails, new LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));

        settings = new LinearLayout(context);
        settings.setOrientation(LinearLayout.VERTICAL);
        settings.setGravity(Gravity.TOP | Gravity.CENTER_HORIZONTAL);
        // Touches on Settings stay there.
        settings.setClickable(true);
        A11y.notAButton(settings);
        settings.setVisibility(GONE);
        settings.setAccessibilityPaneTitle(getResources().getString(R.string.settings_title));
        settingsTitle = new TextView(context);
        settingsTitle.setText(R.string.settings_title);
        settingsTitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, 28);
        settingsTitle.setAccessibilityHeading(true);
        settings.addView(settingsTitle, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        // Settings → Applications → Hidden apps (APPS-19).
        hiddenApps = new HiddenAppsList(context, icons, (revision, index) -> {
            String message = nativeUnhide(revision, index);
            apply();
            toast(message);
        });
        hiddenApps.setVisibility(GONE);
        // Settings → Appearance → Theme (THEME-01), first in SET-02's order.
        themeSettings = new ChoiceSettings(context, R.string.settings_group_appearance,
                R.string.theme, THEMES, THEME_LABELS, theme -> {
                    String message = nativeTheme(theme);
                    apply();
                    toast(message);
                });
        themeSettings.setVisibility(GONE);
        // Settings → Home (HOME-04, HOME-06), after Appearance in SET-02's order.
        homeSettings = new HomeSettings(context, new HomeSettings.Listener() {
            @Override
            public void homeTarget(int target) {
                String message = nativeHomeTarget(target);
                apply();
                toast(message);
            }

            @Override
            public void wrapAround(boolean on) {
                String message = nativeWrapAround(on);
                apply();
                toast(message);
            }
        });
        homeSettings.setVisibility(GONE);
        // Settings → Recent → Time window (REC-11), after Applications in SET-02's order.
        recentSettings = new ChoiceSettings(context, R.string.settings_group_recent,
                R.string.recent_time_window, RECENT_HOURS, recentWindows(getResources()),
                hours -> {
                    String message = nativeRecentWindow(hours);
                    apply();
                    toast(message);
                });
        recentSettings.setVisibility(GONE);
        // Settings → Lock-screen image (EVT-12, ADR 0029, ADR 0030), after Recent in SET-02's
        // order.
        imageSettings = new ImageSettings(context, new ImageSettings.Listener() {
            @Override
            public void dailyImage(boolean on) {
                answered(nativeDailyImage(on));
            }

            @Override
            public void region() {
                chooseRegion();
            }

            @Override
            public void updateNow() {
                answered(nativeImageUpdateNow());
            }

            @Override
            public void unpin() {
                answered(nativeUnpinImage());
            }

            @Override
            public void pastImages() {
                nativeOpenPastImages();
                apply();
            }

            @Override
            public void resetLockScreen() {
                String problem = nativeResetLockScreen();
                apply();
                say(problem != null
                        ? Problems.text(getResources(), problem)
                        : getContext().getString(R.string.lock_screen_reset));
            }

            @Override
            public void openWallpaperSettings() {
                answered(nativeOpenWallpaperSettings());
            }

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
        // Settings → Access (SET-02), after the lock-screen image in SET-02's order.
        accessSettings = new AccessSettings(context, item -> {
            String message;
            switch (item) {
                case AccessSettings.DEFAULT_HOME:
                    message = nativeDefaultHome();
                    break;
                case AccessSettings.NOTIFICATIONS:
                    message = nativeNotificationAccessSettings();
                    break;
                default:
                    message = nativeUsageAccessSettings();
                    break;
            }
            apply();
            toast(message);
        });
        accessSettings.setVisibility(GONE);
        // Settings → About, last (SET-05, SET-06).
        aboutSettings = new AboutSettings(context, new AboutSettings.Listener() {
            @Override
            public void licences() {
                showLicences();
            }

            @Override
            public void resetLauncher() {
                confirmReset();
            }
        });
        LinearLayout groups = new LinearLayout(context);
        groups.setOrientation(LinearLayout.VERTICAL);
        // The bottom padding keeps the last item clear of the screen's edge.
        groups.setPadding(0, 0, 0, dp(24));
        // Settings' own groups give way to its stopped card at the top, and the lock-screen
        // image's group to its own, while they don't work (ADR 0043).
        groups.addView(settingsStopped, cardParams());
        for (View group : new View[] {
            themeSettings, homeSettings, hiddenApps, recentSettings, imageSettings,
            accessSettings, aboutSettings,
        }) {
            groups.addView(group, new LinearLayout.LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
            if (group == imageSettings) {
                groups.addView(imageInSettingsStopped, cardParams());
            }
        }
        settingsScroll = new Scroll(context);
        settingsScroll.addView(groups, new ScrollView.LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        scrolls.add(settingsScroll);
        LinearLayout.LayoutParams scrollParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f);
        scrollParams.topMargin = dp(24);
        scrollParams.setMarginStart(dp(16));
        scrollParams.setMarginEnd(dp(16));
        settings.addView(settingsScroll, scrollParams);
        addView(settings, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));

        // Above Settings and the details screen, which open it (ADR 0032).
        pastImages = new PastImages(context, new PastImages.Listener() {
            @Override
            public byte[] preview(int revision, int index) {
                return nativePastImagePreview(revision, index);
            }

            @Override
            public void pin(int revision, int index) {
                String message = nativePinImage(revision, index);
                apply();
                toast(message);
            }

            @Override
            public void unpin() {
                String message = nativeUnpinImage();
                apply();
                toast(message);
            }

            @Override
            public void tryAgain() {
                String message = nativePastImagesTryAgain();
                apply();
                toast(message);
            }
        });
        pastImages.setVisibility(GONE);
        addView(pastImages, new LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));

        // Above every other screen, until setup is finished (ONB-01, ADR 0036).
        setup = new SetupScreens(context, new SetupScreens.Listener() {
            @Override
            public void next() {
                answered(nativeSetupContinue());
            }

            @Override
            public void allow(int item) {
                // The same shortcuts as Settings → Access (SET-02).
                switch (item) {
                    case AccessSettings.DEFAULT_HOME:
                        answered(nativeDefaultHome());
                        break;
                    case AccessSettings.NOTIFICATIONS:
                        answered(nativeNotificationAccessSettings());
                        break;
                    default:
                        answered(nativeUsageAccessSettings());
                        break;
                }
            }

            @Override
            public void notNow(int item) {
                answered(nativeSetupNotNow(item));
            }

            @Override
            public void imageSwitched(boolean on) {
                answered(nativeSetupImageSwitch(on));
            }

            @Override
            public void region() {
                chooseRegion();
            }

            @Override
            public byte[] preview(int revision) {
                return nativeSetupImagePreview(revision);
            }

            private void answered(String message) {
                apply();
                toast(message);
            }
        });
        setup.setVisibility(GONE);
        scrolls.add(setup);
        addView(setup, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));

        menu = new LinearLayout(context);
        menu.setOrientation(LinearLayout.VERTICAL);
        menuCard = new GradientDrawable();
        menuCard.setCornerRadius(dp(16));
        menu.setBackground(menuCard);
        menu.setClipToOutline(true);
        menu.setElevation(dp(8));
        menu.setVisibility(GONE);
        // Touches on the card, between its items, stay there: none reaches an icon behind it.
        menu.setClickable(true);
        A11y.notAButton(menu);
        // A long app menu in a short window, or at a large font size, scrolls (A11Y-04).
        menuList = new LinearLayout(context);
        menuList.setOrientation(LinearLayout.VERTICAL);
        menuList.setPadding(0, dp(8), 0, dp(8));
        menuScroll = new Scroll(context);
        menuScroll.setOverScrollMode(OVER_SCROLL_IF_CONTENT_SCROLLS);
        menuScroll.addView(menuList, new ScrollView.LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        scrolls.add(menuScroll);
        menu.addView(menuScroll, new LinearLayout.LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
        // Placed from the left whatever the layout direction; placeMenu mirrors it.
        addView(menu, new LayoutParams(
                LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT, Gravity.TOP | Gravity.LEFT));

        // Over everything, in the theme's accent once the first frame says it (A11Y-06).
        ring = FocusRing.attach(this);
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
        // REC-05). Not once Recent has stopped: none of its views' code runs (ADR 0043).
        if (drawing(FEATURE_RECENT)) {
            recentList.window(width, height);
            recentList.viewport(height - safe.top - safe.bottom);
        }
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
        pastImages.setPadding(safe.left, safe.top, safe.right, safe.bottom);
        setup.setPadding(safe.left, safe.top, safe.right, safe.bottom);
        LayoutParams bannerParams = (LayoutParams) banner.getLayoutParams();
        int bannerGap = dp(16);
        bannerParams.setMargins(safe.left + bannerGap, 0, safe.right + bannerGap,
                safe.bottom + bannerGap);
        banner.setLayoutParams(bannerParams);
        LayoutParams stoppedParams = (LayoutParams) applicationsStopped.getLayoutParams();
        stoppedParams.setMargins(safe.left + bannerGap, 0, safe.right + bannerGap,
                safe.bottom + bannerGap);
        applicationsStopped.setLayoutParams(stoppedParams);
        if (drawing(FEATURE_RECENT)) {
            recentList.viewport(getHeight() - safe.top - safe.bottom);
        }
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
        DebugHooks.touched(event);
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
                if (!claimed && !menuOpen() && frame[SETTINGS] == 0 && frame[SETUP_STEP] < 0
                        && !onBanner(event.getX(), event.getY())) {
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
        // A part that stopped runs none of its views' code (ADR 0043).
        int row = frame[CURRENT] == EVENTS && drawing(FEATURE_EVENTS)
                ? eventsList.indexOfRow(target)
                : -1;
        boolean onRecentCard = frame[CURRENT] == RECENT_VIEW && drawing(FEATURE_RECENT)
                && recentList.indexOfCard(target) >= 0;
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
        watchFullyDrawn();
    }

    /**
     * Tells the system the home is fully drawn once a frame shows Applications with every icon
     * in sight, or another view, which shows at once: Android logs the time from the launch,
     * and it is PERF-02's measure (ADR 0041). Checked after each frame until then.
     */
    private void watchFullyDrawn() {
        if (fullyDrawn) {
            return;
        }
        getViewTreeObserver().registerFrameCommitCallback(() -> {
            if (fullyDrawn || !isAttachedToWindow()) {
                return;
            }
            if (frame[CURRENT] != APPLICATIONS || apps.iconsShownInSight()) {
                fullyDrawn = true;
                ((Activity) getContext()).reportFullyDrawn();
            } else {
                watchFullyDrawn();
            }
        });
    }

    /**
     * The system asks for memory back (PERF-07, ADR 0041). While the home is hidden it keeps
     * everything, so Home brings it back fully drawn at once (PERF-01). When memory is low while
     * it shows (Android 10 to 13), or the process is in the background list, which Android
     * empties first, every icon out of sight goes: the cells draw their placeholders, the same
     * size, and ask again as they scroll into sight.
     */
    @SuppressWarnings("deprecation") // The RUNNING_ levels reach only Android 10 to 13.
    void trimMemory(int level) {
        boolean lowWhileShown = level >= ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW
                && level < ComponentCallbacks2.TRIM_MEMORY_UI_HIDDEN;
        if (!lowWhileShown && level < ComponentCallbacks2.TRIM_MEMORY_BACKGROUND) {
            return;
        }
        apps.trimIcons(true);
        folderApps.trimIcons(folderApps.isShown());
        results.trimIcons(results.isShown());
        icons.trim();
    }

    /** The Applications grid, for the debug build's probe. */
    AppsGrid appsGrid() {
        return apps;
    }

    /** The icons, for the debug build's probe. */
    AppIcons icons() {
        return icons;
    }

    /** Whether Applications is the view shown, for the debug build's probe. */
    boolean showsApplications() {
        return frame[CURRENT] == APPLICATIONS;
    }

    @Override
    protected void onDetachedFromWindow() {
        Choreographer.getInstance().removeFrameCallback(this);
        framePosted = false;
        Looper.myQueue().removeIdleHandler(idle);
        idleAsked = false;
        removeCallbacks(longPress);
        icons.stopWatching();
        // The activity is going: its dialog goes with it, and the Rust app keeps the rename
        // open for the next one.
        closeRenameDialog();
        themeFade.clear();
        super.onDetachedFromWindow();
    }

    /** The icons arrived or changed: every grid, and the dragged icon, draw them again. */
    private void refreshIcons() {
        apps.refreshIcons();
        folderApps.refreshIcons();
        results.refreshIcons();
        hiddenApps.refreshIcons();
        // A part that stopped part-way through drawing runs none of its views' code again: its
        // half-built rows could throw where nothing catches it (ADR 0043).
        if (drawing(FEATURE_EVENTS)) {
            eventsList.refreshIcons();
        }
        if (drawing(FEATURE_RECENT)) {
            recentList.refreshIcons();
        }
        shadow.invalidate();
    }

    /** Draws what the Rust app says, and asks for another frame while anything moves. */
    private void apply() {
        nativeRead(frame);
        // A new theme fades in over a picture of the screen in the old one, taken before
        // anything changes (THEME-06, ANIM-19). Not on the first frame after attaching, which
        // has nothing drawn to fade from.
        if (frame[THEME_CHANGE] < 10_000 && frame[BACKGROUND] != background && background != 0) {
            themeFade.capture();
        }
        themeFade.fade(frame[THEME_CHANGE]);
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
            menuCard.setColor(surface);
            colourMenu();
            for (ScrollView scroll : scrolls) {
                Themed.scrolling(scroll, dim);
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
            // A view's element or a key that switched reads the new view's element instead.
            if (!quietSwitch) {
                announce(viewPosition(current));
            }
        }
        if (frame[EDITING] != editingShown) {
            // A11Y-08: "Edit mode", and the view's position again when it ends.
            if (editingShown >= 0) {
                announce(frame[EDITING] != 0
                        ? getResources().getString(R.string.edit_mode)
                        : viewPosition(current));
            }
            editingShown = frame[EDITING];
        }
        if (frame[RECENT_CLOSE] != closingShown) {
            // A11Y-08: "Editing Recent", as edit mode.
            if (closingShown >= 0) {
                announce(frame[RECENT_CLOSE] != 0
                        ? getResources().getString(R.string.recent_close_mode)
                        : viewPosition(current));
            }
            closingShown = frame[RECENT_CLOSE];
        }
        applyFailed();
        // Each part's views are drawn on their own: a bug in one stops that part alone, which
        // then shows its stopped card, and launching apps keeps working (REL-01, ARCH-09,
        // ADR 0043). Applications' and the home's own are not: a bug there is a crash, so it
        // is counted (REL-07).
        if (drawing(FEATURE_EVENTS)) {
            try {
                DebugHooks.fault(FEATURE_EVENTS);
                applyEventsHeader();
                applyEventsList();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_EVENTS, e);
            }
        }
        if (drawing(FEATURE_IMAGE)) {
            try {
                DebugHooks.fault(FEATURE_IMAGE);
                applyImageCard();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_IMAGE, e);
            }
        }
        if (drawing(FEATURE_RECENT)) {
            try {
                DebugHooks.fault(FEATURE_RECENT);
                applyRecent();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_RECENT, e);
            }
        }
        if (drawing(FEATURE_IMAGE)) {
            try {
                applyImageDetails();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_IMAGE, e);
            }
        }
        applySettings();
        if (drawing(FEATURE_IMAGE)) {
            try {
                applyPastImages();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_IMAGE, e);
            }
        }
        if (drawing(FEATURE_SETTINGS)) {
            try {
                DebugHooks.fault(FEATURE_SETTINGS);
                applySetup();
                applyBanner();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_SETTINGS, e);
            }
        }
        applyStopped();
        applyMenu();
        applyRename();
        int offered = (frame[SEARCH] & SEARCH_OFFERED) != 0 ? 1 : 0;
        int density = frame[RECENT_DENSITY_OFFERED] != 0 ? frame[RECENT_ROWS] : 0;
        if (frame[MENU_OFFERED] != menuOffered || offered != searchOffered
                || density != densityOffered || frame[RECENT_EDIT_OFFERED] != editRecentOffered) {
            // Screen readers ask again which actions the view's element offers.
            menuOffered = frame[MENU_OFFERED];
            searchOffered = offered;
            densityOffered = density;
            editRecentOffered = frame[RECENT_EDIT_OFFERED];
            nodes[current].sendAccessibilityEvent(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
        }
        ring.colour(frame[ACCENT], background);
        followLayers();
        if (frame[ANIMATING] != 0 && !framePosted && isAttachedToWindow()) {
            framePosted = true;
            Choreographer.getInstance().postFrameCallback(this);
        } else if (frame[ANIMATING] == 0 && !idleAsked && isAttachedToWindow()) {
            idleAsked = true;
            Looper.myQueue().addIdleHandler(idle);
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
        if (covering != 0) {
            folderCellShown = frame[FOLDER_CELL];
        }
        if (covering != folderCovering) {
            boolean closed = folderCovering == 1;
            folderCovering = covering;
            apps.setImportantForAccessibility(covering != 0
                    ? IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                    : IMPORTANT_FOR_ACCESSIBILITY_AUTO);
            // A keyboard stays in the open folder too: the grid and Applications' own element
            // under it are no stops (A11Y-06).
            apps.setDescendantFocusability(covering != 0
                    ? FOCUS_BLOCK_DESCENDANTS
                    : FOCUS_AFTER_DESCENDANTS);
            nodes[APPLICATIONS].setFocusable(covering == 0);
            if (covering != 0 && !isInTouchMode()) {
                focusOnceLaidOut(folderPanel, folderPanel);
            }
            if (closed && frame[CURRENT] == APPLICATIONS) {
                focusBack(apps.cellAt(folderCellShown));
            }
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
                // A screen reader goes back to the grid, where search was pulled from (A11Y-08).
                if (applications) {
                    focusBack(apps.firstVisibleCell());
                }
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

    /**
     * Shows {@code problem}, a {@link Problems} code from the Rust app, briefly in the device's
     * language (L10N-01); nothing for null.
     */
    private void toast(String problem) {
        say(Problems.text(getResources(), problem));
    }

    /** Shows {@code text} briefly; nothing for null. */
    private void say(String text) {
        if (text != null) {
            Toast.makeText(getContext(), text, Toast.LENGTH_SHORT).show();
        }
    }

    /**
     * Asks for a folder's new name while the Rust app says it is being renamed (FOLD-03): a
     * dialog with the name as it is, at most {@link #MAX_FOLDER_NAME} characters. OK names it,
     * and Cancel, Back or a touch outside leaves it. In its own window, the dialog has the
     * keyboard and Back to itself.
     *
     * <p>It follows MeeGone's theme, not the system's (THEME-03). It is another window, so it
     * can't fade with the screen: when the theme changes while it is open, it opens again in
     * the new one at once, with the name as it was being typed (THEME-06, ADR 0033).
     */
    private void applyRename() {
        int folder = frame[RENAMING];
        boolean light = Color.luminance(background) > 0.5f;
        boolean retheme = renameDialog != null && folder == renameShown && light != renameLight;
        if (folder == renameShown && !retheme) {
            return;
        }
        CharSequence typed = null;
        int selectionStart = 0;
        int selectionEnd = 0;
        if (retheme) {
            typed = renameField.getText().toString();
            selectionStart = renameField.getSelectionStart();
            selectionEnd = renameField.getSelectionEnd();
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
        if (typed == null) {
            field.setText(name);
            field.setSelectAllOnFocus(true);
        } else {
            field.setText(typed);
            int length = field.length();
            field.setSelection(Math.min(Math.max(selectionStart, 0), length),
                    Math.min(Math.max(selectionEnd, 0), length));
        }
        FrameLayout box = new FrameLayout(getContext());
        box.setPaddingRelative(dp(24), dp(8), dp(24), 0);
        box.addView(field);
        AlertDialog dialog = Dialogs.builder(getContext(), light)
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
        renameField = field;
        renameLight = light;
        dialog.getWindow().setSoftInputMode(
                WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE);
        Dialogs.show(dialog);
        // Laid out as the device's light or dark dialogs, matched to MeeGone's theme, and drawn
        // in MeeGone's colours (THEME-03, A11Y-03).
        Dialogs.colour(dialog, surface, foreground, dim, frame[ACCENT], frame[SCRIM]);
        Dialogs.colourField(field, foreground, dim, frame[ACCENT]);
        field.requestFocus();
    }

    /** Closes the rename dialog without telling the Rust app. */
    private void closeRenameDialog() {
        AlertDialog dialog = renameDialog;
        renameDialog = null;
        renameField = null;
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

    /**
     * Tells screen readers of a change, through the polite live region (A11Y-08). Only a change
     * of its text is read, so the same words twice in a row, such as two "Removed from Recent",
     * differ by a character that isn't spoken.
     */
    private void announce(String text) {
        String shown = status.getText().toString();
        boolean again = shown.replace(UNSPOKEN, "").equals(text) && !shown.endsWith(UNSPOKEN);
        status.setText(again ? text + UNSPOKEN : text);
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
        // Nor is it a keyboard's stop there (A11Y-06).
        done.setFocusable(on && !covered);
        if (frame[ACCENT] != doneAccent || background != doneText || foreground != donePress) {
            doneAccent = frame[ACCENT];
            doneText = background;
            donePress = foreground;
            doneShape.setColor(doneAccent);
            // The theme's press, as everywhere else: it darkens the accent in Light and
            // lightens it in Dark, so Done's text keeps its contrast (A11Y-03).
            done.setBackground(new RippleDrawable(SettingsRows.pressed(foreground), doneShape,
                    null));
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

    /**
     * Shows the image details screen over the home views while it is open (WALL-15), read again
     * whenever its revision changes, with Update now as Settings offers it (WALL-11).
     */
    private void applyImageDetails() {
        if (frame[IMAGE_DETAILS] != imageDetailsShown) {
            imageDetailsShown = frame[IMAGE_DETAILS];
            imageDetails.setVisibility(imageDetailsShown != 0 ? VISIBLE : GONE);
            if (imageDetailsShown != 0) {
                imageDetails.opened();
            } else {
                // Its image is decoded again when it opens (PERF-07).
                imageDetails.closed();
                imageDetailsRead = -1;
            }
            coverLayers();
        }
        applyImageStatus();
        if (imageDetailsShown == 0) {
            return;
        }
        imageDetails.colour(foreground, dim, surface, frame[ACCENT]);
        int revision = frame[IMAGE_DETAILS_REVISION];
        if (revision >= 0 && revision != imageDetailsRead) {
            imageDetailsRead = revision;
            imageDetails.fill(nativeImageDetails());
        }
        imageDetails.showUpdateNow(frame[UPDATE_NOW]);
    }

    /**
     * Shows the past week's images over Settings or the details screen while they are open
     * (WALL-17), read again whenever their revision changes. Their previews go once it hides.
     */
    private void applyPastImages() {
        if (frame[PAST_IMAGES] != pastImagesShown) {
            pastImagesShown = frame[PAST_IMAGES];
            pastImages.setVisibility(pastImagesShown != 0 ? VISIBLE : GONE);
            if (pastImagesShown != 0) {
                pastImages.opened();
            } else {
                pastImages.closed();
                pastImagesRead = -1;
            }
            coverLayers();
        }
        if (pastImagesShown == 0) {
            return;
        }
        pastImages.colour(foreground, dim, surface, frame[ACCENT]);
        int revision = frame[PAST_IMAGES_REVISION];
        if (revision >= 0 && revision != pastImagesRead) {
            pastImagesRead = revision;
            pastImages.fill(revision, nativePastImages());
        }
    }

    /**
     * Shows first-run setup over everything but the menu while the Rust app says so, one screen
     * at a time (ONB-01, ADR 0036). Each screen's content is read as the frame's slots 150 to
     * 153 say; the accesses' status and instructions are Settings → Access's own (slots 145 to
     * 147).
     */
    private void applySetup() {
        int step = frame[SETUP_STEP];
        if (setup.show(step)) {
            // A dialog, such as Region's, belonged to the screen before.
            closeSettingsDialog();
            setupImageRead = -1;
            if (step == SetupScreens.LOCK_SCREEN_IMAGE) {
                // The device's language, and so Automatic's region, may have changed.
                regions = nativeRegions();
            }
        }
        if (step != setupShown) {
            setupShown = step;
            coverLayers();
        }
        if (step < 0) {
            return;
        }
        setup.colour(background, foreground, dim, frame[ACCENT]);
        switch (step) {
            case SetupScreens.DEFAULT_HOME:
                setup.showDefaultHome(frame[ACCESS_INSTRUCTIONS] == AccessSettings.DEFAULT_HOME
                        ? instructions
                        : null);
                break;
            case SetupScreens.LOCK_SCREEN_IMAGE:
                applySetupImage();
                break;
            case SetupScreens.ACCESS:
                setup.showAccess(frame[NOTIFICATION_ACCESS] > 0, frame[USAGE_ACCESS] > 0,
                        frame[SETUP_PUT_OFF], frame[ACCESS_INSTRUCTIONS], instructions);
                break;
            default:
                break;
        }
    }

    /**
     * Setup's lock-screen image step: the lock-screen image's own part of setup. While it doesn't
     * work, the step offers Continue alone, as the frame says (ARCH-09, ADR 0043).
     */
    private void applySetupImage() {
        if (!drawing(FEATURE_IMAGE)) {
            setup.showImage(frame[SETUP_IMAGE_SWITCH], null, "");
            return;
        }
        try {
            int switchState = frame[SETUP_IMAGE_SWITCH];
            String reason = switchState == SetupScreens.SWITCH_UNAVAILABLE
                    ? Problems.text(getResources(), nativeImageUnavailableReason())
                    : null;
            setup.showImage(switchState, reason, regionName(frame[REGION]));
            int revision = frame[SETUP_IMAGE_REVISION];
            if (revision >= 0 && revision != setupImageRead) {
                setupImageRead = revision;
                setup.fillImage(revision, nativeSetupImage());
            }
        } catch (RuntimeException | OutOfMemoryError e) {
            stop(FEATURE_IMAGE, e);
        }
    }

    /** Whether this view still draws {@code feature}'s views (ARCH-09). */
    private boolean drawing(int feature) {
        return (failed & 1 << feature) == 0;
    }

    /**
     * Takes on the parts the Rust app says stopped working, as their code panicked or this view
     * reported them: each is logged once and its views are hidden (ARCH-09, ADR 0043).
     */
    private void applyFailed() {
        int stopped = frame[FAILED] & ~failed;
        for (int feature = FEATURE_APPLICATIONS; feature <= FEATURE_SETTINGS; feature++) {
            if ((stopped & 1 << feature) != 0) {
                Log.w(TAG, "Feature " + feature + " stopped working");
                failed |= 1 << feature;
                hide(feature);
            }
        }
    }

    /**
     * {@code feature}'s views threw {@code e}: they are drawn no more, and the Rust app stops the
     * feature too, which closes what it covered and shows its stopped card. Only the
     * exception's kind is logged: its message may quote what it was showing (PRIV-03,
     * ADR 0042).
     */
    private void stop(int feature, Throwable e) {
        Log.w(TAG, "Feature " + feature + " stopped working: " + e.getClass().getSimpleName());
        failed |= 1 << feature;
        hide(feature);
        try {
            nativeFeatureFailed(feature);
        } catch (RuntimeException alreadyStopped) {
            // This view draws the part no more either way; the next frame says what Rust did.
        }
        post(this::apply);
    }

    /**
     * Hides {@code feature}'s own views, without running any of their code: only their
     * visibility changes. Applications' grid stays, so a tap on it still opens an app.
     */
    private void hide(int feature) {
        switch (feature) {
            case FEATURE_EVENTS:
                eventsHeader.setVisibility(GONE);
                eventsList.setVisibility(GONE);
                break;
            case FEATURE_RECENT:
                recentList.setVisibility(GONE);
                recentSettings.setVisibility(GONE);
                break;
            case FEATURE_IMAGE:
                // Region's chooser is the image's, in Settings or setup.
                closeSettingsDialog();
                imageCard.setVisibility(GONE);
                imageSettings.setVisibility(GONE);
                imageDetails.setVisibility(GONE);
                imageDetailsShown = 0;
                pastImages.setVisibility(GONE);
                pastImagesShown = 0;
                break;
            case FEATURE_SETTINGS:
                closeSettingsDialog();
                themeSettings.setVisibility(GONE);
                homeSettings.setVisibility(GONE);
                accessSettings.setVisibility(GONE);
                setup.setVisibility(GONE);
                setupShown = -1;
                banner.setVisibility(GONE);
                bannerShown = 0;
                break;
            default:
                break;
        }
        coverLayers();
    }

    /**
     * The cards that say a part stopped working, where the frame says (ADR 0043). They are
     * this view's own, so they show whatever failed.
     */
    private void applyStopped() {
        int stopped = frame[STOPPED];
        if (stopped != stoppedShown) {
            stoppedShown = stopped;
            applicationsStopped.show((stopped & STOPPED_APPLICATIONS) != 0);
            eventsStopped.show((stopped & STOPPED_EVENTS) != 0);
            recentStopped.show((stopped & STOPPED_RECENT) != 0);
            imageInEventsStopped.show((stopped & STOPPED_IMAGE_IN_EVENTS) != 0);
            imageInSettingsStopped.show((stopped & STOPPED_IMAGE_IN_SETTINGS) != 0);
            settingsStopped.show((stopped & STOPPED_SETTINGS) != 0);
        }
        if (stopped != 0) {
            for (StoppedCard card : stoppedCards) {
                card.colour(foreground, dim, surface, frame[ACCENT]);
            }
        }
    }

    /** A stopped card's place in a column, such as Events' or Settings'. */
    private LinearLayout.LayoutParams cardParams() {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT);
        params.setMargins(dp(16), dp(16), dp(16), dp(8));
        return params;
    }

    /** The banner offering to make MeeGone the default home app, while it shows (ONB-04). */
    private void applyBanner() {
        if (frame[HOME_BANNER] != bannerShown) {
            bannerShown = frame[HOME_BANNER];
            banner.setVisibility(bannerShown != 0 ? VISIBLE : GONE);
        }
        if (bannerShown != 0) {
            banner.colour(foreground, dim, surface, frame[ACCENT]);
        }
    }

    /**
     * Whether the banner, or Applications' stopped card in its place, shows at {@code (x, y)} in
     * the home views.
     */
    private boolean onBanner(float x, float y) {
        return on(banner, x, y) || on(applicationsStopped, x, y);
    }

    /** Whether {@code view}, a child of this one, shows at {@code (x, y)}. */
    private static boolean on(View view, float x, float y) {
        return view.getVisibility() == VISIBLE
                && x >= view.getLeft() && x < view.getRight()
                && y >= view.getTop() && y < view.getBottom();
    }

    /**
     * The daily image's status line, in Settings and on the details screen (WALL-16): read again
     * only when its revision changes, and hidden while the daily image is off.
     */
    private void applyImageStatus() {
        int revision = frame[IMAGE_STATUS_REVISION];
        if (revision == imageStatusRead) {
            return;
        }
        imageStatusRead = revision;
        String[] status = revision >= 0 ? nativeImageStatus() : null;
        imageSettings.showStatus(status);
        imageDetails.showStatus(status);
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
        recentList.leave(frame, RECENT_LEAVE, frame[RECENT_LEAVE_SCALE] / 10_000f);
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
            announce(label != null
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
        announce(getResources().getString(
                rows == 2 ? R.string.action_bigger_cards : R.string.action_more_cards));
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
     * Shows Settings over the home views while it is open, its groups in SET-02's order
     * (ADR 0035). It opens at its top, or at the lock-screen image's group when the home menu's
     * Lock-screen image or the details screen opened it (HOME-10, WALL-15). Each group is offered
     * only while the feature behind it works (ARCH-09).
     */
    private void applySettings() {
        if (frame[SETTINGS] != settingsShown) {
            settingsShown = frame[SETTINGS];
            settings.setVisibility(settingsShown != 0 ? VISIBLE : GONE);
            coverLayers();
            if (settingsShown == 0) {
                closeSettingsDialog();
            } else {
                // The device's language, and so Automatic's region, may have changed.
                regions = nativeRegions();
                regionShown = -2;
                // At the lock-screen image's group, or at its stopped card in its place.
                View at = null;
                if (settingsShown == SETTINGS_AT_LOCK_SCREEN_IMAGE) {
                    at = drawing(FEATURE_IMAGE) ? imageSettings.heading() : imageInSettingsStopped;
                }
                scrollSettingsTo(at);
            }
        }
        if (drawing(FEATURE_SETTINGS)) {
            try {
                applyThemeSettings();
                applyHomeSettings();
                applyAccessSettings();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_SETTINGS, e);
            }
        }
        applyHiddenApps();
        if (drawing(FEATURE_RECENT)) {
            try {
                applyRecentSettings();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_RECENT, e);
            }
        }
        if (drawing(FEATURE_IMAGE)) {
            try {
                applyImageSettings();
            } catch (RuntimeException | OutOfMemoryError e) {
                stop(FEATURE_IMAGE, e);
            }
        }
        aboutSettings.colour(foreground, dim, frame[ACCENT]);
        aboutSettings.show(frame[RESET_OFFERED] != 0);
    }

    /**
     * Scrolls Settings to the group headed {@code heading}, or to a stopped card in a group's
     * place, which screen readers move to, or to its top with null, once it is laid out.
     */
    private void scrollSettingsTo(View heading) {
        ViewTreeObserver observer = settingsScroll.getViewTreeObserver();
        observer.addOnPreDrawListener(new ViewTreeObserver.OnPreDrawListener() {
            @Override
            public boolean onPreDraw() {
                settingsScroll.getViewTreeObserver().removeOnPreDrawListener(this);
                if (heading == null) {
                    settingsScroll.scrollTo(0, 0);
                    return true;
                }
                // Its top within the groups, the scroll's one child.
                int top = 0;
                for (View view = heading; view.getParent() instanceof View
                        && view.getParent() != settingsScroll;
                        view = (View) view.getParent()) {
                    top += view.getTop();
                }
                settingsScroll.scrollTo(0, top);
                heading.performAccessibilityAction(
                        AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null);
                return true;
            }
        });
    }

    /**
     * Settings → Home: "Home button opens" and Wrap-around (HOME-04, HOME-06), offered only while
     * Settings works (ARCH-09).
     */
    private void applyHomeSettings() {
        int target = frame[HOME_TARGET];
        homeSettings.setVisibility(target >= 0 ? VISIBLE : GONE);
        homeSettings.colour(foreground, dim, frame[ACCENT]);
        homeSettings.show(target, frame[WRAP_AROUND] > 0);
    }

    /**
     * Settings → Access: each item's status and shortcut, and the written instructions where a
     * shortcut found no system screen (SET-02, SRS 8.1), offered only while Settings works
     * (ARCH-09).
     */
    private void applyAccessSettings() {
        boolean offered = frame[DEFAULT_HOME] >= 0;
        accessSettings.setVisibility(offered ? VISIBLE : GONE);
        if (!offered) {
            return;
        }
        accessSettings.colour(foreground, dim, frame[ACCENT]);
        int instructed = frame[ACCESS_INSTRUCTIONS];
        if (instructed != instructionsShown) {
            instructionsShown = instructed;
            instructions = instructed >= 0
                    ? Problems.text(getResources(), nativeAccessInstructions())
                    : null;
        }
        accessSettings.show(frame[DEFAULT_HOME] > 0, frame[NOTIFICATION_ACCESS] > 0,
                frame[USAGE_ACCESS] > 0, instructed, instructions);
    }

    /**
     * Each of {@link #RECENT_HOURS} in words, "12 hours", "24 hours" and "3 days", with the
     * locale's digits (L10N-03).
     */
    private static CharSequence[] recentWindows(Resources res) {
        CharSequence[] names = new CharSequence[RECENT_HOURS.length];
        for (int i = 0; i < RECENT_HOURS.length; i++) {
            int hours = RECENT_HOURS[i];
            names[i] = hours % 24 == 0 && hours > 24
                    ? res.getQuantityString(R.plurals.recent_window_days, hours / 24, hours / 24)
                    : res.getQuantityString(R.plurals.recent_window_hours, hours, hours);
        }
        return names;
    }

    /**
     * A region by name, in the device's language (L10N-04): "Automatic (English (United
     * States))" first, then "Deutsch (Deutschland)" and the others.
     */
    private String regionName(int place) {
        if (place < 0 || place >= regions.length) {
            return "";
        }
        return place == 0
                ? getResources().getString(R.string.image_region_automatic,
                        Regions.nameInSentence(regions[place]))
                : Regions.name(regions[place]);
    }

    /** A dialog over Settings, laid out as the device's in MeeGone's theme (THEME-03). */
    private AlertDialog.Builder dialogOverSettings() {
        return Dialogs.builder(getContext(), Color.luminance(background) > 0.5f);
    }

    /** Shows {@code dialog} over Settings, in MeeGone's colours, closing any other. */
    private void showSettingsDialog(AlertDialog dialog) {
        closeSettingsDialog();
        settingsDialog = dialog;
        dialog.setOnDismissListener(shown -> {
            if (settingsDialog == dialog) {
                settingsDialog = null;
            }
        });
        Dialogs.show(dialog);
        Dialogs.colour(dialog, surface, foreground, dim, frame[ACCENT], frame[SCRIM]);
    }

    /** Closes the dialog over Settings, as when Settings closes. */
    private void closeSettingsDialog() {
        AlertDialog dialog = settingsDialog;
        settingsDialog = null;
        if (dialog != null) {
            dialog.dismiss();
        }
    }

    /**
     * Settings → Lock-screen image → Region: Automatic, then the feed's regions by name in the
     * language's alphabetical order (L10N-04), the chosen one checked (WALL-05). A choice is
     * saved at once (SET-03).
     */
    private void chooseRegion() {
        if (regions.length == 0 || frame[REGION] < 0) {
            return;
        }
        String[] names = new String[regions.length];
        for (int place = 0; place < regions.length; place++) {
            names[place] = regionName(place);
        }
        int[] order = Regions.order(names);
        String[] listed = new String[order.length];
        int checked = -1;
        for (int at = 0; at < order.length; at++) {
            listed[at] = names[order[at]];
            if (order[at] == frame[REGION]) {
                checked = at;
            }
        }
        showSettingsDialog(dialogOverSettings()
                .setTitle(R.string.image_region)
                .setSingleChoiceItems(listed, checked, (shown, at) -> {
                    shown.dismiss();
                    String message = nativeRegion(order[at]);
                    apply();
                    toast(message);
                })
                .setNegativeButton(android.R.string.cancel, null)
                .create());
    }

    /**
     * Settings → About → "Open-source licences": what MeeGone includes, then each licence in
     * full (LEGAL-06).
     */
    private void showLicences() {
        AlertDialog.Builder builder = dialogOverSettings();
        StringBuilder text = new StringBuilder(getResources().getString(R.string.licences_intro));
        for (int licence : LICENCES) {
            text.append("\n\n").append(readText(getResources(), licence));
        }
        TextView body = new TextView(builder.getContext());
        body.setText(text);
        body.setTextSize(TypedValue.COMPLEX_UNIT_SP, 14);
        body.setPaddingRelative(dp(24), dp(8), dp(24), dp(8));
        Dialogs.colourText(body, foreground);
        ScrollView scroll = new Scroll(builder.getContext());
        Themed.scrolling(scroll, dim);
        scroll.addView(body);
        showSettingsDialog(builder
                .setTitle(R.string.about_licences)
                .setView(scroll)
                .setPositiveButton(android.R.string.ok, null)
                .create());
    }

    /** A text file among the app's raw resources, or nothing if it can't be read. */
    private static String readText(Resources res, int id) {
        try (InputStream in = res.openRawResource(id)) {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buffer = new byte[8192];
            for (int read = in.read(buffer); read > 0; read = in.read(buffer)) {
                out.write(buffer, 0, read);
            }
            return new String(out.toByteArray(), StandardCharsets.UTF_8);
        } catch (IOException | Resources.NotFoundException e) {
            return "";
        }
    }

    /**
     * Settings → About → Reset launcher asks first, saying what it does (SET-06). Reset puts
     * everything back to its default, closes Settings and shows Applications.
     */
    private void confirmReset() {
        showSettingsDialog(dialogOverSettings()
                .setTitle(R.string.reset_launcher_title)
                .setMessage(R.string.reset_launcher_message)
                .setPositiveButton(R.string.reset_launcher_confirm, (shown, which) -> {
                    String problem = nativeResetLauncher();
                    apply();
                    say(problem != null
                            ? Problems.text(getResources(), problem)
                            : getContext().getString(R.string.launcher_reset));
                })
                .setNegativeButton(android.R.string.cancel, null)
                .create());
    }

    /** Settings → Appearance → Theme (THEME-01): offered only while Settings works (ARCH-09). */
    private void applyThemeSettings() {
        int theme = frame[THEME];
        themeSettings.setVisibility(theme >= 0 ? VISIBLE : GONE);
        themeSettings.colour(foreground, dim, frame[ACCENT]);
        themeSettings.show(theme);
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
     * Settings → Lock-screen image: the daily image's switch (WALL-02, WALL-13), the pause
     * (WALL-14), "Daily image unavailable" (SRS 7.6), Region (WALL-05), "Use mobile data"
     * (WALL-04), "Show card in Events" (EVT-12), "Update now" (WALL-11) and, once it is off, the
     * reset and the wallpaper settings (WALL-12), offered only while the lock-screen image works
     * (ARCH-09).
     */
    private void applyImageSettings() {
        int daily = frame[DAILY_IMAGE];
        imageSettings.setVisibility(daily >= 0 ? VISIBLE : GONE);
        if (daily < 0) {
            return;
        }
        imageSettings.colour(foreground, dim, frame[ACCENT]);
        if (frame[REGION] != regionShown) {
            regionShown = frame[REGION];
            imageSettings.showRegion(regionName(regionShown));
        }
        String reason = daily == ImageSettings.DAILY_IMAGE_UNAVAILABLE
                ? Problems.text(getResources(), nativeImageUnavailableReason())
                : null;
        imageSettings.show(
                daily,
                reason,
                frame[UPDATE_NOW],
                frame[IMAGE_PAUSED] != 0,
                frame[IMAGE_MAY_NOT_SHOW] != 0,
                frame[IMAGE_UNAVAILABLE] != 0,
                frame[SHOW_IMAGE_CARD] > 0,
                frame[USE_MOBILE_DATA] > 0,
                frame[IMAGE_OFF_ACTIONS] != 0);
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
                menuOpener = null;
            } else if (appMenu >= 0) {
                fillAppMenu(frame[APP_MENU_REVISION]);
                menuOpener = menuGrid().cellAt(frame[APP_MENU]);
            } else if (eventsItems != 0) {
                fillEventsMenu(eventsItems, eventsRow);
                menuOpener = eventsList.rowAt(eventsRow);
            }
            coverLayers();
            if (!wasOpen && menuOpen() && !menuItems.isEmpty()) {
                // A screen reader moves to the first item (A11Y-05).
                TextView first = menuItems.get(0);
                menu.post(() -> first.performAccessibilityAction(
                        AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null));
            }
            if (wasOpen && !menuOpen()) {
                // A screen reader goes back to the cell or row the menu was for (A11Y-08).
                focusBack(menuOpener);
                menuOpener = null;
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
     * Edit applications. In edit mode, the moves take Edit applications' place (ADR 0039). Its
     * pane title names the app or the folder.
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
            String name = orEmpty(fields[i * APP_MENU_FIELDS + 1]);
            CharSequence text;
            if (kind == APP_MENU_SHORTCUT) {
                text = name;
                if (!divided && i > 0) {
                    addMenuDivider();
                }
                divided = true;
            } else if (kind == APP_MENU_MAKE_FOLDER) {
                // "Make folder with Maps", the next app named as everywhere (A11Y-01).
                boolean work = "1".equals(fields[i * APP_MENU_FIELDS + 2]);
                text = getResources().getString(R.string.action_make_folder,
                        A11y.appName(getResources(), name, work));
            } else if (kind == APP_MENU_MOVE_INTO) {
                // An unnamed folder shows as "Folder" (FOLD-03).
                text = getResources().getString(R.string.action_move_into,
                        name.isEmpty() ? getResources().getString(R.string.folder) : name);
            } else if (kind >= 0 && kind < APP_MENU_ITEMS.length && APP_MENU_ITEMS[kind] != 0) {
                text = getResources().getText(APP_MENU_ITEMS[kind]);
            } else {
                continue;
            }
            int index = i;
            addMenuItem(text, view -> chooseAppMenuItem(revision, index, kind));
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

    private static String orEmpty(String field) {
        return field == null ? "" : field;
    }

    private static int parseKind(String field) {
        try {
            return Integer.parseInt(field);
        } catch (NumberFormatException e) {
            return -1;
        }
    }

    /**
     * Item {@code index}, of {@code kind}, of the app menu at {@code revision} was chosen. The
     * system may refuse, such as an App info screen that can't open: its message shows briefly.
     * A move says where the app went, as a screen reader's does, and the focus goes with it
     * (A11Y-08, ADR 0039).
     */
    private void chooseAppMenuItem(int revision, int index, int kind) {
        AppsGrid grid = menuGrid();
        int place = frame[APP_MENU];
        View opener = menuOpener;
        String name = grid.label(place);
        String message = nativeAppMenuChosen(revision, index);
        apply();
        toast(message);
        if (name == null) {
            return;
        }
        if (kind >= APP_MENU_MOVE_FIRST && kind <= APP_MENU_MOVE_LAST
                && opener instanceof AppCell) {
            // The cell's view moved with its app, and the focus goes back to it.
            announce(getResources().getString(R.string.app_moved, name,
                    ((AppCell) opener).index() + 1, grid.cellCount()));
            keepInSight(opener);
        } else if (kind == APP_MENU_MAKE_FOLDER || kind == APP_MENU_MOVE_INTO) {
            // The folder is where the app was.
            announce(getResources().getString(R.string.app_grouped, name));
            focusLater(apps.cellAt(place));
        } else if (kind == APP_MENU_MOVE_OUT) {
            announce(getResources().getString(R.string.app_moved_out, name));
            focusLater(folderApps.cellAt(Math.min(place, folderApps.cellCount() - 1)));
        }
    }

    /**
     * Gives {@code view} the focus once the menu has closed, after the focus going back to the
     * cell the menu was for, which has gone.
     */
    private void focusLater(View view) {
        if (view != null) {
            post(() -> focusBack(view));
        }
    }

    private void clearMenu() {
        menuList.removeAllViews();
        menuScroll.scrollTo(0, 0);
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
        item.setMinHeight(A11y.targetPx(this));
        item.setMinWidth(dp(200));
        item.setPaddingRelative(dp(24), 0, dp(24), 0);
        item.setOnClickListener(chosen);
        menuList.addView(item, new LinearLayout.LayoutParams(
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
        menuList.addView(divider, params);
        menuDividers.add(divider);
    }

    /** The menu's items in the theme's text colour, with a light wash when pressed. */
    private void colourMenu() {
        for (TextView item : menuItems) {
            item.setTextColor(foreground);
            item.setBackground(SettingsRows.ripple(foreground));
        }
        for (View divider : menuDividers) {
            divider.setBackgroundColor(frame[DIVIDER]);
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

    /**
     * Screen readers' focus goes back to {@code view}, such as the cell whose menu or folder
     * just closed, which they then read: "Folder: Games, 3 apps" (A11Y-08), and so does a
     * keyboard's (A11Y-06). Not while another screen covers the home views, nor for a view that
     * has gone.
     */
    private void focusBack(View view) {
        if (view == null) {
            return;
        }
        post(() -> {
            if (view.isAttachedToWindow() && view.isShown() && !menuOpen()
                    && frame[SETTINGS] == 0 && frame[SETUP_STEP] < 0 && imageDetailsShown == 0
                    && pastImagesShown == 0) {
                // A keyboard's focus goes back too (A11Y-06).
                if (!isInTouchMode()) {
                    view.requestFocus();
                }
                view.performAccessibilityAction(
                        AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null);
            }
        });
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

    // The container scrolls between the views, a page at a time, for screen readers' scroll
    // gestures and Switch Access's scroll (A11Y-05). Every other action is on each view's own
    // element, which screen readers can focus (ADR 0039).
    @Override
    public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        info.setScrollable(true);
        info.addAction(AccessibilityAction.ACTION_SCROLL_FORWARD);
        info.addAction(AccessibilityAction.ACTION_SCROLL_BACKWARD);
        info.addAction(AccessibilityAction.ACTION_PAGE_LEFT);
        info.addAction(AccessibilityAction.ACTION_PAGE_RIGHT);
    }

    @Override
    public boolean performAccessibilityAction(int action, Bundle arguments) {
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        Boolean next = null;
        if (action == AccessibilityNodeInfo.ACTION_SCROLL_FORWARD) {
            next = true;
        } else if (action == AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD) {
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

    /**
     * Each view's own element, {@link ViewNode} (A11Y-05, ADR 0039). It is named for the view,
     * as in "Applications, 2 of 3". Its press is the view's long-press: the home menu, or Edit
     * Recent. Its actions are Next view, Previous view, Search apps and Bigger or More cards,
     * each while the Rust app offers it. Only the view showing offers anything.
     */
    private final ViewNode.Owner viewNodes = new ViewNode.Owner() {
        @Override
        public void describe(int view, AccessibilityNodeInfo info) {
            info.setContentDescription(viewPosition(view));
            if (view != frame[CURRENT]) {
                return;
            }
            int press = press(view);
            if (press != 0) {
                String label = getResources().getString(press == R.id.action_home_menu
                        ? R.string.action_home_menu
                        : R.string.action_edit_recent);
                info.setClickable(true);
                info.setLongClickable(true);
                info.addAction(new AccessibilityAction(
                        AccessibilityNodeInfo.ACTION_CLICK, label));
                info.addAction(new AccessibilityAction(
                        AccessibilityNodeInfo.ACTION_LONG_CLICK, label));
            }
            info.addAction(new AccessibilityAction(
                    R.id.action_next_view, getResources().getString(R.string.action_next_view)));
            info.addAction(new AccessibilityAction(R.id.action_previous_view,
                    getResources().getString(R.string.action_previous_view)));
            if (view == APPLICATIONS && (frame[SEARCH] & SEARCH_OFFERED) != 0) {
                info.addAction(new AccessibilityAction(
                        R.id.action_search, getResources().getString(R.string.search_action)));
            }
            if (view == RECENT_VIEW && frame[RECENT_DENSITY_OFFERED] != 0) {
                // The other density from the one shown (REC-05).
                boolean bigger = frame[RECENT_ROWS] != 2;
                info.addAction(new AccessibilityAction(
                        bigger ? R.id.action_bigger_cards : R.id.action_more_cards,
                        getResources().getString(bigger
                                ? R.string.action_bigger_cards
                                : R.string.action_more_cards)));
            }
        }

        @Override
        public boolean perform(int view, int action) {
            if (view != frame[CURRENT]) {
                return false;
            }
            if (action == AccessibilityNodeInfo.ACTION_CLICK
                    || action == AccessibilityNodeInfo.ACTION_LONG_CLICK) {
                action = press(view);
            }
            if (action == R.id.action_home_menu) {
                boolean opened = nativeOpenMenu();
                apply();
                return opened;
            }
            if (action == R.id.action_edit_recent) {
                nativeRecentEditRequested();
                apply();
                return frame[RECENT_CLOSE] != 0;
            }
            if (action == R.id.action_search && view == APPLICATIONS) {
                openSearch();
                return (frame[SEARCH] & SEARCH_OPEN) != 0;
            }
            if (action == R.id.action_bigger_cards && view == RECENT_VIEW) {
                return chooseDensity(2);
            }
            if (action == R.id.action_more_cards && view == RECENT_VIEW) {
                return chooseDensity(3);
            }
            if (action == R.id.action_next_view || action == R.id.action_previous_view) {
                return switchView(action == R.id.action_next_view);
            }
            return false;
        }

        /** What a press on {@code view}'s element does now, as an action, or 0 for nothing. */
        private int press(int view) {
            if (view == RECENT_VIEW) {
                // A long-press on Recent enters close mode (REC-07).
                return frame[RECENT_EDIT_OFFERED] != 0 ? R.id.action_edit_recent : 0;
            }
            return frame[MENU_OFFERED] != 0 ? R.id.action_home_menu : 0;
        }
    };

    /**
     * The next or the previous view, from a view's element or a keyboard (A11Y-05, A11Y-06).
     * The new view's element takes the focus, and reads its name, so the status doesn't say it
     * too. Returns whether the views moved.
     */
    private boolean switchView(boolean next) {
        int before = frame[CURRENT];
        boolean moved = nativeShow(next);
        if (moved) {
            quietSwitch = true;
        }
        apply();
        quietSwitch = false;
        if (frame[CURRENT] != before) {
            focusView(nodes[frame[CURRENT]]);
        }
        return moved;
    }

    /**
     * Gives {@code view} the focus: a screen reader's, and a keyboard's out of touch mode
     * (A11Y-06).
     */
    private void focusView(View view) {
        post(() -> {
            if (!view.isAttachedToWindow() || !view.isShown()) {
                return;
            }
            if (!isInTouchMode()) {
                view.requestFocus();
            }
            view.performAccessibilityAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null);
        });
    }

    /** Sets what Back does, which Esc does too (HOME-08, A11Y-06). */
    void setBack(Runnable back) {
        this.back = back;
    }

    // Esc is Back (A11Y-06). Android would spend the first Esc clearing the focus, before any
    // view or the activity sees it, so it is taken here, first. With nothing focused, the
    // activity takes it.
    @Override
    public boolean dispatchKeyEventPreIme(KeyEvent event) {
        if (event.getKeyCode() == KeyEvent.KEYCODE_ESCAPE && event.hasNoModifiers()) {
            if (event.getAction() == KeyEvent.ACTION_DOWN && event.getRepeatCount() == 0) {
                back.run();
            }
            return true;
        }
        return super.dispatchKeyEventPreIme(event);
    }

    // A hardware keyboard (A11Y-06, ADR 0039). Tab, the arrows and Enter are Android's own, and
    // reach every element. The keys below reach what a gesture does, through the same actions
    // screen readers use, so the Rust app decides what each does, as it does for them.
    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        // Edit mode's moves come before the grid's own arrows, which would move the focus.
        if (event.getAction() == KeyEvent.ACTION_DOWN && moveFocusedCell(event)) {
            return true;
        }
        if (super.dispatchKeyEvent(event)) {
            return true;
        }
        return event.getAction() == KeyEvent.ACTION_DOWN && keyDown(event);
    }

    /**
     * A key no element took:
     *
     * <ul>
     *   <li>Menu, or Shift+F10, is a long-press: the focused element's menu, or the view's.
     *   <li>Delete is the ✕: it uninstalls in edit mode and removes from Recent in close mode.
     *   <li>← or → past the view's edge goes to the view beyond it. ↓ from the view's own
     *       element goes into the view.
     *   <li>Ctrl+F, or typing a letter, on Applications opens search, with the letter in it.
     *   <li>Ctrl+= and Ctrl+- on Recent show bigger cards and more cards, as a pinch does.
     * </ul>
     *
     * Returns whether the key did anything.
     */
    private boolean keyDown(KeyEvent event) {
        int code = event.getKeyCode();
        View focused = findFocus();
        if (code == KeyEvent.KEYCODE_MENU
                || (code == KeyEvent.KEYCODE_F10 && event.isShiftPressed())) {
            return event.getRepeatCount() > 0 || longPress(focused);
        }
        if (code == KeyEvent.KEYCODE_DEL || code == KeyEvent.KEYCODE_FORWARD_DEL) {
            return focused != null && (perform(focused, R.id.action_uninstall)
                    || perform(focused, R.id.action_remove_from_recent));
        }
        if (code == KeyEvent.KEYCODE_F && event.isCtrlPressed() && !event.isAltPressed()) {
            return openSearch();
        }
        if (event.isCtrlPressed() && frame[CURRENT] == RECENT_VIEW && topLayer() == LAYER_HOME) {
            // Recent's density, as a zoom: bigger cards, or more of them (REC-05).
            if (code == KeyEvent.KEYCODE_EQUALS || code == KeyEvent.KEYCODE_PLUS
                    || code == KeyEvent.KEYCODE_NUMPAD_ADD) {
                return viewNodes.perform(RECENT_VIEW, R.id.action_bigger_cards);
            }
            if (code == KeyEvent.KEYCODE_MINUS || code == KeyEvent.KEYCODE_NUMPAD_SUBTRACT) {
                return viewNodes.perform(RECENT_VIEW, R.id.action_more_cards);
            }
        }
        if (event.hasNoModifiers()) {
            if (code == KeyEvent.KEYCODE_DPAD_LEFT) {
                return pastTheEdge(focused, FOCUS_LEFT);
            }
            if (code == KeyEvent.KEYCODE_DPAD_RIGHT) {
                return pastTheEdge(focused, FOCUS_RIGHT);
            }
            if (code == KeyEvent.KEYCODE_DPAD_DOWN && focused instanceof ViewNode) {
                return intoTheView(focused);
            }
        }
        return typeToSearch(event);
    }

    /**
     * A long-press without a finger: the focused element's own, such as an app's menu or Edit
     * Recent on a card, or with none, the view's. Returns whether it did anything.
     */
    private boolean longPress(View focused) {
        if (focused != null && perform(focused, AccessibilityNodeInfo.ACTION_LONG_CLICK)) {
            return true;
        }
        return topLayer() == LAYER_HOME && (focused == null || focused instanceof ViewNode)
                && viewNodes.perform(frame[CURRENT], AccessibilityNodeInfo.ACTION_LONG_CLICK);
    }

    /** Does {@code action} on {@code view} if it offers it. Returns whether it did. */
    private static boolean perform(View view, int action) {
        AccessibilityNodeInfo info = view.createAccessibilityNodeInfo();
        for (AccessibilityAction offered : info.getActionList()) {
            if (offered.getId() == action) {
                return view.performAccessibilityAction(action, null);
            }
        }
        return false;
    }

    /**
     * ← or → with nothing further that way in the view goes to the view beyond, as the home
     * views' swipe does (HOME-03): mirrored right to left, and wrapping around as set
     * (HOME-04). Not while anything covers the views, or holds them still: an open folder or
     * search.
     */
    private boolean pastTheEdge(View focused, int direction) {
        FrameLayout page = pages[frame[CURRENT]];
        if (focused == null || topLayer() != LAYER_HOME || frame[FOLDER] >= 0 || searchOpen
                || !inside(page, focused)) {
            return false;
        }
        View next = FocusFinder.getInstance().findNextFocus(this, focused, direction);
        if (next != null && inside(page, next)) {
            return false;
        }
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        switchView((direction == FOCUS_RIGHT) != rtl);
        return true;
    }

    /** ↓ from a view's own element goes to its first element after it. */
    private boolean intoTheView(View node) {
        View next = FocusFinder.getInstance().findNextFocus(this, node, FOCUS_FORWARD);
        if (next != null && inside((View) node.getParent(), next)) {
            return next.requestFocus();
        }
        return false;
    }

    /** Whether {@code view} is {@code ancestor} or inside it. */
    private static boolean inside(View ancestor, View view) {
        for (View at = view; at != null;
                at = at.getParent() instanceof View ? (View) at.getParent() : null) {
            if (at == ancestor) {
                return true;
            }
        }
        return false;
    }

    /**
     * In edit mode, Ctrl with an arrow moves the focused app or folder a place or a row, and
     * Ctrl+Home or Ctrl+End to the start or the end (APPS-12, A11Y-05), mirrored right to left,
     * as a drag and a screen reader's Move actions do. Its new place is announced, and the
     * focus stays on it. Returns whether the key was one of these.
     */
    private boolean moveFocusedCell(KeyEvent event) {
        View focused = findFocus();
        if (!event.isCtrlPressed() || event.isAltPressed() || frame[EDITING] == 0
                || !(focused instanceof AppCell)) {
            return false;
        }
        AppCell cell = (AppCell) focused;
        AppsGrid grid = cell.grid();
        if (grid.surface() == AppsGrid.SEARCH) {
            return false;
        }
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        int index = cell.index();
        int to;
        switch (event.getKeyCode()) {
            case KeyEvent.KEYCODE_DPAD_LEFT:
                to = rtl ? index + 1 : index - 1;
                break;
            case KeyEvent.KEYCODE_DPAD_RIGHT:
                to = rtl ? index - 1 : index + 1;
                break;
            case KeyEvent.KEYCODE_DPAD_UP:
                to = index - grid.columns();
                break;
            case KeyEvent.KEYCODE_DPAD_DOWN:
                to = index + grid.columns();
                break;
            case KeyEvent.KEYCODE_MOVE_HOME:
                to = 0;
                break;
            case KeyEvent.KEYCODE_MOVE_END:
                to = grid.cellCount() - 1;
                break;
            default:
                return false;
        }
        to = Math.max(0, Math.min(to, grid.cellCount() - 1));
        if (to != index && grid.moveTo(cell, to)) {
            keepInSight(cell);
        }
        return true;
    }

    /** Scrolls {@code view} into sight once it is laid out where it now is. */
    private static void keepInSight(View view) {
        ViewTreeObserver observer = view.getViewTreeObserver();
        observer.addOnPreDrawListener(new ViewTreeObserver.OnPreDrawListener() {
            @Override
            public boolean onPreDraw() {
                view.getViewTreeObserver().removeOnPreDrawListener(this);
                view.requestRectangleOnScreen(
                        new Rect(0, 0, view.getWidth(), view.getHeight()), !Scroll.animating());
                return true;
            }
        });
    }

    /**
     * Opens search, as the pull and a screen reader's "Search apps" do, while it is offered
     * (SRCH-01, A11Y-05). Returns whether it opened.
     */
    private boolean openSearch() {
        if ((frame[SEARCH] & SEARCH_OFFERED) == 0) {
            return false;
        }
        nativeOpenSearch();
        apply();
        return (frame[SEARCH] & SEARCH_OPEN) != 0;
    }

    /**
     * Typing a letter on Applications opens search with it, as Android's own launchers do
     * (SRCH-01, A11Y-06). Not with Ctrl, Alt or Meta, which are shortcuts, nor for a space or
     * an accent still waiting for its letter.
     */
    private boolean typeToSearch(KeyEvent event) {
        if (event.isCtrlPressed() || event.isAltPressed() || event.isMetaPressed()) {
            return false;
        }
        int typed = event.getUnicodeChar();
        if (typed == 0 || (typed & KeyCharacterMap.COMBINING_ACCENT) != 0
                || Character.isWhitespace(typed) || Character.isISOControl(typed)) {
            return false;
        }
        if (!openSearch()) {
            return false;
        }
        searchPanel.type(new String(Character.toChars(typed)));
        return true;
    }

    // What can cover the home views, from the lowest, as topLayer answers: the views themselves,
    // the image details, Settings, the past week, setup and a menu.
    private static final int LAYER_HOME = 0;
    private static final int LAYER_DETAILS = 1;
    private static final int LAYER_SETTINGS = 2;
    private static final int LAYER_PAST = 3;
    private static final int LAYER_SETUP = 4;
    private static final int LAYER_MENU = 5;

    /** The topmost of what shows, as last drawn. */
    private int topLayer() {
        if (menuOpen()) {
            return LAYER_MENU;
        }
        if (setupShown >= 0) {
            return LAYER_SETUP;
        }
        if (pastImagesShown != 0) {
            return LAYER_PAST;
        }
        if (settingsShown != 0) {
            return LAYER_SETTINGS;
        }
        return imageDetailsShown != 0 ? LAYER_DETAILS : LAYER_HOME;
    }

    /** The view that is layer {@code layer}: for the home views, the view showing. */
    private View layer(int layer) {
        switch (layer) {
            case LAYER_MENU:
                return menu;
            case LAYER_SETUP:
                return setup;
            case LAYER_PAST:
                return pastImages;
            case LAYER_SETTINGS:
                return settings;
            case LAYER_DETAILS:
                return imageDetails;
            default:
                return pages[frame[CURRENT]];
        }
    }

    /**
     * Only the topmost of the home views, the image details, Settings, the past week, setup and
     * a menu is there for screen readers and for a keyboard. What it covers is skipped, as no
     * finger reaches it (A11Y-06, ADR 0039).
     */
    private void coverLayers() {
        int top = topLayer();
        for (FrameLayout page : pages) {
            cover(page, top > LAYER_HOME);
        }
        cover(imageDetails, top > LAYER_DETAILS);
        cover(settings, top > LAYER_SETTINGS);
        cover(pastImages, top > LAYER_PAST);
        cover(setup, top > LAYER_SETUP);
    }

    private static void cover(ViewGroup layer, boolean covered) {
        layer.setImportantForAccessibility(covered
                ? IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                : IMPORTANT_FOR_ACCESSIBILITY_AUTO);
        // Uncovered, each takes Android's own default back: a scrolling screen's elements
        // before itself.
        layer.setDescendantFocusability(covered
                ? FOCUS_BLOCK_DESCENDANTS
                : layer instanceof ScrollView ? FOCUS_AFTER_DESCENDANTS : FOCUS_BEFORE_DESCENDANTS);
    }

    /**
     * When something opens over the others, or closes, the focus goes to the topmost: to its
     * first element when it opens, and when it closes to the element it was for, or the view's
     * own element (A11Y-06). Only a keyboard's focus, out of touch mode: screen readers' is
     * moved as each opens and closes. A view that went out of sight loses the focus to the
     * view showing.
     */
    private void followLayers() {
        int top = topLayer();
        boolean changed = top != layerShown;
        boolean opened = layerShown >= 0 && top > layerShown;
        if (changed) {
            layerShown = top;
            coverLayers();
        }
        if (isInTouchMode()) {
            return;
        }
        View focused = findFocus();
        View layer = layer(top);
        if (opened || focused == null || !inside(layer, focused)) {
            focusOnceLaidOut(top == LAYER_HOME ? nodes[frame[CURRENT]] : layer, layer);
        }
    }

    /**
     * Gives {@code view}, or its first element, a keyboard's focus once it is laid out, unless
     * the focus is in {@code within} by then, such as back on the cell a menu was for. A scroll
     * view not laid out yet takes every element as off screen, and the focus itself.
     */
    private void focusOnceLaidOut(View view, View within) {
        ViewTreeObserver observer = getViewTreeObserver();
        observer.addOnPreDrawListener(new ViewTreeObserver.OnPreDrawListener() {
            @Override
            public boolean onPreDraw() {
                getViewTreeObserver().removeOnPreDrawListener(this);
                View focused = findFocus();
                if (!isInTouchMode() && view.isShown()
                        && (focused == null || !inside(within, focused))) {
                    // Its first stop in Tab's order, so a scroll view never takes it itself.
                    View first = view instanceof ViewGroup
                            ? FocusFinder.getInstance().findNextFocus(
                                    (ViewGroup) view, null, FOCUS_FORWARD)
                            : null;
                    (first != null ? first : view).requestFocus();
                }
                return true;
            }
        });
    }

    // The debug build's accessibility audit drives and reads the home views through what
    // follows, as a screen reader would (ADR 0038). Release builds never call it.

    /** Whether nothing moves, as of the last frame. */
    boolean auditSettled() {
        return frame[ANIMATING] == 0;
    }

    /** The home view shown, 0 to 2. */
    int auditView() {
        return frame[CURRENT];
    }

    /** The theme chosen in Settings, a value of {@link #THEMES}, or -1 while not offered. */
    int auditTheme() {
        return frame[THEME];
    }

    /** Chooses theme {@code theme}, a value of {@link #THEMES}, as Settings does. */
    void auditSetTheme(int theme) {
        nativeTheme(theme);
        apply();
    }

    /** Recent's rows on one screen: 3 for more cards, 2 for bigger ones. */
    int auditRecentRows() {
        return frame[RECENT_ROWS];
    }

    /** The setup step shown, or -1. */
    int auditSetupStep() {
        return frame[SETUP_STEP];
    }

    /** Whether the image details screen or the past week's images show. */
    boolean auditImageDetails() {
        return frame[IMAGE_DETAILS] != 0;
    }

    boolean auditPastImages() {
        return frame[PAST_IMAGES] != 0;
    }

    /**
     * Whether anything is open over the home views or in them that Back closes: a menu,
     * Settings, a screen over them, a folder, search, edit mode or close mode.
     */
    boolean auditOpen() {
        return menuOpen() || frame[SETTINGS] != 0 || frame[IMAGE_DETAILS] != 0
                || frame[PAST_IMAGES] != 0 || frame[FOLDER] >= 0
                || (frame[SEARCH] & SEARCH_OPEN) != 0 || frame[EDITING] != 0
                || frame[RECENT_CLOSE] != 0 || frame[RENAMING] >= 0;
    }

    /** The dialog showing over the home views, or null. */
    AlertDialog auditDialog() {
        return settingsDialog != null ? settingsDialog : renameDialog;
    }

    /** The scrolling view that shows the most of what is on top, or null. */
    ScrollView auditScroll() {
        if (frame[PAST_IMAGES] != 0) {
            return pastImages;
        } else if (frame[SETTINGS] != 0) {
            return settingsScroll;
        } else if (frame[IMAGE_DETAILS] != 0) {
            return imageDetails;
        } else if (frame[SETUP_STEP] >= 0) {
            return setup;
        } else if (menuOpen()) {
            return menuScroll;
        } else if (frame[FOLDER] >= 0) {
            return folderApps;
        } else if ((frame[SEARCH] & SEARCH_RESULTS) != 0) {
            return results;
        }
        int current = frame[CURRENT];
        return current == APPLICATIONS ? apps : scrolls.get(current == EVENTS ? 0 : 1);
    }

    AppsGrid auditApps() {
        return apps;
    }

    EventsList auditEvents() {
        return eventsList;
    }

    /** The search field, which the audit types into as a user would. */
    EditText auditSearchField() {
        return searchPanel.auditField();
    }

    /** The open folder's name, which renames it when tapped. */
    TextView auditFolderName() {
        return folderPanel.auditName();
    }

    /** Home view {@code view}'s page, 0 to 2, where it is drawn (L10N-02, ADR 0040). */
    View auditPage(int view) {
        return pages[view];
    }

    /** The element of the view shown, which carries the view's actions (ADR 0039). */
    ViewNode auditNode() {
        return nodes[frame[CURRENT]];
    }

    /**
     * What a keyboard's focus may reach now, as a finger may (A11Y-06): the topmost screen or
     * menu; an open folder; or the view shown and the banner below it.
     */
    List<View> auditFocusLayers() {
        List<View> layers = new ArrayList<>();
        int top = topLayer();
        if (top != LAYER_HOME) {
            layers.add(layer(top));
        } else if (frame[FOLDER] >= 0 && frame[FOLDER_OPEN] != 0) {
            layers.add(folderPanel);
        } else {
            layers.add(pages[frame[CURRENT]]);
            layers.add(banner);
            layers.add(applicationsStopped);
        }
        return layers;
    }

    /** Where the focus ring is drawn, in this view's coordinates, or an empty rect. */
    Rect auditRing() {
        Rect drawn = new Rect();
        ring.drawnAt(drawn);
        return drawn;
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
     * Nothing moves and the main thread is idle: the view that came into sight is read again
     * (ADR 0041). Returns whether it was, so the frame is read again.
     */
    private native boolean nativeIdle();

    /**
     * This view caught a bug in {@code feature}'s views, a FEATURE_* number, and stopped
     * drawing them: the Rust app stops the feature too (ARCH-09, ADR 0043).
     */
    private native void nativeFeatureFailed(int feature);

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
     * Settings → Appearance → Theme was set to System, Light or Dark, a value of {@link #THEMES}
     * (THEME-01). Returns null, or a short message to show when the choice couldn't be saved.
     */
    private native String nativeTheme(int theme);

    /**
     * Settings → Home → "Home button opens" was set to the last view, 0, or Applications, 1
     * (HOME-06). Returns null, or a short message to show when the choice couldn't be saved.
     */
    private native String nativeHomeTarget(int target);

    /**
     * Settings → Home → Wrap-around was switched (HOME-04). Returns null, or a short message to
     * show when the choice couldn't be saved.
     */
    private native String nativeWrapAround(boolean on);

    /**
     * Settings → Access → "Default home app" was tapped (HOME-01). Returns null, or a short
     * message to show when something went wrong; instructions show in Access instead.
     */
    private native String nativeDefaultHome();

    /** Settings → Access → "Notification access" was tapped (EVT-03), as nativeDefaultHome. */
    private native String nativeNotificationAccessSettings();

    /** Settings → Access → "Usage access" was tapped (REC-02), as nativeDefaultHome. */
    private native String nativeUsageAccessSettings();

    /** The written instructions below the Access item in {@link #ACCESS_INSTRUCTIONS}, or null. */
    private native String nativeAccessInstructions();

    /**
     * The regions Settings → Lock-screen image → Region offers, as BCP 47 tags: the one
     * Automatic stands for, then the feed's (WALL-05). Empty while it isn't offered.
     */
    private native String[] nativeRegions();

    /**
     * Region was set to Automatic, 0, or the region at that place in {@link #nativeRegions}
     * (WALL-05). Returns null, or a short message to show when the choice couldn't be saved.
     */
    private native String nativeRegion(int place);

    /**
     * Reset launcher was confirmed (SET-06). Returns null, or a short message to show when part
     * of it couldn't be saved.
     */
    private native String nativeResetLauncher();

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

    /**
     * "Daily lock-screen image" was switched (WALL-02, WALL-12). Returns null, or a short
     * message to show when it couldn't be done.
     */
    private native String nativeDailyImage(boolean on);

    /**
     * "Update now", or Resume, was tapped (WALL-11, WALL-14). Returns null, or a short message
     * to show when it couldn't start.
     */
    private native String nativeImageUpdateNow();

    /**
     * "Reset lock screen to home wallpaper" was tapped (WALL-12). Returns null once the lock
     * screen shows the home wallpaper, or a short message saying why it doesn't.
     */
    private native String nativeResetLockScreen();

    /**
     * "Wallpaper settings" was tapped (WALL-12). Returns null, or written instructions when the
     * system has no such screen.
     */
    private native String nativeOpenWallpaperSettings();

    /** Why the device can't set the lock-screen wallpaper, or null while it can (WALL-13). */
    private native String nativeImageUnavailableReason();

    /**
     * What the image details screen shows as of the last frame, in {@link ImageDetails#FIELDS}
     * fields (WALL-15). Empty while it doesn't show.
     */
    private native String[] nativeImageDetails();

    /**
     * The daily image's status line as of the last frame, in {@link
     * ImageSettings#STATUS_FIELDS} fields (WALL-16). Empty while it doesn't show.
     */
    private native String[] nativeImageStatus();

    /**
     * More info was tapped (WALL-15): the image's link opens in the browser. Returns null, or a
     * short message to show when it couldn't open.
     */
    private native String nativeImageMoreInfo();

    /** Settings on the image details screen was tapped: Settings opens over it. */
    private native void nativeImageDetailsSettings();

    /**
     * "Past week" on the details screen, or "Choose from the past week" in Settings, was tapped
     * (WALL-17): the past week's images open over it.
     */
    private native void nativeOpenPastImages();

    /**
     * What the past week's screen shows as of the last frame (WALL-17): two fields, then seven
     * for each image, as {@link PastImages} reads them. Empty while it doesn't show.
     */
    private native String[] nativePastImages();

    /**
     * The preview of the image at {@code index} as read at {@code revision}, a JPEG the Rust app
     * checked, or null (WALL-17, SEC-02).
     */
    private native byte[] nativePastImagePreview(int revision, int index);

    /**
     * Pin was tapped on the image at {@code index} as read at {@code revision} (WALL-17).
     * Returns null, or a short message to show when it couldn't start.
     */
    private native String nativePinImage(int revision, int index);

    /**
     * Unpin was tapped (WALL-17): today's image comes back, as "Update now" fetches it. Returns
     * null, or a short message to show when it couldn't start.
     */
    private native String nativeUnpinImage();

    /**
     * Try again was tapped on the past week's screen, after a problem (WALL-17). Returns null,
     * or a short message to show when it couldn't start.
     */
    private native String nativePastImagesTryAgain();

    /**
     * Continue was tapped in setup, on Welcome or the image screen, whose switch it applies
     * (ONB-01, WALL-02). Returns null, or a short message to show.
     */
    private native String nativeSetupContinue();

    /**
     * Not now was tapped in setup on {@code item}, numbered as {@link AccessSettings} does
     * (ONB-02). Returns null, or a short message to show.
     */
    private native String nativeSetupNotNow(int item);

    /**
     * Setup's image switch was tapped. Nothing is saved before Continue (WALL-02). Returns null,
     * or a short message to show.
     */
    private native String nativeSetupImageSwitch(boolean on);

    /** Setup's image screen as of the last frame, in SetupScreens' fields (ONB-01). */
    private native String[] nativeSetupImage();

    /** Today's preview on setup's image screen as read at {@code revision}, or null (SEC-02). */
    private native byte[] nativeSetupImagePreview(int revision);

    /**
     * The banner's Make default was tapped: the system's dialog opens (ONB-04). Returns null, or
     * a short message to show, such as written instructions.
     */
    private native String nativeHomeBannerMakeDefault();

    /** The banner was dismissed until tomorrow (ONB-04). Returns null, or a short message. */
    private native String nativeHomeBannerDismissed();

    /** Fills {@code frame} with what to draw now, in the slots above. */
    private native void nativeRead(int[] frame);
}
