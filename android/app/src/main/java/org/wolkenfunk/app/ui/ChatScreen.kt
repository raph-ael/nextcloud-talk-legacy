package org.wolkenfunk.app.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.heading
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeoutOrNull
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.materialIcon
import androidx.compose.material.icons.materialPath
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.TextButton
import androidx.compose.ui.text.TextLayoutResult
import org.wolkenfunk.app.Jump
import uniffi.talk_ffi.MentionPick
import uniffi.talk_ffi.MentionView
import org.wolkenfunk.app.SendStatus
import uniffi.talk_ffi.SearchHit
import uniffi.talk_ffi.SearchResults
import uniffi.talk_ffi.SendError
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.UiMessage
import org.wolkenfunk.app.UiState
import org.wolkenfunk.app.ChatState
import org.wolkenfunk.app.countsUnread
import uniffi.talk_ffi.MessageKind
import uniffi.talk_ffi.MessageView
import uniffi.talk_ffi.RoomView
import kotlin.math.abs
import kotlin.math.roundToInt

private const val GROUP_WINDOW_S = 3 * 60

/** Material "reply" glyph (the extended icon set is too large to pull in for one icon). */
private val ReplyIcon = materialIcon(name = "Reply", autoMirror = true) {
    materialPath {
        moveTo(10f, 9f)
        verticalLineTo(5f)
        lineToRelative(-7f, 7f)
        lineToRelative(7f, 7f)
        verticalLineToRelative(-4.1f)
        curveToRelative(5f, 0f, 8.5f, 1.6f, 11f, 5.1f)
        curveToRelative(-1f, -5f, -4f, -10f, -11f, -11f)
        close()
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(sessions: SessionManager, state: UiState, room: RoomView, onOpenInfo: () -> Unit = {}) {
    val chat = state.chats[room.token]
    val messages = chat?.messages.orEmpty()
    val isGroup = room.roomType.toInt() !in setOf(1, 6)
    val lastCommonRead = maxOf(chat?.lastCommonRead ?: 0, room.lastCommonReadMessage)
    var replyTo by remember(room.token) { mutableStateOf<MessageView?>(null) }
    // Edit mode and the question before deleting (docs/chat.md „Bearbeiten und Löschen“).
    var editing by remember(room.token) { mutableStateOf<EditTarget?>(null) }
    var savingId by remember(room.token) { mutableStateOf<Long?>(null) }
    var deleting by remember(room.token) { mutableStateOf<MessageView?>(null) }
    var overlay by remember(room.token) { mutableStateOf<Overlay?>(null) }
    var reactionSheet by remember(room.token) { mutableStateOf<ReactionSheet?>(null) }
    val haptics = LocalHapticFeedback.current
    val react = { msg: MessageView, emoji: String ->
        haptics.performHapticFeedback(HapticFeedbackType.Confirm)
        sessions.setReaction(room.token, msg.id, emoji)
    }
    val openCamera = rememberCameraOpener { overlay = Overlay.Camera }
    val askNotifications = rememberNotificationAsker()
    val pickFromCamera = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia(),
    ) { uri -> if (uri != null) overlay = Overlay.Send(uri, fromCamera = false) }
    // Runs after a place was sent: closes the attachment menu (handed over by the composer).
    var locationSheet by remember(room.token) { mutableStateOf<(() -> Unit)?>(null) }
    // „Kontakt“: permission, the system's chooser and the sheet; `afterContact` closes the attachment menu.
    val contactSend = rememberContactSend(room.token)
    var afterContact by remember(room.token) { mutableStateOf({}) }
    // The file send screen is in the app's colours; the other layers are black.
    DarkSystemBars(overlay != null && overlay !is Overlay.Files)
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    // Window into the history (docs/search.md „Chat an einer Nachricht öffnen“): the list does not reach the end.
    val window = chat?.hasNewer == true

    // Search in the chat (docs/search.md „Suche im Chat“). `searchText` null: closed.
    var searchText by rememberSaveable(room.token) { mutableStateOf<String?>(null) }
    var found by remember(room.token) { mutableStateOf<SearchResults?>(null) }
    // Saved with the search text: coming back from a chat opened at a mention, the chosen hit stays.
    var selected by rememberSaveable(room.token) { mutableIntStateOf(0) }
    // The search text `found` belongs to, and the hit the chat jumped to last.
    var searchedFor by rememberSaveable(room.token) { mutableStateOf<String?>(null) }
    var lastHit by rememberSaveable(room.token) { mutableStateOf<Long?>(null) }
    val searchQuery = searchText?.trim()
    val marks = remember(searchQuery) { searchQuery?.let(::searchWords).orEmpty() }
    val goToHit = { hit: SearchHit ->
        lastHit = hit.messageId
        sessions.jumpTo(room.token, hit.messageId)
    }
    val closeSearch = {
        searchText = null
        found = null
        searchedFor = null
        lastHit = null
    }
    BackHandler(enabled = searchText != null, onBack = closeSearch)
    // Every change of the text searches at once and jumps to the newest hit. A change of the
    // loaded messages (edited, deleted, new) searches again a moment later and keeps the chosen hit.
    LaunchedEffect(room.token, searchQuery, messages) {
        val q = searchQuery ?: return@LaunchedEffect
        if (!searchable(q)) {
            found = null
            selected = 0
            searchedFor = q
            return@LaunchedEffect
        }
        val fresh = searchedFor != q
        fun show(r: SearchResults, jump: Boolean) {
            val kept = found?.hits?.getOrNull(selected)?.messageId?.takeIf { !jump }
            val at = if (kept == null) -1 else r.hits.indexOfFirst { it.messageId == kept }
            found = r
            searchedFor = q
            selected = if (at >= 0) at else if (jump) 0 else selected.coerceIn(0, maxOf(0, r.hits.size - 1))
            if (jump) r.hits.firstOrNull()?.takeIf { it.messageId != lastHit }?.let(goToHit)
        }
        if (!fresh) delay(250)
        val local = sessions.searchMessages(q, room.token, CHAT_SEARCH_LIMIT) ?: return@LaunchedEffect
        if (local.complete || fresh) {
            show(local, fresh)
            if (local.complete) return@LaunchedEffect
            // The history still loads: the server adds its hits once the typing pauses.
            delay(300)
        }
        sessions.searchMessagesEverywhere(q, room.token, CHAT_SEARCH_LIMIT)?.let { show(it, false) }
    }
    // A tapped mention of a person (docs/mentions.md „Antippen“): their chat opens over this
    // one, after a question when there is none yet.
    var startChatWith by remember(room.token) { mutableStateOf<MentionView?>(null) }
    val openMention = { m: MentionView ->
        val direct = sessions.existingDirect(m.id)
        when (direct) {
            null -> startChatWith = m
            room.token -> Unit
            else -> sessions.openOver(direct)
        }
    }
    // The message a jump lights up (docs/search.md „Aufleuchten“).
    var flash by remember(room.token) { mutableStateOf<Jump?>(null) }
    LaunchedEffect(flash) {
        if (flash != null) {
            delay(2200)
            flash = null
        }
    }

    // The edit mode ends when the message was deleted meanwhile.
    val editedId = editing?.msg?.id
    LaunchedEffect(editedId, messages) {
        if (editedId != null && messages.any { it.status == null && it.msg.id == editedId && it.msg.kind == MessageKind.DELETED }) {
            editing = null
        }
    }

    // Reverse layout: index 0 is the newest message at the bottom.
    val reversed = remember(messages) { messages.asReversed() }

    // Ungelesen-Linie (docs/chat.md): taken in openRoom, placed above the first unread message.
    val unreadLine = state.unreadLine?.takeIf { it.token == room.token }
    val (unreadKey, unreadCount) = remember(messages, unreadLine) {
        val counting = unreadLine?.let { line -> messages.filter { countsUnread(it, line.from) } }.orEmpty()
        counting.firstOrNull()?.key to maxOf(unreadLine?.base ?: 0, counting.size)
    }
    val unreadLabel = if (unreadCount == 1) "1 ungelesene Nachricht" else "$unreadCount ungelesene Nachrichten"
    // Opened at a message (hit of the search over all chats): the jump positions the list, not the line.
    val jump = state.jump?.takeIf { it.token == room.token }
    val openedAtMessage = remember(room.token) { jump != null }
    // Opening at the line or at a message: until positioned, neither the read marker nor the autoscroll interferes.
    var opening by remember(room.token) { mutableStateOf(unreadLine != null || openedAtMessage) }
    // A tap on a notification of this chat while it was open: the view starts over, first at
    // the end as a new opening would, then at the line if there is one (docs/notifications.md).
    val followedAtOpen = remember(room.token) { state.chatFollowed }
    LaunchedEffect(room.token, state.chatFollowed) {
        val followed = state.chatFollowed != followedAtOpen
        if (followed) {
            opening = true
            listState.scrollToItem(0)
        }
        if (opening && (followed || !openedAtMessage)) {
            positionAtUnreadLine(sessions, listState, room.token)
            opening = false
        }
    }
    // A jump to a message (docs/search.md): scrolled into the upper part, then it lights up.
    var jumped by rememberSaveable(room.token) { mutableIntStateOf(0) }
    LaunchedEffect(room.token, jump?.seq, jump?.ready) {
        when {
            // The message could not be shown: the chat stands at its end.
            jump == null -> if (openedAtMessage && state.chatFollowed == followedAtOpen) opening = false
            jump.ready && jumped != jump.seq -> {
                positionAtMessage(sessions, listState, room.token, jump.messageId)
                jumped = jump.seq
                opening = false
                flash = jump
                // A mention jumped to counts as seen, also when its message is hidden in the list.
                sessions.mentionsSeen(room.token, listOf(jump.messageId))
                // The keyboard of the hit list may still be closing: the list grows, the message keeps its place.
                withTimeoutOrNull(800) {
                    snapshotFlow { listState.layoutInfo.viewportEndOffset - listState.layoutInfo.viewportStartOffset }
                        .drop(1)
                        .collectLatest {
                            if (!listState.isScrollInProgress) positionAtMessage(sessions, listState, room.token, jump.messageId)
                        }
                }
            }
        }
    }
    // „Zum Ende springen“ out of a window, or sent from one: the list is the end of the chat again.
    val endRev = chat?.endRev ?: 0
    val endRevAtOpen = remember(room.token) { endRev }
    LaunchedEffect(room.token, endRev) {
        if (endRev != endRevAtOpen) listState.scrollToItem(0)
    }

    val windowNow by rememberUpdatedState(window)
    val newestNow by rememberUpdatedState(chat?.newestId)
    LaunchedEffect(room.token, messages.size, room.unreadMessages, window) {
        if (!opening && listState.firstVisibleItemIndex <= 1) sessions.markVisibleRead(room.token)
    }
    // Arriving back at the bottom (scrolling or the jump button) sets the read marker too.
    LaunchedEffect(listState, room.token) {
        snapshotFlow { !opening && !windowNow && listState.firstVisibleItemIndex <= 1 }
            .collect { atBottom -> if (atBottom) sessions.markVisibleRead(room.token) }
    }
    LaunchedEffect(listState, room.token) {
        snapshotFlow { !windowNow && listState.firstVisibleItemIndex <= 1 }.collect { sessions.setChatAtBottom(it) }
    }
    LaunchedEffect(listState, room.token) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .collect { last -> if (last >= reversed.size - 40) sessions.loadOlder(room.token) }
    }
    // In a window the list loads on towards the end as well; each page moves the pointer.
    LaunchedEffect(listState, room.token) {
        snapshotFlow { Triple(windowNow && !opening, listState.firstVisibleItemIndex, newestNow) }
            .collect { (newer, first, _) -> if (newer && first <= 15) sessions.loadNewer(room.token) }
    }
    // Mentions of the user not seen yet (docs/mentions.md „„@“-Knopf im Chat“): one is seen
    // once half of its bubble was on the screen.
    val mentionIds = chat?.mentions.orEmpty()
    val mentionsNow by rememberUpdatedState(mentionIds)
    LaunchedEffect(listState, room.token) {
        snapshotFlow {
            val wanted = mentionsNow
            if (wanted.isEmpty() || opening) {
                emptyList()
            } else {
                val info = listState.layoutInfo
                val height = info.viewportEndOffset - info.viewportStartOffset
                info.visibleItemsInfo.mapNotNull { item ->
                    val id = (item.key as? String)?.toLongOrNull()?.takeIf { it in wanted } ?: return@mapNotNull null
                    val shown = minOf(item.offset + item.size, info.viewportEndOffset) - maxOf(item.offset, info.viewportStartOffset)
                    id.takeIf { shown * 2 >= minOf(item.size, height) }
                }
            }
        }.collect { seen -> if (seen.isNotEmpty()) sessions.mentionsSeen(room.token, seen) }
    }
    // Keep the newest message visible when sending / receiving while at the bottom. Pages
    // loaded below a window are not new messages: the list stays where it is.
    var atEndBefore by remember(room.token) { mutableStateOf(!window) }
    LaunchedEffect(messages.lastOrNull()?.key, window) {
        if (atEndBefore && !window && !opening && listState.firstVisibleItemIndex <= 2) listState.animateScrollToItem(0)
        atEndBefore = !window
    }

    val composerState = rememberSaveableStateHolder()
    Box(Modifier.fillMaxSize()) {
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = {
                    // In the search the arrow closes it; only the next one leaves the chat.
                    if (searchText != null) {
                        IconButton(onClick = closeSearch) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Suche schließen") }
                    } else {
                        IconButton(onClick = { sessions.closeRoom() }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück")
                        }
                    }
                },
                actions = {
                    // Not in the search: next to a text field one would call by accident (docs/search.md „Kopf“).
                    if (searchText == null) {
                        IconButton(onClick = { searchText = "" }) { Icon(Icons.Filled.Search, "Im Chat suchen") }
                        CallButton(sessions, room)
                    }
                },
                title = {
                    val groupRoom = room.roomType.toInt() in setOf(2, 3)
                    if (searchText != null) {
                        val focus = remember { FocusRequester() }
                        LaunchedEffect(Unit) { focus.requestFocus() }
                        TextField(
                            value = searchText.orEmpty(),
                            onValueChange = { searchText = it },
                            placeholder = { Text("Suchen") },
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                            colors = TextFieldDefaults.colors(
                                focusedContainerColor = Color.Transparent,
                                unfocusedContainerColor = Color.Transparent,
                                focusedIndicatorColor = Color.Transparent,
                                unfocusedIndicatorColor = Color.Transparent,
                            ),
                            modifier = Modifier.fillMaxWidth().focusRequester(focus),
                        )
                    } else Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = if (groupRoom) {
                            Modifier
                                .clip(RoundedCornerShape(8.dp))
                                .clickable(onClickLabel = "Gruppeneinstellungen", onClick = onOpenInfo)
                                .semantics(mergeDescendants = true) { contentDescription = "Gruppeneinstellungen" }
                        } else {
                            Modifier
                        },
                    ) {
                        Avatar(room.name, 36.dp, room.token, room.avatarVersion, sessions)
                        Spacer(Modifier.width(12.dp))
                        // docs/connection.md: spinner next to the name, „Keine Verbindung“ below it.
                        val hint = connectionHint(state)
                        TitleWithConnection(offline = hint == ConnectionHint.OFFLINE) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(
                                    room.name,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    fontSize = 18.sp,
                                    modifier = Modifier.weight(1f, fill = false),
                                )
                                ConnectionSpinner(hint == ConnectionHint.CONNECTING, Modifier.padding(start = 8.dp))
                            }
                        }
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).consumeWindowInsets(padding).fillMaxSize().imePadding()) {
            RecordingBanners(sessions, room.token)
            Box(Modifier.weight(1f)) {
                // Opened at a message: nothing of the window shows before the message stands in its place.
                val placing = openedAtMessage && opening
                var waiting by remember(room.token) { mutableStateOf(false) }
                LaunchedEffect(placing) {
                    waiting = false
                    if (placing) {
                        delay(400)
                        waiting = true
                    }
                }
                if (waiting) CircularProgressIndicator(Modifier.align(Alignment.Center).size(24.dp))
                LazyColumn(
                    state = listState,
                    reverseLayout = true,
                    modifier = Modifier.fillMaxSize().graphicsLayer { alpha = if (placing) 0f else 1f },
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 12.dp, vertical = 8.dp),
                ) {
                    itemsIndexed(
                        reversed,
                        key = { _, m -> m.key },
                        contentType = { _, m -> if (m.msg.kind == MessageKind.SYSTEM) 0 else if (m.msg.isOwn) 1 else 2 },
                    ) { i, m ->
                        // reversed: i+1 is older (previous), i-1 is newer (next)
                        MessageRow(
                            sessions,
                            m,
                            prev = reversed.getOrNull(i + 1),
                            next = reversed.getOrNull(i - 1),
                            isGroup = isGroup,
                            read = m.msg.isOwn && m.status == null && m.msg.id <= lastCommonRead,
                            unreadLabel = if (m.key == unreadKey) unreadLabel else null,
                            marks = marks,
                            flash = flash?.takeIf { m.status == null && it.messageId == m.msg.id }?.seq ?: 0,
                            onJump = { sessions.jumpTo(room.token, it) },
                            onMention = openMention,
                            canReact = room.canReact,
                            canChat = room.canChat,
                            canDeleteOthers = room.canDeleteOthers,
                            onReply = { replyTo = it },
                            onEdit = { msg ->
                                scope.launch {
                                    val source = sessions.editSource(room.token, msg.id)
                                    editing = if (source != null) {
                                        EditTarget(msg, source.text, source.mentions)
                                    } else {
                                        // The shown text, and who its names stand for.
                                        EditTarget(msg, msg.text, msg.mentions.map { MentionPick(it.label.removePrefix("@"), it.id) })
                                    }
                                }
                            },
                            onDelete = { deleting = it },
                            onReact = react,
                            onMoreReactions = { reactionSheet = ReactionSheet.Picker(it.id) },
                            onShowReactions = { reactionSheet = ReactionSheet.Who(it.id) },
                            onRetry = { sessions.retry(room.token, it) },
                            onDiscard = { sessions.discard(it) },
                            onOpenImage = { id, bmp -> overlay = Overlay.Viewer(id, bmp) },
                        )
                    }
                    if (chat?.loadingOlder == true || chat?.loading == true) {
                        item { Box(Modifier.fillMaxWidth().padding(16.dp), Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp)) } }
                    } else if (chat != null && !chat.hasMore && messages.isNotEmpty()) {
                        item {
                            Text(
                                "Beginn der Unterhaltung",
                                fontSize = 12.sp,
                                color = LocalTalkColors.current.muted,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth().padding(16.dp),
                            )
                        }
                    }
                }
                ScrollDatePill(listState, reversed, enabled = !opening, Modifier.align(Alignment.TopCenter))
                // In a window the button counts what arrived meanwhile (docs/search.md „Neue Nachrichten“).
                val arrived = chat?.held.orEmpty().count { !it.isOwn && it.kind != MessageKind.SYSTEM }
                // In a window into the history the end is never on the screen: the button is always there.
                val toEnd = farFromEnd(listState) || window
                // Above it the way to the oldest unseen mention; not in the search.
                MentionJump(
                    count = if (searchText == null) mentionIds.size else 0,
                    lifted = toEnd,
                    onClick = { mentionIds.firstOrNull()?.let { sessions.jumpTo(room.token, it) } },
                    Modifier.align(Alignment.BottomEnd),
                )
                JumpToEnd(
                    listState,
                    toEnd,
                    maxOf(room.unreadMessages.toInt(), arrived),
                    window,
                    onLeaveWindow = { sessions.leaveWindow(room.token) },
                    Modifier.align(Alignment.BottomEnd),
                )
            }
            if (searchText != null) {
                val hits = found?.hits.orEmpty()
                ChatSearchBar(
                    counter = when {
                        searchQuery.isNullOrEmpty() -> ""
                        hits.isEmpty() -> "Keine Treffer"
                        else -> "${selected + 1} von ${found?.total ?: hits.size}"
                    },
                    // Newest first: the older hit is the next one in the row.
                    older = selected < hits.size - 1,
                    newer = hits.isNotEmpty() && selected > 0,
                    onOlder = {
                        hits.getOrNull(selected + 1)?.let {
                            selected += 1
                            goToHit(it)
                        }
                    },
                    onNewer = {
                        hits.getOrNull(selected - 1)?.let {
                            selected -= 1
                            goToHit(it)
                        }
                    },
                )
            } else if (room.readOnly) {
                Text(
                    "Diese Unterhaltung ist schreibgeschützt.",
                    color = LocalTalkColors.current.muted,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().navigationBarsPadding().padding(16.dp),
                )
            } else composerState.SaveableStateProvider("composer") {
                // Kept while the search bar stands in its place: the draft is there again afterwards.
                Composer(
                    sessions = sessions,
                    room = room,
                    replyTo = replyTo,
                    onCancelReply = { replyTo = null },
                    editing = editing,
                    saving = savingId != null && savingId == editing?.msg?.id,
                    onCancelEdit = { editing = null },
                    onSaveEdit = { text, mentions ->
                        val id = editing?.msg?.id
                        if (id != null) {
                            savingId = id
                            sessions.editMessage(room.token, id, text, mentions) { error ->
                                if (savingId == id) savingId = null
                                // On an error the mode stays open with the text; the toast says why.
                                if (error == null && editing?.msg?.id == id) editing = null
                            }
                        }
                    },
                    onSend = { text, mentions ->
                        sessions.send(room.token, text, mentions, replyTo)
                        replyTo = null
                        scope.launch { listState.animateScrollToItem(0) }
                    },
                    onOpenCamera = openCamera,
                    onImagePicked = { uri -> overlay = Overlay.Send(uri, fromCamera = false) },
                    onFilesPicked = { uris -> overlay = Overlay.Files(uris) },
                    onLocation = { sent -> locationSheet = sent },
                    onContact = { sent ->
                        afterContact = sent
                        contactSend.open()
                    },
                    onVoiceSent = {
                        sessions.leaveWindow(room.token)
                        sessions.clearUnreadLine()
                        scope.launch { listState.animateScrollToItem(0) }
                    },
                )
            }
        }
    }
    when (val o = overlay) {
        null -> Unit
        Overlay.Camera -> CameraScreen(
            onClose = { overlay = null },
            onCaptured = { uri -> overlay = Overlay.Send(uri, fromCamera = true) },
            onGallery = {
                pickFromCamera.launch(
                    androidx.activity.result.PickVisualMediaRequest(
                        androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageOnly,
                    ),
                )
            },
        )
        is Overlay.Send -> MediaSendScreen(
            uri = o.uri,
            roomName = room.name,
            onClose = {
                if (o.fromCamera) {
                    o.uri.path?.let { java.io.File(it).delete() }
                    overlay = Overlay.Camera
                } else {
                    overlay = null
                }
            },
            onSend = { caption ->
                askNotifications()
                sessions.sendImageUri(room.token, o.uri, caption, replyTo, deleteSource = o.fromCamera)
                replyTo = null
                overlay = null
                scope.launch { listState.animateScrollToItem(0) }
            },
        )
        is Overlay.Files -> FileSendScreen(
            uris = o.uris,
            onClose = { overlay = null },
            send = { files, caption, done ->
                askNotifications()
                sessions.sendFiles(room.token, files, caption, replyTo, done)
            },
            onSent = {
                replyTo = null
                overlay = null
                scope.launch { listState.animateScrollToItem(0) }
            },
        )
        is Overlay.Viewer -> ImageViewer(sessions, o.fileId, o.initial, onClose = { overlay = null })
    }
    locationSheet?.let { sent ->
        LocationSheet(
            onSend = { latitude, longitude, name ->
                // Talk has no quote for places: an open reply bar stays for the next message.
                sessions.sendLocation(room.token, latitude, longitude, name)
                locationSheet = null
                sent()
                scope.launch { listState.animateScrollToItem(0) }
            },
            onDismiss = { locationSheet = null },
        )
    }
    ContactSendSheet(contactSend) { card ->
        askNotifications()
        sessions.sendContact(room.token, card, replyTo)
        replyTo = null
        afterContact()
        scope.launch { listState.animateScrollToItem(0) }
    }
    startChatWith?.let { m ->
        // No chat with this person yet: a tap by accident must not create an empty one.
        AlertDialog(
            onDismissRequest = { startChatWith = null },
            title = { Text("Chat mit ${m.label.removePrefix("@")} beginnen?") },
            confirmButton = {
                TextButton(onClick = {
                    startChatWith = null
                    scope.launch { sessions.openDirect(m.id, over = true) }
                }) { Text("Chat öffnen") }
            },
            dismissButton = { TextButton(onClick = { startChatWith = null }) { Text("Abbrechen") } },
        )
    }
    deleting?.let { d ->
        val text = buildString {
            append(if (d.isOwn) "Sie wird für alle im Chat gelöscht." else "Die Nachricht von ${d.author} wird für alle im Chat gelöscht.")
            if (d.attachment != null) append(" Die Datei selbst bleibt erhalten.")
        }
        ConfirmDialog("Nachricht löschen?", text, "Löschen", onDismiss = { deleting = null }) {
            deleting = null
            sessions.deleteMessage(room.token, d.id)
        }
    }
    when (val r = reactionSheet) {
        null -> Unit
        is ReactionSheet.Picker -> ReactionPickerSheet(
            onPick = { emoji ->
                reactionSheet = null
                haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                sessions.setReaction(room.token, r.messageId, emoji)
            },
            onDismiss = { reactionSheet = null },
        )
        is ReactionSheet.Who -> WhoReactedSheet(
            sessions,
            room.token,
            r.messageId,
            onRemove = {
                haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                sessions.setReaction(room.token, r.messageId, null)
            },
            onDismiss = { reactionSheet = null },
        )
    }
    }
}

