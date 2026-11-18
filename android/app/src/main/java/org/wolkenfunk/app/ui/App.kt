package org.wolkenfunk.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import android.content.Intent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.statusBars
import androidx.compose.ui.platform.LocalContext
import org.wolkenfunk.app.call.CallActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import uniffi.talk_ffi.RoomView
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.SessionState

@Composable
fun TalkApp(sessions: SessionManager) {
    val state by sessions.state.collectAsStateWithLifecycle()
    val call by sessions.calls.ui.collectAsStateWithLifecycle()
    val context = LocalContext.current
    // Over the accounts: the settings stay open when switching from there (docs/accounts.md).
    var settings by rememberSaveable { mutableStateOf(false) }
    var adding by rememberSaveable { mutableStateOf(false) }
    if (adding && state.session != SessionState.READY) adding = false
    // Signed out of the last account from the settings: the next login opens the chat list.
    if (settings && state.session == SessionState.LOGGED_OUT) settings = false
    AskNotificationsAtStart(signedIn = state.session == SessionState.READY)
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
    call?.let { c -> CallBar(c) { context.startActivity(Intent(context, CallActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } }
    Box(Modifier.weight(1f).then(if (call != null) Modifier.consumeWindowInsets(WindowInsets.statusBars) else Modifier)) {
        when (state.session) {
            SessionState.LOADING -> Unit
            SessionState.LOGGED_OUT -> LoginScreen(sessions)
            SessionState.READY -> {
                // Everything below belongs to one account: a switch starts it fresh, so no
                // remembered state (avatars, drafts, scroll) crosses accounts (docs/accounts.md).
                key(state.accountId) {
                val active = state.activeToken
                // „Neue Nachricht“ over the list, group settings over the chat (docs/groups.md).
                var composing by rememberSaveable { mutableStateOf(false) }
                // Search and position of the chat list outlive an opened chat (docs/search.md „Antippen“).
                val listSearch = rememberRoomListSearch()
                val listState = rememberLazyListState()
                var infoFor by rememberSaveable { mutableStateOf<String?>(null) }
                // Group settings belong to the open chat, or come over the list from the chat menu.
                if (infoFor != null && active != null && infoFor != active) infoFor = null
                BackHandler(enabled = active != null) { sessions.closeRoom() }
                AnimatedContent(
                    targetState = active,
                    transitionSpec = {
                        if (targetState != null) {
                            slideInHorizontally { it } togetherWith slideOutHorizontally { -it / 4 }
                        } else {
                            slideInHorizontally { -it / 4 } togetherWith slideOutHorizontally { it }
                        }
                    },
                    label = "nav",
                ) { token ->
                    val room = token?.let { t -> state.rooms.find { it.token == t } }
                    if (token != null && room != null) {
                        ChatScreen(sessions, state, room, onOpenInfo = { infoFor = token })
                    } else {
                        RoomListScreen(
                            sessions,
                            state,
                            listSearch,
                            listState,
                            onCompose = { composing = true },
                            onSettings = { settings = true },
                            onGroupSettings = { infoFor = it },
                            onAddAccount = { adding = true },
                        )
                    }
                }
                androidx.compose.animation.AnimatedVisibility(
                    visible = composing && active == null,
                    enter = slideInHorizontally { it },
                    exit = slideOutHorizontally { it },
                ) {
                    NewMessageFlow(sessions, onClose = { composing = false })
                }
                androidx.compose.animation.AnimatedVisibility(
                    visible = settings && active == null,
                    enter = slideInHorizontally { it },
                    exit = slideOutHorizontally { it },
                ) {
                    SettingsScreen(sessions, state, onBack = { settings = false }, onAddAccount = { adding = true })
                }
                if (settings && active != null) settings = false
                val infoRoom = infoFor?.let { t -> state.rooms.find { it.token == t } }
                var lastInfoRoom by remember { mutableStateOf<RoomView?>(null) }
                if (infoRoom != null) lastInfoRoom = infoRoom
                androidx.compose.animation.AnimatedVisibility(
                    visible = infoRoom != null,
                    enter = slideInHorizontally { it },
                    exit = slideOutHorizontally { it },
                ) {
                    lastInfoRoom?.let { r -> GroupInfoScreen(sessions, infoRoom ?: r, onBack = { infoFor = null }) }
                }
                // Opening a chat from the flow closes it underneath.
                if (composing && active != null) composing = false
                }
                // „Konto hinzufügen“ over everything; the other accounts keep running.
                androidx.compose.animation.AnimatedVisibility(
                    visible = adding,
                    enter = slideInHorizontally { it },
                    exit = slideOutHorizontally { it },
                ) {
                    LoginScreen(sessions, adding = true, onCancel = { adding = false }, onDone = { adding = false })
                }
            }
        }
    }
    }
}
