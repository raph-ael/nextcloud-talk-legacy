package org.wolkenfunk.app

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.service.notification.StatusBarNotification
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.Person
import androidx.core.app.RemoteInput
import androidx.core.content.ContextCompat
import androidx.core.graphics.drawable.IconCompat

/**
 * Notifications for new messages (docs/notifications.md): one per account and chat
 * (`tag = "<Konto>|<Token>"`, `id = 0`) as a `MessagingStyle`, grouped per account with the
 * system's summary. The history lives in the shown notification itself, so it survives the
 * death of the process; every message remembers the id of its server notification, so a
 * „gelesen“ from the server removes exactly these.
 */
object MessageNotifications {
    private const val TAG = "TalkNotices"
    private const val CHANNEL = "messages"
    private const val ID = 0
    private const val ID_SUMMARY = 3000
    private const val ID_OTHER = 3001

    /** Server notification id of every message, in their order; 0: none (own reply, no push). */
    private const val EXTRA_NIDS = "org.wolkenfunk.app.NIDS"
    private const val EXTRA_TITLE = "org.wolkenfunk.app.TITLE"
    private const val EXTRA_GROUP = "org.wolkenfunk.app.GROUP"
    private const val EXTRA_CAN_REPLY = "org.wolkenfunk.app.CAN_REPLY"
    const val KEY_REPLY = "reply"

    /** What the system keeps of a `MessagingStyle` anyway. */
    private const val MAX_LINES = 25
    /** Ids of server notifications deleted lately, per account: their message may come later. */
    private const val MAX_DELETED = 200
    /** How long the own copy of a just posted notification beats the system's list. */
    private const val FRESH_MS = 3_000L
    /** A notification this young is not removed by a chat list without unread messages. */
    private const val SETTLE_MS = 15_000L

    /** One message of the history. [sender] `null`: the own reply. */
    private data class Line(val sender: String?, val text: String, val time: Long, val nid: Long, val icon: IconCompat? = null)

    /** Everything needed to post the notification of a chat again. */
    private data class Chat(
        val title: String,
        val isGroup: Boolean,
        val label: String?,
        val canReply: Boolean,
        val lines: List<Line>,
        val large: Icon?,
    )

    /**
     * `notify()` and `cancel()` take effect a moment later than they return, so the system's
     * list lags behind: what was just posted (or cancelled: `null`) counts for [FRESH_MS].
     */
    private val fresh = HashMap<String, Pair<Chat?, Long>>()

