package io.github.twonet.meegone;

import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.drawable.BitmapDrawable;
import android.util.Log;
import android.view.ViewGroup;

/**
 * The whole screen cross-fading from one theme to the other (THEME-06, ANIM-19, ADR 0033).
 *
 * <p>When a frame brings a new theme's colours while the Rust app says the fade has just
 * started, {@link HomeViews} calls {@link #capture} before it changes anything, while the
 * screen still shows the old theme. The picture of it is drawn above everything in the view's
 * overlay, which takes no touches, so the new screen can be used at once (ANIM-03). Each frame
 * then fades it out as far as the Rust app says, and it goes once the fade ends. Every view
 * keeps its place and state underneath: nothing is made again.
 *
 * <p>The system bars and a dialog are other windows: they switch at once. If there is no
 * memory for the picture, the change is just instant.
 */
final class ThemeFade {
    private static final String TAG = "MeeGone";
    /** The fade's progress at rest, in ten-thousandths. */
    private static final int DONE = 10_000;

    private final ViewGroup screen;
    private BitmapDrawable picture;

    ThemeFade(ViewGroup screen) {
        this.screen = screen;
    }

    /**
     * Takes a picture of the screen as it is drawn now, in the old theme, to fade out over the
     * new one. A fade still playing is caught where it is: the new picture has it in.
     */
    void capture() {
        int width = screen.getWidth();
        int height = screen.getHeight();
        if (width <= 0 || height <= 0) {
            return;
        }
        Bitmap bitmap;
        try {
            bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
            screen.draw(new Canvas(bitmap));
        } catch (OutOfMemoryError | RuntimeException e) {
            Log.w(TAG, "No theme fade: " + e.getClass().getSimpleName());
            return;
        }
        clear();
        picture = new BitmapDrawable(screen.getResources(), bitmap);
        picture.setBounds(0, 0, width, height);
        screen.getOverlay().add(picture);
    }

    /** Fades the old screen out as far as {@code progress}, 0 to 10 000, says. */
    void fade(int progress) {
        if (picture == null) {
            return;
        }
        if (progress >= DONE) {
            clear();
        } else {
            picture.setAlpha(255 * (DONE - Math.max(progress, 0)) / DONE);
        }
    }

    /** Lets the picture go, such as when the fade ends or the screen is taken away. */
    void clear() {
        if (picture == null) {
            return;
        }
        screen.getOverlay().remove(picture);
        picture.getBitmap().recycle();
        picture = null;
    }
}
