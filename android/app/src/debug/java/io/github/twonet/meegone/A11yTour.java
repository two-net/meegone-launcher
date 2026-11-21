package io.github.twonet.meegone;

import android.app.AlertDialog;
import android.os.Build;
import android.util.Log;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.widget.CompoundButton;
import android.widget.EditText;
import android.widget.ScrollView;
import android.widget.TextView;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.List;

/**
 * Debug builds only: goes through every screen of MeeGone in the Light theme, then the Dark one,
 * and has {@link A11yAudit} check each as it is drawn (#37, ADR 0038). It gets there as a screen
 * reader would, through each view's own element and the elements' own actions (A11Y-05,
 * ADR 0039), and waits for everything to come to rest before each check. Last, in each theme,
 * it leaves touch mode as a keyboard does and checks the focus ring on an app and in a menu
 * (A11Y-06). The home stays out of touch mode until the next touch.
 *
 * <p>Every scrolling screen is checked a page at a time to its end. Each dialog is cancelled,
 * Reset launcher's included. The theme and Recent's density are put back as they were found, so
 * the tour changes nothing it doesn't restore. Setup is checked only while it shows, on a first
 * run; then the tour answers it Continue and Not now, with the daily image off.
 *
 * <p>It logs {@code START}, any {@code FAIL} and {@code SKIP} lines, and {@code DONE screens=…
 * checked=… failed=…} under {@link A11yAudit#TAG}; {@code tools/a11y-audit.sh} reads them.
 */
final class A11yTour {
    /** Settings → Appearance → Theme's values, as {@code HomeViews.THEMES}. */
    private static final int LIGHT = 1;
    private static final int DARK = 2;
    private static final int EVENTS = 0;
    private static final int APPLICATIONS = 1;
    private static final int RECENT = 2;
    private static final long POLL_MS = 50;
    /** After the motion stops: icons decoding, a dialog's window drawing, a pane title read. */
    private static final long QUIET_MS = 500;
    /** The longest the tour waits for anything, such as the past week's previews. */
    private static final long PATIENCE_MS = 8000;
    private static final int MAX_PAGES = 12;
    private static final int MAX_GRID_PAGES = 3;

    private static boolean running;

    private final HomeActivity activity;
    private final HomeViews views;
    private final A11yAudit audit;
    private final Deque<Step> steps = new ArrayDeque<>();
    private String theme = "Found";
    private int screens;

    /** One thing the tour does, which calls {@code next} when it is done. */
    private interface Step {
        void run(Runnable next);
    }

    private A11yTour(HomeActivity activity, HomeViews views) {
        this.activity = activity;
        this.views = views;
        audit = new A11yAudit(activity.getResources().getDisplayMetrics().density);
    }

    /** Starts the tour once the home views have settled, unless one is running. */
    static void start(HomeActivity activity, HomeViews views) {
        if (running) {
            return;
        }
        running = true;
        A11yTour tour = new A11yTour(activity, views);
        A11yAudit.later(2 * QUIET_MS, tour::begin);
    }

    private void begin() {
        float scale = activity.getResources().getConfiguration().fontScale;
        Log.i(A11yAudit.TAG, "START font scale " + scale + ", animations "
                + (Scroll.animating() ? "on" : "removed") + ", Android " + Build.VERSION.SDK_INT);
        int themeFound = views.auditTheme();
        int rowsFound = views.auditRecentRows();
        if (views.auditSetupStep() >= 0) {
            steps.add(this::setup);
        }
        for (int each : new int[] {LIGHT, DARK}) {
            queueTheme(each);
        }
        for (int each : new int[] {LIGHT, DARK}) {
            queueKeyboard(each);
        }
        steps.add(act(() -> {
            if (themeFound >= 0) {
                views.auditSetTheme(themeFound);
            }
        }));
        steps.add(this::closeAll);
        steps.add(next -> {
            if (views.auditRecentRows() != rowsFound) {
                goTo(RECENT, () -> {
                    viewAction(rowsFound == 2
                            ? R.id.action_bigger_cards : R.id.action_more_cards);
                    settle(next);
                });
            } else {
                next.run();
            }
        });
        steps.add(next -> goTo(APPLICATIONS, next));
        steps.add(next -> {
            Log.i(A11yAudit.TAG, "DONE screens=" + screens + " checked=" + audit.checked()
                    + " failed=" + audit.failed());
            running = false;
            next.run();
        });
        runNext();
    }

