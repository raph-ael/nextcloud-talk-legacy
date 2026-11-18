package org.wolkenfunk.app.ui

import android.provider.Settings
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
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
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.heading
import uniffi.talk_ffi.SearchResults
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
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
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.stateDescription
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
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.UiState
import uniffi.talk_ffi.ConnectionState
import uniffi.talk_ffi.RoomView
import uniffi.talk_ffi.SectionView

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RoomListScreen(
    sessions: SessionManager,
    state: UiState,
    search: RoomListSearch,
    listState: LazyListState,
    onCompose: () -> Unit = {},
    onSettings: () -> Unit = {},
    onGroupSettings: (String) -> Unit = {},
    onAddAccount: () -> Unit = {},
) {
    val searching = search.active
    // Account menu at the own avatar, from two accounts on (docs/accounts.md).
    var accountMenu by rememberSaveable { mutableStateOf(false) }
    // Chat menu after a long press (docs/settings.md); follows the live room.
    var menuFor by rememberSaveable { mutableStateOf<String?>(null) }
    val query = search.query
    var menu by remember { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val focus = remember { FocusRequester() }

    // Sections come from the core (filtered, collapsed ones reduced); recomputed on every
    // RoomsChanged (roomsRev), new search text and another open chat.
    val sections = remember(state.roomsRev, state.rooms, query, state.activeToken) {
        sessions.roomSections(query.trim(), state.activeToken)
    }
    // Relative times ("5 Min.") need a clock.
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(30_000)
            now = System.currentTimeMillis()
        }
    }

    // Hits in the messages (docs/search.md): the device answers with every keystroke; while
    // the history still loads, the server adds its hits 300 ms after the last one. A new
    // room list searches again, so a message that just arrived is found.
    val q = query.trim()
    val inMessages = searchable(q)
    LaunchedEffect(q, search.limit, state.roomsRev) {
        if (!inMessages) {
            search.results = null
            search.resultsFor = null
            return@LaunchedEffect
        }
        val wanted = q to search.limit
        fun show(r: SearchResults) {
            search.results = r
            search.resultsFor = wanted
        }
        var shown = sessions.searchMessages(q, null, search.limit) ?: return@LaunchedEffect
        // The same search again (back from a chat, new room list): the list with the
        // server's hits stays until the new one with them is there.
        val again = search.resultsFor == wanted
        if (shown.complete || !again) show(shown)
        if (shown.complete) return@LaunchedEffect
        if (!again) delay(300)
        show(sessions.searchMessagesEverywhere(q, null, search.limit)?.also { shown = it } ?: shown)
        // The line „Ältere Nachrichten werden noch geladen“ follows the loading.
        while (shown.chatsDone < shown.chatsTotal) {
            delay(1000)
            val local = sessions.searchMessages(q, null, search.limit) ?: break
            if (local.chatsDone == shown.chatsDone) continue
            shown = if (local.complete) local else sessions.searchMessagesEverywhere(q, null, search.limit) ?: local
            show(shown)
        }
    }
    val found = search.results.takeIf { inMessages }
    val hits = found?.hits.orEmpty()
    // 50 more once the end of the list is reached.
    LaunchedEffect(listState) {
        snapshotFlow {
            val info = listState.layoutInfo
            info.totalItemsCount > 0 && (info.visibleItemsInfo.lastOrNull()?.index ?: 0) >= info.totalItemsCount - 3
        }.collect { atEnd ->
            val r = search.results
            if (atEnd && r != null && r.more && r.hits.size >= search.limit) search.limit += SEARCH_PAGE
        }
    }
    val keyboard = LocalSoftwareKeyboardController.current
    BackHandler(enabled = searching) {
        search.close()
        scope.launch { listState.scrollToItem(0) }
    }

    // docs/connection.md: spinner next to the title, „Keine Verbindung“ below it when offline.
    val hint = connectionHint(state)
    val listEmpty = sections.isEmpty() && query.isEmpty()

    Scaffold(
        floatingActionButton = {
            if (!searching) {
                FloatingActionButton(
                    onClick = onCompose,
                    containerColor = LocalTalkColors.current.accent,
                    contentColor = Color.White,
                ) { Icon(Icons.Filled.Edit, "Neue Nachricht") }
            }
        },
        topBar = {
            TopAppBar(
                title = {
                    if (searching) {
                        // Not when coming back from a hit: the keyboard would cover the list.
                        LaunchedEffect(Unit) { if (query.isEmpty()) focus.requestFocus() }
                        TextField(
                            value = query,
                            onValueChange = {
                                if (it.trim() != q) scope.launch { listState.scrollToItem(0) }
                                search.query = it
                                search.limit = SEARCH_PAGE
                            },
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
                        TitleWithConnection(offline = hint == ConnectionHint.OFFLINE, spinner = true) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text("Chats", fontSize = 22.sp)
                                // An empty list that still loads has its spinner in the middle.
                                ConnectionSpinner(
                                    hint == ConnectionHint.CONNECTING && !listEmpty,
                                    Modifier.padding(start = 8.dp),
                                )
                            }
                        }
                    }
                },
                navigationIcon = {
                    if (searching) {
                        IconButton(onClick = { search.close(); scope.launch { listState.scrollToItem(0) } }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, "Suche schließen")
                        }
                    } else {
                        OwnAvatarButton(sessions, state, onSettings = onSettings, onAccounts = { accountMenu = true })
                    }
                },
                actions = {
                    if (!searching) {
                        IconButton(onClick = { search.active = true }) { Icon(Icons.Default.Search, "Suchen") }
                    }
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, "Menü") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(
                                text = { Text("Einstellungen") },
                                onClick = { menu = false; onSettings() },
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
            LazyColumn(Modifier.fillMaxSize(), state = listState) {
                if (query.isEmpty()) item(key = "recordings", contentType = "recordings") {
                    RecordingBanners(sessions) { sessions.openRoom(it) }
                }
                for (section in sections) {
                    if (section.title.isNotEmpty()) {
                        if (section.kind == "archived") {
                            // No category behind it: it cannot be collapsed (docs/categories.md „Suche“).
                            item(key = "h-${section.id}", contentType = "title") { SectionTitle(section.title) }
                        } else {
                            item(key = "h-${section.id}", contentType = "header") {
                                SectionHeader(section) { sessions.toggleSection(section.id) }
                            }
                        }
                    }
                    items(section.rooms, key = { "${section.id}:${it.token}" }, contentType = { "room" }) { room ->
                        RoomRow(room, sessions, now, onMenu = { menuFor = room.token }) { sessions.openRoom(room.token) }
                    }
                }
                if (hits.isNotEmpty()) {
                    item(key = "h-messages", contentType = "title") { SectionTitle("Nachrichten") }
                    items(hits, key = { "m:${it.token}:${it.messageId}" }, contentType = { "hit" }) { hit ->
                        val version = state.rooms.find { it.token == hit.token }?.avatarVersion.orEmpty()
                        SearchHitRow(hit, version, sessions, now) {
                            keyboard?.hide()
                            sessions.openRoomAt(hit.token, hit.messageId)
                        }
                    }
                }
                when {
                    sections.isNotEmpty() || hits.isNotEmpty() -> {}
                    // The hits to this search text are still on their way.
                    inMessages && search.resultsFor?.first != q -> {}
                    // Empty list (docs/connection.md): still loading, really empty, or offline
                    // (then the header says so and the middle stays free).
                    !listEmpty || state.connection == ConnectionState.ONLINE -> item {
                        Text(
                            if (listEmpty) "Noch keine Chats" else "Keine Treffer",
                            color = LocalTalkColors.current.muted,
                            modifier = Modifier.fillMaxWidth().padding(32.dp),
                        )
                    }
                    hint == ConnectionHint.CONNECTING -> item {
                        Box(Modifier.fillParentMaxSize(), Alignment.Center) {
                            ConnectionSpinner(true, size = 24.dp, stroke = 2.dp)
                        }
                    }
                }
                if (found != null && found.chatsDone < found.chatsTotal) {
                    item(key = "history", contentType = "history") {
                        Text(
                            "Ältere Nachrichten werden noch geladen (${found.chatsDone} von ${found.chatsTotal} Chats)",
                            style = ListType.body,
                            color = LocalTalkColors.current.muted,
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 16.dp),
                        )
                    }
                }
            }
        }
    }

    val menuRoom = menuFor?.let { t -> state.rooms.find { it.token == t } }
    if (menuFor != null && menuRoom == null) menuFor = null
    menuRoom?.let { r ->
        ChatMenu(
            sessions, r,
            onDismiss = { menuFor = null },
            onGroupSettings = { menuFor = null; onGroupSettings(r.token) },
        )
    }
    if (accountMenu && state.accounts.size < 2) accountMenu = false
    if (accountMenu) {
        AccountMenu(sessions, state, onDismiss = { accountMenu = false }, onAdd = onAddAccount, onSettings = onSettings)
    }
}