/** Oldest delivered message loaded is still after [from]: the first unread one may be missing. */
private fun gapAfter(chat: ChatState, from: Long): Boolean =
    chat.hasMore && (chat.messages.firstOrNull { it.status == null }?.msg?.id ?: Long.MAX_VALUE) > from

/**
 * Opens the chat at the Ungelesen-Linie once the server page is there: loads older pages (at
 * most 10) until the first unread message is, then scrolls it into the upper quarter, never
 * past the end. If it is still missing, the oldest loaded message ends up at the top.
 */
private suspend fun positionAtUnreadLine(sessions: SessionManager, listState: LazyListState, token: String) {
    val line = sessions.state.value.unreadLine?.takeIf { it.token == token } ?: return
    sessions.state.first { it.chats[token]?.synced == true }
    for (page in 0 until 10) {
        val chat = sessions.state.value.chats[token] ?: return
        // Everything after the read marker is loaded once the oldest loaded message is not newer.
        if (!gapAfter(chat, line.from)) break
        sessions.loadOlder(token)
        sessions.state.first { it.chats[token]?.loadingOlder != true }
    }
    val chat = sessions.state.value.chats[token] ?: return
    val messages = chat.messages
    val first = messages.firstOrNull { countsUnread(it, line.from) }
    val target = first ?: messages.firstOrNull()?.takeIf { gapAfter(chat, line.from) } ?: return
    scrollToKey(sessions, listState, token, target.key, if (first != null) 0.25f else 0f)
}

