package io.github.twonet.meegone;

import android.content.res.Resources;
import android.os.Build;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction;

/**
 * What every view shares to be named and reached the same way (ADR 0038): the smallest touch
 * target (A11Y-02), an app's name as screen readers read it, a state that Android 10 reads too,
 * and the containers that take touches only so that none goes through them (A11Y-01).
 */
final class A11y {
    /** The smallest a touch target gets, each way (A11Y-02). */
    static final float TARGET_DP = 48f;

    private A11y() {}

    /** {@link #TARGET_DP} in px. */
    static int targetPx(View view) {
        return Math.round(TARGET_DP * view.getResources().getDisplayMetrics().density);
    }

    /**
     * An app's name as screen readers read it, wherever it is named: its label, with "work" for
     * a work-profile app, e.g. "Mail, work" (A11Y-01).
     */
    static String appName(Resources res, String label, boolean work) {
        return work ? res.getString(R.string.app_work, label) : label;
    }

    /**
     * What screen readers read for {@code view}: {@code description}, then {@code state} unless
     * it is null, such as "Unavailable" or "Expanded". Android 10 has no state description, so
     * there the state follows the description (A11Y-01).
     */
    static void describe(View view, CharSequence description, CharSequence state) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            view.setContentDescription(description);
            view.setStateDescription(state);
        } else {
            view.setContentDescription(state == null
                    ? description
                    : view.getResources().getString(R.string.described_state, description, state));
        }
    }

    /**
     * {@code view} is clickable only so that touches on it stay there, and the home views don't
     * take its empty space for theirs (ADR 0008). Screen readers don't offer it as a button,
     * which would say "double-tap to activate" over a whole screen (A11Y-01).
     */
    static void notAButton(View view) {
        view.setAccessibilityDelegate(new View.AccessibilityDelegate() {
            @Override
            public void onInitializeAccessibilityNodeInfo(View host,
                    AccessibilityNodeInfo info) {
                super.onInitializeAccessibilityNodeInfo(host, info);
                info.setClickable(false);
                info.removeAction(AccessibilityAction.ACTION_CLICK);
            }
        });
    }
}