    private void queueTheme(int chosen) {
        String name = chosen == LIGHT ? "Light" : "Dark";
        steps.add(act(() -> {
            theme = name;
            views.auditSetTheme(chosen);
        }));
        steps.add(this::closeAll);

        // Events, and a notification's menu.
        steps.add(next -> goTo(EVENTS, next));
        steps.add(pages("Events", MAX_PAGES));
        steps.add(next -> {
            View row = views.auditEvents().rowAt(0);
            if (row == null) {
                skip("Notification menu", "no notifications");
                next.run();
                return;
            }
            row.performAccessibilityAction(AccessibilityNodeInfo.ACTION_LONG_CLICK, null);
            settle(() -> check("Notification menu", () -> closeAll(next)));
        });

        // Applications: the grid, the home menu, an app's menu, edit mode, a folder, search.
        steps.add(next -> goTo(APPLICATIONS, next));
        steps.add(pages("Applications", MAX_GRID_PAGES));
        steps.add(next -> {
            viewAction(AccessibilityNodeInfo.ACTION_CLICK);
            settle(() -> check("Home menu", () -> closeAll(next)));
        });
        steps.add(next -> {
            AppCell app = cell(false);
            if (app == null) {
                skip("App menu", "no apps");
                next.run();
                return;
            }
            app.performAccessibilityAction(AccessibilityNodeInfo.ACTION_LONG_CLICK, null);
            settle(() -> check("App menu", () -> closeAll(next)));
        });
        steps.add(next -> {
            viewAction(AccessibilityNodeInfo.ACTION_CLICK);
            settle(() -> {
                click(R.string.menu_edit_applications);
                settle(() -> check("Edit mode", () -> {
                    AppCell folder = cell(true);
                    if (folder == null) {
                        skip("Folder in edit mode", "no folder");
                        closeAll(next);
                        return;
                    }
                    folder.performClick();
                    settle(() -> check("Folder in edit mode", () -> closeAll(next)));
                }));
            });
        });
        steps.add(next -> {
            AppCell folder = cell(true);
            if (folder == null) {
                skip("Folder", "no folder");
                next.run();
                return;
            }
            folder.performClick();
            settle(() -> check("Folder", () -> {
                views.auditFolderName().performClick();
                settle(() -> check("Rename dialog", () -> closeAll(next)));
            }));
        });
        steps.add(next -> {
            viewAction(R.id.action_search);
            settle(() -> {
                type("a");
                settle(() -> check("Search results", () -> {
                    type("zqxjvkzqxj");
                    settle(() -> check("Search, nothing found", () -> closeAll(next)));
                }));
            });
        });

        // Recent: the cards, the other density, close mode.
        steps.add(next -> goTo(RECENT, next));
        steps.add(pages("Recent", MAX_PAGES));
        steps.add(next -> {
            boolean bigger = views.auditRecentRows() != 2;
            if (!viewAction(bigger ? R.id.action_bigger_cards : R.id.action_more_cards)) {
                skip("Recent, other density", "not offered");
                next.run();
                return;
            }
            settle(() -> check(bigger ? "Recent, bigger cards" : "Recent, more cards", () -> {
                viewAction(bigger ? R.id.action_more_cards : R.id.action_bigger_cards);
                settle(next);
            }));
        });
        steps.add(next -> {
            // Recent's element: its press is Edit Recent.
            if (!viewAction(AccessibilityNodeInfo.ACTION_CLICK)) {
                skip("Recent close mode", "not offered");
                next.run();
                return;
            }
            settle(() -> check("Recent close mode", () -> closeAll(next)));
        });

        // Settings and its dialogs, never confirming Reset launcher.
        steps.add(next -> goTo(APPLICATIONS, next));
        steps.add(next -> {
            viewAction(AccessibilityNodeInfo.ACTION_CLICK);
            settle(() -> {
                click(R.string.menu_launcher_settings);
                settle(next);
            });
        });
        steps.add(pages("Settings", MAX_PAGES));
        steps.add(dialog("Region dialog", R.string.image_region));
        steps.add(dialog("Licences dialog", R.string.about_licences));
        steps.add(dialog("Reset launcher dialog", R.string.reset_launcher));
        steps.add(this::closeAll);

        // The image details screen and the past week, once there is an image.
        steps.add(next -> {
            viewAction(AccessibilityNodeInfo.ACTION_CLICK);
            settle(() -> {
                click(R.string.menu_lock_screen_image);
                settle(() -> {
                    if (!views.auditImageDetails()) {
                        skip("Image details", "no image yet");
                        closeAll(next);
                        return;
                    }
                    pages("Image details", MAX_PAGES).run(() -> {
                        click(R.string.image_past_week);
                        // The previews come from the network.
                        A11yAudit.later(PATIENCE_MS / 2, () -> settle(() -> {
                            if (!views.auditPastImages()) {
                                skip("Past week", "didn't open");
                                closeAll(next);
                                return;
                            }
                            pages("Past week", MAX_PAGES).run(() -> closeAll(next));
                        }));
                    });
                });
            });
        });
    }