/** Height of the list between its paddings. */
private fun LazyListState.innerHeight(): Int =
    layoutInfo.run { viewportEndOffset - viewportStartOffset - beforeContentPadding - afterContentPadding }

/** Places the message with [key] so that its top is [fromTop] of the list's height below the upper edge. */
private suspend fun scrollToKey(sessions: SessionManager, listState: LazyListState, token: String, key: String, fromTop: Float) {
    // New messages may arrive meanwhile and the list composes a frame later: look the item up
    // by key each time and try again when it is not where the index said.
    repeat(5) {
        val current = sessions.state.value.chats[token]?.messages ?: return
        val pos = current.indexOfFirst { it.key == key }
        if (pos < 0) return
        val index = current.size - 1 - pos
        withTimeoutOrNull(1000) { snapshotFlow { listState.layoutInfo.totalItemsCount }.first { it > index } }
        listState.scrollToItem(index)
        val item = withTimeoutOrNull(500) {
            snapshotFlow { listState.layoutInfo.visibleItemsInfo.firstOrNull { it.key == key } }.filterNotNull().first()
        } ?: return@repeat
        // Reverse layout: the item sits at the bottom now; scrolling backwards lifts it.
        listState.scrollBy(-(listState.innerHeight() * (1 - fromTop) - item.size))
        return
    }
}

