package io.github.twonet.meegone;

import android.content.res.Resources;

/**
 * Every problem the Rust app or the {@link Bridge} reports, by its code, and its words in the
 * device's language (L10N-01, ADR 0040). Rust never writes words for the user: a failure reaches
 * the host as one of these codes, the same as {@code Problem::code} in
 * {@code crates/core/data/src/problem.rs}, and is shown as its {@code problem_<code>} string
 * resource. A {@code Bridge} method that fails returns one of them, too.
 *
 * <p>{@code cargo test -p arch-check} checks that the codes here, in Rust and in the string
 * resources are the same.
 */
final class Problems {
    static final String ACCESS_DENIED = "access_denied";
    static final String SYSTEM_FAILED = "system_failed";
    static final String APP_LIST_UNAVAILABLE = "app_list_unavailable";
    static final String SHORTCUTS_NEED_DEFAULT_HOME = "shortcuts_need_default_home";
    static final String NOTIFICATIONS_UNREADABLE = "notifications_unreadable";
    static final String RECENT_UNREADABLE = "recent_unreadable";
    static final String APP_UNAVAILABLE = "app_unavailable";
    static final String SHORTCUT_UNAVAILABLE = "shortcut_unavailable";
    static final String UNINSTALL_UNAVAILABLE = "uninstall_unavailable";
    static final String APP_INFO_UNAVAILABLE = "app_info_unavailable";
    static final String NOTIFICATION_UNAVAILABLE = "notification_unavailable";
    static final String NOTIFICATIONS_DISMISS_UNAVAILABLE = "notifications_dismiss_unavailable";
    static final String NOTIFICATION_SETTINGS_UNAVAILABLE = "notification_settings_unavailable";
    static final String ALARM_UNAVAILABLE = "alarm_unavailable";
    static final String BROWSER_UNAVAILABLE = "browser_unavailable";
    static final String WEB_SEARCH_UNAVAILABLE = "web_search_unavailable";
    static final String HTTPS_LINKS_ONLY = "https_links_only";
    static final String DEFAULT_HOME_INSTRUCTIONS = "default_home_instructions";
    static final String NOTIFICATION_ACCESS_INSTRUCTIONS = "notification_access_instructions";
    static final String USAGE_ACCESS_INSTRUCTIONS = "usage_access_instructions";
    static final String WALLPAPER_SETTINGS_INSTRUCTIONS = "wallpaper_settings_instructions";
    static final String NO_CONNECTION = "no_connection";
    static final String TOO_MANY_REDIRECTS = "too_many_redirects";
    static final String CONNECTION_INSECURE = "connection_insecure";
    static final String HTTPS_ONLY = "https_only";
    static final String DOWNLOAD_TOO_LARGE = "download_too_large";
    static final String DOWNLOAD_FAILED = "download_failed";
    static final String STORAGE_FULL = "storage_full";
    static final String SAVED_DATA_UNUSABLE = "saved_data_unusable";
    static final String STORAGE_BUDGET = "storage_budget";
    static final String WALLPAPER_NOT_SUPPORTED = "wallpaper_not_supported";
    static final String WALLPAPER_NOT_ALLOWED = "wallpaper_not_allowed";
    static final String WALLPAPER_NOT_SET = "wallpaper_not_set";
    static final String LOCK_SCREEN_RESET_FAILED = "lock_screen_reset_failed";
    static final String IMAGE_JOB_UNAVAILABLE = "image_job_unavailable";
    static final String SCREEN_SIZE_UNKNOWN = "screen_size_unknown";
    static final String IMAGE_UNREADABLE = "image_unreadable";
    static final String IMAGE_TOO_SMALL = "image_too_small";
    static final String IMAGE_TOO_LARGE = "image_too_large";
    static final String PREVIEW_TOO_LARGE = "preview_too_large";
    static final String FEED_FORMAT_CHANGED = "feed_format_changed";
    static final String NO_SUCH_REGION = "no_such_region";
    static final String IMAGE_NO_LONGER_LISTED = "image_no_longer_listed";
    static final String IMAGE_NOT_OFFERED = "image_not_offered";
    static final String IMAGE_NOT_CACHED = "image_not_cached";

    private Problems() {}

    /**
     * The words for {@code code}, or null for none. A code this host doesn't know, which only
     * a mismatched build could send, reads as {@link #SYSTEM_FAILED}.
     */
    static String text(Resources res, String code) {
        return code == null || code.isEmpty() ? null : res.getString(resource(code));
    }