    /**
     * In theme {@code chosen}, a keyboard's focus on an app of the grid, or on Applications' own
     * element, then in the home menu, where it goes to the first item; the ring is checked each
     * time (A11Y-06, ADR 0039).
     */
    private void queueKeyboard(int chosen) {
        String name = chosen == LIGHT ? "Light" : "Dark";
        steps.add(act(() -> {
            theme = name;
            views.auditSetTheme(chosen);
        }));
        steps.add(this::closeAll);
        steps.add(next -> goTo(APPLICATIONS, next));
        steps.add(next -> {
            AppCell app = cell(false);
            View focused = app != null ? app : views.auditNode();
            focused.requestFocusFromTouch();
            settle(() -> ring("Focus ring", () -> {
                viewAction(AccessibilityNodeInfo.ACTION_CLICK);
                settle(() -> ring("Focus ring in the home menu", () -> closeAll(next)));
            }));
        });
    }

    /** Checks the focus ring where it is drawn. */
    private void ring(String screen, Runnable next) {
        screens++;
        audit.checkRing(activity.getWindow(), activity.getWindow().getDecorView(),
                views.auditRing(), screen, theme, next);
    }

    /**
     * Does {@code action} on the shown view's own element, as a screen reader does from its
     * actions menu. Returns whether it did anything.
     */
    private boolean viewAction(int action) {
        return views.auditNode().performAccessibilityAction(action, null);
    }

    /**
     * Setup's step on screen, in both themes; then its answer, Continue or Not now, with the
     * daily image off so nothing is downloaded, and the next step, until setup ends.
     */
    private void setup(Runnable next) {
        int step = views.auditSetupStep();
        if (step < 0) {
            next.run();
            return;
        }
        String screen = "Setup step " + (step + 1);
        theme = "Light";
        views.auditSetTheme(LIGHT);
        settle(() -> check(screen, () -> {
            theme = "Dark";
            views.auditSetTheme(DARK);
            settle(() -> check(screen, () -> {
                if (step == SetupScreens.LOCK_SCREEN_IMAGE) {
                    List<View> on = new ArrayList<>();
                    A11yAudit.each(activity.getWindow().getDecorView(),
                            view -> view instanceof CompoundButton && view.isShown()
                                    && ((CompoundButton) view).isChecked(),
                            on::add);
                    for (View toggle : on) {
                        toggle.performClick();
                    }
                }
                boolean answered = click(R.string.setup_not_now) || click(R.string.setup_continue);
                if (!answered) {
                    skip(screen, "no answer to give");
                    next.run();
                    return;
                }
                settle(() -> setup(next));
            }));
        }));
    }

    /** Opens the dialog the Settings item named {@code item} opens, checks it, and cancels it. */
    private Step dialog(String screen, int item) {
        return next -> {
            if (!click(item)) {
                skip(screen, "not offered");
                next.run();
                return;
            }
            settle(() -> check(screen, () -> {
                AlertDialog shown = views.auditDialog();
                if (shown != null) {
                    shown.cancel();
                }
                settle(next);
            }));
        };
    }

    /**
     * Checks the screen as it shows, then a page further down at a time, to its end or at most
     * {@code most} pages, and goes back to its top.
     */
    private Step pages(String screen, int most) {
        return next -> page(screen, 1, most, () -> {
            ScrollView scroll = views.auditScroll();
            if (scroll != null) {
                scroll.scrollTo(0, 0);
            }
            settle(next);
        });
    }

