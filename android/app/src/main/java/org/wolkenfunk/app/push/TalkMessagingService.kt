package org.wolkenfunk.app.push

import android.util.Log
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.wolkenfunk.app.TalkApplication

/**
 * FCM entry point. The proxy sends data messages (`subject`, `signature`) with high
 * priority; such a message may start the ringing foreground service from the background.
 * The application (and with it the session) is already up when this runs.
 */
class TalkMessagingService : FirebaseMessagingService() {
    private val sessions get() = (application as TalkApplication).sessions

    override fun onNewToken(token: String) {
        Log.i(Push.TAG, "new FCM token")
        sessions.onPushToken(token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        val subject = message.data["subject"] ?: return
        if (message.priority != message.originalPriority) {
            // Deprioritized by FCM (e.g. too many high-priority messages): starting the
            // ringing service may be refused, the notification still shows.
            Log.w(Push.TAG, "push deprioritized (${message.originalPriority} → ${message.priority})")
        }
        // Runs on a background thread; stay here until the call rings (foreground service
        // started), so the process is not frozen in between.
        runBlocking {
            withTimeoutOrNull(HANDLE_TIMEOUT_MS) { sessions.handlePush(subject) }
                ?: Log.w(Push.TAG, "push not handled in time")
        }
    }

    private companion object {
        /** FCM allows about 10 s (high priority) before the process may be frozen again. */
        const val HANDLE_TIMEOUT_MS = 9_000L
    }
}