/**
 * Shows a message a jump leads to (docs/search.md „Position“): its upper edge at about a
 * fifth of the list's height, placed at once, without gliding over other messages.
 */
private suspend fun positionAtMessage(sessions: SessionManager, listState: LazyListState, token: String, id: Long) {
    val messages = sessions.state.value.chats[token]?.messages ?: return
    // A message that is hidden in the list: the one before it stands for it.
    val target = messages.lastOrNull { it.status == null && it.msg.id <= id } ?: messages.firstOrNull { it.status == null } ?: return
    scrollToKey(sessions, listState, token, target.key, 0.2f)
}

/**
 * Date of the topmost visible message while scrolling (docs/chat.md „Datum beim Scrollen“):
 * fades in with the scroll, out 0.4 s after it ends; never at the bottom. Decoration only.
 */
@Composable
private fun ScrollDatePill(listState: LazyListState, reversed: List<UiMessage>, enabled: Boolean, modifier: Modifier) {
    val colors = LocalTalkColors.current
    val reduceMotion = rememberReduceMotion()
    var visible by remember { mutableStateOf(false) }
    // Only a scroll the user started (drag and its fling), not one caused by new messages.
    val dragged = listState.interactionSource.collectIsDraggedAsState()
    LaunchedEffect(listState, enabled) {
        var byUser = false
        snapshotFlow { Triple(listState.isScrollInProgress, dragged.value, listState.firstVisibleItemIndex <= 1) }
            .collectLatest { (scrolling, drag, atBottom) ->
                if (drag) byUser = true
                when {
                    !enabled || atBottom -> visible = false
                    scrolling && byUser -> visible = true
                    !scrolling -> {
                        byUser = false
                        delay(400)
                        visible = false
                    }
                }
            }
    }
    val messages by rememberUpdatedState(reversed)
    val label by remember(listState) {
        derivedStateOf {
            // Reverse layout: the last visible item is the topmost one.
            val top = listState.layoutInfo.visibleItemsInfo.lastOrNull { it.index < messages.size }
            top?.let { messages[it.index].msg.timestamp }?.let(::dayLabel).orEmpty()
        }
    }
    AnimatedVisibility(
        visible = visible && label.isNotEmpty(),
        modifier = modifier.padding(top = 8.dp).clearAndSetSemantics {},
        enter = if (reduceMotion) EnterTransition.None else fadeIn(tween(100)) + slideInVertically(tween(100)) { -it / 2 },
        exit = if (reduceMotion) ExitTransition.None else fadeOut(tween(100)) + slideOutVertically(tween(100)) { -it / 2 },
    ) {
        Text(
            label,
            fontSize = 12.sp,
            color = colors.muted,
            modifier = Modifier
                .shadow(2.dp, CircleShape)
                .clip(CircleShape)
                .background(colors.floating)
                .padding(horizontal = 10.dp, vertical = 4.dp),
        )
    }
}

