package org.sirplus.talk

import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.LruCache
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
import uniffi.talk_ffi.ClientPlatform
import uniffi.talk_ffi.CoreEvent
import uniffi.talk_ffi.CoreException
import uniffi.talk_ffi.CoreListener
import uniffi.talk_ffi.LoginFlow
import uniffi.talk_ffi.MessageKind
import uniffi.talk_ffi.MessageView
import uniffi.talk_ffi.ReplyPreview
import uniffi.talk_ffi.RoomView
import uniffi.talk_ffi.TalkSession
import uniffi.talk_ffi.newReferenceId

enum class SessionState { LOADING, LOGGED_OUT, READY }
enum class SendStatus { SENDING, FAILED }

/** A message as shown in the UI; optimistic messages carry a [status]. */
data class UiMessage(val msg: MessageView, val status: SendStatus? = null) {
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
        _state.value = UiState(session = SessionState.LOGGED_OUT)
    }

    // --- core events (called on a Rust thread) ---------------------------------------

    override fun onEvent(event: CoreEvent) {
        when (event) {
            is CoreEvent.RoomsChanged -> _state.update { it.copy(rooms = event.rooms) }
            is CoreEvent.MessagesAdded -> updateChat(event.token) { it.copy(messages = merge(it.messages, event.messages)) }
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
            replyTo = replyTo?.let { ReplyPreview(it.id, it.author, it.text, it.isOwn) },
            reactions = emptyMap(),
            reactionsSelf = emptyList(),
        )
        updateChat(token) { it.copy(messages = it.messages + UiMessage(optimistic, SendStatus.SENDING)) }
        deliver(token, optimistic)
    }

    fun retry(token: String, msg: MessageView) {
        updateChat(token) { c ->
            c.copy(messages = c.messages.map { if (it.msg.referenceId == msg.referenceId) it.copy(status = SendStatus.SENDING) else it })
        }
        deliver(token, msg)
    }

    private fun deliver(token: String, msg: MessageView) {
        val s = session ?: return
        scope.launch {
            try {
                val sent = s.send(token, msg.text, msg.referenceId, msg.replyTo?.id)
                updateChat(token) { it.copy(messages = merge(it.messages, listOf(sent))) }
            } catch (e: CoreException) {
                updateChat(token) { c ->
                    c.copy(messages = c.messages.map {
                        if (it.msg.referenceId == msg.referenceId && it.status != null) it.copy(status = SendStatus.FAILED) else it
                    })
                }
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

    /** Server-rendered conversation avatar; `null` for SVG avatars (initials are drawn instead). */
    suspend fun avatar(token: String, version: String, dark: Boolean): ImageBitmap? {
        val key = "$token|$version|$dark"
        avatars.get(key)?.let { return it }
        val s = session ?: return null
        val bytes = try {
            s.avatar(token, dark)
        } catch (e: CoreException) {
            return null
        }
        val bitmap = withContext(Dispatchers.Default) {
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
        } ?: return null
        avatars.put(key, bitmap)
        return bitmap
    }
}
