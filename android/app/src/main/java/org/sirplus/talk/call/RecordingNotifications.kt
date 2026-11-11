package org.sirplus.talk.call

import android.Manifest
import android.annotation.SuppressLint
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import org.sirplus.talk.MainActivity
import org.sirplus.talk.Notifications
import uniffi.talk_ffi.RecordingNotice
import uniffi.talk_ffi.RecordingNoticeKind

/**
 * Texts of the recording notices (banners and notifications) and the quiet notification
 * per new notice while the app runs (channel "Aufnahmen"); notices handled here or
 * elsewhere take their notification with them.
 */
object RecordingNotifications {
    private const val CHANNEL = "recordings"
    /** One id for all; the notice id goes into the tag. */
    private const val ID = 3000
    private const val TAG_PREFIX = "recording:"

    fun createChannel(context: Context) {
        context.getSystemService(NotificationManager::class.java)?.createNotificationChannel(
            NotificationChannel(CHANNEL, "Aufnahmen", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Aufnahmen, Transkripte und Zusammenfassungen von Anrufen"
            },
        )
    }

    val RecordingNotice.failed: Boolean
        get() = kind == RecordingNoticeKind.RECORDING_FAILED || kind == RecordingNoticeKind.TRANSCRIPT_FAILED ||
            kind == RecordingNoticeKind.SUMMARY_FAILED

    /** "Aufnahme verfügbar · ‹Datei›", or the failure. */
    fun title(n: RecordingNotice): String {
        val what = when (n.kind) {
            RecordingNoticeKind.RECORDING -> "Aufnahme verfügbar"
            RecordingNoticeKind.TRANSCRIPT -> "Transkript verfügbar"
            RecordingNoticeKind.SUMMARY -> "Zusammenfassung verfügbar"
            RecordingNoticeKind.RECORDING_FAILED -> "Aufnahme fehlgeschlagen"
            RecordingNoticeKind.TRANSCRIPT_FAILED -> "Transkript fehlgeschlagen"
            RecordingNoticeKind.SUMMARY_FAILED -> "Zusammenfassung fehlgeschlagen"
        }
        return if (n.failed || n.fileName.isBlank()) what else "$what · ${n.fileName}"
    }

    /**
     * The full current list: notifies the notices in [fresh] (new since the core's first
     * list after start, which is the baseline and never announced) and withdraws
     * notifications of notices that are gone (handled here or elsewhere).
     */
    @SuppressLint("MissingPermission") // checked by allowed()
    fun update(context: Context, items: List<RecordingNotice>, fresh: Set<Long>) {
        val current = items.map { it.id }.toSet()
        cancelWhere(context) { it !in current }
        if (!allowed(context)) return
        val nm = NotificationManagerCompat.from(context)
        for (n in items) {
            if (n.id in fresh) runCatching { nm.notify(TAG_PREFIX + n.id, ID, build(context, n)) }
        }
    }

    fun clear(context: Context) = cancelWhere(context) { true }

    private fun cancelWhere(context: Context, predicate: (Long) -> Boolean) {
        val nm = context.getSystemService(NotificationManager::class.java) ?: return
        for (sbn in runCatching { nm.activeNotifications }.getOrDefault(emptyArray())) {
            val id = sbn.tag?.takeIf { sbn.id == ID && it.startsWith(TAG_PREFIX) }?.removePrefix(TAG_PREFIX)?.toLongOrNull() ?: continue
            if (predicate(id)) nm.cancel(sbn.tag, ID)
        }
    }

    private fun build(context: Context, n: RecordingNotice) =
        NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(if (n.failed) android.R.drawable.stat_notify_error else android.R.drawable.stat_sys_download_done)
            .setContentTitle(title(n))
            .setContentText(n.roomName)
            .setWhen(n.timestamp * 1000)
            .setShowWhen(true)
            .setSilent(true)
            .setAutoCancel(true)
            .setContentIntent(
                PendingIntent.getActivity(
                    context, n.id.hashCode(),
                    Intent(context, MainActivity::class.java)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                        .putExtra(Notifications.EXTRA_TOKEN, n.token),
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                ),
            )
            .build()

    private fun allowed(context: Context) = Build.VERSION.SDK_INT < 33 ||
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
}
