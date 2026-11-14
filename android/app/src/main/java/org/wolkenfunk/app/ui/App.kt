package org.wolkenfunk.app.ui

import androidx.activity.compose.BackHandler
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
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
    call?.let { c -> CallBar(c) { context.startActivity(Intent(context, CallActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } }
    Box(Modifier.weight(1f).then(if (call != null) Modifier.consumeWindowInsets(WindowInsets.statusBars) else Modifier)) {
        when (state.session) {
            SessionState.LOADING -> Unit
            SessionState.LOGGED_OUT -> LoginScreen(sessions)
            SessionState.READY -> {
                val active = state.activeToken
                // „Neue Nachricht“ over the list, group settings over the chat (docs/groups.md).
                var composing by rememberSaveable { mutableStateOf(false) }
                var infoFor by rememberSaveable { mutableStateOf<String?>(null) }
                if (infoFor != null && infoFor != active) infoFor = null
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
                        RoomListScreen(sessions, state, onCompose = { composing = true })
                    }
                }
                androidx.compose.animation.AnimatedVisibility(
                    visible = composing && active == null,
                    enter = slideInHorizontally { it },
                    exit = slideOutHorizontally { it },
                ) {
                    NewMessageFlow(sessions, onClose = { composing = false })
                }
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
        }
    }
    }
}
