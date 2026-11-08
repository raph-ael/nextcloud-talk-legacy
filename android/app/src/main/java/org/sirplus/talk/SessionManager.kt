package org.sirplus.talk

import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.LruCache
import android.widget.Toast
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.lifecycle.Lifecycle
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
import uniffi.talk_ffi.ReplyPreview
import uniffi.talk_ffi.RoomView
import uniffi.talk_ffi.TalkSession
import uniffi.talk_ffi.newReferenceId

enum class SessionState { LOADING, LOGGED_OUT, READY }
enum class SendStatus { SENDING, FAILED }

/**
 * A message as shown in the UI; optimistic messages carry a [status], optimistic file
 * messages also the [localFile] being uploaded.
 */
data class UiMessage(val msg: MessageView, val status: SendStatus? = null, val localFile: String? = null) {
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
    /** Decoded local images of optimistic sends, by reference id, until the server echo arrives. */
    private val localPreviews = ConcurrentHashMap<String, ImageBitmap>()

    init {
        val account = store.load()
        if (account == null) _state.update { it.copy(session = SessionState.LOGGED_OUT) }
        else startSession(account)
    }

    // --- session ------------------------------------------------------------------

    private fun startSession(account: StoredAccount) {
        val s = TalkSession(account.server, account.loginName, account.appPassword, ClientPlatform.ANDROID, this)
        session = s
        _state.update {
            it.copy(session = SessionState.READY, server = account.server, user = account.loginName)
        }
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
        _state.update { st ->
            val chat = st.chats[token] ?: ChatState()
            st.copy(activeToken = token, chats = st.chats + (token to chat.copy(loading = chat.messages.isEmpty())))
        }
        scope.launch {
            try {
                val page = s.openChat(token)
                val room = _state.value.rooms.find { it.token == token }
                updateChat(token) {
                    it.copy(
                        messages = merge(it.messages, page.messages),
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

    fun send(token: String, text: String, replyTo: MessageView?) {
        val optimistic = MessageView(
            id = -System.currentTimeMillis(),
            token = token,
            actorType = "users",
            actorId = "",
            author = "",
            timestamp = System.currentTimeMillis() / 1000,
            text = text,
            kind = MessageKind.TEXT,
            systemMessage = "",
            referenceId = newReferenceId(),
            isOwn = true,
            edited = false,
            markdown = true,
            replyTo = replyTo?.let { ReplyPreview(it.id, it.author, it.text, it.isOwn, it.attachment) },
            reactions = emptyMap(),
            reactionsSelf = emptyList(),
            attachment = null,
        )
        updateChat(token) { it.copy(messages = it.messages + UiMessage(optimistic, SendStatus.SENDING)) }
        deliver(token, optimistic)
    }

    /**
     * Sends a prepared image (see [org.sirplus.talk.media.prepareImage]) with an optional caption.
     * Shows it immediately from the local file; the server echo takes over without flicker.
     */
    fun sendImage(token: String, image: PreparedImage, caption: String, replyTo: MessageView?) {
        val ref = newReferenceId()
        val optimistic = MessageView(
            id = -System.currentTimeMillis(),
            token = token,
            actorType = "users",
            actorId = "",
            author = "",
            timestamp = System.currentTimeMillis() / 1000,
            text = caption.trim(),
            kind = MessageKind.TEXT,
            systemMessage = "",
            referenceId = ref,
            isOwn = true,
            edited = false,
            markdown = true,
            replyTo = replyTo?.let { ReplyPreview(it.id, it.author, it.text, it.isOwn, it.attachment) },
            reactions = emptyMap(),
            reactionsSelf = emptyList(),
            attachment = FileAttachment(
                id = "",
                name = image.name,
                path = "",
                mimetype = "image/jpeg",
                size = image.file.length(),
                width = image.width.toLong(),
                height = image.height.toLong(),
                previewAvailable = true,
                link = "",
                voiceMessage = false,
            ),
        )
        scope.launch {
            // Decode first so the optimistic bubble shows the photo from its first frame.
            withContext(Dispatchers.IO) { decodePreview(image.file) }?.let { localPreviews[ref] = it }
            updateChat(token) {
                it.copy(messages = it.messages + UiMessage(optimistic, SendStatus.SENDING, image.file.path))
            }
            deliverFile(token, optimistic, image.file.path)
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

    fun retry(token: String, msg: MessageView) {
        val local = _state.value.chats[token]?.messages?.find { it.msg.referenceId == msg.referenceId }?.localFile
        updateChat(token) { c ->
            c.copy(messages = c.messages.map { if (it.msg.referenceId == msg.referenceId) it.copy(status = SendStatus.SENDING) else it })
        }
        if (local != null) scope.launch { deliverFile(token, msg, local) } else deliver(token, msg)
    }

    private suspend fun deliverFile(token: String, msg: MessageView, path: String) {
        val s = session ?: return
        val a = msg.attachment ?: return
        try {
            s.sendFile(token, path, a.name, a.mimetype, msg.text.ifEmpty { null }, msg.replyTo?.id, msg.referenceId)
            // The message itself arrives through the live poll (matched by reference id).
        } catch (e: CoreException) {
            markFailed(token, msg.referenceId)
        }
    }

    /** Hands the decoded local image to the server message so its bubble does not reload. */
    private fun adoptLocalPreviews(messages: List<MessageView>) {
        if (localPreviews.isEmpty()) return
        for (m in messages) {
            val bitmap = localPreviews.remove(m.referenceId) ?: continue
            val id = m.attachment?.id ?: continue
            previews.put(id, bitmap)
            aspects[id] = bitmap.width.toFloat() / bitmap.height
            // The upload copy is no longer needed once the server has the file.
            _state.value.chats[m.token]?.messages?.find { it.msg.referenceId == m.referenceId }?.localFile
                ?.let { runCatching { File(it).delete() } }
        }
    }

    private fun markFailed(token: String, referenceId: String) {
        updateChat(token) { c ->
            c.copy(messages = c.messages.map {
                if (it.msg.referenceId == referenceId && it.status != null) it.copy(status = SendStatus.FAILED) else it
            })
        }
    }

    private fun deliver(token: String, msg: MessageView) {
        val s = session ?: return
        scope.launch {
            try {
                val sent = s.send(token, msg.text, msg.referenceId, msg.replyTo?.id)
                updateChat(token) { it.copy(messages = merge(it.messages, listOf(sent))) }
            } catch (e: CoreException) {
                markFailed(token, msg.referenceId)
            }
        }
    }

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

    /** Local image of an optimistic send, until the server message replaces it. */
    fun localPreview(referenceId: String): ImageBitmap? = localPreviews[referenceId]

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
