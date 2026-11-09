package org.sirplus.talk

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
import org.sirplus.talk.media.PreparedImage
import org.sirplus.talk.media.decodeSampled
import org.sirplus.talk.media.prepareImage
import java.io.File
import java.util.concurrent.ConcurrentHashMap
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
import uniffi.talk_ffi.ReplyPreview
import uniffi.talk_ffi.RoomView
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
)

data class UiState(
    val session: SessionState = SessionState.LOADING,
    val server: String = "",
    val user: String = "",
    val rooms: List<RoomView> = emptyList(),
    val chats: Map<String, ChatState> = emptyMap(),
    val online: Boolean = true,
    val activeToken: String? = null,
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

    init {
        val account = store.load()
        if (account == null) _state.update { it.copy(session = SessionState.LOGGED_OUT) }
        else startSession(account)
        // Network back: retry waiting messages right away instead of after the backoff.
        context.getSystemService(ConnectivityManager::class.java)?.registerDefaultNetworkCallback(
            object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) {
                    session?.networkChanged()
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
        val s = TalkSession(account.server, account.loginName, account.appPassword, ClientPlatform.ANDROID, this, dataDir)
        session = s
        _state.update {
            // Cached conversation list: shown in the first frame, before any request.
            it.copy(session = SessionState.READY, server = account.server, user = account.loginName, rooms = s.rooms())
        }
        // Unsent messages of an earlier run continue in the background worker as well.
        scheduleOutboxIfNeeded()
        scope.launch {
            try {
                s.start()
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
        session = null
        if (revoke) runCatching { s.logout() } else s.shutdown()
        s.close()
        store.clear()
        // Unsent messages belong to this account: never deliver them under another one.
        withContext(Dispatchers.IO) { File(context.noBackupFilesDir, "talk").deleteRecursively() }
        OutboxWorker.cancel(context)
        localPreviews.clear()
        notifiedFailures.clear()
        avatars.evictAll()
        previews.evictAll()
        _state.value = UiState(session = SessionState.LOGGED_OUT)
    }

    // --- core events (called on a Rust thread) ---------------------------------------

    override fun onEvent(event: CoreEvent) {
        when (event) {
            is CoreEvent.RoomsChanged -> _state.update { it.copy(rooms = event.rooms) }
            is CoreEvent.MessagesAdded -> {
                adoptLocalPreviews(event.messages)
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

    // --- chat actions ----------------------------------------------------------------

    fun openRoom(token: String) {
        val s = session ?: return
        val previous = _state.value.activeToken
        if (previous == token) return
        previous?.let { s.closeChat(it) }
        // Cached messages and unsent ones (both survive restarts): shown in the first frame.
        val cached = s.cachedChat(token)
        _state.update { st ->
            val chat = st.chats[token] ?: ChatState()
            val messages = merge(chat.messages, cached.messages)
            val older = cached.oldestId != null && (chat.oldestId == null || cached.oldestId!! < chat.oldestId!!)
            st.copy(
                activeToken = token,
                chats = st.chats + (token to chat.copy(
                    loading = messages.none { it.status == null },
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
                    // Outdated cache: drop what was shown (unsent messages stay).
                    val base = if (page.reset) it.messages.filter { m -> m.status != null } else it.messages
                    it.copy(
                        messages = merge(base, page.messages),
                        hasMore = page.hasMore,
                        oldestId = page.oldestId,
                        lastCommonRead = maxOf(page.lastCommonRead ?: 0, room?.lastCommonReadMessage ?: 0),
                        loading = false,
                    )
                }
                markVisibleRead(token)
            } catch (e: CoreException) {
                updateChat(token) { it.copy(loading = false) }
            }
        }
    }

    fun closeRoom() {
        val token = _state.value.activeToken ?: return
        session?.closeChat(token)
        _state.update { it.copy(activeToken = null) }
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
        s.enqueueText(token, text, replyTo?.toReply(), newReferenceId())
        OutboxWorker.schedule(context)
    }

    private fun MessageView.toReply() = ReplyPreview(id, author, text, isOwn, attachment)

    /**
     * Sends a prepared image (see [org.sirplus.talk.media.prepareImage]) with an optional
     * caption. The outbox takes over the file; the bubble shows the local image until
     * the server message replaces it without flicker.
     */
    fun sendImage(token: String, image: PreparedImage, caption: String, replyTo: MessageView?) {
        val s = session ?: return
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
    }
}
