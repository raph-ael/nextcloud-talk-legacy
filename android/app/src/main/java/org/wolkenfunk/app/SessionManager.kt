package org.wolkenfunk.app

import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.LruCache
import android.widget.Toast
import androidx.compose.ui.graphics.ImageBitmap
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
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.wolkenfunk.app.media.PreparedImage
import org.wolkenfunk.app.media.decodeSampled
import org.wolkenfunk.app.media.prepareImage
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import uniffi.talk_ffi.CallIceCandidate
import uniffi.talk_ffi.CallView
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
import uniffi.talk_ffi.decryptPush
import uniffi.talk_ffi.generatePushKey
import uniffi.talk_ffi.ClientPlatform
import uniffi.talk_ffi.CoreEvent
import uniffi.talk_ffi.CoreException
import uniffi.talk_ffi.CoreListener
import uniffi.talk_ffi.FileAttachment
import uniffi.talk_ffi.LoginFlow
import uniffi.talk_ffi.MessageKind
import uniffi.talk_ffi.MessageView
import uniffi.talk_ffi.OutboxIdle
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
)

/**
 * Ungelesen-Linie of the open chat (docs/chat.md „Datum und Ungelesen-Linie“), pure UI state
 * taken when opening the chat. Place and number derive from the loaded messages: above the
 * first one that [counts][countsUnread], number `max(base, loaded ones that count)`.
 */
data class UnreadLine(val token: String, val from: Long, val base: Int)

/** Counts for the Ungelesen-Linie: delivered, after [from], not own, no system message. */
fun countsUnread(m: UiMessage, from: Long): Boolean =
    m.status == null && m.msg.id > from && !m.msg.isOwn && m.msg.kind != MessageKind.SYSTEM

data class UiState(
    val session: SessionState = SessionState.LOADING,
    val server: String = "",
    val user: String = "",
    val rooms: List<RoomView> = emptyList(),
    /** Bumped on every `RoomsChanged`: sections may change (collapsed, categories) with equal rooms. */
    val roomsRev: Int = 0,
    val chats: Map<String, ChatState> = emptyMap(),
    val online: Boolean = true,
    val activeToken: String? = null,
    val unreadLine: UnreadLine? = null,
)

/**
 * App-wide state holder: owns the Rust [TalkSession], turns core events into an
 * immutable [UiState] and exposes the user actions.
 */
class SessionManager(private val context: Context) : CoreListener {
    private val store = AccountStore(context)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state
    private val _call = MutableStateFlow<CallView?>(null)
    /** The call in progress (also a ringing incoming one); `null`: none. */
    val call: StateFlow<CallView?> = _call

    private var session: TalkSession? = null
    private var loginFlow: LoginFlow? = null
    private val avatars = LruCache<String, ImageBitmap>(64)
    /** Chat image previews by file id, bounded by bytes. */
    private val previews = object : LruCache<String, ImageBitmap>(PREVIEW_CACHE_BYTES) {
        override fun sizeOf(key: String, value: ImageBitmap) = value.width * value.height * 4
    }
    /** Width/height of previews seen in this process, so bubbles keep their size on re-layout. */
    private val aspects = ConcurrentHashMap<String, Float>()
    /** Decoded local images of pending sends, by reference id, until the server echo arrives. */
    private val localPreviews = ConcurrentHashMap<String, ImageBitmap>()
    /** Failed sends already announced by a notification. */
    private val notifiedFailures = ConcurrentHashMap.newKeySet<String>()

    /** Voice messages: recording, drafts, playback. */
    val voice = VoiceManager(context, scope, { session }, { _state.value })

    /** Calls: everything around the media (tones, audio, notifications, call screen). */
    val calls = CallController(context, scope, CallActionsImpl()) {
        voice.interrupt()
        voice.player.stop()
    }

