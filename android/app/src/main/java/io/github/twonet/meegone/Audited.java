package io.github.twonet.meegone;

import android.graphics.Rect;
import java.util.List;

/**
 * A view that draws text or takes taps in places that aren't views of their own, such as an app
 * cell's label and the ✕ over its icon. The debug build's accessibility audit asks it for them,
 * to check their contrast and their size as it checks views (ADR 0038). Release builds never
 * ask.
 */
interface Audited {
    /** Adds the bounds of each text it draws, in its own coordinates. */
    void auditTexts(List<Rect> out);

    /** Adds each place a tap acts on that isn't a view, in its own coordinates. */
    void auditTargets(List<Rect> out);
}
