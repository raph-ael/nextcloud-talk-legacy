package org.wolkenfunk.app

import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.SystemClock
import android.util.LruCache
import android.widget.Toast
import android.content.ActivityNotFoundException
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.ui.graphics.ImageBitmap
import androidx.core.content.FileProvider
import androidx.compose.ui.graphics.asImageBitmap
import android.net.ConnectivityManager
import android.net.Network
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.wolkenfunk.app.media.PickedFile
import org.wolkenfunk.app.media.PreparedFile
import org.wolkenfunk.app.media.PreparedImage
import org.wolkenfunk.app.media.prepareFile
import org.wolkenfunk.app.media.decodeSampled
import org.wolkenfunk.app.media.prepareImage
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import uniffi.talk_ffi.CallIceCandidate
import uniffi.talk_ffi.CallView
import uniffi.talk_ffi.CategoryView
import uniffi.talk_ffi.ChatPage
import uniffi.talk_ffi.SearchResults
import java.util.concurrent.atomic.AtomicInteger
import uniffi.talk_ffi.PeerState
import uniffi.talk_ffi.SdpType
import org.wolkenfunk.app.call.CallActions
import org.wolkenfunk.app.call.CallVideo
import org.wolkenfunk.app.call.CallController
import org.wolkenfunk.app.call.CallNotifications
import org.wolkenfunk.app.call.MediaReports
import org.wolkenfunk.app.call.WebRtcMediaEngine
import org.wolkenfunk.app.push.Push
import android.util.Log
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull
import uniffi.talk_ffi.PushMessage
import uniffi.talk_ffi.RecordingNotice
import org.wolkenfunk.app.call.RecordingNotifications
import uniffi.talk_ffi.decryptPushFor
import uniffi.talk_ffi.PushKey
import uniffi.talk_ffi.CallState
import uniffi.talk_ffi.resolveAccount
import uniffi.talk_ffi.revokeAppPassword
import org.wolkenfunk.app.media.stagingDir
import uniffi.talk_ffi.generatePushKey
import uniffi.talk_ffi.ClientPlatform
import uniffi.talk_ffi.ConnectionState
import uniffi.talk_ffi.ContactCard
import uniffi.talk_ffi.CoreEvent
import uniffi.talk_ffi.CoreException
import uniffi.talk_ffi.CoreListener
import uniffi.talk_ffi.FileAttachment
import uniffi.talk_ffi.LoginFlow
import uniffi.talk_ffi.EditDraft
import uniffi.talk_ffi.MentionCandidate
import uniffi.talk_ffi.MentionPick
import uniffi.talk_ffi.MessageKind
import uniffi.talk_ffi.MessageView
import uniffi.talk_ffi.PendingMessage
import uniffi.talk_ffi.PendingState
import uniffi.talk_ffi.MemberView
import uniffi.talk_ffi.ReactionView
import uniffi.talk_ffi.UserView
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import uniffi.talk_ffi.ReplyPreview
import uniffi.talk_ffi.RoomView
import uniffi.talk_ffi.SectionView
import uniffi.talk_ffi.SendError
import uniffi.talk_ffi.TalkSession
import uniffi.talk_ffi.newReferenceId
import uniffi.talk_ffi.pushChatParts
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.BitmapShader
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Shader
import androidx.compose.ui.graphics.asAndroidBitmap
import org.wolkenfunk.app.ui.withAttachment

/** So long a tapped search hit waits for its window before the chat opens with a spinner. */
private const val AROUND_WAIT_MS = 400L

/** Suggestions after an „@“ come at most this many (docs/mentions.md „Abfrage“). */
private const val MENTION_LIMIT = 50u

enum class SessionState { LOADING, LOGGED_OUT, READY }
enum class SendStatus { SENDING, WAITING, FAILED }

/**
 * A message as shown in the UI. Messages from the outbox (not yet delivered) carry a
 * [status], file messages also their [localFile], upload [progress] (0..1, negative when
 * unknown) and, once failed, the [error].
 */
data class UiMessage(
    val msg: MessageView,
    val status: SendStatus? = null,
    val localFile: String? = null,
    val progress: Float = -1f,
    val error: SendError? = null,
) {
    val key: String get() = if (status != null) msg.referenceId else msg.id.toString()
}

data class ChatState(
    val messages: List<UiMessage> = emptyList(),
    val hasMore: Boolean = true,
    val oldestId: Long? = null,
    val lastCommonRead: Long = 0,
    val loading: Boolean = true,
    val loadingOlder: Boolean = false,
    /** The server page of this opening has arrived (or failed): more than the cache. */
    val synced: Boolean = false,
    /**
     * Window into the history (docs/search.md „Chat an einer Nachricht öffnen“): newer
     * messages exist below the list, [newestId] is the pointer for `loadNewer`.
     */
    val hasNewer: Boolean = false,
    /** Set by the pages of a window, also once it reached the end; null for a chat opened at its end. */
    val newestId: Long? = null,
    val loadingNewer: Boolean = false,
    /** Messages that arrived while the list was a window: they follow at its end, the jump button counts them. */
    val held: List<MessageView> = emptyList(),
    /** Changes when the list is replaced: a page asked for before does not belong to it any more. */
    val rev: Int = 0,
    /** Counts the jumps from a window to the end of the chat: the view follows. */
    val endRev: Int = 0,
    /**
     * Messages that mention the user and were not seen yet, oldest first: the „@“ button
     * (docs/mentions.md). Merged from every page the chat gets: with many unread messages the
     * first page knows only the newest mentions.
     */
    val mentions: List<Long> = emptyList(),
    /** Mentions seen since the chat was opened: a later page does not bring them back. */
    val mentionsSeen: Set<Long> = emptySet(),
) {
    /** Adds mentions to the list of the „@“ button, leaving out what was seen. */
    fun withMentions(ids: List<Long>): ChatState {
        val fresh = ids.filter { it !in mentionsSeen && it !in mentions }
        return if (fresh.isEmpty()) this else copy(mentions = (mentions + fresh).sorted())
    }
}

/**
 * What stands in the input field of a chat: the text, the cursor and the mentions picked
 * from the suggestions (docs/mentions.md „Entwurf“).
 */
class TextDraft(val text: String, val cursor: Int, val picks: List<MentionPick>)

/**
 * A message the open chat shows (search hit, quoted message): the screen scrolls it into the
 * upper part and lights it up. Not [ready] while the chat still opens at it.
 */
data class Jump(val token: String, val messageId: Long, val seq: Int, val ready: Boolean)

/**
 * Ungelesen-Linie of the open chat (docs/chat.md „Datum und Ungelesen-Linie“), pure UI state
 * taken when opening the chat. Place and number derive from the loaded messages: above the
 * first one that [counts][countsUnread], number `max(base, loaded ones that count)`.
 */
data class UnreadLine(val token: String, val from: Long, val base: Int)

/** Counts for the Ungelesen-Linie: delivered, after [from], not own, no system message. */
fun countsUnread(m: UiMessage, from: Long): Boolean =
    m.status == null && m.msg.id > from && !m.msg.isOwn && m.msg.kind != MessageKind.SYSTEM

/** An account for the account menu and the settings (docs/accounts.md). */
data class AccountView(
    val id: String,
    val name: String,
    /** Without `https://`, as in the head of the settings. */
    val server: String,
    val userId: String,
    /** `badge().chats`: conversations with something new that counts. */
    val chats: Int,
)

data class UiState(
    val session: SessionState = SessionState.LOADING,
    /** Id of the account shown (docs/accounts.md); everything below belongs to it. */
    val accountId: String = "",
    val server: String = "",
    /** Name of the account (display name from the server, else the login name). */
    val user: String = "",
    /** Own Talk user id (for the own avatar); the login name until the server confirmed it. */
    val userId: String = "",
    val rooms: List<RoomView> = emptyList(),
    /** Bumped on every `RoomsChanged`: sections may change (collapsed, categories) with equal rooms. */
    val roomsRev: Int = 0,
    val chats: Map<String, ChatState> = emptyMap(),
    /** Connection of the shown account (docs/connection.md); a session begins as connecting. */
    val connection: ConnectionState = ConnectionState.CONNECTING,
    /** `SystemClock.elapsedRealtime()` when [connection] last left `ONLINE`: the spinner waits 500 ms from here. */
    val connectionSince: Long = 0,
    val activeToken: String? = null,
    val unreadLine: UnreadLine? = null,
    val jump: Jump? = null,
    /**
     * Chats below the open one, the nearest last: it was opened from a mention there, and
     * „Zurück“ leads back (docs/mentions.md „Antippen“).
     */
    val returnTo: List<String> = emptyList(),
    /** The open chat (or the list) was reached by going back: it slides in from the left. */
    val navBack: Boolean = false,
    /** Counts taps on a notification of the chat that was open already: its view starts over. */
    val chatFollowed: Int = 0,
    /** All signed-in accounts in the order they signed in (the active one included). */
    val accounts: List<AccountView> = emptyList(),
)

/** Server address as shown everywhere (docs/accounts.md): without scheme and trailing slash. */
fun shownServer(server: String) = server.trim().removePrefix("https://").removePrefix("http://").trimEnd('/')

/**
 * App-wide state holder: one runtime (a started [TalkSession] with its own listener) per
 * account, turns the active account's core events into an immutable [UiState] and exposes
 * the user actions. Inactive accounts deliver badge, calls, `authFailed` and failed sends
 * (docs/accounts.md „Im Hintergrund“).
 */
class SessionManager(private val context: Context) {
    private val store = AccountStore(context)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state
    private val _call = MutableStateFlow<CallView?>(null)
    /** The call in progress (also a ringing incoming one); `null`: none. */
    val call: StateFlow<CallView?> = _call

    /**
     * One account: its session and what belongs to it alone. The listener knows the
     * account; events from the core carry none.
     */
    private inner class Runtime(@Volatile var account: AccountEntry) : CoreListener {
        val id: String get() = account.id
        lateinit var session: TalkSession
        /** Push registration of this process run (again on every start and token change). */
        var pushJob: Job? = null
        @Volatile var pushRegistered = false
        /** Failed sends already announced by a notification. */
        val notifiedFailures: MutableSet<String> = ConcurrentHashMap.newKeySet()
        /** Notice ids of the last core list; `null` until the first one (the baseline) arrived. */
        var recordingsKnown: Set<Long>? = null
        var recordings: List<RecordingNotice> = emptyList()
        /** docs/connection.md: kept per account, also while another one is shown. Main thread. */
        var connection = ConnectionState.CONNECTING
        var connectionSince = SystemClock.elapsedRealtime()
        @Volatile var rooms: List<RoomView> = emptyList()
        @Volatile var badge = 0
        /** `resolveAccount` for the adopted account's key is in flight. */
        var resolving = false
        /** FCM lost pushes: the core's `messageNotices` count although push is on. */
        @Volatile var catchUp = false

        override fun onEvent(event: CoreEvent) = this@SessionManager.onEvent(this, event)
    }

    /** Running accounts in sign-in order; replaced as a whole on the main thread. */
    @Volatile private var runtimes: Map<String, Runtime> = emptyMap()
    /** The account the UI shows. */
    @Volatile private var active: Runtime? = null
    /** The account of the call in progress (control and media reports go there). */
    @Volatile private var callOwner: Runtime? = null

