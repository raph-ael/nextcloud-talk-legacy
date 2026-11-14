package org.wolkenfunk.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * After a reboot or an app update, unsent messages must go out without the user opening
 * the app. WorkManager alone does not guarantee that on every device (on Samsung its job
 * did not survive a reboot), so this receiver starts the process: [TalkApplication]
 * resumes the session, which re-schedules [OutboxWorker] when the outbox is not empty.
 */
class StartupReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        // Touching the application is enough; SessionManager does the rest.
        (context.applicationContext as TalkApplication).sessions.currentSession()?.let {
            if (it.outboxUnsent() > 0u) OutboxWorker.schedule(context)
        }
    }
}