    private val _recordings = MutableStateFlow<List<RecordingNotice>>(emptyList())
    /** Finished call recordings (transcripts, summaries, failures) waiting for the user. */
    val recordings: StateFlow<List<RecordingNotice>> = _recordings
    /** Notices with a share/dismiss request in flight (buttons disabled meanwhile). */
    private val _recordingsBusy = MutableStateFlow<Set<Long>>(emptySet())
    val recordingsBusy: StateFlow<Set<Long>> = _recordingsBusy
    /** Notice ids of the last core list; `null` until the first one (the baseline) arrived. */
    private var recordingsKnown: Set<Long>? = null

    /** Push registration of this process run (again on every start and token change). */
    private var pushJob: Job? = null
    private var pushRegistered = false

    /** WebRTC for the core; one per process, handed to every session. */
    private val media = WebRtcMediaEngine(context, MediaReportsImpl(), calls)

    /** Video of the running call (received per peer, our camera). */
    val callVideo: CallVideo get() = media

    init {
        val account = store.load()
        if (account == null) _state.update { it.copy(session = SessionState.LOGGED_OUT) }
        else startSession(account)
        // Network back: retry waiting messages right away instead of after the backoff.
        context.getSystemService(ConnectivityManager::class.java)?.registerDefaultNetworkCallback(
            object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) {
                    session?.networkChanged()
                    // Registration failed offline: try again now.
                    scope.launch { if (!pushRegistered) registerPush() }
                }
            },
        )
        // Images prepared but never handed to the outbox (process died in between).
        scope.launch(Dispatchers.IO) {
            val cutoff = System.currentTimeMillis() - 3_600_000
            File(context.noBackupFilesDir, "talk/staging").listFiles()
                ?.filter { it.lastModified() < cutoff }
                ?.forEach { it.delete() }
        }
        // Leaving the app with unsent messages: hand them to the background worker.
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStop(owner: LifecycleOwner) = scheduleOutboxIfNeeded()
        })
    }

    /** The running session, for the background worker (`null` when signed out). */
    fun currentSession(): TalkSession? = session

    private fun scheduleOutboxIfNeeded() {
        val s = session ?: return
        if (s.outboxUnsent() > 0u) OutboxWorker.schedule(context)
    }

    // --- session ------------------------------------------------------------------

    private fun startSession(account: StoredAccount) {
        val dataDir = File(context.noBackupFilesDir, "talk").path
        recordingsKnown = null
        val s = TalkSession(account.server, account.loginName, account.appPassword, ClientPlatform.ANDROID, this, dataDir)
        session = s
        s.setMediaEngine(media)
        _state.update {
            // Cached conversation list: shown in the first frame, before any request.
            it.copy(session = SessionState.READY, server = account.server, user = account.loginName, rooms = s.rooms())
        }
        // Unsent messages of an earlier run continue in the background worker as well.
        scheduleOutboxIfNeeded()
        scope.launch {
            try {
                s.start()
                onRecordings(s.recordingNotices())
                registerPush()
            } catch (e: CoreException.Unauthorized) {
                signOut(revoke = false)
            } catch (e: CoreException) {
                // Offline start: the core keeps retrying in the background.
            }
        }
    }

    /** Login Flow v2: opens the browser, resolves once the user approved. */
    suspend fun login(server: String) {
        loginFlow?.cancel()
        val flow = LoginFlow.start(server, ClientPlatform.ANDROID)
        loginFlow = flow
        context.startActivity(
            Intent(Intent.ACTION_VIEW, Uri.parse(flow.loginUrl())).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        try {
            val result = flow.wait()
            val account = StoredAccount(result.server, result.loginName, result.appPassword)
            store.save(account)
            startSession(account)
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

    fun logout() = scope.launch { signOut(revoke = true) }

    private suspend fun signOut(revoke: Boolean) {
        val s = session ?: return
        voice.cancel()
        voice.player.stop()
        s.hangUpAndWait(2000u)
        pushJob?.cancel()
        if (revoke && pushRegistered) runCatching { s.unregisterPush() }
        pushRegistered = false
        Push.deleteToken(context)
        session = null
        if (revoke) runCatching { s.logout() } else s.shutdown()
        s.close()
        store.clear()
        // Unsent messages belong to this account: never deliver them under another one.
        withContext(Dispatchers.IO) { File(context.noBackupFilesDir, "talk").deleteRecursively() }
        OutboxWorker.cancel(context)
        localPreviews.clear()
        notifiedFailures.clear()
        _recordings.value = emptyList()
        recordingsKnown = null
        RecordingNotifications.clear(context)
        avatars.evictAll()
        previews.evictAll()
        _state.value = UiState(session = SessionState.LOGGED_OUT)
    }

    // --- push -------------------------------------------------------------------------

    /**
     * Registers this device at the server and the push proxy: on every start and with a
     * new FCM token ([token]). Main thread; a running registration is not repeated.
     */
    private fun registerPush(token: String? = null) {
        val s = session ?: return
        if (!Push.enabled(context)) return
        if (token == null && pushJob?.isActive == true) return
        pushJob?.cancel()
        pushJob = scope.launch {
            try {
                val fcm = token ?: Push.token()
                val key = store.pushKey() ?: generatePushKey().also { store.savePushKey(it) }
                s.registerPush(fcm, Push.proxyUrl, key)
                if (session === s) pushRegistered = true
                Log.i(Push.TAG, "registered")
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Push.TAG, "registration failed: ${e.message}")
            }
        }
    }

    /** FCM issued a new token (called on a background thread). */
    fun onPushToken(token: String) {
        scope.launch { registerPush(token) }
    }

    /**
     * A push arrived (FCM thread, process possibly just started). Returns once a call
     * rings here, so the ringing foreground service runs before FCM's window closes.
     */
    suspend fun handlePush(subject: String) {
        val key = store.pushKey()
        if (key == null) {
            Log.w(Push.TAG, "push without device key")
            return
        }
        val message = try {
            decryptPush(key, subject)
        } catch (e: CoreException) {
            Log.w(Push.TAG, "push not decryptable: ${e.message}")
            return
        }
        val s = withContext(Dispatchers.Main) { session } ?: return
        when (message) {
            is PushMessage.Call -> {
                Log.i(Push.TAG, "call push")
                if (!s.handlePush(message, null)) return
                withTimeoutOrNull(RING_WAIT_MS) { calls.ui.first { it?.token == message.token } }
                // The incoming call is handled on the main thread right after the state is set.
                withContext(Dispatchers.Main) { }
            }
            is PushMessage.Delete, PushMessage.DeleteAll -> s.handlePush(message, null)
            // Chat pushes are not forwarded by the proxy yet; the app has no message notifications.
            is PushMessage.Chat, is PushMessage.Other -> Unit
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

    /** Main thread. */
    private fun onRecordings(items: List<RecordingNotice>, fresh: Set<Long> = emptySet()) {
        _recordings.value = items
        _recordingsBusy.update { busy -> busy.filterTo(mutableSetOf()) { id -> items.any { it.id == id } } }
        RecordingNotifications.update(context, items, fresh)
    }

    fun startRecording(video: Boolean) = recordingAction("Die Aufnahme konnte nicht gestartet werden") { it.startRecording(video) }

    fun stopRecording() = recordingAction("Die Aufnahme konnte nicht beendet werden") { it.stopRecording() }

    /** Answer to the consent dialog: join the call although it may be recorded. */
    fun consentToRecording() {
        session?.consentToRecording()
    }

    /** "In Chat teilen": posts the file into its conversation. */
    fun shareRecording(id: Long) = noticeAction(id, "Teilen nicht möglich") { it.shareRecording(id) }

    /** "Verwerfen" / "OK": removes the notice (the file stays in the user's files). */
    fun dismissRecording(id: Long) = noticeAction(id, "Verwerfen nicht möglich") { it.dismissRecording(id) }

    private fun recordingAction(fallback: String, block: suspend (TalkSession) -> Unit) {
        val s = session ?: return
        scope.launch {
            try {
                block(s)
            } catch (e: CoreException) {
                Toast.makeText(context, coreMessage(e, fallback), Toast.LENGTH_LONG).show()
            }
        }
    }

    private fun noticeAction(id: Long, fallback: String, block: suspend (TalkSession) -> Unit) {
        val s = session ?: return
        if (id in _recordingsBusy.value) return
        _recordingsBusy.update { it + id }
        scope.launch {
            try {
                block(s)
                // The core sends RecordingsChanged; drop the notice right away anyway.
                onRecordings(_recordings.value.filter { it.id != id })
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

    private inner class CallActionsImpl : CallActions {
        override fun accept(token: String) {
            val s = session ?: return
            scope.launch {
                try {
                    s.acceptCall(token)
                } catch (e: CoreException) {
                    Toast.makeText(context, callError(e), Toast.LENGTH_LONG).show()
                }
            }
        }

        override fun decline(token: String) {
            session?.declineCall(token)
        }

        override fun hangUp() {
            session?.hangUp()
        }

        override fun setMuted(muted: Boolean) {
            session?.setCallMuted(muted)
        }

        override fun setCamera(on: Boolean) {
            session?.setCameraEnabled(on)
        }
    }

    private inner class MediaReportsImpl : MediaReports {
        override fun localDescription(peerId: String, type: SdpType, sdp: String) {
            session?.mediaLocalDescription(peerId, type, sdp)
        }

        override fun localCandidate(peerId: String, candidate: CallIceCandidate) {
            session?.mediaLocalCandidate(peerId, candidate)
        }

        override fun peerState(peerId: String, state: PeerState) {
            session?.mediaPeerState(peerId, state)
        }

        override fun peerError(peerId: String, message: String) {
            session?.mediaPeerError(peerId, message)
        }

        override fun audioLevel(peerId: String, level: Float) {
            session?.mediaAudioLevel(peerId, level)
        }

        override fun cameraFailed(reason: String) {
            session?.mediaCameraState(false)
            scope.launch(Dispatchers.Main) { Toast.makeText(context, reason, Toast.LENGTH_LONG).show() }
        }
    }

    /** Avatar of a call participant (group tiles). */
    suspend fun userAvatar(userId: String, dark: Boolean): ImageBitmap? {
        val key = "user|$userId|$dark"
        avatars.get(key)?.let { return it }
        val s = session ?: return null
        val bytes = try {
            s.userAvatar(userId, dark)
        } catch (e: CoreException) {
            return null
        }
        val bitmap = withContext(Dispatchers.Default) { decodeAvatar(bytes) } ?: return null
        avatars.put(key, bitmap)
        return bitmap
    }

    fun cachedUserAvatar(userId: String, dark: Boolean): ImageBitmap? = avatars.get("user|$userId|$dark")

    // --- core events (called on a Rust thread) ---------------------------------------

    override fun onEvent(event: CoreEvent) {
        when (event) {
            is CoreEvent.RoomsChanged -> scope.launch { onRooms(event.rooms) }
            is CoreEvent.ParticipantsChanged -> _participants.tryEmit(event.token)
            is CoreEvent.MessagesAdded -> {
                adoptLocalPreviews(event.messages)
                trackUnreadLine(event.token, event.messages)
                updateChat(event.token) { it.copy(messages = merge(it.messages, event.messages)) }
            }
            is CoreEvent.MessagesUpdated -> updateChat(event.token) { c ->
                val byId = event.messages.associateBy { it.id }
                c.copy(messages = c.messages.map { m -> byId[m.msg.id]?.let { UiMessage(it) } ?: m })
            }
            is CoreEvent.ReadStatus -> updateChat(event.token) {
                it.copy(lastCommonRead = maxOf(it.lastCommonRead, event.lastCommonRead))
            }
            is CoreEvent.Connection -> _state.update { it.copy(online = event.online) }
            CoreEvent.AuthFailed -> scope.launch { signOut(revoke = false) }
            is CoreEvent.OutboxChanged -> {
                updateChat(event.token) { c -> c.copy(messages = withPending(c.messages, event.items)) }
                announceFailures(event.token, event.items)
            }
            is CoreEvent.CallChanged -> scope.launch {
                _call.value = event.call
                calls.onCall(event.call)
            }
            is CoreEvent.CallMissed -> CallNotifications.showMissed(
                context, event.token, if (event.isGroup) event.roomName else event.callerName.ifBlank { event.roomName },
            )
            is CoreEvent.RecordingsChanged -> scope.launch {
                val ids = event.items.map { it.id }.toSet()
                // The first list after start is the baseline: banners yes, notifications no.
                val fresh = recordingsKnown?.let { ids - it }.orEmpty()
                recordingsKnown = ids
                onRecordings(event.items, fresh)
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
    private fun announceFailures(token: String, items: List<PendingMessage>) {
        val failedNow = items.filter { it.state == PendingState.FAILED }.map { it.message.referenceId }
        val fresh = failedNow.filter { notifiedFailures.add(it) }
        if (fresh.isEmpty()) return
        val st = _state.value
        val visible = st.activeToken == token &&
            ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)
        if (!visible) {
            val room = st.rooms.find { it.token == token }?.name ?: "Unterhaltung"
            Notifications.sendFailed(context, token, room)
        }
    }

    /**
     * New messages in the open chat. A shown line counts them by itself (they are loaded and
     * newer). Without one, a line starts above them when the user is not at the bottom or the
     * app is not in front; otherwise they are simply read, and the hidden snapshot is dropped
     * so they do not raise a line later.
     */
    private fun trackUnreadLine(token: String, incoming: List<MessageView>) {
        val st = _state.value
        if (st.activeToken != token) return
        val chat = st.chats[token] ?: return
        if (!chat.synced) return
        val newest = chat.messages.lastOrNull { it.status == null }?.msg?.id ?: return
        if (incoming.none { it.id > newest && !it.isOwn && it.kind != MessageKind.SYSTEM }) return
        val line = st.unreadLine?.takeIf { it.token == token }
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

    private fun updateChat(token: String, f: (ChatState) -> ChatState) {
        _state.update { s ->
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
    private fun onRooms(rooms: List<RoomView>) {
        val before = _state.value
        _state.update { it.copy(rooms = rooms, roomsRev = it.roomsRev + 1) }
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

    /** Opens the one-to-one chat with [userId]; `false` (and a toast) on failure. */
    suspend fun openDirect(userId: String): Boolean {
        val s = session ?: return false
        val r = s.openDirect(userId)
        val token = r.token ?: return toast(r.error ?: "Chat konnte nicht geöffnet werden")
        openRoom(token)
        return true
    }

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

    private fun toast(text: String): Boolean {
        Toast.makeText(context, text, Toast.LENGTH_SHORT).show()
        return false
    }

    // --- chat actions ----------------------------------------------------------------

    fun openRoom(token: String) {
        val s = session ?: return
        val previous = _state.value.activeToken
        if (previous == token) return
        previous?.let { s.closeChat(it) }
        // Cached messages and unsent ones (both survive restarts): shown in the first frame.
        val cached = s.cachedChat(token)
        // Snapshot before anything can set the read marker (docs/chat.md).
        val line = _state.value.rooms.find { it.token == token }?.takeIf { it.lastReadMessage > 0 }
            ?.let { UnreadLine(token, it.lastReadMessage, it.unreadMessages.toInt()) }
        chatAtBottom = true
        _state.update { st ->
            val chat = st.chats[token] ?: ChatState()
            val messages = merge(chat.messages, cached.messages)
            val older = cached.oldestId != null && (chat.oldestId == null || cached.oldestId!! < chat.oldestId!!)
            st.copy(
                activeToken = token,
                unreadLine = line,
                chats = st.chats + (token to chat.copy(
                    loading = messages.none { it.status == null },
                    synced = false,
                    messages = withPending(messages, s.pending(token)),
                    hasMore = if (older) cached.hasMore else chat.hasMore,
                    oldestId = if (older) cached.oldestId else chat.oldestId,
                    lastCommonRead = maxOf(chat.lastCommonRead, cached.lastCommonRead ?: 0),
                )),
            )
        }
        scope.launch {
            try {
                val page = s.openChat(token)
                val room = _state.value.rooms.find { it.token == token }
                updateChat(token) {
                    // Outdated cache, or a hole between what was shown and the newest page (more
                    // than a page arrived meanwhile): drop what was shown (unsent messages stay).
                    val shownNewest = it.messages.lastOrNull { m -> m.status == null }?.msg?.id
                    val hole = shownNewest != null && page.oldestId != null && shownNewest < page.oldestId!!
                    val base = if (page.reset || hole) it.messages.filter { m -> m.status != null } else it.messages
                    it.copy(
                        messages = merge(base, page.messages),
                        hasMore = page.hasMore,
                        oldestId = page.oldestId,
                        lastCommonRead = maxOf(page.lastCommonRead ?: 0, room?.lastCommonReadMessage ?: 0),
                        loading = false,
                        synced = true,
                    )
                }
                // With a line the chat opens there; the screen marks read once at the bottom.
                val loaded = _state.value.chats[token]?.messages.orEmpty()
                if (line == null || loaded.none { countsUnread(it, line.from) }) markVisibleRead(token)
            } catch (e: CoreException) {
                updateChat(token) { it.copy(loading = false, synced = true) }
            }
        }
    }

    fun closeRoom() {
        val token = _state.value.activeToken ?: return
        session?.closeChat(token)
        _state.update { it.copy(activeToken = null, unreadLine = null) }
    }

    fun loadOlder(token: String) {
        val s = session ?: return
        val chat = _state.value.chats[token] ?: return
        val before = chat.oldestId ?: return
        if (!chat.hasMore || chat.loadingOlder) return
        updateChat(token) { it.copy(loadingOlder = true) }
        scope.launch {
            try {
                val page = s.loadOlder(token, before)
                updateChat(token) {
                    it.copy(
                        messages = merge(it.messages, page.messages),
                        hasMore = page.hasMore,
                        oldestId = page.oldestId ?: it.oldestId,
                        loadingOlder = false,
                    )
                }
            } catch (e: CoreException) {
                updateChat(token) { it.copy(loadingOlder = false) }
            }
        }
    }

    /** Queues a text message in the outbox; it shows up at once and is delivered reliably. */
    fun send(token: String, text: String, replyTo: MessageView?) {
        val s = session ?: return
        clearUnreadLine()
        s.enqueueText(token, text, replyTo?.toReply(), newReferenceId())
        OutboxWorker.schedule(context)
    }

    private fun MessageView.toReply() = ReplyPreview(id, author, text, isOwn, attachment)

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
        val s = session ?: return
        clearUnreadLine()
        val ref = newReferenceId()
        scope.launch {
            // Decode first so the pending bubble shows the photo from its first frame.
            withContext(Dispatchers.IO) { decodePreview(image.file) }?.let { localPreviews[ref] = it }
            val queued = withContext(Dispatchers.IO) {
                runCatching {
                    s.enqueueFile(
                        token, image.file.path, image.name, "image/jpeg",
                        image.width.toLong(), image.height.toLong(), caption.trim(), replyTo?.toReply(), ref,
                    )
                }
            }
            if (queued.isFailure) {
                localPreviews.remove(ref)
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

    /** Sends a failed message again (or a waiting one right now). */
    fun retry(token: String, msg: MessageView) {
        notifiedFailures.remove(msg.referenceId)
        session?.retrySend(msg.referenceId)
        OutboxWorker.schedule(context)
    }

    /** Drops a pending or failed message. */
    fun discard(msg: MessageView) {
        localPreviews.remove(msg.referenceId)
        session?.discard(msg.referenceId)
    }

    /** Hands the decoded local image to the server message so its bubble does not reload. */
    private fun adoptLocalPreviews(messages: List<MessageView>) {
        if (localPreviews.isEmpty()) return
        for (m in messages) {
            val bitmap = localPreviews.remove(m.referenceId) ?: continue
            val id = m.attachment?.id ?: continue
            previews.put(id, bitmap)
            aspects[id] = bitmap.width.toFloat() / bitmap.height
        }
    }

    /** For the background worker: waits until nothing is being sent actively. */
    suspend fun waitOutboxIdle(): OutboxIdle? = session?.waitOutboxIdle()

    /** Marks the newest message read while the chat is visible and the app is in front. */
    fun markVisibleRead(token: String) {
        val s = session ?: return
        if (!ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) return
        val st = _state.value
        val room = st.rooms.find { it.token == token } ?: return
        val newest = st.chats[token]?.messages?.lastOrNull { it.status == null }?.msg ?: return
        if (room.unreadMessages > 0 || newest.id > room.lastReadMessage) {
            scope.launch { runCatching { s.markRead(token, newest.id) } }
        }
    }

    fun refresh() = scope.launch { runCatching { session?.refreshRooms() } }

    /** The chat list in sections (docs/categories.md): filtered, collapsed ones already reduced. */
    fun roomSections(query: String, activeToken: String?): List<SectionView> =
        session?.roomSections(query, activeToken).orEmpty()

    /** Opens or closes a section; the list follows via `RoomsChanged`, an error comes as a toast. */
    fun toggleSection(id: String) {
        val s = session ?: return
        scope.launch {
            s.toggleSection(id)?.let { Toast.makeText(context, it, Toast.LENGTH_LONG).show() }
        }
    }

    /** Already decoded avatar, for drawing the first frame without the initials flashing. */
    fun cachedAvatar(token: String, version: String, dark: Boolean): ImageBitmap? =
        avatars.get(avatarKey(token, version, dark))

    /** Server-rendered conversation avatar; `null` for SVG avatars (initials are drawn instead). */
    suspend fun avatar(token: String, version: String, dark: Boolean): ImageBitmap? {
        val key = avatarKey(token, version, dark)
        avatars.get(key)?.let { return it }
        val s = session ?: return null
        val bytes = try {
            s.avatar(token, dark)
        } catch (e: CoreException) {
            return null
        }
        val bitmap = withContext(Dispatchers.Default) { decodeAvatar(bytes) } ?: return null
        avatars.put(key, bitmap)
        return bitmap
    }

    private fun avatarKey(token: String, version: String, dark: Boolean) = "$token|$version|$dark"

    /** Already decoded chat image preview (synchronous, for the first frame). */
    fun cachedPreview(fileId: String): ImageBitmap? = previews.get(fileId)

    /** Known aspect ratio (width / height) of a file's preview, if it was loaded before. */
    fun previewAspect(fileId: String): Float? = aspects[fileId]

    /** Local image of a pending send, until the server message replaces it. */
    fun localPreview(referenceId: String): ImageBitmap? = localPreviews[referenceId]

    /** Decodes the outbox copy of a pending image (e.g. after an app restart). */
    suspend fun loadLocalPreview(referenceId: String, path: String?): ImageBitmap? {
        localPreviews[referenceId]?.let { return it }
        if (path == null) return null
        val bitmap = withContext(Dispatchers.IO) { decodePreview(File(path)) } ?: return null
        localPreviews[referenceId] = bitmap
        return bitmap
    }

    /** Chat-sized preview of a shared image, from the server's preview endpoint. */
    suspend fun preview(fileId: String): ImageBitmap? {
        previews.get(fileId)?.let { return it }
        val s = session ?: return null
        val bytes = try {
            s.filePreview(fileId, PREVIEW_PX.toUInt(), PREVIEW_PX.toUInt())
        } catch (e: CoreException) {
            return null
        }
        val bitmap = withContext(Dispatchers.Default) { decodeBytes(bytes, previewEdge()) } ?: return null
        previews.put(fileId, bitmap)
        aspects[fileId] = bitmap.width.toFloat() / bitmap.height
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
    }
}