    private void page(String screen, int at, int most, Runnable done) {
        check(at == 1 ? screen : screen + ", page " + at, () -> {
            ScrollView scroll = views.auditScroll();
            if (at >= most || scroll == null || !scroll.canScrollVertically(1)) {
                done.run();
                return;
            }
            int height = scroll.getHeight() - scroll.getPaddingTop() - scroll.getPaddingBottom();
            scroll.scrollBy(0, Math.max(1, height * 3 / 4));
            settle(() -> page(screen, at + 1, most, done));
        });
    }

    /** Checks what shows: the dialog over the home views if there is one, or the home views. */
    private void check(String screen, Runnable next) {
        screens++;
        AlertDialog dialog = views.auditDialog();
        if (dialog != null && dialog.isShowing() && dialog.getWindow() != null) {
            audit.check(dialog.getWindow(), dialog.getWindow().getDecorView(), true, screen,
                    theme, next);
        } else {
            audit.check(activity.getWindow(), activity.getWindow().getDecorView(), false,
                    views.auditFocusLayers(), screen, theme, next);
        }
    }

    /** Closes everything open, as Back does, one at a time. */
    private void closeAll(Runnable next) {
        closeAll(next, 0);
    }

    private void closeAll(Runnable next, int tries) {
        AlertDialog dialog = views.auditDialog();
        if (dialog != null) {
            dialog.cancel();
            settle(() -> closeAll(next, tries + 1));
        } else if (views.auditOpen() && tries < 12) {
            back();
            settle(() -> closeAll(next, tries + 1));
        } else {
            next.run();
        }
    }

    /** Shows home view {@code view}, a screen reader's swipe at a time. */
    private void goTo(int view, Runnable next) {
        goTo(view, next, 0);
    }

    private void goTo(int view, Runnable next, int tries) {
        int current = views.auditView();
        if (current == view || tries > 4) {
            next.run();
            return;
        }
        viewAction(view > current ? R.id.action_next_view : R.id.action_previous_view);
        settle(() -> goTo(view, next, tries + 1));
    }

    /**
     * Taps the shown element whose text is {@code text}, or the element it is in that takes
     * taps. Returns whether there was one.
     */
    private boolean click(int text) {
        String wanted = activity.getString(text);
        List<View> found = new ArrayList<>();
        View root = views.auditDialog() != null
                ? views.auditDialog().getWindow().getDecorView()
                : activity.getWindow().getDecorView();
        A11yAudit.each(root, view -> view instanceof TextView && view.isShown()
                && wanted.contentEquals(((TextView) view).getText()), found::add);
        for (View view : found) {
            for (View at = view; at != null;
                    at = at.getParent() instanceof View ? (View) at.getParent() : null) {
                if (at.isClickable() && at.isEnabled()) {
                    return at.performClick();
                }
            }
        }
        return false;
    }

    /** Types {@code query} into search's field, as a keyboard would. */
    private void type(String query) {
        EditText field = views.auditSearchField();
        field.setText(query);
        field.setSelection(query.length());
    }

    /** The first cell of the grid that is a folder, or an app, or null. */
    private AppCell cell(boolean folder) {
        AppsGrid grid = views.auditApps();
        for (int i = 0; i < grid.cellCount(); i++) {
            AppCell cell = grid.cellAt(i);
            if (cell != null && cell.isFolder() == folder) {
                return cell;
            }
        }
        return null;
    }

    @SuppressWarnings("deprecation") // The activity's Back, which the Rust app handles.
    private void back() {
        activity.onBackPressed();
    }

    private Step act(Runnable action) {
        return next -> {
            action.run();
            settle(next);
        };
    }

    private void skip(String screen, String why) {
        Log.i(A11yAudit.TAG, "SKIP " + screen + " " + theme + " " + why);
    }

    private void runNext() {
        Step step = steps.poll();
        if (step != null) {
            step.run(this::runNext);
        }
    }

    /**
     * Waits until nothing moves and no layout is pending, then a little longer for what draws
     * after, and runs {@code then}; or, past {@link #PATIENCE_MS}, runs it anyway.
     */
    private void settle(Runnable then) {
        settle(then, 0);
    }

    private void settle(Runnable then, long waited) {
        A11yAudit.later(POLL_MS, () -> {
            boolean still = views.auditSettled() && !views.isLayoutRequested();
            if (still || waited > PATIENCE_MS) {
                A11yAudit.later(QUIET_MS, then);
            } else {
                settle(then, waited + POLL_MS);
            }
        });
    }
}