    private val session: TalkSession? get() = active?.session
    private var loginFlow: LoginFlow? = null
    private val avatars = LruCache<String, ImageBitmap>(64)
    /** Chat image previews by account and file id, bounded by bytes. */
    private val previews = object : LruCache<String, ImageBitmap>(PREVIEW_CACHE_BYTES) {
        override fun sizeOf(key: String, value: ImageBitmap) = value.width * value.height * 4
    }
    /** Width/height of previews seen in this process, so bubbles keep their size on re-layout. */
    private val aspects = ConcurrentHashMap<String, Float>()
    /** Decoded local images of pending sends, by account and reference id, until the server echo arrives. */
    private val localPreviews = ConcurrentHashMap<String, ImageBitmap>()

    /** Voice messages: recording, drafts, playback (of the active account). */
    val voice = VoiceManager(context, scope, { session }, { _state.value })

    /** Calls: everything around the media (tones, audio, notifications, call screen). */
    val calls = CallController(context, scope, CallActionsImpl(), { callLabel() }) {
        voice.interrupt()
        voice.player.stop()
    }

    private val _recordings = MutableStateFlow<List<RecordingNotice>>(emptyList())
    /** Finished call recordings (transcripts, summaries, failures) of the active account. */
    val recordings: StateFlow<List<RecordingNotice>> = _recordings
    /** Notices with a share/dismiss request in flight (buttons disabled meanwhile). */
    private val _recordingsBusy = MutableStateFlow<Set<Long>>(emptySet())
    val recordingsBusy: StateFlow<Set<Long>> = _recordingsBusy

    /** WebRTC for the core; one per process, handed to every session. */
    private val media = WebRtcMediaEngine(context, MediaReportsImpl(), calls)

    /** Video of the running call (received per peer, our camera). */
    val callVideo: CallVideo get() = media

