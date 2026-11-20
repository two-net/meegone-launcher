package io.github.twonet.meegone;

import android.content.Context;
import android.graphics.Insets;
import android.graphics.Rect;
import android.graphics.drawable.GradientDrawable;
import android.os.Build;
import android.text.Editable;
import android.text.InputType;
import android.text.TextUtils;
import android.text.TextWatcher;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.function.Consumer;

/**
 * App search (SRCH-01 to SRCH-05, ADR 0016): a field in a pill at the top of the Applications
 * page, search's results in their own {@link AppsGrid} below it, and what shows when nothing
 * matches.
 *
 * <p>The field sits in a band below the safe area's top. It shows as far as the frame says the
 * band is pulled down, sliding in and fading with the pull (ANIM-16); the Applications grid below
 * moves down with it. While the field is out of sight this view is invisible, but still laid out,
 * so the results grid reports where the band is. Each change of the field's text goes to the
 * Rust app at once (SRCH-02), and the keyboard's action key, or a hardware Enter, launches the
 * first result or searches the web (SRCH-03, SRCH-05).
 *
 * <p>The field owns its text while the user types. It takes the keyboard when search opens, and
 * lets it go and empties when search closes. Only after a configuration change, such as a new
 * density, made a new one does it take the text back from the Rust app; a theme change keeps the
 * field as it is (THEME-06, ADR 0033). The results scroll above the keyboard: their
 * grid's bottom padding is the larger of the safe area and the keyboard.
 */
final class SearchPanel extends FrameLayout {
    private static final float FIELD_DP = 48f;
    /** Room above and below the field, inside its band. */
    private static final float GAP_DP = 8f;
    /** The field's sides, from the safe area's, as the grid's cells. */
    private static final float MARGIN_DP = 16f;
    private static final float TEXT_SP = 16f;
    private static final float NONE_SP = 16f;
    /** How far below the band the empty result's text starts. */
    private static final float NONE_TOP_DP = 24f;

    private final EditText field;
    private final GradientDrawable pill;
    private final AppsGrid results;
    /** What shows when nothing matches: a line saying so and, if offered, a web search. */
    private final LinearLayout nothing;
    private final TextView none;
    private final TextView web;
    private final int gap;
    private final int margin;
    private final int noneTop;
    /** The band, from the safe area's top to just below the field, from this view's top. */
    private int bandTop;
    private int bandBottom;
    /** The band on screen, as reported to the Rust app with the results grid's layout. */
    private final Rect band = new Rect();
    private Insets safe = Insets.NONE;
    private int keyboard;
    private View screen;
    /** The field's text is being set from here, not typed: it isn't sent. */
    private boolean muted;
    private int foreground = -1;
    private int dim = -1;
    private int surface = -1;
    private int accent = -1;

    /**
     * {@code results} is search's own grid. {@code changed} takes the field's text after each
     * change, {@code submitted} runs on the action key and {@code webSearched} on "Search the
     * web".
     */
    SearchPanel(Context context, AppsGrid results, Consumer<String> changed, Runnable submitted,
            Runnable webSearched) {
        super(context);
        this.results = results;
        float density = getResources().getDisplayMetrics().density;
        gap = Math.round(GAP_DP * density);
        margin = Math.round(MARGIN_DP * density);
        noneTop = Math.round(NONE_TOP_DP * density);
        setVisibility(INVISIBLE);
        setClipToPadding(false);

        addView(results, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        results.setVisibility(INVISIBLE);

        nothing = new LinearLayout(context);
        nothing.setOrientation(LinearLayout.VERTICAL);
        nothing.setGravity(Gravity.CENTER_HORIZONTAL);
        nothing.setVisibility(GONE);
        none = new TextView(context);
        none.setText(R.string.search_none);
        none.setTextSize(TypedValue.COMPLEX_UNIT_SP, NONE_SP);
        none.setGravity(Gravity.CENTER);
        // Read out as it appears (A11Y-08).
        none.setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);
        nothing.addView(none, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        web = new TextView(context);
        web.setTextSize(TypedValue.COMPLEX_UNIT_SP, NONE_SP);
        web.setGravity(Gravity.CENTER);
        web.setSingleLine(true);
        web.setEllipsize(TextUtils.TruncateAt.MIDDLE);
        web.setMinHeight(Math.round(FIELD_DP * density));
        web.setPadding(margin, 0, margin, 0);
        web.setOnClickListener(view -> webSearched.run());
        LinearLayout.LayoutParams webParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT);
        webParams.topMargin = gap;
        nothing.addView(web, webParams);
        addView(nothing, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT,
                Gravity.TOP));