/**
 * Section title („Favoriten“, a category, „Chats“), tappable over the full width to collapse it.
 * A small muted chevron next to the title points right when collapsed, down when open.
 */
@Composable
private fun SectionHeader(section: SectionView, onToggle: () -> Unit) {
    val reduceMotion = rememberReduceMotion()
    val angle by animateFloatAsState(
        targetValue = if (section.collapsed) 0f else 90f,
        animationSpec = if (reduceMotion) snap() else tween(150),
        label = "chevron",
    )
    val hidden = section.total.toInt() - section.rooms.size
    val label = buildString {
        append(section.title)
        if (section.kind == "custom") append(", Kategorie")
        if (section.collapsed && hidden > 0) append(if (hidden == 1) ", 1 Chat ausgeblendet" else ", $hidden Chats ausgeblendet")
    }
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onToggle)
            .clearAndSetSemantics {
                contentDescription = label
                role = Role.Button
                stateDescription = if (section.collapsed) "eingeklappt" else "ausgeklappt"
            }
            .padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 8.dp),
    ) {
        Text(
            section.title,
            style = ListType.section,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f, fill = false),
        )
        Icon(
            Icons.AutoMirrored.Filled.KeyboardArrowRight,
            contentDescription = null,
            tint = LocalTalkColors.current.muted,
            modifier = Modifier.padding(start = 4.dp).size(20.dp).rotate(angle),
        )
    }
}

