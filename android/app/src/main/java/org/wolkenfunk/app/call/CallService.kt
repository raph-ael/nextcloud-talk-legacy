package org.wolkenfunk.app.call

import android.Manifest
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import org.wolkenfunk.app.TalkApplication

/**
 * Keeps the process and the microphone alive while a call has audio (from `startAudio`
 * to `stopAudio`): foreground type phoneCall|microphone, self-managed without Telecom
 * (like Signal). All call logic lives in [CallController]; this only holds the
 * foreground state and shows its notification.
 */
class CallService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val controller = (application as TalkApplication).sessions.calls
        val call = controller.ui.value
        val ringing = intent?.getBooleanExtra(EXTRA_RINGING, false) == true
        if (ringing && call == null) {
            stopSelf()
            return START_NOT_STICKY
        }
        val id = if (ringing) CallNotifications.ID_INCOMING else CallNotifications.ID_ONGOING
        val notification = if (ringing) CallNotifications.incoming(this, call!!) else CallNotifications.ongoing(this, call)
        // Ringing needs no microphone yet; the call itself does.
        val mic = !ringing &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        val types = if (Build.VERSION.SDK_INT >= 30) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL or (if (mic) ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE else 0)
        } else if (Build.VERSION.SDK_INT >= 29) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_PHONE_CALL
        } else {
            0
        }
        try {
            ServiceCompat.startForeground(this, id, notification, types)
            Log.i(TAG, if (ringing) "foreground (ringing)" else "foreground (call)")
        } catch (e: Exception) {
            // E.g. started while in the background without an exemption: the call still
            // runs while the app is visible, but say so in the log.
            Log.e(TAG, "startForeground failed", e)
            stopSelf()
        }
        return START_NOT_STICKY
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // Swiping the app away does not end the call (like a phone call); the
        // notification and the call screen stay reachable.
        super.onTaskRemoved(rootIntent)
    }

    companion object {
        private const val TAG = "TalkCallService"

        private const val EXTRA_RINGING = "ringing"

        /** The call has audio: foreground with microphone. */
        fun start(context: Context) = start(context, ringing = false)

        /**
         * Ringing while the app is in the background: keeps the process from being frozen,
         * so the core can stop the ringing (answered elsewhere, missed). May be refused
         * from the background; the caller still shows the notification.
         */
        fun startRinging(context: Context): Boolean = start(context, ringing = true)

        private fun start(context: Context, ringing: Boolean): Boolean = try {
            ContextCompat.startForegroundService(
                context, Intent(context, CallService::class.java).putExtra(EXTRA_RINGING, ringing),
            )
            true
        } catch (e: Exception) {
            Log.e(TAG, "could not start (ringing=$ringing)", e)
            false
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, CallService::class.java))
        }
    }
}

/** Decline (ringing notification) and hang up (ongoing notification). */
class CallActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val calls = (context.applicationContext as TalkApplication).sessions.calls
        when (intent.action) {
            ACTION_DECLINE -> calls.decline(intent.getStringExtra(EXTRA_TOKEN) ?: return)
            ACTION_HANGUP -> calls.hangUp()
        }
    }

    companion object {
        const val ACTION_DECLINE = "org.wolkenfunk.app.call.DECLINE"
        const val ACTION_HANGUP = "org.wolkenfunk.app.call.HANGUP"
        const val EXTRA_TOKEN = "token"
    }
}
