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
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.runtime.mutableLongStateOf
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
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.withStyle
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
    val (favorites, others) = remember(rooms) { rooms.partition { it.isFavorite } }
    // Relative times ("5 Min.") need a clock.
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(30_000)
            now = System.currentTimeMillis()
        }
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
                        Text("Chats", fontSize = 22.sp)
                    }
                },
                navigationIcon = {
                    if (searching) {
                        IconButton(onClick = { searching = false; query = "" }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, "Suche schließen")
                        }
                    } else {
                        Box(
                            Modifier.padding(start = 12.dp, end = 8.dp).clip(CircleShape).clickable { menu = true },
                        ) { Avatar(state.user, 32.dp) }
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
                if (favorites.isNotEmpty()) {
                    item(key = "h-fav", contentType = "header") { SectionHeader("Favoriten") }
                    items(favorites, key = { it.token }, contentType = { "room" }) { room ->
                        RoomRow(room, sessions, now) { sessions.openRoom(room.token) }
                    }
                    if (others.isNotEmpty()) item(key = "h-chats", contentType = "header") { SectionHeader("Chats") }
                }
                items(others, key = { it.token }, contentType = { "room" }) { room ->
                    RoomRow(room, sessions, now) { sessions.openRoom(room.token) }
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
private fun SectionHeader(title: String) {
    Text(
        title,
        style = ListType.section,
        modifier = Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 8.dp),
    )
}

/** One conversation, laid out with Signal's metrics (48dp avatar, 2-line summary, 84dp min height). */
@Composable
private fun RoomRow(room: RoomView, sessions: SessionManager, now: Long, onClick: () -> Unit) {
    val colors = LocalTalkColors.current
    val onSurface = MaterialTheme.colorScheme.onSurface
    val unread = room.unreadMessages > 0
    val last = room.lastMessage
    val type = room.roomType.toInt()
    val preview = remember(last, type) {
        buildAnnotatedString {
            when {
                last == null -> {}
                last.text.isEmpty() -> append("Nachricht gelöscht")
                last.isSystem || type == 1 || type == 6 -> append(last.text)
                else -> {
                    withStyle(SpanStyle(fontWeight = FontWeight.Medium, color = onSurface)) {
                        append(if (last.isOwn) "Du" else last.author.substringBefore(' '))
                        append(": ")
                    }
                    append(last.text)
                }
            }
        }
    }
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 84.dp)
            .clickable(onClick = onClick)
            .padding(horizontal = 24.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Avatar(room.name, 48.dp, room.token, room.avatarVersion, sessions)
        Spacer(Modifier.width(20.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    room.name,
                    style = ListType.title,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(12.dp))
                Text(
                    listTime(room.lastActivity, now),
                    style = ListType.body,
                    color = if (unread) onSurface else colors.muted,
                    fontWeight = if (unread) FontWeight.Bold else FontWeight.Normal,
                    maxLines = 1,
                )
            }
            Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    preview,
                    style = ListType.body,
                    color = colors.muted,
                    fontStyle = if (last?.isSystem == true) FontStyle.Italic else FontStyle.Normal,
                    minLines = 2,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (room.hasCall) {
                    Box(Modifier.padding(top = 6.dp).size(8.dp).clip(CircleShape).background(Color(0xFF2FB35A)))
                }
                if (room.unreadMention) Badge("@", colors.accent)
                if (unread) Badge(if (room.unreadMessages > 99) "99+" else room.unreadMessages.toString(), colors.accent)
            }
        }
    }
}

@Composable
private fun Badge(text: String, color: Color) {
    Box(
        Modifier
            .padding(top = 2.dp)
            .heightIn(min = 22.dp)
            .widthIn(min = 22.dp)
            .clip(RoundedCornerShape(11.dp))
            .background(color)
            .padding(horizontal = 6.dp, vertical = 2.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = Color.White, fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium)
    }
}