    fun createChannel(context: Context) {
        val nm = context.getSystemService(NotificationManager::class.java) ?: return
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, "Nachrichten", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "Neue Nachrichten und Erwähnungen"
                enableVibration(true)
            },
        )
    }

    /** May the app show notifications at all (permission from Android 13 on, app switch before). */
    fun enabled(context: Context): Boolean =
        androidx.core.app.NotificationManagerCompat.from(context).areNotificationsEnabled()

    private fun allowed(context: Context) = Build.VERSION.SDK_INT < 33 ||
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    private fun tag(account: String, token: String) = Notifications.chatKey(account, token)

    private fun group(account: String) = "messages|$account"

    private fun manager(context: Context) = context.getSystemService(NotificationManager::class.java)

    /**
     * A new message for the chat [token] of [account]: extends its notification, with sound.
     *
     * [title]: name of the chat. [sender]: who wrote, empty when unknown (then the chat's
     * name stands for it). [nid]: id of the server notification, 0 without push. [avatar]:
     * picture of the chat, already round. [label]: „{Name} · {Server}“ from two accounts on.
     */
    @Synchronized
    fun show(
        context: Context,
        account: String,
        label: String?,
        token: String,
        title: String,
        isGroup: Boolean,
        canReply: Boolean,
        sender: String,
        text: String,
        nid: Long,
        avatar: Bitmap?,
    ) {
        if (!allowed(context)) return
        if (nid != 0L && nid in deleted(context, account)) return
        val key = tag(account, token)
        val before = current(context, key)
        // FCM may deliver a push twice.
        if (nid != 0L && before?.lines?.any { it.nid == nid } == true) return
        val icon = avatar?.let { IconCompat.createWithBitmap(it) }
        val line = Line(
            // One-to-one: the chat is the sender, under the name the chat list shows.
            sender = if (isGroup) sender.ifBlank { title } else title,
            text = text.ifBlank { "Neue Nachricht" },
            time = System.currentTimeMillis(),
            nid = nid,
            // One-to-one: the chat's picture is the sender's.
            icon = if (isGroup) null else icon,
        )
        val chat = Chat(
            title = title,
            isGroup = isGroup,
            label = label,
            canReply = canReply,
            lines = (before?.lines.orEmpty() + line).takeLast(MAX_LINES),
            large = if (isGroup) avatar?.let { Icon.createWithBitmap(it) } ?: before?.large else null,
        )
        post(context, account, token, chat, alert = true)
    }

    /** „Antworten“ was sent off: the notification shows the own reply (and stops spinning). */
    @Synchronized
    fun replied(context: Context, account: String, token: String, text: String?) {
        val key = tag(account, token)
        val before = current(context, key) ?: run {
            // Gone meanwhile: the system still waits for an answer to the action.
            manager(context)?.cancel(key, ID)
            return
        }
        val lines = if (text.isNullOrBlank()) before.lines else before.lines + Line(null, text, System.currentTimeMillis(), 0)
        post(context, account, token, before.copy(lines = lines.takeLast(MAX_LINES)), alert = false)
    }

    /** The chat was opened, read or is gone. */
    @Synchronized
    fun cancel(context: Context, account: String, token: String) {
        val key = tag(account, token)
        fresh[key] = null to SystemClock.elapsedRealtime()
        val nm = manager(context) ?: return
        nm.cancel(key, ID)
        dropSummaryIfAlone(context, account, except = key)
    }

    /**
     * The server says these notifications are read elsewhere: their messages go, in this
     * account only. A notification without a message left disappears. The ids are kept for
     * a while, a message may arrive after its deletion.
     */
    @Synchronized
    fun removeIds(context: Context, account: String, ids: Collection<Long>) {
        if (ids.isEmpty()) return
        rememberDeleted(context, account, ids)
        for ((key, token) in chatsOf(context, account)) {
            val chat = current(context, key) ?: continue
            val kept = chat.lines.filter { it.nid == 0L || it.nid !in ids }
            if (kept.size == chat.lines.size) continue
            if (kept.none { it.sender != null }) cancel(context, account, token)
            else post(context, account, token, chat.copy(lines = kept), alert = false)
        }
    }

    /**
     * The chat list of [account] says these chats have nothing unread: their notifications
     * go, unless younger than [SETTLE_MS]. That protects a fresh notification from a chat
     * list that was fetched before it.
     */
    @Synchronized
    fun reconcile(context: Context, account: String, read: Set<String>) {
        if (read.isEmpty()) return
        val prefix = "$account|"
        val now = System.currentTimeMillis()
        for (sbn in active(context)) {
            val key = sbn.tag ?: continue
            if (sbn.id != ID || sbn.notification.channelId != CHANNEL || !key.startsWith(prefix)) continue
            val token = key.removePrefix(prefix)
            if (token !in read || now - sbn.postTime < SETTLE_MS) continue
            // Posted again a moment ago (the system's list lags behind).
            if (fresh[key]?.let { it.first != null && SystemClock.elapsedRealtime() - it.second < FRESH_MS } == true) continue
            cancel(context, account, token)
        }
    }

    /** All message notifications of [account] (the server deleted all, or it signed out). */
    @Synchronized
    fun clear(context: Context, account: String, forget: Boolean = false) {
        val nm = manager(context) ?: return
        val seen = ArrayList<Long>()
        for ((key, _) in chatsOf(context, account)) {
            current(context, key)?.lines?.mapNotNullTo(seen) { it.nid.takeIf { n -> n != 0L } }
            fresh[key] = null to SystemClock.elapsedRealtime()
            nm.cancel(key, ID)
        }
        nm.cancel(account, ID_SUMMARY)
        nm.cancel(account, ID_OTHER)
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        if (forget) prefs.edit().remove(deletedKey(account)).apply() else rememberDeleted(context, account, seen)
    }

    /** A push that is no chat message (e.g. „Push testen“): shown as it is. */
    fun showOther(context: Context, account: String, label: String?, text: String) {
        if (!allowed(context) || text.isBlank()) return
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_wolkenfunk)
            .setColor(Notifications.ACCENT)
            .setContentTitle("Wolkenfunk")
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setSubText(label)
            .setAutoCancel(true)
            .setContentIntent(Notifications.openChat(context, null, null))
            .build()
        runCatching { manager(context)?.notify(account, ID_OTHER, n) }
    }

    // --- building ---------------------------------------------------------------------

    private fun post(context: Context, account: String, token: String, chat: Chat, alert: Boolean) {
        val nm = manager(context) ?: return
        val key = tag(account, token)
        val me = Person.Builder().setName("Du").build()
        val style = NotificationCompat.MessagingStyle(me)
        // The system does not draw the sub text of a MessagingStyle: from two accounts on the
        // account line also stands in the conversation title (docs/notifications.md).
        val heading = when {
            chat.label != null -> "${chat.title} – ${chat.label}"
            chat.isGroup -> chat.title
            else -> null
        }
        style.setConversationTitle(heading).setGroupConversation(chat.isGroup)
        for (l in chat.lines) {
            val person = l.sender?.let { Person.Builder().setName(it).setIcon(l.icon).build() }
            style.addMessage(l.text, l.time, person)
        }
        val last = chat.lines.last()
        val b = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_wolkenfunk)
            .setColor(Notifications.ACCENT)
            .setContentTitle(chat.title)
            .setContentText(if (chat.isGroup && last.sender != null) "${last.sender}: ${last.text}" else last.text)
            .setSubText(chat.label)
            .setStyle(style)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setWhen(last.time)
            .setShowWhen(true)
            .setAutoCancel(true)
            .setOnlyAlertOnce(!alert)
            .setGroup(group(account))
            .setGroupAlertBehavior(NotificationCompat.GROUP_ALERT_CHILDREN)
            .setContentIntent(Notifications.openChat(context, account, token))
            .addExtras(
                Bundle().apply {
                    putLongArray(EXTRA_NIDS, chat.lines.map { it.nid }.toLongArray())
                    putString(EXTRA_TITLE, chat.title)
                    putBoolean(EXTRA_GROUP, chat.isGroup)
                    putBoolean(EXTRA_CAN_REPLY, chat.canReply)
                },
            )
        chat.large?.let { b.setLargeIcon(it) }
        if (chat.canReply) {
            b.addAction(
                NotificationCompat.Action.Builder(
                    R.drawable.ic_stat_wolkenfunk, "Antworten",
                    action(context, MessageActionReceiver.ACTION_REPLY, account, token, mutable = true),
                )
                    .addRemoteInput(RemoteInput.Builder(KEY_REPLY).setLabel("Antworten").build())
                    .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_REPLY)
                    .setShowsUserInterface(false)
                    .setAllowGeneratedReplies(true)
                    .build(),
            )
        }
        b.addAction(
            NotificationCompat.Action.Builder(
                R.drawable.ic_stat_wolkenfunk, "Als gelesen markieren",
                action(context, MessageActionReceiver.ACTION_READ, account, token, mutable = false),
            )
                .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_MARK_AS_READ)
                .setShowsUserInterface(false)
                .build(),
        )
        val now = SystemClock.elapsedRealtime()
        fresh.entries.removeAll { now - it.value.second >= FRESH_MS }
        fresh[key] = chat to now
        runCatching {
            // Android wants the single ones and the summary, not one instead of the other.
            nm.notify(account, ID_SUMMARY, summary(context, account, chat.label))
            nm.notify(key, ID, b.build())
        }.onFailure { Log.e(TAG, "notify", it) }
    }

    private fun summary(context: Context, account: String, label: String?): Notification =
        NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_wolkenfunk)
            .setColor(Notifications.ACCENT)
            .setContentTitle("Wolkenfunk")
            .setSubText(label)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setGroup(group(account))
            .setGroupSummary(true)
            .setGroupAlertBehavior(NotificationCompat.GROUP_ALERT_CHILDREN)
            // A tap on the closed group lands here: into the account, its chat list. Not
            // auto-cancelled, that would take the chats' notifications along unread.
            .setContentIntent(Notifications.openAccount(context, account))
            .build()

    private fun action(context: Context, action: String, account: String, token: String, mutable: Boolean): PendingIntent {
        val intent = Intent(context, MessageActionReceiver::class.java)
            .setAction(action)
            // Extras do not tell pending intents apart: the chat is part of the address.
            .setData(Uri.Builder().scheme("wolkenfunk").authority("chat").appendPath(account).appendPath(token).build())
            .putExtra(Notifications.EXTRA_ACCOUNT, account)
            .putExtra(Notifications.EXTRA_TOKEN, token)
        // The system writes the typed answer into the intent: it must be mutable there.
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or
            if (mutable && Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else if (mutable) 0 else PendingIntent.FLAG_IMMUTABLE
        return PendingIntent.getBroadcast(context, 0, intent, flags)
    }

    // --- reading back -----------------------------------------------------------------

    /** The notification of a chat as it is shown now; `null`: none. */
    private fun current(context: Context, key: String): Chat? {
        fresh[key]?.let { (chat, at) ->
            if (SystemClock.elapsedRealtime() - at < FRESH_MS) return chat
            fresh.remove(key)
        }
        val sbn = active(context).find { it.id == ID && it.tag == key } ?: return null
        return restore(sbn.notification)
    }

    private fun restore(n: Notification): Chat? {
        val style = NotificationCompat.MessagingStyle.extractMessagingStyleFromNotification(n) ?: return null
        val extras = n.extras
        val nids = extras.getLongArray(EXTRA_NIDS) ?: LongArray(0)
        val messages = style.messages
        // The system may have dropped the oldest ones: the ids belong to the newest.
        val shift = nids.size - messages.size
        val lines = messages.mapIndexed { i, m ->
            Line(
                sender = m.person?.name?.toString(),
                text = m.text?.toString().orEmpty(),
                time = m.timestamp,
                nid = nids.getOrNull(i + shift) ?: 0L,
                icon = m.person?.icon,
            )
        }
        if (lines.isEmpty()) return null
        return Chat(
            title = extras.getString(EXTRA_TITLE) ?: extras.getCharSequence(Notification.EXTRA_TITLE)?.toString().orEmpty(),
            isGroup = extras.getBoolean(EXTRA_GROUP),
            label = extras.getCharSequence(Notification.EXTRA_SUB_TEXT)?.toString(),
            canReply = extras.getBoolean(EXTRA_CAN_REPLY, true),
            lines = lines,
            large = n.getLargeIcon(),
        )
    }

    private fun active(context: Context): List<StatusBarNotification> =
        runCatching { manager(context)?.activeNotifications?.toList() }.getOrNull().orEmpty()

    /** Tag and token of every chat of [account] with a notification (shown or just posted). */
    private fun chatsOf(context: Context, account: String): List<Pair<String, String>> {
        val prefix = "$account|"
        val keys = active(context).filter { it.id == ID && it.notification.channelId == CHANNEL }.mapNotNull { it.tag } +
            fresh.filterValues { it.first != null }.keys
        return keys.filter { it.startsWith(prefix) }.distinct().map { it to it.removePrefix(prefix) }
    }

    /** The summary goes with its last chat (the system does not always do that for us). */
    private fun dropSummaryIfAlone(context: Context, account: String, except: String) {
        if (chatsOf(context, account).none { it.first != except && current(context, it.first) != null }) {
            manager(context)?.cancel(account, ID_SUMMARY)
        }
    }

    // --- deleted before they arrived --------------------------------------------------

    private const val PREFS = "notices"

    private fun deletedKey(account: String) = "deleted.$account"

    private fun deleted(context: Context, account: String): List<Long> =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(deletedKey(account), null)
            ?.split(',')?.mapNotNull { it.toLongOrNull() }.orEmpty()

    private fun rememberDeleted(context: Context, account: String, ids: Collection<Long>) {
        if (ids.isEmpty()) return
        val all = (deleted(context, account) + ids).distinct().takeLast(MAX_DELETED)
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString(deletedKey(account), all.joinToString(",")).apply()
    }
}

/**
 * „Antworten“ and „Als gelesen markieren“ of a message notification. Both go to the account
 * of the notification without making it the active one, also when the process was not
 * running: the application (and with it every account) is up before this runs.
 */
class MessageActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val sessions = (context.applicationContext as TalkApplication).sessions
        val account = intent.getStringExtra(Notifications.EXTRA_ACCOUNT) ?: return
        val token = intent.getStringExtra(Notifications.EXTRA_TOKEN) ?: return
        when (intent.action) {
            ACTION_REPLY -> {
                val text = RemoteInput.getResultsFromIntent(intent)?.getCharSequence(MessageNotifications.KEY_REPLY)
                sessions.replyFromNotification(account, token, text?.toString().orEmpty())
            }
            ACTION_READ -> {
                // The server call outlives onReceive.
                val pending = goAsync()
                sessions.markReadFromNotification(account, token) { pending.finish() }
            }
        }
    }

    companion object {
        const val ACTION_REPLY = "org.wolkenfunk.app.message.REPLY"
        const val ACTION_READ = "org.wolkenfunk.app.message.READ"
    }
}