        field = new EditText(context);
        field.setSingleLine(true);
        field.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_FILTER
                | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
        field.setImeOptions(EditorInfo.IME_ACTION_GO | EditorInfo.IME_FLAG_NO_EXTRACT_UI);
        field.setHint(R.string.search_hint);
        field.setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP);
        field.setMinHeight(Math.round(FIELD_DP * density));
        field.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
        field.setPaddingRelative(margin, 0, margin, 0);
        pill = new GradientDrawable();
        pill.setCornerRadius(FIELD_DP * density / 2f);
        field.setBackground(pill);
        field.setVisibility(INVISIBLE);
        field.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence text, int start, int count, int after) {}

            @Override
            public void onTextChanged(CharSequence text, int start, int before, int count) {}

            @Override
            public void afterTextChanged(Editable text) {
                if (!muted) {
                    changed.accept(text.toString());
                }
            }
        });
        field.setOnEditorActionListener((view, action, event) -> {
            boolean enter = event != null && event.getKeyCode() == KeyEvent.KEYCODE_ENTER;
            if (action == EditorInfo.IME_ACTION_GO || enter) {
                // A hardware Enter sends a down and an up; only the down submits (A11Y-06).
                if (event == null || event.getAction() == KeyEvent.ACTION_DOWN) {
                    submitted.run();
                }
                return true;
            }
            return false;
        });
        addView(field, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT,
                Gravity.TOP));
    }

    /** The view the screen's coordinates start from: the home views. */
    void setScreen(View screen) {
        this.screen = screen;
    }

    /**
     * Keeps the field inside the safe area (HOME-11), and the results above the keyboard, which
     * is {@code keyboard} px tall, or 0 while it is hidden.
     */
    void setInsets(Insets safe, int keyboard) {
        if (safe.equals(this.safe) && keyboard == this.keyboard) {
            return;
        }
        this.safe = safe;
        this.keyboard = keyboard;
        LayoutParams params = (LayoutParams) field.getLayoutParams();
        params.topMargin = safe.top + gap;
        params.leftMargin = safe.left + margin;
        params.rightMargin = safe.right + margin;
        field.setLayoutParams(params);
        LayoutParams nothingParams = (LayoutParams) nothing.getLayoutParams();
        nothingParams.leftMargin = safe.left + margin;
        nothingParams.rightMargin = safe.right + margin;
        nothing.setLayoutParams(nothingParams);
        padResults();
    }

    /** The results start below the band, and end above the bars or the keyboard. */
    private void padResults() {
        results.setSafeArea(Insets.of(safe.left, bandBottom > 0 ? bandBottom : safe.top,
                safe.right, Math.max(safe.bottom, keyboard)));
    }

    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        super.onLayout(changed, left, top, right, bottom);
        int newTop = safe.top;
        int newBottom = field.getBottom() + gap;
        if (newTop != bandTop || newBottom != bandBottom) {
            bandTop = newTop;
            bandBottom = newBottom;
            padResults();
            LayoutParams params = (LayoutParams) nothing.getLayoutParams();
            params.topMargin = bandBottom + noneTop;
            nothing.setLayoutParams(params);
        }
        // The Rust app measures the pull against the band (ANIM-16).
        int[] at = AppsGrid.offsetIn(this, screen);
        band.set(at[0], at[1] + bandTop, at[0] + right - left, at[1] + bandBottom);
        results.setPanel(band);
    }

    /** How tall the band is, in px: how far the field shows when search is open. */
    private int bandHeight() {
        return Math.max(1, bandBottom - bandTop);
    }

    /**
     * Draws the frame's search: {@code open} or not, the band showing {@code reveal} px, the
     * results or the grid below, and, when {@code nothingFound}, that nothing matches, with a
     * web search if {@code webOffered} (SRCH-05).
     */
    void apply(boolean open, int reveal, boolean resultsShown, boolean nothingFound,
            boolean webOffered, int foreground, int dim, int surface, int accent) {
        colour(foreground, dim, surface, accent);
        // Screen readers announce search once it is open, not while it is being pulled
        // (A11Y-08).
        CharSequence pane = open ? getResources().getString(R.string.search_action) : null;
        if (!TextUtils.equals(pane, getAccessibilityPaneTitle())) {
            setAccessibilityPaneTitle(pane);
        }
        boolean shown = open || reveal > 0;
        setVisibility(shown ? VISIBLE : INVISIBLE);
        field.setVisibility(shown ? VISIBLE : INVISIBLE);
        float height = bandHeight();
        field.setTranslationY(reveal - height);
        field.setAlpha(Math.max(0f, Math.min(1f, reveal / height)));
        results.setVisibility(resultsShown ? VISIBLE : INVISIBLE);
        nothing.setVisibility(nothingFound ? VISIBLE : GONE);
        web.setVisibility(webOffered ? VISIBLE : GONE);
        if (webOffered) {
            String query = field.getText().toString().trim();
            String text = getResources().getString(R.string.search_web, query);
            if (!text.contentEquals(web.getText())) {
                web.setText(text);
            }
        }
    }

    private void colour(int foreground, int dim, int surface, int accent) {
        if (foreground == this.foreground && dim == this.dim && surface == this.surface
                && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.surface = surface;
        this.accent = accent;
        pill.setColor(surface);
        field.setTextColor(foreground);
        field.setHintTextColor(dim);
        Themed.textField(field, accent);
        none.setTextColor(dim);
        web.setTextColor(accent);
        web.setBackground(SettingsRows.ripple(foreground));
    }

    /**
     * Search opened: the field takes the keyboard (SRCH-01). {@code text}, if not null, is what
     * it says, as after a configuration change made this view anew.
     */
    void focus(String text) {
        if (text != null && !text.contentEquals(field.getText())) {
            muted = true;
            field.setText(text);
            field.setSelection(field.length());
            muted = false;
        }
        field.requestFocus();
        // Once the field is attached and focused, the keyboard rises.
        field.post(this::showKeyboard);
    }

    private void showKeyboard() {
        if (!field.hasFocus()) {
            return;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = field.getWindowInsetsController();
            if (controller != null) {
                controller.show(WindowInsets.Type.ime());
                return;
            }
        }
        InputMethodManager keyboards = getContext().getSystemService(InputMethodManager.class);
        if (keyboards != null) {
            keyboards.showSoftInput(field, InputMethodManager.SHOW_IMPLICIT);
        }
    }

    /** Search closed: the keyboard goes, and the field empties without telling anyone. */
    void dismiss() {
        InputMethodManager keyboards = getContext().getSystemService(InputMethodManager.class);
        if (keyboards != null) {
            keyboards.hideSoftInputFromWindow(field.getWindowToken(), 0);
        }
        field.clearFocus();
        muted = true;
        field.setText("");
        muted = false;
    }

    /** The first result's icon on the screen, which a launch grows from, or an empty rect. */
    Rect firstIcon() {
        AppCell first = results.cellAt(0);
        return first != null && first.isShown() ? first.iconOnScreen() : new Rect();
    }

    /** The field, for the debug build's accessibility audit (ADR 0038). */
    EditText auditField() {
        return field;
    }
}
