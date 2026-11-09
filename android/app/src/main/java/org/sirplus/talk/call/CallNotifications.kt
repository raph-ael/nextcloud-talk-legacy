package org.sirplus.talk.call

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.Person
import androidx.core.content.ContextCompat
import org.sirplus.talk.MainActivity
import org.sirplus.talk.Notifications
import uniffi.talk_ffi.CallState
import uniffi.talk_ffi.CallView

/** Call notifications: ringing (full screen, CallStyle), ongoing (service), missed. */
object CallNotifications {
    private const val CHANNEL_INCOMING = "calls_incoming"
    private const val CHANNEL_ONGOING = "calls_ongoing"
    private const val CHANNEL_MISSED = "calls_missed"
    const val ID_INCOMING = 2001
    const val ID_ONGOING = 2002
    private const val ID_MISSED_BASE = 2100

    fun createChannels(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java) ?: return
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_INCOMING, "Anrufe", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "Eingehende Anrufe"
                // The app rings and vibrates itself (ringer mode aware); the channel stays quiet.
                setSound(null, null)
                enableVibration(false)
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            },
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ONGOING, "Laufender Anruf", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Solange ein Anruf läuft"
                setShowBadge(false)
            },
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_MISSED, "Verpasste Anrufe", NotificationManager.IMPORTANCE_DEFAULT),
        )
    }

    private fun allowed(context: Context) = Build.VERSION.SDK_INT < 33 ||
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    /** Android 14+: the user may have denied full-screen notifications for calls. */
    fun canUseFullScreen(context: Context): Boolean =
        Build.VERSION.SDK_INT < 34 || context.getSystemService(NotificationManager::class.java)?.canUseFullScreenIntent() != false

    private fun person(call: CallView): Person {
        val name = if (call.isGroup) call.roomName else call.callerName.ifBlank { call.roomName }
        return Person.Builder().setName(name).setImportant(true).build()
    }

    private fun callActivity(context: Context, accept: Boolean): PendingIntent {
        val intent = Intent(context, CallActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        if (accept) intent.action = CallActivity.ACTION_ACCEPT
        return PendingIntent.getActivity(
            context, if (accept) 1 else 0, intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun action(context: Context, action: String, token: String): PendingIntent =
        PendingIntent.getBroadcast(
            context, action.hashCode(),
            Intent(context, CallActionReceiver::class.java).setAction(action).putExtra(CallActionReceiver.EXTRA_TOKEN, token),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

    @SuppressLint("MissingPermission") // checked by allowed()
    fun showIncoming(context: Context, call: CallView) {
        if (!allowed(context)) return
        val full = callActivity(context, accept = false)
        val n = NotificationCompat.Builder(context, CHANNEL_INCOMING)
            .setSmallIcon(android.R.drawable.sym_call_incoming)
            .setContentTitle(person(call).name)
            .setContentText(if (call.isGroup) "Gruppenanruf" else "Talk-Anruf")
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOngoing(true)
            .setSilent(true)
            .setContentIntent(full)
            .setFullScreenIntent(full, true)
            .setStyle(
                NotificationCompat.CallStyle.forIncomingCall(
                    person(call),
                    action(context, CallActionReceiver.ACTION_DECLINE, call.token),
                    callActivity(context, accept = true),
                ),
            )
            .build()
        runCatching { NotificationManagerCompat.from(context).notify(ID_INCOMING, n) }
    }

    fun cancelIncoming(context: Context) {
        NotificationManagerCompat.from(context).cancel(ID_INCOMING)
    }

    /** Notification of the foreground service while the call runs. */
    fun ongoing(context: Context, call: CallView?): Notification {
        val b = NotificationCompat.Builder(context, CHANNEL_ONGOING)
            .setSmallIcon(android.R.drawable.sym_call_outgoing)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setOngoing(true)
            .setSilent(true)
            .setContentIntent(callActivity(context, accept = false))
        if (call == null) return b.setContentTitle("Anruf").build()
        val text = when (call.state) {
            CallState.DIALING -> "Anrufen …"
            CallState.RINGING -> if (call.isGroup) "Warte auf andere …" else "Es klingelt …"
            CallState.CONNECTING -> "Verbindung wird hergestellt …"
            CallState.RECONNECTING -> "Verbindung wird neu hergestellt …"
            CallState.CONNECTED -> "Talk-Anruf"
            else -> ""
        }
        b.setContentText(text)
        call.startedAt?.takeIf { call.state == CallState.CONNECTED }?.let {
            b.setUsesChronometer(true).setWhen(it).setShowWhen(true)
        }
        b.setStyle(
            NotificationCompat.CallStyle.forOngoingCall(
                person(call),
                action(context, CallActionReceiver.ACTION_HANGUP, call.token),
            ),
        )
        return b.build()
    }

    @SuppressLint("MissingPermission") // checked by allowed()
    fun updateOngoing(context: Context, call: CallView) {
        if (!allowed(context)) return
        runCatching { NotificationManagerCompat.from(context).notify(ID_ONGOING, ongoing(context, call)) }
    }

    @SuppressLint("MissingPermission") // checked by allowed()
    fun showMissed(context: Context, call: CallView) {
        if (!allowed(context)) return
        val open = PendingIntent.getActivity(
            context, call.token.hashCode(),
            Intent(context, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                .putExtra(Notifications.EXTRA_TOKEN, call.token),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val n = NotificationCompat.Builder(context, CHANNEL_MISSED)
            .setSmallIcon(android.R.drawable.sym_call_missed)
            .setContentTitle("Verpasster Anruf")
            .setContentText(person(call).name)
            .setCategory(NotificationCompat.CATEGORY_MISSED_CALL)
            .setAutoCancel(true)
            .setContentIntent(open)
            .build()
        runCatching { NotificationManagerCompat.from(context).notify(ID_MISSED_BASE + (call.token.hashCode() and 0xffff), n) }
    }
}