/** A section title that is no button: „Archiviert“ and „Nachrichten“ of a search. */
@Composable
private fun SectionTitle(title: String) {
    Text(
        title,
        style = ListType.section,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier
            .fillMaxWidth()
            .semantics { heading() }
            .padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 8.dp),
    )
}

/** System setting „Animationen entfernen“ (animator duration scale 0). */
@Composable
internal fun rememberReduceMotion(): Boolean {
    val context = LocalContext.current
    return remember {
        Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
    }
}

/** One conversation, laid out with Signal's metrics (48dp avatar, 2-line summary, 84dp min height). */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun RoomRow(room: RoomView, sessions: SessionManager, now: Long, onMenu: () -> Unit, onClick: () -> Unit) {
    val colors = LocalTalkColors.current
    val haptics = LocalHapticFeedback.current
    // docs/settings.md „Stummgeschaltete Chats“: 2 counts only mentions, 3 is off.
    val level = room.notificationLevel.toInt()
    val off = level == 3
    val countColor = if (level == 1 || (level == 2 && room.unreadMention)) colors.accent else mutedBadge()
    val onSurface = MaterialTheme.colorScheme.onSurface
    val unread = room.unreadMessages > 0
    val last = room.lastMessage
    val type = room.roomType.toInt()
    val preview = remember(last, type, onSurface) {
        buildAnnotatedString {
            val text = last?.let { withAttachment(it.text, it.attachment, it.location) }.orEmpty()
            when {
                last == null -> {}
                text.isEmpty() -> append("Nachricht gelöscht")
                last.isSystem || type == 1 || type == 6 -> append(text)
                else -> {
                    withStyle(SpanStyle(fontWeight = FontWeight.Medium, color = onSurface)) {
                        append(if (last.isOwn) "Du" else last.author.substringBefore(' '))
                        append(": ")
                    }
                    append(text)
                }
            }
        }
    }
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 84.dp)
            .combinedClickable(
                onClick = onClick,
                onLongClick = {
                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    onMenu()
                },
            )
            .semantics { customActions = listOf(CustomAccessibilityAction("Chat-Menü") { onMenu(); true }) }
            .padding(horizontal = 24.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Avatar(room.name, 48.dp, room.token, room.avatarVersion, sessions)
        Spacer(Modifier.width(20.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        room.name,
                        style = ListType.title,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    if (off) {
                        Icon(
                            TalkIcons.NotificationsOff,
                            contentDescription = "stummgeschaltet",
                            tint = colors.muted,
                            modifier = Modifier.padding(start = 4.dp).size(16.dp),
                        )
                    }
                }
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
                if (unread) Badge(if (room.unreadMessages > 99) "99+" else room.unreadMessages.toString(), countColor)
            }
        }
    }
}

/** Grey count of muted chats; dark enough for white text in both designs. */
@Composable
private fun mutedBadge(): Color = if (isDarkTheme()) Color(0xFF5C5C61) else LocalTalkColors.current.muted

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
