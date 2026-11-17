package io.github.twonet.meegone;

import android.service.notification.NotificationListenerService;
import android.service.notification.StatusBarNotification;

/**
 * MeeGone's notification listener (EVT-04, EVT-10, ADR 0020). The system binds it while the user
 * grants MeeGone notification access, and unbinds it when they revoke it.
 *
 * <p>It keeps nothing. {@link Bridge} asks it for the active notifications when the Rust app
 * reads them, and it tells {@link Bridge} when they change. Nothing here logs what a
 * notification says (EVT-13).
 *
 * <p>The system calls it on the main thread, which is the only thread that reads {@link
 * #connected}.
 */
public final class NotificationListener extends NotificationListenerService {
    /** The listener while the system has it connected, or null. Cleared as it goes. */
    private static NotificationListener connected;

    /** The connected listener, or null while the system hasn't bound it. Main thread only. */
    static NotificationListener connected() {
        return connected;
    }

    @Override
    public void onListenerConnected() {
        connected = this;
        // Access was just granted, or the system bound the listener after a restart.
        Bridge.notificationsChanged();
    }

    @Override
    public void onListenerDisconnected() {
        disconnect();
    }

    @Override
    public void onDestroy() {
        disconnect();
        super.onDestroy();
    }

    @Override
    public void onNotificationPosted(StatusBarNotification posted) {
        Bridge.notificationsChanged();
    }

    @Override
    public void onNotificationRemoved(StatusBarNotification removed) {
        Bridge.notificationsChanged();
    }

    /** Access was revoked, or the system unbound the listener (REL-01). */
    private void disconnect() {
        if (connected == this) {
            connected = null;
            Bridge.notificationsChanged();
        }
    }
}
