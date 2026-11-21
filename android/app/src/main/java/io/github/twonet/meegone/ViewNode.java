package io.github.twonet.meegone;

import android.content.Context;
import android.os.Bundle;
import android.view.KeyEvent;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;

/**
 * A home view's own element (A11Y-05, A11Y-06, ADR 0039). It covers its page under the content
 * and draws nothing.
 *
 * <ul>
 *   <li><b>Reaching it:</b> screen readers come to it first in each view, or by touching empty
 *       space, and read its name, such as "Applications, 2 of 3". A keyboard's Tab reaches it
 *       first in each view, and the arrows switch views from it.
 *   <li><b>Its press:</b> a click or a long-click does what a long-press on empty space does
 *       there: the home menu in Events and Applications, or Edit Recent in Recent.
 *   <li><b>Its other actions:</b> they switch views, open search and change Recent's density.
 *       They are the ones a gesture has (HOME-03, SRCH-01, REC-05). Screen readers list them in
 *       their actions menu, and Switch Access and Voice Access offer them.
 * </ul>
 *
 * <p>The views' container carried these actions before. Screen readers never focus a
 * container, so TalkBack couldn't offer them. Touches pass through this element, so the home
 * views' own long-press stays the gesture's (ADR 0008). What it offers is the owner's, which
 * reads it from the Rust app's frame.
 */
final class ViewNode extends View {
    /** What the element of each view offers and does. */
    interface Owner {
        /** Names {@code view}'s element and adds the actions it offers now, its press first. */
        void describe(int view, AccessibilityNodeInfo info);

        /**
         * Does {@code action} for {@code view}: a click or a long-click is its press. Returns
         * whether anything happened, or false for an action it doesn't know.
         */
        boolean perform(int view, int action);
    }

    private final int view;
    private final Owner owner;

    ViewNode(Context context, int view, Owner owner) {
        super(context);
        this.view = view;
        this.owner = owner;
        // A keyboard's stop, and a screen reader's, but not a touch's.
        setFocusable(FOCUSABLE);
        setFocusableInTouchMode(false);
        setScreenReaderFocusable(true);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        setDefaultFocusHighlightEnabled(false);
    }

    /** The home view it belongs to: Events, Applications or Recent, from 0. */
    int view() {
        return view;
    }

    @Override
    public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
        super.onInitializeAccessibilityNodeInfo(info);
        owner.describe(view, info);
    }

    @Override
    public boolean performAccessibilityAction(int action, Bundle arguments) {
        return owner.perform(view, action) || super.performAccessibilityAction(action, arguments);
    }

    // Enter, Space and the D-pad's centre are its click. Menu and Shift+F10 are its long-click,
    // which the home views map for every element (A11Y-06).
    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        return confirms(keyCode) || super.onKeyDown(keyCode, event);
    }

    @Override
    public boolean onKeyUp(int keyCode, KeyEvent event) {
        if (confirms(keyCode)) {
            owner.perform(view, AccessibilityNodeInfo.ACTION_CLICK);
            return true;
        }
        return super.onKeyUp(keyCode, event);
    }

    /** Whether {@code keyCode} is a key that clicks, as Android's own views take it. */
    private static boolean confirms(int keyCode) {
        switch (keyCode) {
            case KeyEvent.KEYCODE_DPAD_CENTER:
            case KeyEvent.KEYCODE_ENTER:
            case KeyEvent.KEYCODE_NUMPAD_ENTER:
            case KeyEvent.KEYCODE_SPACE:
                return true;
            default:
                return false;
        }
    }
}