/** Full-width line with „N ungelesene Nachrichten“ below (docs/chat.md). */
@Composable
private fun UnreadDivider(label: String) {
    Column(
        Modifier.fillMaxWidth().padding(top = 24.dp, bottom = 16.dp).semantics(mergeDescendants = true) {},
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        HorizontalDivider(
            // Across the list's side padding too.
            Modifier.layout { measurable, constraints ->
                val extra = 24.dp.roundToPx()
                val width = constraints.maxWidth + extra
                val p = measurable.measure(constraints.copy(minWidth = width, maxWidth = width))
                layout(constraints.maxWidth, p.height) { p.place(-extra / 2, 0) }
            },
            thickness = 1.dp,
            color = LocalTalkColors.current.muted,
        )
        Text(
            label,
            fontSize = 13.sp,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.padding(top = 3.dp),
        )
    }
}

private sealed interface ReactionSheet {
    val messageId: Long
    data class Picker(override val messageId: Long) : ReactionSheet
    data class Who(override val messageId: Long) : ReactionSheet
}

/**
 * Round floating button "Zum Ende springen" (docs/chat.md): visible once the list is more
 * than half a screen away from the newest message, with the unread count as badge.
 */
@Composable
private fun JumpToEnd(listState: LazyListState, visible: Boolean, unread: Int, window: Boolean, onLeaveWindow: () -> Unit, modifier: Modifier) {
    val colors = LocalTalkColors.current
    val scope = rememberCoroutineScope()
    val label = when {
        unread == 1 -> "Zum Ende springen, 1 neue Nachricht"
        unread > 1 -> "Zum Ende springen, $unread neue Nachrichten"
        else -> "Zum Ende springen"
    }
    AnimatedVisibility(
        visible = visible,
        modifier = modifier.padding(end = 14.dp, bottom = 14.dp),
        enter = fadeIn(tween(150)) + scaleIn(tween(150), initialScale = 0.6f),
        exit = fadeOut(tween(150)) + scaleOut(tween(150), targetScale = 0.6f),
    ) {
        // Extra room at the top for the badge.
        Box(Modifier.padding(top = 10.dp, start = 10.dp)) {
            Box(
                Modifier
                    .size(40.dp)
                    .shadow(3.dp, CircleShape)
                    .clip(CircleShape)
                    .background(colors.floating)
                    .clickable {
                        if (window) {
                            // The end of the chat replaces the list; the screen follows it.
                            onLeaveWindow()
                        } else {
                            scope.launch {
                                // Far away: jump close without animation, then glide the rest.
                                if (listState.firstVisibleItemIndex > 15) listState.scrollToItem(10)
                                listState.animateScrollToItem(0)
                            }
                        }
                    }
                    .semantics(mergeDescendants = true) {
                        contentDescription = label
                        role = Role.Button
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(TalkIcons.ChevronDown, null, tint = colors.muted, modifier = Modifier.size(26.dp))
            }
            if (unread > 0) CountBadge(unread, Modifier.align(Alignment.TopCenter))
        }
    }
}

/** The list is more than half a screen away from the newest message. */
@Composable
private fun farFromEnd(listState: LazyListState): Boolean {
    val far by remember(listState) {
        derivedStateOf {
            val info = listState.layoutInfo
            val half = (info.viewportEndOffset - info.viewportStartOffset) / 2
            val index = listState.firstVisibleItemIndex
            val offset = listState.firstVisibleItemScrollOffset
            if (info.visibleItemsInfo.isEmpty()) {
                // Not laid out yet (a restored place in the list): nothing to tell.
                false
            } else if (index == 0) {
                offset > half
            } else {
                // Items scrolled past are not measured any more; estimate them by the visible ones.
                val avg = info.visibleItemsInfo.map { it.size }.average().takeIf { !it.isNaN() } ?: 0.0
                index * avg + offset > half
            }
        }
    }
    return far
}

/** Full-screen layers above the chat. */
private sealed interface Overlay {
    data object Camera : Overlay
    data class Send(val uri: android.net.Uri, val fromCamera: Boolean) : Overlay
    data class Files(val uris: List<android.net.Uri>) : Overlay
    data class Viewer(val fileId: String, val initial: androidx.compose.ui.graphics.ImageBitmap?) : Overlay
}

/** Light status/navigation bar icons while a black full-screen layer is shown. */
@Composable
private fun DarkSystemBars(active: Boolean) {
    val view = androidx.compose.ui.platform.LocalView.current
    androidx.compose.runtime.DisposableEffect(active) {
        val window = (view.context as? android.app.Activity)?.window
        val controller = window?.let { androidx.core.view.WindowCompat.getInsetsController(it, view) }
        val light = controller?.isAppearanceLightStatusBars
        val lightNav = controller?.isAppearanceLightNavigationBars
        if (active && controller != null) {
            controller.isAppearanceLightStatusBars = false
            controller.isAppearanceLightNavigationBars = false
        }
        onDispose {
            if (active && controller != null) {
                controller.isAppearanceLightStatusBars = light ?: true
                controller.isAppearanceLightNavigationBars = lightNav ?: true
            }
        }
    }
}

private fun continues(a: UiMessage?, b: UiMessage?): Boolean {
    if (a == null || b == null) return false
    if (a.msg.kind == MessageKind.SYSTEM || b.msg.kind == MessageKind.SYSTEM) return false
    return a.msg.actorId == b.msg.actorId && a.msg.isOwn == b.msg.isOwn &&
        abs(b.msg.timestamp - a.msg.timestamp) <= GROUP_WINDOW_S && sameDay(a.msg.timestamp, b.msg.timestamp)
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MessageRow(
    sessions: SessionManager,
    m: UiMessage,
    prev: UiMessage?,
    next: UiMessage?,
    isGroup: Boolean,
    read: Boolean,
    unreadLabel: String?,
    /** Search words to mark in the text (docs/search.md „Hervorhebung“); empty outside the search. */
    marks: List<String>,
    /** Not 0: a jump just led here, the bubble lights up once for this number. */
    flash: Int,
    onJump: (Long) -> Unit,
    /** A mention of a person was tapped (docs/mentions.md „Antippen“). */
    onMention: (MentionView) -> Unit,
    canReact: Boolean,
    canChat: Boolean,
    canDeleteOthers: Boolean,
    onReply: (MessageView) -> Unit,
    onEdit: (MessageView) -> Unit,
    onDelete: (MessageView) -> Unit,
    onReact: (MessageView, String) -> Unit,
    onMoreReactions: (MessageView) -> Unit,
    onShowReactions: (MessageView) -> Unit,
    onRetry: (MessageView) -> Unit,
    onDiscard: (MessageView) -> Unit,
    onOpenImage: (fileId: String, preview: androidx.compose.ui.graphics.ImageBitmap?) -> Unit,
) {
    val colors = LocalTalkColors.current
    val msg = m.msg
    Column(Modifier.fillMaxWidth()) {
        if (prev == null || !sameDay(prev.msg.timestamp, msg.timestamp)) {
            // Plain text like Signal; a heading so screen readers can jump from day to day.
            Text(
                dayLabel(msg.timestamp),
                fontSize = 13.sp,
                fontWeight = FontWeight.Medium,
                color = colors.muted,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(top = 16.dp, bottom = 8.dp).semantics { heading() },
            )
        }
        if (unreadLabel != null) UnreadDivider(unreadLabel)
        if (msg.kind == MessageKind.SYSTEM) {
            Text(
                msg.text,
                fontSize = 13.sp,
                color = colors.muted,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 6.dp),
            )
            return
        }

        val first = !continues(prev, m)
        val last = !continues(m, next)
        val own = msg.isOwn
        val big = 18.dp
        val small = 4.dp
        val shape = if (own) {
            RoundedCornerShape(big, if (first) big else small, if (last) big else small, big)
        } else {
            RoundedCornerShape(if (first) big else small, big, big, if (last) big else small)
        }
        val attachment = msg.attachment.takeIf { msg.kind != MessageKind.DELETED }
        val image = attachment?.isImage() == true
        // A place (docs/attachments.md „Blase“): the map like a photo, its name below. `text` is the name.
        val location = msg.location.takeIf { msg.kind != MessageKind.DELETED }
        val jumbo = msg.kind == MessageKind.TEXT && attachment == null && location == null && msg.replyTo == null && isJumboEmoji(msg.text)
        val audio = attachment != null && !image && (attachment.voiceMessage || attachment.mimetype.startsWith("audio/"))
        // Signal: an audio bubble carries its time in the player's footer row.
        val metaInVoice = audio && msg.text.isEmpty()
        // ... and has more room above the player when nothing (name, quote) precedes it.
        val audioOnTop = audio && !(isGroup && !own && first) && msg.replyTo == null
        // Photos and maps sit edge to edge in the bubble (3dp frame); everything else keeps the text inset.
        val media = image || location != null
        val inset = if (media) Modifier.padding(horizontal = 9.dp) else Modifier

        // Swipe right to reply (Signal gesture).
        val offset = remember { Animatable(0f) }
        val haptics = LocalHapticFeedback.current
        val scope = rememberCoroutineScope()
        var menu by remember { mutableStateOf(false) }
        // How far the reaction pills hang under the bubble: the long-press menu starts below them.
        var pillsHeight by remember { mutableIntStateOf(0) }
        val pillsOverlap = with(LocalDensity.current) { 4.dp.roundToPx() }
        val clipboard = LocalClipboardManager.current
        val replyable = msg.kind == MessageKind.TEXT && m.status == null
        // Reactable like „Antworten“, files and voice messages included (docs/chat.md).
        val reactable = replyable && canReact
        val threshold = 160f
        // Lit up for about 1.5 s after a jump, then fading (docs/search.md „Aufleuchten“).
        val glow = remember { Animatable(0f) }
        LaunchedEffect(flash) {
            if (flash != 0) {
                glow.snapTo(1f)
                delay(1500)
                glow.animateTo(0f, tween(400))
            } else {
                glow.snapTo(0f)
            }
        }

        Box(
            Modifier
                .fillMaxWidth()
                .padding(top = if (first) 8.dp else 2.dp)
                .pointerInput(msg.id, replyable) {
                    if (!replyable) return@pointerInput
                    var triggered = false
                    detectHorizontalDragGestures(
                        onDragEnd = {
                            if (triggered) onReply(msg)
                            triggered = false
                            scope.launch { offset.animateTo(0f) }
                        },
                        onDragCancel = { scope.launch { offset.animateTo(0f) } },
                    ) { _, dx ->
                        val v = (offset.value + dx).coerceIn(0f, threshold * 1.3f)
                        scope.launch { offset.snapTo(v) }
                        if (!triggered && v >= threshold) {
                            triggered = true
                            haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        }
                    }
                },
        ) {
            if (replyable) {
                // Always placed; only the draw phase reads the swipe offset (no recomposition per frame).
                Icon(
                    ReplyIcon,
                    null,
                    tint = colors.muted,
                    modifier = Modifier
                        .align(Alignment.CenterStart)
                        .padding(start = 4.dp)
                        .graphicsLayer { alpha = ((offset.value - 8f) / threshold).coerceIn(0f, 1f) },
                )
            }
            // The pills sit under the bubble, 4 dp pushed into its lower edge and drawn above it;
            // the row is that much lower (docs/chat.md „Pillen unter der Blase“). They move with the swipe.
            Column(
                Modifier.fillMaxWidth().offset { IntOffset(offset.value.roundToInt(), 0) },
                verticalArrangement = Arrangement.spacedBy((-4).dp),
                horizontalAlignment = if (own) Alignment.End else Alignment.Start,
            ) {
                Row(verticalAlignment = Alignment.Bottom) {
                    if (isGroup && !own) {
                        Box(Modifier.width(36.dp)) { if (last) UserAvatar(sessions, msg.author, if (msg.actorType == "users") msg.actorId else "", 28.dp) }
                    }
                    Box {
                        Column(
                            Modifier
                                .then(
                                    when {
                                        image -> Modifier.width(ChatImageWidth + 6.dp)
                                        location != null -> Modifier.width(LocationMapWidth + 6.dp)
                                        else -> Modifier.widthIn(max = 300.dp)
                                    },
                                )
                                .clip(shape)
                                .background(
                                    when {
                                        jumbo -> androidx.compose.ui.graphics.Color.Transparent
                                        own -> colors.bubbleOwn
                                        else -> colors.bubbleOther
                                    },
                                )
                                .drawBehind {
                                    val lit = glow.value
                                    if (lit > 0f) drawRect(colors.searchMark.copy(alpha = colors.searchMark.alpha * lit))
                                }
                                .combinedClickable(
                                    onClick = { if (m.status == SendStatus.FAILED || m.status == SendStatus.WAITING) onRetry(msg) },
                                    onLongClick = {
                                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                        menu = true
                                    },
                                )
                                .graphicsLayer { alpha = if (m.status == SendStatus.FAILED) 0.6f else 1f }
                                .padding(
                                    horizontal = if (jumbo) 0.dp else if (media) 3.dp else 12.dp,
                                )
                                .padding(
                                    top = if (media) 3.dp else if (audioOnTop) 12.dp else 7.dp,
                                    bottom = if (media) 3.dp else 7.dp,
                                ),
                        ) {
                            val textColor = if (own && !jumbo) colors.bubbleOwnText else colors.bubbleOtherText
                            if (isGroup && !own && first) {
                                Text(
                                    msg.author,
                                    color = colorFor(msg.author),
                                    fontSize = 13.sp,
                                    fontWeight = FontWeight.SemiBold,
                                    modifier = inset.padding(top = if (media) 4.dp else 0.dp, bottom = if (media) 4.dp else 0.dp),
                                )
                            }
                            msg.replyTo?.let { r ->
                                val quoted = withAttachment(r.text, r.attachment, r.location)
                                Column(
                                    inset
                                        .padding(top = 2.dp, bottom = 6.dp)
                                        .clip(RoundedCornerShape(6.dp))
                                        .background(textColor.copy(alpha = 0.1f))
                                        // A tap jumps to the quoted message; a deleted one does nothing (docs/search.md „Zitat antippen“).
                                        .then(
                                            if (quoted.isEmpty()) {
                                                Modifier
                                            } else {
                                                Modifier.combinedClickable(
                                                    onClickLabel = "Zur Nachricht springen",
                                                    onClick = { onJump(r.id) },
                                                    onLongClick = {
                                                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                                        menu = true
                                                    },
                                                )
                                            },
                                        )
                                        .padding(horizontal = 8.dp, vertical = 4.dp),
                                ) {
                                    Text(if (r.isOwn) "Du" else r.author, color = textColor, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                                    Text(quoted.ifEmpty { "Nachricht gelöscht" }, color = textColor.copy(alpha = 0.85f), fontSize = 13.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                }
                            }
                            val metaOnImage = image && msg.text.isEmpty()
                            val showMeta = last || m.status != null
                            // Rounded like the bubble, but squarer where text continues above/below.
                            val above = (isGroup && !own && first) || msg.replyTo != null
                            val below = !metaOnImage
                            val inner = androidx.compose.foundation.shape.CornerSize(4.dp)
                            val imgShape = RoundedCornerShape(
                                if (above) inner else shape.topStart,
                                if (above) inner else shape.topEnd,
                                if (below) inner else shape.bottomEnd,
                                if (below) inner else shape.bottomStart,
                            )
                            if (image) {
                                ChatImage(
                                    sessions,
                                    m,
                                    shape = imgShape,
                                    onOpen = { bmp -> onOpenImage(attachment!!.id, bmp) },
                                    onLongPress = {
                                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                        menu = true
                                    },
                                ) {
                                    if (metaOnImage && showMeta) {
                                        Box(
                                            Modifier
                                                .align(Alignment.BottomEnd)
                                                .padding(6.dp)
                                                .clip(RoundedCornerShape(10.dp))
                                                .background(androidx.compose.ui.graphics.Color.Black.copy(alpha = 0.4f))
                                                .padding(horizontal = 6.dp, vertical = 2.dp),
                                        ) {
                                            MessageMeta(m, own, read, androidx.compose.ui.graphics.Color.White, colors.danger)
                                        }
                                    }
                                }
                            } else if (audio) {
                                val metaColor = textColor.copy(alpha = 0.75f)
                                VoiceBubble(
                                    sessions.voice,
                                    m,
                                    textColor,
                                    metaColor,
                                    own,
                                    Modifier.padding(bottom = if (msg.text.isEmpty()) 0.dp else 4.dp),
                                    onLongPress = {
                                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                        menu = true
                                    },
                                    meta = if (metaInVoice && showMeta) {
                                        { MessageMeta(m, own, read, metaColor, colors.danger) }
                                    } else {
                                        null
                                    },
                                )
                            } else if (attachment != null) {
                                FileRow(
                                    sessions,
                                    m,
                                    textColor,
                                    Modifier.padding(bottom = if (msg.text.isEmpty()) 0.dp else 4.dp),
                                    onRetry = { onRetry(msg) },
                                    onLongPress = {
                                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                        menu = true
                                    },
                                )
                            } else if (location != null) {
                                val context = androidx.compose.ui.platform.LocalContext.current
                                LocationMap(
                                    location.latitude,
                                    location.longitude,
                                    Modifier
                                        .size(LocationMapWidth, LocationMapHeight)
                                        .clip(imgShape)
                                        .combinedClickable(
                                            onClickLabel = "In Karten öffnen",
                                            onClick = {
                                                if (m.status == SendStatus.FAILED || m.status == SendStatus.WAITING) {
                                                    onRetry(msg)
                                                } else {
                                                    openInMaps(context, location.latitude, location.longitude, msg.text)
                                                }
                                            },
                                            onLongClick = {
                                                haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                                menu = true
                                            },
                                        ),
                                )
                                Text(
                                    locationName(msg.text),
                                    color = textColor,
                                    style = TextStyle(fontSize = 15.sp, lineHeight = 21.sp),
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = inset.padding(top = 6.dp),
                                )
                            }
                            when {
                                // The name of a place stands under its map already.
                                location != null -> Unit
                                image && msg.text.isEmpty() -> Unit
                                attachment != null && msg.text.isEmpty() -> Unit
                                msg.kind == MessageKind.DELETED ->
                                    Text("Diese Nachricht wurde gelöscht.", color = textColor, fontStyle = FontStyle.Italic)
                                jumbo -> Text(msg.text, fontSize = 44.sp)
                                else -> {
                                    // Mentions first (docs/mentions.md „In der Nachricht“), then the search words, which win over them.
                                    val shown = remember(msg.text, msg.markdown, msg.mentions, own, textColor, marks, colors.searchMark, colors.mentionSelf) {
                                        val mentioned = markMentions(
                                            renderMessage(msg.text, msg.markdown, textColor),
                                            msg.mentions,
                                            own,
                                            textColor.copy(alpha = 0.12f),
                                            colors.mentionSelf,
                                        )
                                        markWords(mentioned, marks, colors.searchMark)
                                    }
                                    val layout = remember { arrayOfNulls<TextLayoutResult>(1) }
                                    val tapped by rememberUpdatedState { i: Int -> msg.mentions.getOrNull(i)?.let(onMention) }
                                    Text(
                                        shown,
                                        color = textColor,
                                        style = TextStyle(fontSize = 15.sp, lineHeight = 21.sp),
                                        onTextLayout = { layout[0] = it },
                                        modifier = inset
                                            .padding(top = if (image) 6.dp else 0.dp)
                                            .mentionTaps(shown, msg.mentions.map { it.label }, { layout[0] }) { tapped(it) },
                                    )
                                }
                            }
                            if (showMeta && !metaOnImage && !metaInVoice) {
                                Box(
                                    Modifier
                                        .align(Alignment.End)
                                        .then(inset)
                                        .padding(top = 2.dp, bottom = if (media) 4.dp else 0.dp),
                                ) {
                                    MessageMeta(
                                        m,
                                        own,
                                        read,
                                        if (jumbo) colors.muted else textColor.copy(alpha = 0.75f),
                                        colors.danger,
                                    )
                                }
                            }
                        }
                        if (menu) {
                            // What is allowed is worked out when the menu opens (docs/chat.md „Bearbeiten und Löschen“).
                            val now = remember { System.currentTimeMillis() / 1000 }
                            val delivered = m.status == null
                            val editable = delivered && msg.editable && canChat && (msg.editUntil?.let { now < it } ?: true)
                            val deletable = delivered && msg.deletable && if (own) canChat else canDeleteOthers
                            val actions = buildList {
                                if (m.status == SendStatus.FAILED || m.status == SendStatus.WAITING) {
                                    add(MenuAction("Erneut senden") { onRetry(msg) })
                                }
                                if (replyable) add(MenuAction("Antworten") { onReply(msg) })
                                if (editable) add(MenuAction("Bearbeiten") { onEdit(msg) })
                                if (location != null) {
                                    // The name and, in the second line, the place on openstreetmap.org.
                                    add(
                                        MenuAction("Kopieren") {
                                            clipboard.setText(AnnotatedString(locationName(msg.text) + "\n" + osmLink(location.latitude, location.longitude)))
                                        },
                                    )
                                } else if (msg.text.isNotEmpty()) {
                                    add(MenuAction("Kopieren") { clipboard.setText(AnnotatedString(msg.text)) })
                                }
                                // In the outbox „Löschen“ only discards locally, without asking.
                                if (!delivered) {
                                    add(MenuAction("Löschen", danger = true) { onDiscard(msg) })
                                } else if (deletable) {
                                    add(MenuAction("Löschen", danger = true) { onDelete(msg) })
                                }
                            }
                            MessageMenu(
                                own = own,
                                mine = if (reactable) msg.reactionsSelf else null,
                                actions = actions,
                                onReact = { onReact(msg, it) },
                                onMoreReactions = { onMoreReactions(msg) },
                                onDismiss = { menu = false },
                                below = if (msg.reactions.isEmpty()) 0 else pillsHeight,
                            )
                        }
                    }
                }
                ReactionPills(
                    msg.id,
                    msg.reactions,
                    msg.reactionsSelf,
                    // 8 dp in from the bubble's edge, off its round corner.
                    Modifier
                        .padding(start = if (isGroup && !own) 44.dp else 8.dp, end = 8.dp)
                        .onSizeChanged { pillsHeight = (it.height - pillsOverlap).coerceAtLeast(0) },
                    onOpen = { onShowReactions(msg) },
                )
            }
        }
    }
}

private fun failureText(e: SendError?): String = when (e) {
    SendError.FORBIDDEN -> "keine Berechtigung"
    SendError.NOT_FOUND -> "Unterhaltung nicht gefunden"
    SendError.TOO_LARGE -> "Datei zu groß"
    SendError.QUOTA_EXCEEDED -> "Speicher voll"
    SendError.FILE_MISSING -> "Datei fehlt"
    else -> "Tippen zum Wiederholen"
}

/** Time, "edited", send status and read ticks below (or on) a bubble. */
@Composable
private fun MessageMeta(m: UiMessage, own: Boolean, read: Boolean, color: androidx.compose.ui.graphics.Color, danger: androidx.compose.ui.graphics.Color) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
        if (m.msg.edited) Text("bearbeitet ·", fontSize = 12.sp, color = color)
        Text(
            when (m.status) {
                SendStatus.SENDING -> "Wird gesendet …"
                SendStatus.WAITING -> "Wartet auf Netz …"
                SendStatus.FAILED -> "Nicht gesendet · " + failureText(m.error)
                null -> messageTime(m.msg.timestamp)
            },
            fontSize = 12.sp,
            color = if (m.status == SendStatus.FAILED) danger else color,
        )
        if (own && m.status == null) Text(if (read) "✓✓" else "✓", fontSize = 12.sp, color = color)
    }
}

/**
 * Call button in the chat header. Tap: call (or join the running call). Debug builds:
 * long press starts a silent call (nobody is rung) for tests.
 */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun CallButton(sessions: SessionManager, room: uniffi.talk_ffi.RoomView) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val withPermissions = rememberCallPermissions()
    val debug = (context.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0
    fun call(silent: Boolean) = withPermissions {
        sessions.startCall(room.token, silent)
        context.startActivity(
            android.content.Intent(context, org.wolkenfunk.app.call.CallActivity::class.java)
                .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }
    Box(
        Modifier
            .size(48.dp)
            .clip(androidx.compose.foundation.shape.CircleShape)
            .combinedClickable(
                onClick = { call(silent = false) },
                onLongClick = if (debug) ({ call(silent = true) }) else null,
                onClickLabel = if (room.hasCall) "Anruf beitreten" else "Anrufen",
            ),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            TalkIcons.Call,
            if (room.hasCall) "Anruf beitreten" else "Anrufen",
            tint = if (room.hasCall) androidx.compose.ui.graphics.Color(0xFF2FB25B) else androidx.compose.material3.LocalContentColor.current,
        )
    }
}