    private static int resource(String code) {
        switch (code) {
            case ACCESS_DENIED:
                return R.string.problem_access_denied;
            case SYSTEM_FAILED:
                return R.string.problem_system_failed;
            case APP_LIST_UNAVAILABLE:
                return R.string.problem_app_list_unavailable;
            case SHORTCUTS_NEED_DEFAULT_HOME:
                return R.string.problem_shortcuts_need_default_home;
            case NOTIFICATIONS_UNREADABLE:
                return R.string.problem_notifications_unreadable;
            case RECENT_UNREADABLE:
                return R.string.problem_recent_unreadable;
            case APP_UNAVAILABLE:
                return R.string.problem_app_unavailable;
            case SHORTCUT_UNAVAILABLE:
                return R.string.problem_shortcut_unavailable;
            case UNINSTALL_UNAVAILABLE:
                return R.string.problem_uninstall_unavailable;
            case APP_INFO_UNAVAILABLE:
                return R.string.problem_app_info_unavailable;
            case NOTIFICATION_UNAVAILABLE:
                return R.string.problem_notification_unavailable;
            case NOTIFICATIONS_DISMISS_UNAVAILABLE:
                return R.string.problem_notifications_dismiss_unavailable;
            case NOTIFICATION_SETTINGS_UNAVAILABLE:
                return R.string.problem_notification_settings_unavailable;
            case ALARM_UNAVAILABLE:
                return R.string.problem_alarm_unavailable;
            case BROWSER_UNAVAILABLE:
                return R.string.problem_browser_unavailable;
            case WEB_SEARCH_UNAVAILABLE:
                return R.string.problem_web_search_unavailable;
            case HTTPS_LINKS_ONLY:
                return R.string.problem_https_links_only;
            case DEFAULT_HOME_INSTRUCTIONS:
                return R.string.problem_default_home_instructions;
            case NOTIFICATION_ACCESS_INSTRUCTIONS:
                return R.string.problem_notification_access_instructions;
            case USAGE_ACCESS_INSTRUCTIONS:
                return R.string.problem_usage_access_instructions;
            case WALLPAPER_SETTINGS_INSTRUCTIONS:
                return R.string.problem_wallpaper_settings_instructions;
            case NO_CONNECTION:
                return R.string.problem_no_connection;
            case TOO_MANY_REDIRECTS:
                return R.string.problem_too_many_redirects;
            case CONNECTION_INSECURE:
                return R.string.problem_connection_insecure;
            case HTTPS_ONLY:
                return R.string.problem_https_only;
            case DOWNLOAD_TOO_LARGE:
                return R.string.problem_download_too_large;
            case DOWNLOAD_FAILED:
                return R.string.problem_download_failed;
            case STORAGE_FULL:
                return R.string.problem_storage_full;
            case SAVED_DATA_UNUSABLE:
                return R.string.problem_saved_data_unusable;
            case STORAGE_BUDGET:
                return R.string.problem_storage_budget;
            case WALLPAPER_NOT_SUPPORTED:
                return R.string.problem_wallpaper_not_supported;
            case WALLPAPER_NOT_ALLOWED:
                return R.string.problem_wallpaper_not_allowed;
            case WALLPAPER_NOT_SET:
                return R.string.problem_wallpaper_not_set;
            case LOCK_SCREEN_RESET_FAILED:
                return R.string.problem_lock_screen_reset_failed;
            case IMAGE_JOB_UNAVAILABLE:
                return R.string.problem_image_job_unavailable;
            case SCREEN_SIZE_UNKNOWN:
                return R.string.problem_screen_size_unknown;
            case IMAGE_UNREADABLE:
                return R.string.problem_image_unreadable;
            case IMAGE_TOO_SMALL:
                return R.string.problem_image_too_small;
            case IMAGE_TOO_LARGE:
                return R.string.problem_image_too_large;
            case PREVIEW_TOO_LARGE:
                return R.string.problem_preview_too_large;
            case FEED_FORMAT_CHANGED:
                return R.string.problem_feed_format_changed;
            case NO_SUCH_REGION:
                return R.string.problem_no_such_region;
            case IMAGE_NO_LONGER_LISTED:
                return R.string.problem_image_no_longer_listed;
            case IMAGE_NOT_OFFERED:
                return R.string.problem_image_not_offered;
            case IMAGE_NOT_CACHED:
                return R.string.problem_image_not_cached;
            default:
                return R.string.problem_system_failed;
        }
    }
}
