package org.wolkenfunk.app

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

/** Notification channels and the notifications around sending. */
object Notifications {
    private const val CHANNEL_SENDING = "sending"
    private const val CHANNEL_FAILED = "send_failed"
    const val ID_SENDING = 1001
    const val EXTRA_TOKEN = "org.wolkenfunk.app.TOKEN"

    /** Accent color of every notification (Wolkenfunk blue). */
    const val ACCENT = 0xFF2F7BF5.toInt()

    fun createChannels(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java) ?: return
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_SENDING, "Senden im Hintergrund", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Wird angezeigt, solange große Anhänge hochgeladen werden"
                setShowBadge(false)
            },
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_FAILED, "Nicht gesendete Nachrichten", NotificationManager.IMPORTANCE_DEFAULT).apply {
                description = "Wenn eine Nachricht endgültig nicht gesendet werden konnte"
            },
        )
    }

    /** Ongoing notification of the upload foreground service. */
    fun sending(context: Context): Notification =
        NotificationCompat.Builder(context, CHANNEL_SENDING)
            .setSmallIcon(R.drawable.ic_stat_wolkenfunk)
            .setColor(ACCENT)
            .setContentTitle("Anhänge werden gesendet …")
            .setProgress(0, 0, true)
            .setOngoing(true)
            .setSilent(true)
            .setContentIntent(openApp(context, null))
            .build()

    fun sendFailed(context: Context, token: String, roomName: String) {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED &&
            android.os.Build.VERSION.SDK_INT >= 33
        ) {
            return
        }
        val n = NotificationCompat.Builder(context, CHANNEL_FAILED)
            .setSmallIcon(R.drawable.ic_stat_wolkenfunk)
            .setColor(ACCENT)
            .setContentTitle("Nachricht nicht gesendet")
            .setContentText("An $roomName – tippen zum Öffnen")
            .setAutoCancel(true)
            .setContentIntent(openApp(context, token))
            .build()
        runCatching { NotificationManagerCompat.from(context).notify(token.hashCode(), n) }
    }

    private fun openApp(context: Context, token: String?): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        if (token != null) intent.putExtra(EXTRA_TOKEN, token)
        return PendingIntent.getActivity(
            context,
            token?.hashCode() ?: 0,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }
}
