package io.github.twonet.meegone;

import android.app.AlertDialog;
import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.InsetDrawable;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.widget.Button;
import android.widget.CheckedTextView;
import android.widget.EditText;
import android.widget.ListView;
import android.widget.TextView;

/**
 * MeeGone's dialogs: the folder's rename, the region, the licences and Reset launcher's
 * question. Each is laid out by the device's light or dark dialog theme, matched to MeeGone's,
 * and drawn in MeeGone's own colours (THEME-03): a 12 dp card in the surface colour over the
 * scrim, its title, message, list and field in the text colour, and its buttons in the accent,
 * each at least 48 dp tall (A11Y-02). Every pairing is then one of the tokens' that the Rust
 * app's tests hold to their contrast, whatever the device's dialogs look like (A11Y-03,
 * ADR 0038).
 */
final class Dialogs {
    private static final float CORNER_DP = 12f;
    /** How far the card stays from the screen's edges, as the platform's dialogs do. */
    private static final float INSET_DP = 16f;

    private Dialogs() {}

    /** A dialog in the device's light or dark dialog theme, as {@code light} says. */
    static AlertDialog.Builder builder(Context context, boolean light) {
        return new AlertDialog.Builder(context, light
                ? android.R.style.Theme_DeviceDefault_Light_Dialog_Alert
                : android.R.style.Theme_DeviceDefault_Dialog_Alert);
    }

    /**
     * Draws {@code dialog}, already shown, in the theme's colours, with the scrim behind it at
     * {@code scrim}, 0 to 255.
     */
    static void colour(AlertDialog dialog, int surface, int foreground, int dim, int accent,
            int scrim) {
        Window window = dialog.getWindow();
        if (window == null) {
            return;
        }
        float density = dialog.getContext().getResources().getDisplayMetrics().density;
        GradientDrawable card = new GradientDrawable();
        card.setColor(surface);
        card.setCornerRadius(CORNER_DP * density);
        window.setBackgroundDrawable(new InsetDrawable(card, Math.round(INSET_DP * density)));
        window.setDimAmount(scrim / 255f);
        int titleId = dialog.getContext().getResources()
                .getIdentifier("alertTitle", "id", "android");
        if (titleId != 0) {
            View title = window.findViewById(titleId);
            if (title instanceof TextView) {
                ((TextView) title).setTextColor(foreground);
            }
        }
        View message = window.findViewById(android.R.id.message);
        if (message instanceof TextView) {
            ((TextView) message).setTextColor(foreground);
        }
        for (int which : new int[] {
            AlertDialog.BUTTON_POSITIVE, AlertDialog.BUTTON_NEGATIVE, AlertDialog.BUTTON_NEUTRAL,
        }) {
            Button button = dialog.getButton(which);
            if (button != null) {
                button.setTextColor(accent);
                button.setBackground(SettingsRows.ripple(foreground));
                // Some devices' dialog buttons are shorter (A11Y-02).
                button.setMinHeight(A11y.targetPx(button));
                button.setMinimumHeight(A11y.targetPx(button));
                button.setMinWidth(A11y.targetPx(button));
                button.setMinimumWidth(A11y.targetPx(button));
            }
        }
        ListView list = dialog.getListView();
        if (list != null) {
            list.setSelector(SettingsRows.ripple(foreground));
            for (int i = 0; i < list.getChildCount(); i++) {
                colourItem(list.getChildAt(i), foreground, dim, accent);
            }
            // Rows the list adds as it scrolls.
            list.setOnHierarchyChangeListener(new ViewGroup.OnHierarchyChangeListener() {
                @Override
                public void onChildViewAdded(View parent, View child) {
                    colourItem(child, foreground, dim, accent);
                }

                @Override
                public void onChildViewRemoved(View parent, View child) {}
            });
            Themed.scrolling(list, dim);
        }
    }

    /** One text of the dialog's own, such as the licences, in the text colour. */
    static void colourText(TextView text, int foreground) {
        text.setTextColor(foreground);
    }

    /** A field in the dialog: its text, its hint in the secondary colour, and the accent. */
    static void colourField(EditText field, int foreground, int dim, int accent) {
        field.setTextColor(foreground);
        field.setHintTextColor(dim);
        field.setBackgroundTintList(ColorStateList.valueOf(accent));
        Themed.textField(field, accent);
    }

    /** A choice in a list: its text, and its radio button in the accent when chosen. */
    private static void colourItem(View item, int foreground, int dim, int accent) {
        if (item instanceof TextView) {
            ((TextView) item).setTextColor(foreground);
        }
        if (item instanceof CheckedTextView) {
            int[][] states = {{android.R.attr.state_checked}, {}};
            ((CheckedTextView) item).setCheckMarkTintList(
                    new ColorStateList(states, new int[] {accent, dim}));
        }
    }
}
