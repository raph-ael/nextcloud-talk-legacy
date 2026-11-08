package org.sirplus.talk.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.sirplus.talk.SessionManager
import org.sirplus.talk.SessionState

@Composable
fun TalkApp(sessions: SessionManager) {
    val state by sessions.state.collectAsStateWithLifecycle()
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        when (state.session) {
            SessionState.LOADING -> Unit
            SessionState.LOGGED_OUT -> LoginScreen(sessions)
            SessionState.READY -> {
                val active = state.activeToken
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
                        ChatScreen(sessions, state, room)
                    } else {
                        RoomListScreen(sessions, state)
                    }
                }
            }
        }
    }
}
