package org.sirplus.talk.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.sirplus.talk.SessionManager
import org.sirplus.talk.UiState
import uniffi.talk_ffi.RoomView

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RoomListScreen(sessions: SessionManager, state: UiState) {
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    var menu by remember { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val focus = remember { FocusRequester() }

    val rooms = remember(state.rooms, query) {
        val q = query.trim().lowercase()
        state.rooms.filter { !it.isArchived && (q.isEmpty() || it.name.lowercase().contains(q)) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    if (searching) {
                        LaunchedEffect(Unit) { focus.requestFocus() }
                        TextField(
                            value = query,
                            onValueChange = { query = it },
                            placeholder = { Text("Suchen") },
                            singleLine = true,
                            colors = TextFieldDefaults.colors(
                                focusedContainerColor = Color.Transparent,
                                unfocusedContainerColor = Color.Transparent,
                                focusedIndicatorColor = Color.Transparent,
                                unfocusedIndicatorColor = Color.Transparent,
                            ),
                            modifier = Modifier.fillMaxWidth().focusRequester(focus),
                        )
                    } else {
                        Text("Chats", fontWeight = FontWeight.Bold)
                    }
                },
                navigationIcon = {
                    if (searching) {
                        IconButton(onClick = { searching = false; query = "" }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, "Suche schließen")
                        }
                    }
                },
                actions = {
                    if (!searching) {
                        IconButton(onClick = { searching = true }) { Icon(Icons.Default.Search, "Suchen") }
                    }
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, "Menü") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(
                                text = {
                                    Column {
                                        Text(state.user, fontWeight = FontWeight.SemiBold)
                                        Text(
                                            state.server.removePrefix("https://"),
                                            color = LocalTalkColors.current.muted,
                                            fontSize = 13.sp,
                                        )
                                    }
                                },
                                onClick = {},
                                enabled = false,
                            )
                            DropdownMenuItem(
                                text = { Text("Abmelden", color = LocalTalkColors.current.danger) },
                                onClick = { menu = false; sessions.logout() },
                            )
                        }
                    }
                },
            )
        },
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = refreshing,
            onRefresh = {
                refreshing = true
                scope.launch {
                    sessions.refresh().join()
                    delay(300)
                    refreshing = false
                }
            },
            modifier = Modifier.padding(padding).fillMaxSize(),
        ) {
            LazyColumn(Modifier.fillMaxSize()) {
                if (!state.online) item { OfflineBanner() }
                items(rooms, key = { it.token }) { room ->
                    RoomRow(room, sessions) { sessions.openRoom(room.token) }
                }
                if (rooms.isEmpty()) {
                    item {
                        Text(
                            if (query.isNotEmpty()) "Keine Treffer" else "Noch keine Chats",
                            color = LocalTalkColors.current.muted,
                            modifier = Modifier.fillMaxWidth().padding(32.dp),
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun OfflineBanner() {
    Text(
        "Keine Verbindung – wird erneut versucht …",
        fontSize = 13.sp,
        color = Color(0xFF6A5000),
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 6.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(Color(0xFFFFF3CD))
            .padding(10.dp),
    )
}

@Composable
private fun RoomRow(room: RoomView, sessions: SessionManager, onClick: () -> Unit) {
    val colors = LocalTalkColors.current
    val unread = room.unreadMessages > 0
    val last = room.lastMessage
    val type = room.roomType.toInt()
    val preview = when {
        last == null -> ""
        last.text.isEmpty() -> "Nachricht gelöscht"
        last.isSystem || type == 1 || type == 6 -> last.text
        else -> "${if (last.isOwn) "Du" else last.author.substringBefore(' ')}: ${last.text}"
    }
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Avatar(room.name, 52.dp, room.token, room.avatarVersion, sessions)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    room.name,
                    fontWeight = if (unread) FontWeight.Bold else FontWeight.SemiBold,
                    fontSize = 16.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    listTime(room.lastActivity),
                    fontSize = 12.sp,
                    color = if (unread) colors.accent else colors.muted,
                    fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Normal,
                )
            }
            Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    preview,
                    fontSize = 14.sp,
                    color = if (unread) MaterialTheme.colorScheme.onSurface else colors.muted,
                    fontStyle = if (last?.isSystem == true) FontStyle.Italic else FontStyle.Normal,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (room.hasCall) {
                    Box(Modifier.padding(top = 6.dp).size(8.dp).clip(CircleShape).background(Color(0xFF2FB35A)))
                }
                if (unread) {
                    Box(
                        Modifier
                            .padding(top = 2.dp)
                            .widthIn(min = 22.dp)
                            .clip(RoundedCornerShape(11.dp))
                            .background(colors.accent)
                            .padding(horizontal = 7.dp, vertical = 2.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            when {
                                room.unreadMention -> "@"
                                room.unreadMessages > 99 -> "99+"
                                else -> room.unreadMessages.toString()
                            },
                            color = Color.White,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Bold,
                        )
                    }
                }
            }
        }
    }
}
