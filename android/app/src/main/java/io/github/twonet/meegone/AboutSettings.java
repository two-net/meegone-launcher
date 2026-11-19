package io.github.twonet.meegone;

import android.content.Context;
import android.content.pm.PackageManager;
import android.util.Log;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * Settings → About (SET-05, ADR 0035): the version, the privacy notice (PRIV-04), the statement
 * that MeeGone is independent of Nokia and Microsoft (LEGAL-05), and "Open-source licences",
 * which lists the third-party code MeeGone includes with their licences (LEGAL-06). Last comes
 * "Reset launcher", which asks first (SET-06), offered while the Rust app says it can reset
 * everything.
 *
 * <p>All of it is the host's own text: the version is the installed package's, and the rest is in
 * the app's resources, so it is translated with them (L10N-01).
 */
final class AboutSettings extends LinearLayout {
    /** What the user did here. */
    interface Listener {
        /** "Open-source licences" was tapped. */
        void licences();

        /** "Reset launcher" was tapped, before it is confirmed. */
        void resetLauncher();
    }

    private static final String TAG = "MeeGone";

    private final TextView group;
    private final TextView versionTitle;
    private final TextView version;
    private final TextView privacyTitle;
    private final TextView privacy;
    private final TextView independent;
    private final TextView licences;
    private final TextView reset;
    private int foreground = -1;
    private int dim = -1;
    private int accent = -1;

    AboutSettings(Context context, Listener listener) {
        super(context);
        SettingsRows.startGroup(this);
        group = SettingsRows.heading(this, R.string.settings_group_about);
        versionTitle = new TextView(context);
        version = new TextView(context);
        SettingsRows.titled(this, versionTitle, version, R.string.about_version, null);
        version.setText(versionName(context));
        privacyTitle = new TextView(context);
        privacy = new TextView(context);
        SettingsRows.titled(this, privacyTitle, privacy, R.string.about_privacy, null);
        privacy.setText(R.string.about_privacy_notice);
        independent = SettingsRows.paragraph(this, R.string.about_independent);
        licences = SettingsRows.action(this, R.string.about_licences);
        licences.setVisibility(VISIBLE);
        licences.setOnClickListener(view -> listener.licences());
        reset = SettingsRows.action(this, R.string.reset_launcher);
        reset.setOnClickListener(view -> listener.resetLauncher());
    }

    /** The installed package's version, such as "0.1.0", or an empty line if it can't be read. */
    private static String versionName(Context context) {
        try {
            String name = context.getPackageManager()
                    .getPackageInfo(context.getPackageName(), 0)
                    .versionName;
            return name != null ? name : "";
        } catch (PackageManager.NameNotFoundException | RuntimeException e) {
            Log.w(TAG, "Couldn't read MeeGone's version: " + e.getClass().getSimpleName());
            return "";
        }
    }

    /** Offers Reset launcher while the Rust app can reset everything (SET-06, ARCH-09). */
    void show(boolean resetOffered) {
        reset.setVisibility(resetOffered ? VISIBLE : GONE);
    }

    /** The theme's colours (THEME-03). */
    void colour(int foreground, int dim, int accent) {
        if (foreground == this.foreground && dim == this.dim && accent == this.accent) {
            return;
        }
        this.foreground = foreground;
        this.dim = dim;
        this.accent = accent;
        group.setTextColor(dim);
        versionTitle.setTextColor(foreground);
        version.setTextColor(dim);
        privacyTitle.setTextColor(foreground);
        privacy.setTextColor(dim);
        independent.setTextColor(dim);
        SettingsRows.colourButton(licences, foreground, dim, accent);
        SettingsRows.colourButton(reset, foreground, dim, accent);
    }
}