    init {
        store.migrate()
        for (entry in store.accounts()) {
            when (val pw = store.password(entry.id)) {
                is Secret.Present -> startRuntime(entry, pw.value)
                // Not readable now (keystore): nothing is deleted, the next start tries again.
                Secret.Unreadable -> Log.w(TAG, "account ${entry.id} not readable, not started")
                Secret.Missing -> Log.w(TAG, "account ${entry.id} without password, not started")
            }
        }
        val first = store.activeId()?.let { runtimes[it] } ?: runtimes.values.firstOrNull()
        if (first == null) _state.update { it.copy(session = SessionState.LOGGED_OUT) } else activate(first)
        // Network back: retry waiting messages right away instead of after the backoff.
        val connectivity = context.getSystemService(ConnectivityManager::class.java)
        connectivity?.registerDefaultNetworkCallback(
            object : ConnectivityManager.NetworkCallback() {
                // No network at all (docs/connection.md): the core says so at once instead of
                // after the next failed request. A change to another network has a successor
                // and arrives as onAvailable.
                override fun onLost(network: Network) {
                    if (connectivity.activeNetwork != null) return
                    for (rt in runtimes.values) rt.session.networkLost()
                }

                override fun onAvailable(network: Network) {
                    for (rt in runtimes.values) rt.session.networkChanged()
                    // Registration failed offline: try again now.
                    scope.launch {
                        for (rt in runtimes.values) if (!rt.pushRegistered) registerPush(rt)
                        retryRevokes()
                    }
                }
            },
        )
        // App passwords of accounts signed out without a network go back now.
        retryRevokes()
        // Images prepared but never handed to an outbox (process died in between).
        scope.launch(Dispatchers.IO) {
            val cutoff = System.currentTimeMillis() - 3_600_000
            for (dir in listOf(stagingDir(context), File(context.noBackupFilesDir, "talk/staging"))) {
                dir.listFiles()?.filter { it.lastModified() < cutoff }?.forEach { it.delete() }
            }
        }
        // Leaving the app with unsent messages: hand them to the background worker.
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStop(owner: LifecycleOwner) = scheduleOutboxIfNeeded()

            // Back in front: open connections are often dead by now, so the core retries at
            // once (docs/connection.md). With a chat open the user sees it, its notification goes.
            override fun onResume(owner: LifecycleOwner) {
                for (rt in runtimes.values) rt.session.networkChanged()
                val st = _state.value
                st.activeToken?.let { MessageNotifications.cancel(context, st.accountId, it) }
            }
        })
    }

    /** All running sessions, for the background worker (empty when signed out). */
    fun allSessions(): List<TalkSession> = runtimes.values.map { it.session }

    private fun scheduleOutboxIfNeeded() {
        if (runtimes.values.any { it.session.outboxUnsent() > 0u }) OutboxWorker.schedule(context)
    }

    // --- accounts (docs/accounts.md) ---------------------------------------------------

    /** Starts the runtime of a stored account (main thread). */
    private fun startRuntime(entry: AccountEntry, appPassword: String): Runtime {
        val rt = Runtime(entry)
        val s = TalkSession(entry.server, entry.loginName, appPassword, ClientPlatform.ANDROID, rt, entry.dir)
        rt.session = s
        s.setMediaEngine(media)
        rt.rooms = s.rooms()
        rt.badge = s.badge().chats.toInt()
        refreshIdentity(rt)
        runtimes = runtimes + (entry.id to rt)
        // Unsent messages of an earlier run continue in the background worker as well.
        scheduleOutboxIfNeeded()
        scope.launch {
            try {
                s.start()
                if (rt.id !in runtimes) return@launch
                refreshIdentity(rt)
                resolveKey(rt)
                onRecordings(rt, s.recordingNotices())
                registerPush(rt)
            } catch (e: CoreException.Unauthorized) {
                expire(rt)
            } catch (e: CoreException) {
                // Offline start: the core keeps retrying in the background.
            }
        }
        return rt
    }

    /** User id and name from the session (cache, after `start()` current), stored when changed. */
    private fun refreshIdentity(rt: Runtime) {
        val a = rt.account
        val fresh = a.copy(userId = rt.session.userId().ifEmpty { a.userId }, name = rt.session.displayName().ifEmpty { a.name })
        if (fresh == a) return
        rt.account = fresh
        store.update(fresh)
        if (rt.id in runtimes) publishAccounts()
        if (active === rt) _state.update { if (it.accountId == rt.id) it.copy(user = fresh.name, userId = fresh.userId) else it }
    }

    /** The adopted account has no key yet: ask the server once it runs. */
    private suspend fun resolveKey(rt: Runtime) {
        if (rt.account.key.isNotEmpty() || rt.resolving) return
        val pw = (store.password(rt.id) as? Secret.Present)?.value ?: return
        val a = rt.account
        rt.resolving = true
        val identity = try {
            runCatching { resolveAccount(a.server, a.loginName, pw, ClientPlatform.ANDROID) }.getOrNull()
        } finally {
            rt.resolving = false
        }
        if (identity == null || rt.id !in runtimes) return
        rt.account = rt.account.copy(key = identity.key)
        store.update(rt.account)
    }

    /** Shows [rt]: its chat list at once from its runtime, without a loading state. */
    private fun activate(rt: Runtime) {
        val old = active
        if (old != null && old !== rt) {
            _state.value.activeToken?.let { old.session.closeChat(it) }
        }
        voice.switchAccount(rt.id)
        active = rt
        store.setActive(rt.id)
        val before = _state.value
        _state.value = UiState(
            session = SessionState.READY,
            accountId = rt.id,
            server = rt.account.server,
            user = rt.account.name,
            userId = rt.session.userId(),
            rooms = rt.session.rooms(),
            roomsRev = before.roomsRev + 1,
            connection = rt.connection,
            connectionSince = rt.connectionSince,
            accounts = accountViews(),
        )
        _recordings.value = rt.recordings
        _recordingsBusy.value = emptySet()
    }

    private fun accountViews(): List<AccountView> = runtimes.values.map {
        AccountView(it.id, it.account.name, shownServer(it.account.server), it.account.userId, it.badge)
    }

    private fun publishAccounts() {
        val views = accountViews()
        _state.update { if (it.session == SessionState.READY) it.copy(accounts = views) else it }
    }

    /** „{Name} · {Server}“ of [rt] from two accounts on, else `null` (docs/accounts.md). */
    private fun label(rt: Runtime?): String? =
        if (rt == null || runtimes.size < 2) null else "${rt.account.name} · ${shownServer(rt.account.server)}"

    /** Account line of the call in progress (ringing display), from two accounts on. */
    fun callLabel(): String? = label(callOwner)

    /** Account of the call in progress, for its room and avatars. */
    fun callAccountId(): String? = callOwner?.id

    /** The room of the call in its own account. */
    fun callRoom(token: String): RoomView? = callOwner?.rooms?.find { it.token == token }

    /** A call rings or runs: switching accounts is locked. */
    fun inCall(): Boolean = _call.value?.let { it.state != CallState.ENDED } == true

    /**
     * Switches to account [id]. During a call only with [force] (accepting a call of that
     * account); otherwise the toast „Während eines Anrufs nicht möglich“. `false`: not switched.
     */
    fun switchAccount(id: String, force: Boolean = false): Boolean {
        val rt = runtimes[id] ?: return false
        if (rt === active) return true
        if (!force && inCall()) {
            toast("Während eines Anrufs nicht möglich")
            return false
        }
        activate(rt)
        return true
    }

    /**
     * Login Flow v2 (first account or „Konto hinzufügen“): opens the browser, resolves once
     * the user approved and the server told who signed in. Throws when that fails; then
     * nothing is stored.
     */
    suspend fun login(server: String) {
        loginFlow?.cancel()
        val flow = LoginFlow.start(server, ClientPlatform.ANDROID)
        loginFlow = flow
        context.startActivity(
            Intent(Intent.ACTION_VIEW, Uri.parse(flow.loginUrl())).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        try {
            val result = flow.wait()
            val identity = resolveAccount(result.server, result.loginName, result.appPassword, ClientPlatform.ANDROID)
            val known = store.accounts().find { it.key == identity.key }
            when {
                known != null && known.id in runtimes -> {
                    // Already signed in: back to it, the fresh app password goes back.
                    scope.launch {
                        runCatching { revokeAppPassword(result.server, result.loginName, result.appPassword, ClientPlatform.ANDROID) }
                    }
                    switchAccount(known.id)
                    toast("Dieses Konto ist schon angemeldet")
                }
                known != null -> {
                    // Stored, but its password was not readable: it gets the fresh one.
                    if (!store.replacePassword(known.id, result.appPassword)) throw CoreException.Server("store")
                    val rt = startRuntime(known.copy(loginName = result.loginName), result.appPassword)
                    publishAccounts()
                    activate(rt)
                }
                else -> {
                    val id = store.newId()
                    val entry = AccountEntry(
                        id = id,
                        server = result.server,
                        loginName = result.loginName,
                        userId = identity.userId,
                        name = identity.displayName,
                        key = identity.key,
                        dir = store.newDir(id),
                    )
                    if (!store.add(entry, result.appPassword)) throw CoreException.Server("store")
                    val rt = startRuntime(entry, result.appPassword)
                    publishAccounts()
                    activate(rt)
                }
            }
            // Bring the app back after approving in the browser.
            context.startActivity(
                Intent(context, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT),
            )
        } finally {
            if (loginFlow === flow) loginFlow = null
        }
    }

    fun cancelLogin() {
        loginFlow?.cancel()
        loginFlow = null
    }

    /** „Abmelden“: signs out the active account only. */
    fun logout() {
        val rt = active ?: return
        scope.launch { signOut(rt, revoke = true) }
    }

    /** The server rejected the account (`authFailed`): removed without asking, with a notice. */
    private suspend fun expire(rt: Runtime) {
        if (rt.id !in runtimes) return
        val name = rt.account.name
        val server = shownServer(rt.account.server)
        signOut(rt, revoke = false)
        if (ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) {
            Toast.makeText(context, "$name ($server) wurde abgemeldet. Bitte neu anmelden.", Toast.LENGTH_LONG).show()
        }
    }

    /**
     * Signs out one account: push, `logout()` (with [revoke]), then its entry, secret, push
     * key and data folder. The other accounts keep running, with their notifications and outbox.
     */
    private suspend fun signOut(rt: Runtime, revoke: Boolean) {
        if (rt.id !in runtimes) return
        val s = rt.session
        val password = if (revoke) (store.password(rt.id) as? Secret.Present)?.value else null
        // The UI leaves the account first, so nothing calls into it any more.
        runtimes = runtimes - rt.id
        drafts.keys.removeAll { it.startsWith("${rt.id}|") }
        if (active === rt) {
            voice.cancel()
            voice.player.stop()
            _state.value.activeToken?.let { s.closeChat(it) }
            active = null
            val next = runtimes.values.firstOrNull()
            if (next != null) {
                activate(next)
            } else {
                voice.switchAccount(null)
                _state.value = UiState(session = SessionState.LOGGED_OUT)
                _recordings.value = emptyList()
            }
        } else {
            publishAccounts()
        }
        if (callOwner === rt) s.hangUpAndWait(2000u)
        rt.pushJob?.cancel()
        if (revoke && rt.pushRegistered) runCatching { s.unregisterPush() }
        rt.pushRegistered = false
        // The FCM token goes only with the last account.
        if (runtimes.isEmpty()) Push.deleteToken(context)
        if (revoke) {
            val gone = try {
                s.logout()
                true
            } catch (e: CoreException.Unauthorized) {
                true
            } catch (e: Exception) {
                false
            }
            // Not handed back (no network): the server would keep pushing to an account that
            // is gone here. Remembered and made up for at the next start (docs/notifications.md).
            if (!gone && password != null) store.addPendingRevoke(PendingRevoke(rt.account.server, rt.account.loginName, password))
        } else {
            s.shutdown()
        }
        s.close()
        store.remove(rt.id)
        // Unsent messages belong to this account: never deliver them under another one.
        withContext(Dispatchers.IO) { File(rt.account.dir).deleteRecursively() }
        if (runtimes.values.none { it.session.outboxUnsent() > 0u }) OutboxWorker.cancel(context)
        val prefix = "${rt.id}|"
        localPreviews.keys.removeAll { it.startsWith(prefix) }
        aspects.keys.removeAll { it.startsWith(prefix) }
        avatars.snapshot().keys.filter { it.startsWith(prefix) }.forEach { avatars.remove(it) }
        previews.snapshot().keys.filter { it.startsWith(prefix) }.forEach { previews.remove(it) }
        RecordingNotifications.clear(context, rt.id)
        MessageNotifications.clear(context, rt.id, forget = true)
        // With one account left, notifications lose their account line from now on.
        publishAccounts()
    }

    // --- push (docs/accounts.md „Push“) -------------------------------------------------

    /**
     * Registers this device at the server and the push proxy for [rt], with its own device
     * key: on every start and with a new FCM token ([token]). Main thread; a running
     * registration is not repeated.
     */
    private fun registerPush(rt: Runtime, token: String? = null) {
        if (rt.id !in runtimes) return
        if (!Push.enabled(context)) return
        if (token == null && rt.pushJob?.isActive == true) return
        rt.pushJob?.cancel()
        rt.pushJob = scope.launch {
            try {
                val fcm = token ?: Push.token()
                val key = when (val k = store.pushKey(rt.id)) {
                    is Secret.Present -> k.value
                    Secret.Missing -> generatePushKey().also { store.savePushKey(rt.id, it) }
                    // Never replace a key that is only unreadable right now.
                    Secret.Unreadable -> return@launch
                }
                rt.session.registerPush(fcm, Push.proxyUrl, key)
                if (rt.id in runtimes) rt.pushRegistered = true
                Log.i(Push.TAG, "registered ${rt.id}")
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Push.TAG, "registration failed: ${e.message}")
            }
        }
    }

    /** FCM issued a new token (called on a background thread): every account registers it. */
    fun onPushToken(token: String) {
        scope.launch { for (rt in runtimes.values) registerPush(rt, token) }
    }

    /**
     * A push arrived (FCM thread, process possibly just started; all runtimes exist since
     * the application started). The key that decrypts names the account. Returns once a
     * call rings here, so the ringing foreground service runs before FCM's window closes,
     * and for a chat once its new messages are fetched.
     */
    suspend fun handlePush(subject: String) {
        val keys = store.accounts().mapNotNull { a ->
            (store.pushKey(a.id) as? Secret.Present)?.let { PushKey(a.id, it.value) }
        }
        if (keys.isEmpty()) {
            Log.w(Push.TAG, "push without device key")
            return
        }
        val hit = decryptPushFor(keys, subject)
        if (hit == null) {
            Log.w(Push.TAG, "push for no account")
            return
        }
        val rt = runtimes[hit.accountId] ?: return
        val s = rt.session
        when (val message = hit.message) {
            is PushMessage.Call -> {
                Log.i(Push.TAG, "call push")
                if (!s.handlePush(message, null)) return
                withTimeoutOrNull(RING_WAIT_MS) { calls.ui.first { it?.token == message.token } }
                // The incoming call is handled on the main thread right after the state is set.
                withContext(Dispatchers.Main) { }
            }
            // Read elsewhere: the notifications with these ids go, in this account only.
            is PushMessage.Delete -> {
                MessageNotifications.removeIds(context, rt.id, message.notificationIds)
                s.handlePush(message, null)
            }
            PushMessage.DeleteAll -> {
                MessageNotifications.clear(context, rt.id)
                s.handlePush(message, null)
            }
            // First the notification, then the messages themselves, so the chat shows them in
            // its first frame (docs/notifications.md). The wait is short: FCM hands over one
            // push after the other, and a call may be next. The fetching itself goes on.
            is PushMessage.Chat -> {
                showPushed(rt, message)
                val fetch = scope.launch { runCatching { s.handlePush(message, null) } }
                withTimeoutOrNull(CHAT_FETCH_WAIT_MS) { fetch.join() }
            }
            is PushMessage.Other -> MessageNotifications.showOther(context, rt.id, label(rt), message.subject)
        }
    }

    /** FCM dropped pushes (too many waiting, or offline for weeks): every account reloads. */
    fun onPushesLost() {
        scope.launch {
            for (rt in runtimes.values) launch {
                rt.catchUp = true
                try {
                    runCatching { rt.session.refreshRooms() }
                } finally {
                    rt.catchUp = false
                }
            }
        }
    }

    // --- message notifications (docs/notifications.md) ------------------------------------

    /** Exactly this chat is open in the active account and the app is in front. */
    private fun chatInFront(rt: Runtime, token: String): Boolean {
        val st = _state.value
        return st.accountId == rt.id && st.activeToken == token &&
            ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)
    }

    /**
     * The notification for a chat push, from the push alone and without network: title from
     * the chat list, sender and text from the push, the picture from the avatar cache.
     */
    private suspend fun showPushed(rt: Runtime, push: PushMessage.Chat) {
        if (chatInFront(rt, push.token)) return
        val room = rt.rooms.find { it.token == push.token }
        val parts = pushChatParts(push.subject, room?.name.orEmpty())
        // A chat the list does not know yet: the first line („Bob in Projekt“) as it is.
        val title = room?.name ?: parts.sender.ifBlank { "Wolkenfunk" }
        MessageNotifications.show(
            context, rt.id, label(rt), push.token,
            title = title,
            isGroup = room != null && isGroup(room),
            canReply = room?.readOnly != true,
            sender = parts.sender,
            text = parts.text,
            nid = push.notificationId,
            avatar = noticeAvatar(rt, room),
        )
    }

    /**
     * `messageNotices` of the core. With push the server announces, so they count only
     * without push in this build and after FCM lost pushes; what came in before the start
     * is never shown (the app is in front then anyway).
     */
    private fun onNotices(rt: Runtime, event: CoreEvent.MessageNotices) {
        if (event.sinceStart) return
        if (Push.enabled(context) && !rt.catchUp) return
        scope.launch {
            for (item in event.items) {
                if (rt.id !in runtimes || chatInFront(rt, item.token)) continue
                val room = rt.rooms.find { it.token == item.token }
                val avatar = noticeAvatar(rt, room)
                // Read while the picture loaded.
                if (rt.rooms.find { it.token == item.token }?.unreadMessages == 0L) continue
                val m = item.message
                MessageNotifications.show(
                    context, rt.id, label(rt), item.token,
                    title = item.roomName,
                    isGroup = item.isGroup,
                    canReply = room?.readOnly != true,
                    sender = if (m != null && !m.isSystem) m.author else "",
                    text = m?.let { withAttachment(it.text, it.attachment, it.location) }.orEmpty(),
                    nid = 0,
                    avatar = avatar,
                )
            }
        }
    }

    private fun isGroup(room: RoomView) = room.roomType.toInt() !in setOf(1, 5, 6)

    /** Round picture of the chat: from memory, else the core's cache (a short wait at most). */
    private suspend fun noticeAvatar(rt: Runtime, room: RoomView?): Bitmap? {
        room ?: return null
        val dark = (context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        val image = withTimeoutOrNull(AVATAR_WAIT_MS) { avatar(room.token, room.avatarVersion, dark, rt.id) } ?: return null
        return withContext(Dispatchers.Default) { runCatching { round(image.asAndroidBitmap()) }.getOrNull() }
    }

    private fun round(src: Bitmap): Bitmap {
        val size = minOf(src.width, src.height)
        val out = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val soft = if (src.config == Bitmap.Config.HARDWARE) src.copy(Bitmap.Config.ARGB_8888, false) else src
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { shader = BitmapShader(soft, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP) }
        Canvas(out).drawCircle(size / 2f, size / 2f, size / 2f, paint)
        return out
    }

    /**
     * „Antworten“ in a notification: at once into the outbox of the notification's account,
     * which does not become the active one. The notification is set again in any case,
     * else its progress circle keeps turning.
     */
    fun replyFromNotification(account: String, token: String, text: String) {
        val rt = runtimes[account]
        if (rt == null) {
            MessageNotifications.cancel(context, account, token)
            return
        }
        val body = text.trim()
        if (body.isNotEmpty()) {
            rt.session.enqueueText(token, body, emptyList(), null, newReferenceId())
            OutboxWorker.schedule(context)
        }
        MessageNotifications.replied(context, account, token, body)
    }

    /** „Als gelesen markieren“ in a notification; [done] once the server answered. */
    fun markReadFromNotification(account: String, token: String, done: () -> Unit) {
        MessageNotifications.cancel(context, account, token)
        val rt = runtimes[account] ?: return done()
        scope.launch {
            try {
                val error = withTimeoutOrNull(ACTION_WAIT_MS) { rt.session.markRoomRead(token) }
                if (error != null) toast(error)
            } finally {
                done()
            }
        }
    }

    /** This build has push (Firebase config and proxy address). */
    fun pushAvailable(): Boolean = Push.enabled(context)

    /** „Push testen“ in the active account; `false`: the server did not take it. */
    suspend fun testPush(): Boolean {
        val s = session ?: return false
        return try {
            s.testPush()
            true
        } catch (e: CoreException) {
            false
        }
    }

    // --- sign-out without network -------------------------------------------------------

    private var revoking = false

    /** Hands back the app passwords of accounts that were signed out without a network. */
    private fun retryRevokes() {
        if (revoking) return
        val pending = store.pendingRevokes()
        if (pending.isEmpty()) return
        revoking = true
        scope.launch {
            try {
                for (p in pending) {
                    val gone = try {
                        revokeAppPassword(p.server, p.loginName, p.appPassword, ClientPlatform.ANDROID)
                        true
                    } catch (e: CoreException.Unauthorized) {
                        true
                    } catch (e: Exception) {
                        false
                    }
                    if (gone) store.dropPendingRevoke(p)
                }
            } finally {
                revoking = false
            }
        }
    }

    // --- calls ------------------------------------------------------------------------

    /** Calls [token] (or joins its running call). `silent`: nobody is rung (tests only). */
    fun startCall(token: String, silent: Boolean = false) {
        val s = session ?: return
        scope.launch {
            try {
                s.startCall(token, silent, null)
            } catch (e: CoreException) {
                Toast.makeText(context, callError(e), Toast.LENGTH_LONG).show()
            }
        }
    }

    // --- call recording (docs/calls.md "Aufnahme") ----------------------------------------

    /** Main thread. Banners show the active account's notices, notifications come from all. */
    private fun onRecordings(rt: Runtime, items: List<RecordingNotice>, fresh: Set<Long> = emptySet()) {
        if (rt.id !in runtimes) return
        rt.recordings = items
        if (active === rt) {
            _recordings.value = items
            _recordingsBusy.update { busy -> busy.filterTo(mutableSetOf()) { id -> items.any { it.id == id } } }
        }
        RecordingNotifications.update(context, rt.id, label(rt), items, fresh)
    }

    /** Recording in the call: goes to the call's account. */
    fun startRecording(video: Boolean) = recordingAction("Die Aufnahme konnte nicht gestartet werden") { it.startRecording(video) }

    fun stopRecording() = recordingAction("Die Aufnahme konnte nicht beendet werden") { it.stopRecording() }

    /** Answer to the consent dialog: join the call although it may be recorded. */
    fun consentToRecording() {
        (callOwner ?: active)?.session?.consentToRecording()
    }

    /** "In Chat teilen": posts the file into its conversation. */
    fun shareRecording(id: Long) = noticeAction(id, "Teilen nicht möglich") { it.shareRecording(id) }

    /** "Verwerfen" / "OK": removes the notice (the file stays in the user's files). */
    fun dismissRecording(id: Long) = noticeAction(id, "Verwerfen nicht möglich") { it.dismissRecording(id) }

    private fun recordingAction(fallback: String, block: suspend (TalkSession) -> Unit) {
        val s = (callOwner ?: active)?.session ?: return
        scope.launch {
            try {
                block(s)
            } catch (e: CoreException) {
                Toast.makeText(context, coreMessage(e, fallback), Toast.LENGTH_LONG).show()
            }
        }
    }

    private fun noticeAction(id: Long, fallback: String, block: suspend (TalkSession) -> Unit) {
        val rt = active ?: return
        if (id in _recordingsBusy.value) return
        _recordingsBusy.update { it + id }
        scope.launch {
            try {
                block(rt.session)
                // The core sends RecordingsChanged; drop the notice right away anyway.
                onRecordings(rt, rt.recordings.filter { it.id != id })
            } catch (e: CoreException) {
                _recordingsBusy.update { it - id }
                Toast.makeText(context, coreMessage(e, fallback), Toast.LENGTH_LONG).show()
            }
        }
    }

    /** The readable part of a core error: `call: <code>: <message>` carries a German text. */
    private fun coreMessage(e: CoreException, fallback: String): String = when (e) {
        is CoreException.Network -> "$fallback: keine Verbindung"
        is CoreException.Server -> e.detail.takeIf { it.startsWith("call: ") }
            ?.substringAfter(": ")?.substringAfter(": ", "")?.ifBlank { null } ?: fallback
        else -> fallback
    }

    // TODO: the FFI maps the core's `Call { code, message }` to `Server`; show its text once it has its own variant.
    private fun callError(e: CoreException): String = when (e) {
        is CoreException.Network -> "Anruf nicht möglich: keine Verbindung"
        else -> "Anruf nicht möglich"
    }

    /** Everything about the call goes to the runtime of the call, not the active one. */
    private inner class CallActionsImpl : CallActions {
        override fun accept(token: String) {
            val rt = callOwner ?: return
            scope.launch {
                try {
                    rt.session.acceptCall(token)
                    // Accepted in another account: the app follows the call there.
                    if (rt.id in runtimes) switchAccount(rt.id, force = true)
                } catch (e: CoreException) {
                    Toast.makeText(context, callError(e), Toast.LENGTH_LONG).show()
                }
            }
        }

        override fun decline(token: String) {
            callOwner?.session?.declineCall(token)
        }

        override fun hangUp() {
            callOwner?.session?.hangUp()
        }

        override fun setMuted(muted: Boolean) {
            callOwner?.session?.setCallMuted(muted)
        }

        override fun setCamera(on: Boolean) {
            callOwner?.session?.setCameraEnabled(on)
        }
    }

    private inner class MediaReportsImpl : MediaReports {
        private val s: TalkSession? get() = callOwner?.session

        override fun localDescription(peerId: String, type: SdpType, sdp: String) {
            s?.mediaLocalDescription(peerId, type, sdp)
        }

        override fun localCandidate(peerId: String, candidate: CallIceCandidate) {
            s?.mediaLocalCandidate(peerId, candidate)
        }

        override fun peerState(peerId: String, state: PeerState) {
            s?.mediaPeerState(peerId, state)
        }

        override fun peerError(peerId: String, message: String) {
            s?.mediaPeerError(peerId, message)
        }

        override fun audioLevel(peerId: String, level: Float) {
            s?.mediaAudioLevel(peerId, level)
        }

        override fun cameraFailed(reason: String) {
            s?.mediaCameraState(false)
            scope.launch(Dispatchers.Main) { Toast.makeText(context, reason, Toast.LENGTH_LONG).show() }
        }
    }

    /** The runtime for avatars: [account] when given and running, else the active one. */
    private fun runtimeFor(account: String?): Runtime? = account?.let { runtimes[it] } ?: active

    /** Avatar of a user ([account]: of that account, e.g. the call's; else the active one). */
    suspend fun userAvatar(userId: String, dark: Boolean, account: String? = null): ImageBitmap? {
        val rt = runtimeFor(account) ?: return null
        val key = "${rt.id}|user|$userId|$dark"
        avatars.get(key)?.let { return it }
        val bytes = try {
            rt.session.userAvatar(userId, dark)
        } catch (e: CoreException) {
            return null
        }
        val bitmap = withContext(Dispatchers.Default) { decodeAvatar(bytes) } ?: return null
        avatars.put(key, bitmap)
        return bitmap
    }

    fun cachedUserAvatar(userId: String, dark: Boolean, account: String? = null): ImageBitmap? =
        runtimeFor(account)?.let { avatars.get("${it.id}|user|$userId|$dark") }

    // --- core events (called on a Rust thread) ---------------------------------------

    /**
     * Events of one runtime. Only the active one fills the UI; inactive ones deliver badge,
     * calls, `authFailed`, failed sends and recordings, the rest is dropped.
     */
    private fun onEvent(rt: Runtime, event: CoreEvent) {
        val shown = active === rt
        when (event) {
            is CoreEvent.RoomsChanged -> scope.launch {
                if (rt.id !in runtimes) return@launch
                rt.rooms = event.rooms
                // Started offline: name and (adopted account) key once the server answers.
                refreshIdentity(rt)
                if (rt.account.key.isEmpty()) scope.launch { resolveKey(rt) }
                val badge = runCatching { rt.session.badge().chats.toInt() }.getOrDefault(rt.badge)
                if (badge != rt.badge) {
                    rt.badge = badge
                    publishAccounts()
                }
                if (active === rt) onRooms(rt, event.rooms)
                // Read elsewhere without a deletion from the server reaching us.
                val read = event.rooms.filter { it.unreadMessages == 0L }.mapTo(HashSet()) { it.token }
                launch(Dispatchers.Default) { MessageNotifications.reconcile(context, rt.id, read) }
            }
            is CoreEvent.ParticipantsChanged -> if (shown) _participants.tryEmit(event.token)
            is CoreEvent.MessagesAdded -> if (shown) {
                adoptLocalPreviews(rt.id, event.messages)
                trackUnreadLine(rt.id, event.token, event.messages)
                // Who mentions the user feeds the „@“ button until the bubble was seen (docs/mentions.md).
                val mentioning = event.messages.filter { it.mentionsMe }.map { it.id }
                updateChat(rt.id, event.token) {
                    // In a window they would leave a gap: kept for its end (docs/search.md).
                    val next = if (it.hasNewer) {
                        val ids = event.messages.mapTo(HashSet()) { m -> m.id }
                        it.copy(held = (it.held.filter { h -> h.id !in ids } + event.messages).sortedBy { m -> m.id })
                    } else {
                        it.copy(messages = merge(it.messages, event.messages))
                    }
                    next.withMentions(mentioning)
                }
            }
            is CoreEvent.MessagesUpdated -> updateChat(rt.id, event.token) { c ->
                val byId = event.messages.associateBy { it.id }
                c.copy(
                    messages = c.messages.map { m -> byId[m.msg.id]?.let { UiMessage(it) } ?: m },
                    held = if (c.held.isEmpty()) c.held else c.held.map { m -> byId[m.id] ?: m },
                    // A deleted message mentions nobody any more.
                    mentions = c.mentions.filter { id -> byId[id]?.kind != MessageKind.DELETED },
                )
            }
            is CoreEvent.MessagesRemoved -> updateChat(rt.id, event.token) { c ->
                val gone = event.ids.toSet()
                c.copy(
                    messages = c.messages.filter { m -> m.status != null || m.msg.id !in gone },
                    held = c.held.filter { m -> m.id !in gone },
                    mentions = c.mentions.filter { id -> id !in gone },
                )
            }
            is CoreEvent.ReadStatus -> updateChat(rt.id, event.token) {
                it.copy(lastCommonRead = maxOf(it.lastCommonRead, event.lastCommonRead))
            }
            // On the main queue like RoomsChanged: at the start the chat list arrives before
            // `online`, and an empty list must not show „Noch keine Chats“ in between.
            is CoreEvent.Connection -> scope.launch {
                if (rt.connection == ConnectionState.ONLINE) rt.connectionSince = SystemClock.elapsedRealtime()
                rt.connection = event.state
                _state.update {
                    if (it.accountId == rt.id) it.copy(connection = rt.connection, connectionSince = rt.connectionSince) else it
                }
            }
            CoreEvent.AuthFailed -> scope.launch { expire(rt) }
            is CoreEvent.OutboxChanged -> {
                // Unsent messages stand at the end of the chat, not in a window.
                updateChat(rt.id, event.token) { c -> if (c.hasNewer) c else c.copy(messages = withPending(c.messages, event.items)) }
                announceFailures(rt, event.token, event.items)
            }
            is CoreEvent.CallChanged -> {
                // The owner is known before any media report of this call arrives.
                if (event.call != null) callOwner = rt
                scope.launch {
                    if (event.call == null) {
                        // Late end of an earlier call: another account already rings or calls.
                        if (callOwner != null && callOwner !== rt) return@launch
                        callOwner = null
                    }
                    _call.value = event.call
                    calls.onCall(event.call)
                }
            }
            is CoreEvent.CallMissed -> CallNotifications.showMissed(
                context, rt.id, label(rt), event.token,
                if (event.isGroup) event.roomName else event.callerName.ifBlank { event.roomName },
            )
            is CoreEvent.RecordingsChanged -> scope.launch {
                val ids = event.items.map { it.id }.toSet()
                // The first list after start is the baseline: banners yes, notifications no.
                val fresh = rt.recordingsKnown?.let { ids - it }.orEmpty()
                rt.recordingsKnown = ids
                onRecordings(rt, event.items, fresh)
            }
            is CoreEvent.MessageNotices -> onNotices(rt, event)
            // Read, archived or gone: after the notices before it (they wait on the main thread).
            is CoreEvent.NoticesCleared -> scope.launch {
                for (token in event.tokens) MessageNotifications.cancel(context, rt.id, token)
            }
        }
    }

    /** Replaces the pending bubbles of a chat by the outbox state. */
    private fun withPending(messages: List<UiMessage>, pending: List<PendingMessage>): List<UiMessage> =
        messages.filter { it.status == null } + pending.map { it.toUi() }

    private fun PendingMessage.toUi() = UiMessage(
        msg = message,
        status = when (state) {
            PendingState.SENDING -> SendStatus.SENDING
            PendingState.WAITING -> SendStatus.WAITING
            PendingState.FAILED -> SendStatus.FAILED
        },
        localFile = localPath,
        progress = progress,
        error = error,
    )

    /** Notification for messages that finally failed while the user was not looking. */
    private fun announceFailures(rt: Runtime, token: String, items: List<PendingMessage>) {
        val failedNow = items.filter { it.state == PendingState.FAILED }.map { it.message.referenceId }
        val fresh = failedNow.filter { rt.notifiedFailures.add(it) }
        if (fresh.isEmpty()) return
        val st = _state.value
        val visible = st.accountId == rt.id && st.activeToken == token &&
            ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)
        if (!visible) {
            val room = rt.rooms.find { it.token == token }?.name ?: "Unterhaltung"
            Notifications.sendFailed(context, rt.id, label(rt), token, room)
        }
    }

    /**
     * New messages in the open chat. A shown line counts them by itself (they are loaded and
     * newer). Without one, a line starts above them when the user is not at the bottom or the
     * app is not in front; otherwise they are simply read, and the hidden snapshot is dropped
     * so they do not raise a line later.
     */
    private fun trackUnreadLine(account: String, token: String, incoming: List<MessageView>) {
        val st = _state.value
        if (st.accountId != account || st.activeToken != token) return
        val chat = st.chats[token] ?: return
        val line = st.unreadLine?.takeIf { it.token == token }
        if (chat.hasNewer) {
            // A window is never at the bottom: the line starts above the first message kept
            // for its end, unless the line from opening counts unread ones already.
            val fresh = incoming.filter { !it.isOwn && it.kind != MessageKind.SYSTEM }
            if (fresh.isEmpty()) return
            val counting = line != null && (
                line.base > 0 || chat.messages.any { countsUnread(it, line.from) } ||
                    chat.held.any { it.id > line.from && !it.isOwn && it.kind != MessageKind.SYSTEM }
                )
            if (!counting) _state.update { it.copy(unreadLine = UnreadLine(token, fresh.minOf { m -> m.id } - 1, 0)) }
            return
        }
        if (!chat.synced) return
        val newest = chat.messages.lastOrNull { it.status == null }?.msg?.id ?: return
        if (incoming.none { it.id > newest && !it.isOwn && it.kind != MessageKind.SYSTEM }) return
        if (line != null && chat.messages.any { countsUnread(it, line.from) }) return
        val inFront = ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)
        val next = if (!chatAtBottom || !inFront) UnreadLine(token, newest, 0) else null
        _state.update { it.copy(unreadLine = next) }
    }

    /** Whether the open chat shows its newest message (reported by the chat screen). */
    @Volatile private var chatAtBottom = true

    fun setChatAtBottom(atBottom: Boolean) {
        chatAtBottom = atBottom
    }

    /** Own message sent (any way): the Ungelesen-Linie goes away. */
    fun clearUnreadLine() {
        if (_state.value.unreadLine != null) _state.update { it.copy(unreadLine = null) }
    }

    /** Changes a loaded chat of [account]; nothing when another account is shown meanwhile. */
    private fun updateChat(account: String, token: String, f: (ChatState) -> ChatState) {
        _state.update { s ->
            if (s.accountId != account) return@update s
            val chat = s.chats[token] ?: return@update s
            s.copy(chats = s.chats + (token to f(chat)))
        }
    }

    private fun merge(existing: List<UiMessage>, incoming: List<MessageView>): List<UiMessage> {
        val byId = LinkedHashMap<Long, UiMessage>()
        val pending = ArrayList<UiMessage>()
        for (m in existing) if (m.status != null) pending += m else byId[m.msg.id] = m
        for (m in incoming) {
            byId[m.id] = UiMessage(m)
            if (m.referenceId.isNotEmpty()) pending.removeAll { it.msg.referenceId == m.referenceId }
        }
        return byId.values.sortedBy { it.msg.id } + pending
    }

    /**
     * New room list. When the open chat disappeared (removed, group deleted, left) it closes;
     * „Du bist nicht mehr in dieser Gruppe“ only when we did not leave or delete it ourselves.
     */
    private fun onRooms(rt: Runtime, rooms: List<RoomView>) {
        val before = _state.value
        if (before.accountId != rt.id) return
        _state.update { if (it.accountId == rt.id) it.copy(rooms = rooms, roomsRev = it.roomsRev + 1) else it }
        val active = before.activeToken ?: return
        val gone = before.rooms.find { it.token == active } ?: return
        if (rooms.any { it.token == active }) return
        closeRoom()
        val self = leaving.remove(active)
        if (!self && gone.roomType.toInt() in setOf(2, 3)) {
            Toast.makeText(context, "Du bist nicht mehr in dieser Gruppe", Toast.LENGTH_LONG).show()
        }
    }

    // --- groups (docs/groups.md) -------------------------------------------------------

    private val _participants = MutableSharedFlow<String>(extraBufferCapacity = 16)
    /** Tokens whose member list changed (`participantsChanged`): reload an open list. */
    val participantsChanged: SharedFlow<String> = _participants
    /** Groups we are leaving or deleting ourselves (no „nicht mehr in dieser Gruppe“). */
    private val leaving = ConcurrentHashMap.newKeySet<String>()

    fun canCreateGroups(): Boolean = session?.canCreateGroups() ?: false

    /** Users for „Neue Nachricht“; `null` when the search failed. */
    suspend fun searchUsers(query: String, excludeToken: String? = null): List<UserView>? {
        val s = session ?: return null
        return try {
            s.searchUsers(query, excludeToken)
        } catch (e: CoreException) {
            null
        }
    }

    /** Members of a group; `null` when they could not be loaded. */
    suspend fun members(token: String): List<MemberView>? {
        val s = session ?: return null
        return try {
            s.members(token)
        } catch (e: CoreException) {
            null
        }
    }

    /**
     * Opens the one-to-one chat with [userId]; `false` (and a toast) on failure. [over]: from
     * a mention in the open chat, „Zurück“ leads there again.
     */
    suspend fun openDirect(userId: String, over: Boolean = false): Boolean {
        val s = session ?: return false
        val r = s.openDirect(userId)
        val token = r.token ?: return toast(r.error ?: "Chat konnte nicht geöffnet werden")
        if (over) openOver(token) else openRoom(token)
        return true
    }

    /** The one-to-one chat with a person when there is one already; creates nothing. */
    fun existingDirect(userId: String): String? = session?.existingDirect(userId)

    /** Opens a chat from a mention in the open one (docs/mentions.md „Antippen“): „Zurück“ leads back. */
    fun openOver(token: String) = showRoom(token, followed = false, nav = Nav.OVER)

    /** Creates a group and opens it; `false` (and a toast) on failure. */
    suspend fun createGroup(name: String, userIds: List<String>, photo: ByteArray?): Boolean {
        val s = session ?: return false
        val r = s.createGroup(name, userIds, photo)
        val token = r.token ?: return toast(r.error ?: "Gruppe konnte nicht erstellt werden")
        openRoom(token)
        return true
    }

    /** Runs a group change; the core's error text becomes a toast. `true` on success. */
    suspend fun groupAction(action: suspend (TalkSession) -> String?): Boolean {
        val s = session ?: return toast("Keine Verbindung")
        val error = action(s) ?: return true
        return toast(error)
    }

    /** Leaves or deletes a group; on success chat and settings close (no „nicht mehr“ hint). */
    suspend fun leaveOrDelete(token: String, delete: Boolean): Boolean {
        leaving += token
        val ok = groupAction { if (delete) it.deleteGroup(token) else it.leaveGroup(token) }
        if (!ok) {
            leaving -= token
        } else if (_state.value.activeToken == token) {
            closeRoom()
            leaving -= token
        }
        return ok
    }

    /** Whether we must appoint a moderator before leaving; `null` (and a toast) on failure. */
    suspend fun leaveNeedsModerator(token: String): Boolean? {
        val s = session ?: return null
        return try {
            s.leaveNeedsModerator(token)
        } catch (e: CoreException) {
            toast("Keine Verbindung")
            null
        }
    }

    fun groupLink(token: String): String = session?.groupLink(token).orEmpty()

    // --- chat menu (docs/settings.md) --------------------------------------------------

    /** Runs a chat-menu change in the background; the core's error text becomes a toast. */
    fun chatAction(action: suspend (TalkSession) -> String?) {
        scope.launch { groupAction(action) }
    }

    /** The server keeps categories: the chat menu offers „Kategorie“. */
    fun categoriesSupported(): Boolean = session?.categoriesSupported() ?: false

    /** All categories in order („Favoriten“ and „Chats“ included). */
    fun categories(): List<CategoryView> = session?.categories().orEmpty()

    /** „Neue Kategorie …“ from the chat menu, with these chats in it; `false` (and a toast) on failure. */
    suspend fun createCategory(name: String, tokens: List<String>): Boolean {
        val s = session ?: return toast("Keine Verbindung")
        val error = s.createCategory(name, tokens).error ?: return true
        return toast(error)
    }

    private fun toast(text: String): Boolean {
        Toast.makeText(context, text, Toast.LENGTH_SHORT).show()
        return false
    }

    // --- chat actions ----------------------------------------------------------------

    /**
     * Opens a chat. [followed] is a tap on its notification: if the chat is open already it
     * keeps its messages and its poll, but the view stands as after a new opening, at the
     * Ungelesen-Linie or at the end (docs/notifications.md „War der Chat schon offen“).
     */
    fun openRoom(token: String, followed: Boolean = false) = showRoom(token, followed, Nav.FRESH)

    /** How a chat is reached: anew, over the open one (from a mention), or back to the one below. */
    private enum class Nav { FRESH, OVER, BACK }

    private fun showRoom(token: String, followed: Boolean, nav: Nav) {
        val rt = active ?: return
        val s = rt.session
        val previous = _state.value.activeToken
        MessageNotifications.cancel(context, rt.id, token)
        // Snapshot before anything can set the read marker (docs/chat.md).
        val line = unreadLineAtOpening(token)
        if (previous == token) {
            if (followed) {
                leaveWindow(token)
                chatAtBottom = true
                _state.update { it.copy(unreadLine = line, chatFollowed = it.chatFollowed + 1) }
            }
            return
        }
        previous?.let { s.closeChat(it) }
        // Cached messages and unsent ones (both survive restarts): shown in the first frame.
        val cached = s.cachedChat(token)
        chatAtBottom = true
        jumpSeq++
        val rev = listRev.incrementAndGet()
        _state.update { st ->
            // What a window left behind is not the end of the chat: start from the cache.
            val chat = st.chats[token]?.takeIf { !it.hasNewer } ?: ChatState()
            val messages = merge(chat.messages, cached.messages)
            val older = cached.oldestId != null && (chat.oldestId == null || cached.oldestId!! < chat.oldestId!!)
            st.copy(
                activeToken = token,
                unreadLine = line,
                jump = null,
                returnTo = when (nav) {
                    Nav.FRESH -> emptyList()
                    // A chat that is below already is not stacked twice.
                    Nav.OVER -> (st.returnTo + listOfNotNull(previous)).takeWhile { it != token }
                    Nav.BACK -> st.returnTo.takeWhile { it != token }
                },
                navBack = nav == Nav.BACK,
                chats = st.chats + (token to chat.copy(
                    mentions = cached.unreadMentions,
                    mentionsSeen = emptySet(),
                    loading = messages.none { it.status == null },
                    synced = false,
                    messages = withPending(messages, s.pending(token)),
                    hasMore = if (older) cached.hasMore else chat.hasMore,
                    oldestId = if (older) cached.oldestId else chat.oldestId,
                    lastCommonRead = maxOf(chat.lastCommonRead, cached.lastCommonRead ?: 0),
                    newestId = null,
                    loadingOlder = false,
                    rev = rev,
                )),
            )
        }
        scope.launch {
            if (!onOpened(rt, token, try { s.openChat(token) } catch (e: CoreException) { null })) return@launch
            // With a line the chat opens there; the screen marks read once at the bottom.
            if (_state.value.accountId != rt.id) return@launch
            val loaded = _state.value.chats[token]?.messages.orEmpty()
            if (line == null || loaded.none { countsUnread(it, line.from) }) markVisibleRead(token)
        }
    }

    private fun unreadLineAtOpening(token: String): UnreadLine? =
        _state.value.rooms.find { it.token == token }?.takeIf { it.lastReadMessage > 0 }
            ?.let { UnreadLine(token, it.lastReadMessage, it.unreadMessages.toInt()) }

    /**
     * The newest page of an opening ([page] null: it failed). A window into the history keeps
     * its list, `loadNewer` leads to this page. Returns whether the page arrived.
     */
    private fun onOpened(rt: Runtime, token: String, page: ChatPage?): Boolean {
        val room = _state.value.rooms.find { it.token == token }
        if (page != null) updateChat(rt.id, token) { it.withMentions(page.unreadMentions) }
        updateChat(rt.id, token) {
            // A chat that opens at a message keeps its spinner until the window is there.
            val waiting = it.hasNewer && it.loading
            if (page == null) return@updateChat it.copy(loading = waiting, synced = true)
            val read = maxOf(it.lastCommonRead, page.lastCommonRead ?: 0, room?.lastCommonReadMessage ?: 0)
            if (it.hasNewer) return@updateChat it.copy(lastCommonRead = read, loading = waiting, synced = true)
            // Outdated cache, or a hole between what was shown and the newest page (more
            // than a page arrived meanwhile): drop what was shown (unsent messages stay).
            val shownNewest = it.messages.lastOrNull { m -> m.status == null }?.msg?.id
            val hole = shownNewest != null && page.oldestId != null && shownNewest < page.oldestId!!
            if ((page.reset || hole) && it.newestId != null) {
                // The window ended where the cache did, and the chat goes on behind a gap: a window again.
                return@updateChat it.copy(
                    messages = it.messages.filter { m -> m.status == null },
                    hasNewer = true,
                    lastCommonRead = read,
                    loading = false,
                    synced = true,
                )
            }
            val base = if (page.reset || hole) it.messages.filter { m -> m.status != null } else it.messages
            // A window that reached the end may reach further back than the newest page.
            val own = it.newestId != null && it.oldestId != null && (page.oldestId == null || it.oldestId <= page.oldestId!!)
            it.copy(
                messages = merge(base, page.messages),
                hasMore = if (own) it.hasMore else page.hasMore,
                oldestId = if (own) it.oldestId else page.oldestId,
                lastCommonRead = maxOf(page.lastCommonRead ?: 0, room?.lastCommonReadMessage ?: 0),
                loading = false,
                synced = true,
            )
        }
        return page != null
    }

    fun closeRoom() {
        val st = _state.value
        val token = st.activeToken ?: return
        // Opened from a mention: back to the chat it stood in.
        val below = st.returnTo.lastOrNull { t -> st.rooms.any { it.token == t } }
        if (below != null) return showRoom(below, followed = false, nav = Nav.BACK)
        session?.closeChat(token)
        jumpSeq++
        _state.update { it.copy(activeToken = null, unreadLine = null, jump = null, returnTo = emptyList(), navBack = true) }
    }

    fun loadOlder(token: String) {
        val rt = active ?: return
        val s = rt.session
        val chat = _state.value.chats[token] ?: return
        val before = chat.oldestId ?: return
        if (!chat.hasMore || chat.loadingOlder) return
        val rev = chat.rev
        updateChat(rt.id, token) { it.copy(loadingOlder = true) }
        scope.launch {
            val page = try { s.loadOlder(token, before) } catch (e: CoreException) { null }
            updateChat(rt.id, token) {
                when {
                    // The list was replaced meanwhile (window, jump to the end).
                    it.rev != rev -> it
                    page == null -> it.copy(loadingOlder = false)
                    else -> it.copy(
                        messages = merge(it.messages, page.messages),
                        hasMore = page.hasMore,
                        oldestId = page.oldestId ?: it.oldestId,
                        loadingOlder = false,
                    ).withMentions(page.unreadMentions)
                }
            }
        }
    }

    // --- search and the window into the history (docs/search.md) --------------------------

    private var jumpSeq = 0
    private val listRev = AtomicInteger()

    /** Searches the messages on the device, in one chat or ([token] null) in all; off the main thread. */
    suspend fun searchMessages(query: String, token: String?, limit: Int): SearchResults? {
        val s = session ?: return null
        return withContext(Dispatchers.IO) { runCatching { s.searchMessages(query, token, limit.toUInt()) }.getOrNull() }
    }

    /** The same with the server's hits added; null when that failed (no network). */
    suspend fun searchMessagesEverywhere(query: String, token: String?, limit: Int): SearchResults? {
        val s = session ?: return null
        return try { s.searchMessagesEverywhere(query, token, limit.toUInt()) } catch (e: CoreException) { null }
    }

    /**
     * Opens a chat at a message (a hit of the search over all chats). The window around the
     * message is asked for first: if that fails at once (no network), the search stays as it
     * was. After [AROUND_WAIT_MS] the chat opens anyway and waits there. `openChat` starts
     * the live messages as always, but the list is the window, never the end of the chat.
     */
    fun openRoomAt(token: String, messageId: Long) {
        val rt = active ?: return
        val s = rt.session
        if (_state.value.activeToken == token) return jumpTo(token, messageId)
        val seq = ++jumpSeq
        scope.launch {
            val around = async {
                try { Result.success(s.messagesAround(token, messageId)) } catch (e: CoreException) { Result.failure(e) }
            }
            val early = withTimeoutOrNull(AROUND_WAIT_MS) { around.await() }
            // Something else was opened meanwhile.
            if (jumpSeq != seq || active !== rt) return@launch
            (early?.exceptionOrNull() as? CoreException)?.let {
                Toast.makeText(context, jumpError(it), Toast.LENGTH_SHORT).show()
                return@launch
            }
            val previous = _state.value.activeToken
            MessageNotifications.cancel(context, rt.id, token)
            val line = unreadLineAtOpening(token)
            previous?.let { s.closeChat(it) }
            chatAtBottom = false
            val rev = listRev.incrementAndGet()
            _state.update { st ->
                st.copy(
                    activeToken = token,
                    unreadLine = line,
                    jump = Jump(token, messageId, seq, ready = false),
                    returnTo = emptyList(),
                    navBack = false,
                    chats = st.chats + (token to ChatState(hasNewer = true, lastCommonRead = st.chats[token]?.lastCommonRead ?: 0, rev = rev)),
                )
            }
            launch { onOpened(rt, token, try { s.openChat(token) } catch (e: CoreException) { null }) }
            val page = (early ?: around.await()).getOrElse { e ->
                val open = _state.value.accountId == rt.id && _state.value.activeToken == token
                val waiting = open && _state.value.chats[token]?.let { it.hasNewer && it.loading } == true
                if (jumpSeq == seq && e is CoreException) Toast.makeText(context, jumpError(e), Toast.LENGTH_SHORT).show()
                // Nothing to show there: the chat stands at its end, as after a usual opening.
                if (waiting) leaveWindow(token)
                return@launch
            }
            if (_state.value.accountId != rt.id || _state.value.activeToken != token) return@launch
            // Something else was asked for meanwhile; then this window only ends the waiting.
            if (jumpSeq != seq && _state.value.chats[token]?.loading != true) return@launch
            showWindow(rt, token, page, seq)
        }
    }

    /**
     * Shows a message of the open chat (hit of the search in the chat, tapped quote): scrolls
     * to it when it is loaded, else the window around it replaces the list.
     */
    fun jumpTo(token: String, messageId: Long) {
        val rt = active ?: return
        val st = _state.value
        val chat = st.chats[token] ?: return
        if (st.activeToken != token) return
        val seq = ++jumpSeq
        if (chat.messages.any { it.status == null && it.msg.id == messageId }) {
            _state.update { it.copy(jump = Jump(token, messageId, seq, ready = true)) }
            return
        }
        scope.launch {
            val page = try {
                rt.session.messagesAround(token, messageId)
            } catch (e: CoreException) {
                if (jumpSeq == seq) Toast.makeText(context, jumpError(e), Toast.LENGTH_SHORT).show()
                return@launch
            }
            if (jumpSeq != seq || _state.value.accountId != rt.id || _state.value.activeToken != token) return@launch
            _state.update { it.copy(jump = Jump(token, messageId, seq, ready = false)) }
            showWindow(rt, token, page, seq)
        }
    }

    private fun jumpError(e: CoreException) = if (e is CoreException.Network) "Keine Verbindung" else "Nachricht nicht gefunden"

    /** Replaces the list of a chat by a window; the jump [seq] that waits for it becomes ready. */
    private fun showWindow(rt: Runtime, token: String, page: ChatPage, seq: Int) {
        val pending = rt.session.pending(token)
        val rev = listRev.incrementAndGet()
        updateChat(rt.id, token) { c ->
            val next = c.copy(
                messages = merge(emptyList(), page.messages),
                hasMore = page.hasMore,
                oldestId = page.oldestId,
                newestId = page.newestId,
                lastCommonRead = maxOf(c.lastCommonRead, page.lastCommonRead ?: 0),
                // Coming from the end of the chat nothing is kept: the cache has it all.
                held = if (c.hasNewer) c.held else emptyList(),
                hasNewer = true,
                loading = false,
                loadingOlder = false,
                loadingNewer = false,
                rev = rev,
            )
            (if (page.hasNewer) next else atEnd(next, pending)).withMentions(page.unreadMentions)
        }
        _state.update { if (it.jump?.seq == seq) it.copy(jump = it.jump.copy(ready = true)) else it }
    }

    /** The window reached the end of the chat: what arrived meanwhile follows, then the unsent messages. */
    private fun atEnd(c: ChatState, pending: List<PendingMessage>): ChatState {
        val last = c.messages.lastOrNull { it.status == null }?.msg?.id ?: 0
        return c.copy(
            messages = withPending(merge(c.messages, c.held.filter { it.id > last }), pending),
            hasNewer = false,
            held = emptyList(),
        )
    }

    /** The next page below a window; at the end of the chat the list goes on as usual. */
    fun loadNewer(token: String) {
        val rt = active ?: return
        val s = rt.session
        val chat = _state.value.chats[token] ?: return
        val after = chat.newestId ?: return
        if (!chat.hasNewer || chat.loadingNewer || chat.loading) return
        val rev = chat.rev
        updateChat(rt.id, token) { it.copy(loadingNewer = true) }
        scope.launch {
            val page = try { s.loadNewer(token, after) } catch (e: CoreException) { null }
            val pending = s.pending(token)
            updateChat(rt.id, token) {
                when {
                    it.rev != rev -> it
                    page == null -> it.copy(loadingNewer = false)
                    else -> {
                        val next = it.copy(
                            messages = merge(it.messages, page.messages),
                            newestId = page.newestId ?: it.newestId,
                            loadingNewer = false,
                        )
                        (if (page.hasNewer) next else atEnd(next, pending)).withMentions(page.unreadMentions)
                    }
                }
            }
        }
    }

    /**
     * „Zum Ende springen“ in a window, and before sending from one: the cached end of the
     * chat replaces the list, with what arrived meanwhile and the unsent messages.
     */
    fun leaveWindow(token: String) {
        val rt = active ?: return
        if (_state.value.chats[token]?.hasNewer != true) return
        val cached = rt.session.cachedChat(token)
        val pending = rt.session.pending(token)
        val rev = listRev.incrementAndGet()
        jumpSeq++
        updateChat(rt.id, token) { c ->
            if (!c.hasNewer) return@updateChat c
            atEnd(
                c.copy(
                    messages = merge(emptyList(), cached.messages),
                    hasMore = cached.hasMore,
                    oldestId = cached.oldestId,
                    newestId = null,
                    lastCommonRead = maxOf(c.lastCommonRead, cached.lastCommonRead ?: 0),
                    // Nothing cached and the newest page still on its way: it fills the list.
                    loading = cached.messages.isEmpty() && !c.synced,
                    loadingOlder = false,
                    loadingNewer = false,
                    rev = rev,
                    endRev = c.endRev + 1,
                ),
                pending,
            ).withMentions(cached.unreadMentions)
        }
        _state.update { if (it.jump?.token == token) it.copy(jump = null) else it }
    }

    /** Queues a text message in the outbox; it shows up at once and is delivered reliably. */
    /** [mentions]: what was picked from the suggestions; the core turns the names into the server's notation. */
    fun send(token: String, text: String, mentions: List<MentionPick>, replyTo: MessageView?) {
        val s = session ?: return
        leaveWindow(token)
        clearUnreadLine()
        s.enqueueText(token, text, mentions, replyTo?.toReply(), newReferenceId())
        OutboxWorker.schedule(context)
    }

    // --- mentions (docs/mentions.md) -----------------------------------------------------

    /** Who can be mentioned in a chat for what was typed after the „@“; answers from memory. */
    suspend fun mentionCandidates(token: String, query: String): List<MentionCandidate> =
        session?.mentionCandidates(token, query, MENTION_LIMIT).orEmpty()

    /** The bubbles of these mentions were seen (or jumped to): the „@“ button lets go of them. */
    fun mentionsSeen(token: String, ids: Collection<Long>) {
        val rt = active ?: return
        updateChat(rt.id, token) { c ->
            val seen = ids.filter { it in c.mentions }
            if (seen.isEmpty()) c else c.copy(mentions = c.mentions - seen.toSet(), mentionsSeen = c.mentionsSeen + seen)
        }
    }

    /** Text drafts live as long as the app does, per account and chat. */
    private val drafts = ConcurrentHashMap<String, TextDraft>()

    fun draft(token: String): TextDraft? = active?.let { drafts["${it.id}|$token"] }

    fun setDraft(token: String, draft: TextDraft?) {
        val key = "${(active ?: return).id}|$token"
        if (draft == null) drafts.remove(key) else drafts[key] = draft
    }

    private fun MessageView.toReply() = ReplyPreview(id, author, text, isOwn, attachment, location)

    /**
     * Sets the own reaction like Signal (docs/chat.md „Reaktionen“): the core replaces,
     * removes on the same emoji again and shows the change at once; errors become a toast.
     */
    fun setReaction(token: String, messageId: Long, emoji: String?) {
        val s = session ?: return
        scope.launch {
            s.setReaction(token, messageId, emoji)?.let { Toast.makeText(context, it, Toast.LENGTH_SHORT).show() }
        }
    }

    /**
     * Text and mentions for the input field when editing (docs/chat.md „Bearbeiten und
     * Löschen“); `null`: take `MessageView.text` and `MessageView.mentions`.
     */
    suspend fun editSource(token: String, messageId: Long): EditDraft? = session?.editSource(token, messageId)

    /**
     * Saves an edited message; the new version arrives as `MessagesUpdated` before [onDone].
     * An error becomes a toast and is handed to [onDone], so the caller keeps the edit mode open.
     * Runs in the manager's scope: leaving the chat does not cut the request off.
     */
    fun editMessage(token: String, messageId: Long, text: String, mentions: List<MentionPick>, onDone: (error: String?) -> Unit) {
        val s = session ?: return onDone(null)
        scope.launch {
            val error = s.editMessage(token, messageId, text, mentions)
            error?.let { Toast.makeText(context, it, Toast.LENGTH_SHORT).show() }
            onDone(error)
        }
    }

    /** Deletes a message for everybody; the deleted bubble comes from the core, errors become a toast. */
    fun deleteMessage(token: String, messageId: Long) {
        val s = session ?: return
        scope.launch {
            s.deleteMessage(token, messageId)?.let { Toast.makeText(context, it, Toast.LENGTH_SHORT).show() }
        }
    }

    /** Who reacted, own first; `null` when it could not be loaded. */
    suspend fun reactions(token: String, messageId: Long): List<ReactionView>? {
        val s = session ?: return null
        return try {
            s.reactions(token, messageId)
        } catch (e: CoreException) {
            null
        }
    }

    /**
     * Sends a prepared image (see [org.wolkenfunk.app.media.prepareImage]) with an optional
     * caption. The outbox takes over the file; the bubble shows the local image until
     * the server message replaces it without flicker.
     */
    fun sendImage(token: String, image: PreparedImage, caption: String, replyTo: MessageView?) {
        val rt = active ?: return
        val s = rt.session
        leaveWindow(token)
        clearUnreadLine()
        val ref = newReferenceId()
        val localKey = "${rt.id}|$ref"
        scope.launch {
            // Decode first so the pending bubble shows the photo from its first frame.
            withContext(Dispatchers.IO) { decodePreview(image.file) }?.let { localPreviews[localKey] = it }
            val queued = withContext(Dispatchers.IO) {
                runCatching {
                    s.enqueueFile(
                        token, image.file.path, image.name, "image/jpeg",
                        image.width.toLong(), image.height.toLong(), caption.trim(), emptyList(), replyTo?.toReply(), ref,
                    )
                }
            }
            if (queued.isFailure) {
                localPreviews.remove(localKey)
                image.file.delete()
                Toast.makeText(context, "Bild konnte nicht gespeichert werden", Toast.LENGTH_SHORT).show()
                return@launch
            }
            OutboxWorker.schedule(context)
        }
    }

    /**
     * Downscales/re-encodes [uri] off the main thread, then sends it like [sendImage].
     * [deleteSource]: a temporary camera capture to remove afterwards.
     */
    fun sendImageUri(token: String, uri: Uri, caption: String, replyTo: MessageView?, deleteSource: Boolean) {
        scope.launch {
            val prepared = withContext(Dispatchers.IO) {
                runCatching { prepareImage(context, uri) }.also {
                    if (deleteSource) uri.path?.let { p -> File(p).delete() }
                }.getOrNull()
            }
            if (prepared == null) {
                Toast.makeText(context, "Bild konnte nicht gelesen werden", Toast.LENGTH_SHORT).show()
                return@launch
            }
            sendImage(token, prepared, caption, replyTo)
        }
    }

    /**
     * Sends files unchanged (docs/attachments.md „Datei senden“): copies them into the staging
     * folder, then queues them in the given order, the quote on the first and [caption] on the
     * last one. If one cannot be read nothing is queued: [onDone] gets the unreadable ones, so
     * the send screen keeps the rest. Runs in the manager's scope, [onDone] on the main thread.
     */
    fun sendFiles(token: String, files: List<PickedFile>, caption: String, replyTo: MessageView?, onDone: (unreadable: List<Uri>) -> Unit) {
        val rt = active ?: return onDone(emptyList())
        leaveWindow(token)
        scope.launch {
            val prepared = ArrayList<PreparedFile>()
            val unreadable = ArrayList<Uri>()
            withContext(Dispatchers.IO) {
                for (f in files) {
                    runCatching { prepareFile(context, f) }.onSuccess { prepared += it }.onFailure { unreadable += f.uri }
                }
            }
            if (unreadable.isNotEmpty()) {
                prepared.forEach { it.file.delete() }
                Toast.makeText(context, "Datei konnte nicht gelesen werden", Toast.LENGTH_SHORT).show()
                onDone(unreadable)
                return@launch
            }
            if (_state.value.let { it.accountId == rt.id && it.activeToken == token }) clearUnreadLine()
            var failed = false
            for ((i, p) in prepared.withIndex()) {
                val ref = newReferenceId()
                val localKey = "${rt.id}|$ref"
                // An image shows itself in the pending bubble from the first frame, like a photo.
                if (p.mime.startsWith("image/")) {
                    withContext(Dispatchers.IO) { decodePreview(p.file) }?.let { localPreviews[localKey] = it }
                }
                val queued = withContext(Dispatchers.IO) {
                    runCatching {
                        rt.session.enqueueFile(
                            token, p.file.path, p.name, p.mime, p.width.toLong(), p.height.toLong(),
                            if (i == prepared.lastIndex) caption.trim() else "",
                            // The caption field of the preview has no suggestions (docs/mentions.md „Nicht im Umfang“).
                            emptyList(),
                            if (i == 0) replyTo?.toReply() else null,
                            ref,
                        )
                    }
                }
                if (queued.isFailure) {
                    localPreviews.remove(localKey)
                    p.file.delete()
                    failed = true
                }
            }
            if (failed) Toast.makeText(context, "Datei konnte nicht gespeichert werden", Toast.LENGTH_SHORT).show()
            OutboxWorker.schedule(context)
            onDone(emptyList())
        }
    }

    /** Files being fetched for opening, by account and file id: their tile shows a spinner. */
    private val openingFiles = mutableStateMapOf<String, Boolean>()

    /** Whether [fileId] is being fetched after a tap (snapshot state: the tile follows it). */
    fun isOpening(fileId: String): Boolean = active?.let { openingFiles["${it.id}|$fileId"] } == true

    /**
     * Fetches a received file and hands it to the app that opens it (docs/attachments.md
     * „Empfangene Datei öffnen“). The core keeps the file, so the next time needs no network.
     * A second tap meanwhile does nothing.
     */
    fun openFile(a: FileAttachment) {
        val rt = active ?: return
        val key = "${rt.id}|${a.id}"
        if (openingFiles.put(key, true) != null) return
        scope.launch {
            val shared = try {
                val path = rt.session.fetchFile(a.path, a.size)
                withContext(Dispatchers.IO) { openCopy(rt.id, a, File(path)) }
            } catch (e: Exception) {
                null
            }
            openingFiles.remove(key)
            if (shared == null) {
                Toast.makeText(context, "Datei konnte nicht geladen werden", Toast.LENGTH_SHORT).show()
                return@launch
            }
            // Loaded while the app went to the background: it is there for the next tap.
            if (!ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) return@launch
            val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", shared)
            val view = Intent(Intent.ACTION_VIEW)
                // A contact goes to the contacts app, which knows the card by this type.
                .setDataAndType(uri, if (a.contactName.isNotEmpty()) "text/x-vcard" else a.mimetype.ifBlank { "application/octet-stream" })
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            try {
                context.startActivity(view)
            } catch (e: ActivityNotFoundException) {
                Toast.makeText(context, "Keine App zum Öffnen von ${a.name}", Toast.LENGTH_SHORT).show()
            }
        }
    }

    /**
     * A copy of the fetched file under its real name, in the cache folder the FileProvider
     * serves: other apps go by the name and its ending, not by the core's name for it. (A hard
     * link would save the space, but Android does not let apps create one.) Blocking.
     */
    private fun openCopy(account: String, a: FileAttachment, source: File): File {
        val root = File(context.cacheDir, "open")
        // What was opened earlier goes after a day; the core still has the files.
        val old = System.currentTimeMillis() - 24 * 3600 * 1000L
        root.listFiles()?.filter { it.lastModified() < old }?.forEach { it.deleteRecursively() }
        val dir = File(root, "$account-${a.id}".replace(Regex("[^A-Za-z0-9._-]"), "_")).apply { mkdirs() }
        val name = a.name.replace('/', '_').replace('\u0000', '_').trim().trim('.').ifBlank { "Datei" }
        val target = File(dir, name)
        if (!target.exists() || target.length() != source.length()) source.copyTo(target, overwrite = true)
        dir.setLastModified(System.currentTimeMillis())
        return target
    }

    /**
     * Queues the own position as a place (docs/attachments.md „Standort“): it shows up at once
     * and waits for the network like a text. Talk has no quote for places.
     */
    fun sendLocation(token: String, latitude: Double, longitude: Double, name: String) {
        val s = session ?: return
        leaveWindow(token)
        clearUnreadLine()
        s.enqueueLocation(token, latitude, longitude, name, newReferenceId())
        OutboxWorker.schedule(context)
    }

    /**
     * Queues a contact (docs/attachments.md „Kontakt“): the core makes the vCard from [card],
     * which holds what was left ticked, and sends it like a file, with the quote.
     */
    fun sendContact(token: String, card: ContactCard, replyTo: MessageView?) {
        val rt = active ?: return
        leaveWindow(token)
        clearUnreadLine()
        scope.launch {
            val queued = withContext(Dispatchers.IO) {
                runCatching { rt.session.enqueueContact(token, card, replyTo?.toReply(), newReferenceId()) }
            }
            if (queued.isFailure) {
                Toast.makeText(context, "Kontakt konnte nicht gespeichert werden", Toast.LENGTH_SHORT).show()
                return@launch
            }
            OutboxWorker.schedule(context)
        }
    }

    /** Sends a failed message again (or a waiting one right now). */
    fun retry(token: String, msg: MessageView) {
        val rt = active ?: return
        rt.notifiedFailures.remove(msg.referenceId)
        rt.session.retrySend(msg.referenceId)
        OutboxWorker.schedule(context)
    }

    /** Drops a pending or failed message. */
    fun discard(msg: MessageView) {
        val rt = active ?: return
        localPreviews.remove("${rt.id}|${msg.referenceId}")
        rt.session.discard(msg.referenceId)
    }

    /** Hands the decoded local image to the server message so its bubble does not reload. */
    private fun adoptLocalPreviews(account: String, messages: List<MessageView>) {
        if (localPreviews.isEmpty()) return
        for (m in messages) {
            val bitmap = localPreviews.remove("$account|${m.referenceId}") ?: continue
            val id = m.attachment?.id ?: continue
            previews.put("$account|$id", bitmap)
            aspects["$account|$id"] = bitmap.width.toFloat() / bitmap.height
        }
    }

    /** Marks the newest message read while the chat is visible and the app is in front. */
    fun markVisibleRead(token: String) {
        val s = session ?: return
        if (!ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) return
        val st = _state.value
        val room = st.rooms.find { it.token == token } ?: return
        val chat = st.chats[token] ?: return
        // In a window into the history the read marker does not move (docs/search.md).
        if (chat.hasNewer) return
        val newest = chat.messages.lastOrNull { it.status == null }?.msg ?: return
        if (room.unreadMessages > 0 || newest.id > room.lastReadMessage) {
            scope.launch { runCatching { s.markRead(token, newest.id) } }
        }
    }

    fun refresh() = scope.launch { runCatching { session?.refreshRooms() } }

    /**
     * The chat list in sections (docs/categories.md): filtered, collapsed ones are empty.
     * `split`: „Gruppen“ and „Direktchats“ instead of „Chats“ (the device setting).
     */
    fun roomSections(query: String, activeToken: String?, split: Boolean): List<SectionView> =
        session?.roomSections(query, activeToken, split).orEmpty()

    /** „Sortierung“ of the shown account (docs/categories.md); read again when `roomsRev` changes. */
    fun sortOrder(): String = session?.sortOrder() ?: "activity"

    /** `activity` or `alphabetical`; the list follows via `RoomsChanged`, an error comes as a toast. */
    fun setSortOrder(order: String) {
        val s = session ?: return
        scope.launch {
            s.setSortOrder(order)?.let { Toast.makeText(context, it, Toast.LENGTH_LONG).show() }
        }
    }

    /** Opens or closes a section; the list follows via `RoomsChanged`, an error comes as a toast. */
    fun toggleSection(id: String) {
        val s = session ?: return
        scope.launch {
            s.toggleSection(id)?.let { Toast.makeText(context, it, Toast.LENGTH_LONG).show() }
        }
    }

    /** Already decoded avatar, for drawing the first frame without the initials flashing. */
    fun cachedAvatar(token: String, version: String, dark: Boolean, account: String? = null): ImageBitmap? =
        runtimeFor(account)?.let { avatars.get(avatarKey(it.id, token, version, dark)) }

    /**
     * Server-rendered conversation avatar; `null` for SVG avatars (initials are drawn instead).
     * [account]: of that account (e.g. the call's), else the active one.
     */
    suspend fun avatar(token: String, version: String, dark: Boolean, account: String? = null): ImageBitmap? {
        val rt = runtimeFor(account) ?: return null
        val key = avatarKey(rt.id, token, version, dark)
        avatars.get(key)?.let { return it }
        val bytes = try {
            rt.session.avatar(token, dark)
        } catch (e: CoreException) {
            return null
        }
        val bitmap = withContext(Dispatchers.Default) { decodeAvatar(bytes) } ?: return null
        avatars.put(key, bitmap)
        return bitmap
    }

    private fun avatarKey(account: String, token: String, version: String, dark: Boolean) = "$account|$token|$version|$dark"

    /** Already decoded chat image preview (synchronous, for the first frame). */
    fun cachedPreview(fileId: String): ImageBitmap? = active?.let { previews.get("${it.id}|$fileId") }

    /** Known aspect ratio (width / height) of a file's preview, if it was loaded before. */
    fun previewAspect(fileId: String): Float? = active?.let { aspects["${it.id}|$fileId"] }

    /** Local image of a pending send, until the server message replaces it. */
    fun localPreview(referenceId: String): ImageBitmap? = active?.let { localPreviews["${it.id}|$referenceId"] }

    /** Decodes the outbox copy of a pending image (e.g. after an app restart). */
    suspend fun loadLocalPreview(referenceId: String, path: String?): ImageBitmap? {
        val key = "${active?.id ?: return null}|$referenceId"
        localPreviews[key]?.let { return it }
        if (path == null) return null
        val bitmap = withContext(Dispatchers.IO) { decodePreview(File(path)) } ?: return null
        localPreviews[key] = bitmap
        return bitmap
    }

    /** Chat-sized preview of a shared image, from the server's preview endpoint. */
    suspend fun preview(fileId: String): ImageBitmap? {
        val rt = active ?: return null
        val key = "${rt.id}|$fileId"
        previews.get(key)?.let { return it }
        val bytes = try {
            rt.session.filePreview(fileId, PREVIEW_PX.toUInt(), PREVIEW_PX.toUInt())
        } catch (e: CoreException) {
            return null
        }
        val bitmap = withContext(Dispatchers.Default) { decodeBytes(bytes, previewEdge()) } ?: return null
        previews.put(key, bitmap)
        aspects[key] = bitmap.width.toFloat() / bitmap.height
        return bitmap
    }

    /** Large version for the full-screen viewer (not cached). */
    suspend fun fullImage(fileId: String): ImageBitmap? {
        val s = session ?: return null
        val bytes = try {
            s.filePreview(fileId, FULL_PX.toUInt(), FULL_PX.toUInt())
        } catch (e: CoreException) {
            return null
        }
        return withContext(Dispatchers.Default) { decodeBytes(bytes, FULL_PX) }
    }

    /** Longest edge a chat bubble shows, in px (bubbles are at most ~300dp wide). */
    private fun previewEdge() = (300 * context.resources.displayMetrics.density).toInt()

    private fun decodePreview(file: File): ImageBitmap? = decodeSampled(file, previewEdge())?.asImageBitmap()

    private fun decodeBytes(bytes: ByteArray, maxEdge: Int): ImageBitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0) return null
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxEdge) sample *= 2
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample })
            ?.asImageBitmap()
    }

    /** Decodes downsampled to roughly the largest size shown (avoids huge texture uploads while scrolling). */
    private fun decodeAvatar(bytes: ByteArray): ImageBitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0) return null
        val target = (AVATAR_MAX_DP * context.resources.displayMetrics.density).toInt()
        var sample = 1
        while (minOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= target) sample *= 2
        val opts = BitmapFactory.Options().apply { inSampleSize = sample }
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)?.asImageBitmap()
    }

    private companion object {
        const val AVATAR_MAX_DP = 56
        /** Requested preview size; the server keeps the aspect ratio inside this box. */
        const val PREVIEW_PX = 1024
        const val FULL_PX = 2048
        const val PREVIEW_CACHE_BYTES = 48 * 1024 * 1024
        /** How long a call push waits for the ringing to start. */
        const val RING_WAIT_MS = 3_000L
        /** How long a chat push keeps the push service waiting for the messages (usually under 0.5 s). */
        const val CHAT_FETCH_WAIT_MS = 4_000L
        /** A notification waits this long for the chat's picture (the core's cache answers at once). */
        const val AVATAR_WAIT_MS = 1_000L
        /** A broadcast receiver has about ten seconds. */
        const val ACTION_WAIT_MS = 8_000L
        const val TAG = "TalkAccounts"
    }
}
