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
import androidx.compose.ui.platform.LocalClipboardManager
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
import org.wolkenfunk.app.SendStatus
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
    DarkSystemBars(overlay != null)
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()

    // Reverse layout: index 0 is the newest message at the bottom.
    val reversed = remember(messages) { messages.asReversed() }

    // Ungelesen-Linie (docs/chat.md): taken in openRoom, placed above the first unread message.
    val unreadLine = state.unreadLine?.takeIf { it.token == room.token }
    val (unreadKey, unreadCount) = remember(messages, unreadLine) {
        val counting = unreadLine?.let { line -> messages.filter { countsUnread(it, line.from) } }.orEmpty()
        counting.firstOrNull()?.key to maxOf(unreadLine?.base ?: 0, counting.size)
    }
    val unreadLabel = if (unreadCount == 1) "1 ungelesene Nachricht" else "$unreadCount ungelesene Nachrichten"
    // Opening at the line: until positioned, neither the read marker nor the autoscroll interferes.
    var opening by remember(room.token) { mutableStateOf(unreadLine != null) }
    LaunchedEffect(room.token) {
        if (opening) {
            positionAtUnreadLine(sessions, listState, room.token)
            opening = false
        }
    }

    LaunchedEffect(room.token, messages.size, room.unreadMessages) {
        if (!opening && listState.firstVisibleItemIndex <= 1) sessions.markVisibleRead(room.token)
    }
    // Arriving back at the bottom (scrolling or the jump button) sets the read marker too.
    LaunchedEffect(listState, room.token) {
        snapshotFlow { !opening && listState.firstVisibleItemIndex <= 1 }
            .collect { atBottom -> if (atBottom) sessions.markVisibleRead(room.token) }
    }
    LaunchedEffect(listState, room.token) {
        snapshotFlow { listState.firstVisibleItemIndex <= 1 }.collect { sessions.setChatAtBottom(it) }
    }
    LaunchedEffect(listState, room.token) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .collect { last -> if (last >= reversed.size - 40) sessions.loadOlder(room.token) }
    }
    // Keep the newest message visible when sending / receiving while at the bottom.
    LaunchedEffect(messages.lastOrNull()?.key) {
        if (!opening && listState.firstVisibleItemIndex <= 2) listState.animateScrollToItem(0)
    }

    Box(Modifier.fillMaxSize()) {
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = {
                    IconButton(onClick = { sessions.closeRoom() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück")
                    }
                },
                actions = { CallButton(sessions, room) },
                title = {
                    val groupRoom = room.roomType.toInt() in setOf(2, 3)
                    Row(
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
                        Text(room.name, maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 18.sp)
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).consumeWindowInsets(padding).fillMaxSize().imePadding()) {
            if (!state.online) OfflineBanner()
            RecordingBanners(sessions, room.token)
            Box(Modifier.weight(1f)) {
                LazyColumn(
                    state = listState,
                    reverseLayout = true,
                    modifier = Modifier.fillMaxSize(),
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
                            canReact = room.canReact,
                            onReply = { replyTo = it },
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
                JumpToEnd(listState, room.unreadMessages.toInt(), Modifier.align(Alignment.BottomEnd))
            }
            if (room.readOnly) {
                Text(
                    "Diese Unterhaltung ist schreibgeschützt.",
                    color = LocalTalkColors.current.muted,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().navigationBarsPadding().padding(16.dp),
                )
            } else {
                Composer(
                    token = room.token,
                    replyTo = replyTo,
                    onCancelReply = { replyTo = null },
                    onSend = { text ->
                        sessions.send(room.token, text, replyTo)
                        replyTo = null
                        scope.launch { listState.animateScrollToItem(0) }
                    },
                    onOpenCamera = openCamera,
                    onImagePicked = { uri -> overlay = Overlay.Send(uri, fromCamera = false) },
                    voice = sessions.voice,
                    onVoiceSent = {
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
        is Overlay.Viewer -> ImageViewer(sessions, o.fileId, o.initial, onClose = { overlay = null })
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
    val fromTop = if (first != null) 0.25f else 0f
    // New messages may arrive meanwhile and the list composes a frame later: look the item up
    // by key each time and try again when it is not where the index said.
    repeat(5) {
        val current = sessions.state.value.chats[token]?.messages ?: return
        val pos = current.indexOfFirst { it.key == target.key }
        if (pos < 0) return
        val index = current.size - 1 - pos
        withTimeoutOrNull(1000) { snapshotFlow { listState.layoutInfo.totalItemsCount }.first { it > index } }
        listState.scrollToItem(index)
        val item = withTimeoutOrNull(500) {
            snapshotFlow { listState.layoutInfo.visibleItemsInfo.firstOrNull { it.key == target.key } }.filterNotNull().first()
        } ?: return@repeat
        val info = listState.layoutInfo
        val height = info.viewportEndOffset - info.viewportStartOffset - info.beforeContentPadding - info.afterContentPadding
        // Reverse layout: the item sits at the bottom now; scrolling backwards lifts it.
        listState.scrollBy(-(height * (1 - fromTop) - item.size))
        return
    }
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
private fun JumpToEnd(listState: LazyListState, unread: Int, modifier: Modifier) {
    val colors = LocalTalkColors.current
    val scope = rememberCoroutineScope()
    val visible by remember(listState) {
        derivedStateOf {
            val info = listState.layoutInfo
            val half = (info.viewportEndOffset - info.viewportStartOffset) / 2
            val index = listState.firstVisibleItemIndex
            val offset = listState.firstVisibleItemScrollOffset
            if (index == 0) {
                offset > half
            } else {
                // Items scrolled past are not measured any more; estimate them by the visible ones.
                val avg = info.visibleItemsInfo.map { it.size }.average().takeIf { !it.isNaN() } ?: 0.0
                index * avg + offset > half
            }
        }
    }
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
                        scope.launch {
                            // Far away: jump close without animation, then glide the rest.
                            if (listState.firstVisibleItemIndex > 15) listState.scrollToItem(10)
                            listState.animateScrollToItem(0)
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
            if (unread > 0) {
                Box(
                    Modifier
                        .align(Alignment.TopCenter)
                        .offset(y = (-10).dp)
                        .heightIn(min = 20.dp)
                        .widthIn(min = 20.dp)
                        .clip(RoundedCornerShape(10.dp))
                        .background(colors.accent)
                        .clearAndSetSemantics {}
                        .padding(horizontal = 5.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        if (unread > 99) "99+" else unread.toString(),
                        color = Color.White,
                        fontSize = 11.sp,
                        lineHeight = 14.sp,
                        fontWeight = FontWeight.Medium,
                    )
                }
            }
        }
    }
}

/** Full-screen layers above the chat. */
private sealed interface Overlay {
    data object Camera : Overlay
    data class Send(val uri: android.net.Uri, val fromCamera: Boolean) : Overlay
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
    canReact: Boolean,
    onReply: (MessageView) -> Unit,
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
        val jumbo = msg.kind == MessageKind.TEXT && attachment == null && msg.replyTo == null && isJumboEmoji(msg.text)
        val audio = attachment != null && !image && (attachment.voiceMessage || attachment.mimetype.startsWith("audio/"))
        // Signal: an audio bubble carries its time in the player's footer row.
        val metaInVoice = audio && msg.text.isEmpty() && msg.reactions.isEmpty()
        // ... and has more room above the player when nothing (name, quote) precedes it.
        val audioOnTop = audio && !(isGroup && !own && first) && msg.replyTo == null
        // Photos sit edge to edge in the bubble (3dp frame); everything else keeps the text inset.
        val inset = if (image) Modifier.padding(horizontal = 9.dp) else Modifier

        // Swipe right to reply (Signal gesture).
        val offset = remember { Animatable(0f) }
        val haptics = LocalHapticFeedback.current
        val scope = rememberCoroutineScope()
        var menu by remember { mutableStateOf(false) }
        val clipboard = LocalClipboardManager.current
        val replyable = msg.kind == MessageKind.TEXT && m.status == null
        // Reactable like „Antworten“, files and voice messages included (docs/chat.md).
        val reactable = replyable && canReact
        val threshold = 160f

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
            Row(
                Modifier.fillMaxWidth().offset { IntOffset(offset.value.roundToInt(), 0) },
                horizontalArrangement = if (own) Arrangement.End else Arrangement.Start,
                verticalAlignment = Alignment.Bottom,
            ) {
                if (isGroup && !own) {
                    Box(Modifier.width(36.dp)) { if (last) Avatar(msg.author, 28.dp) }
                }
                Box {
                    Column(
                        Modifier
                            .then(if (image) Modifier.width(ChatImageWidth + 6.dp) else Modifier.widthIn(max = 300.dp))
                            .clip(shape)
                            .background(
                                when {
                                    jumbo -> androidx.compose.ui.graphics.Color.Transparent
                                    own -> colors.bubbleOwn
                                    else -> colors.bubbleOther
                                },
                            )
                            .combinedClickable(
                                onClick = { if (m.status == SendStatus.FAILED || m.status == SendStatus.WAITING) onRetry(msg) },
                                onLongClick = {
                                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                    menu = true
                                },
                            )
                            .graphicsLayer { alpha = if (m.status == SendStatus.FAILED) 0.6f else 1f }
                            .padding(
                                horizontal = if (jumbo) 0.dp else if (image) 3.dp else 12.dp,
                            )
                            .padding(
                                top = if (image) 3.dp else if (audioOnTop) 12.dp else 7.dp,
                                bottom = if (image) 3.dp else 7.dp,
                            ),
                    ) {
                        val textColor = if (own && !jumbo) colors.bubbleOwnText else colors.bubbleOtherText
                        if (isGroup && !own && first) {
                            Text(
                                msg.author,
                                color = colorFor(msg.author),
                                fontSize = 13.sp,
                                fontWeight = FontWeight.SemiBold,
                                modifier = inset.padding(top = if (image) 4.dp else 0.dp, bottom = if (image) 4.dp else 0.dp),
                            )
                        }
                        msg.replyTo?.let { r ->
                            Column(
                                inset
                                    .padding(top = 2.dp, bottom = 6.dp)
                                    .clip(RoundedCornerShape(6.dp))
                                    .background(textColor.copy(alpha = 0.1f))
                                    .padding(horizontal = 8.dp, vertical = 4.dp),
                            ) {
                                Text(if (r.isOwn) "Du" else r.author, color = textColor, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                                Text(withAttachment(r.text, r.attachment).ifEmpty { "Nachricht gelöscht" }, color = textColor.copy(alpha = 0.85f), fontSize = 13.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            }
                        }
                        val metaOnImage = image && msg.text.isEmpty() && msg.reactions.isEmpty()
                        val showMeta = last || m.status != null
                        if (image) {
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
                                meta = if (metaInVoice && showMeta) {
                                    { MessageMeta(m, own, read, metaColor, colors.danger) }
                                } else {
                                    null
                                },
                            )
                        } else if (attachment != null) {
                            FileRow(attachment, textColor, Modifier.padding(bottom = if (msg.text.isEmpty()) 0.dp else 4.dp))
                        }
                        when {
                            image && msg.text.isEmpty() -> Unit
                            attachment != null && msg.text.isEmpty() -> Unit
                            msg.kind == MessageKind.DELETED ->
                                Text("Diese Nachricht wurde gelöscht.", color = textColor, fontStyle = FontStyle.Italic)
                            jumbo -> Text(msg.text, fontSize = 44.sp)
                            else -> Text(
                                remember(msg.text, msg.markdown, textColor) { renderMessage(msg.text, msg.markdown, textColor) },
                                color = textColor,
                                style = TextStyle(fontSize = 15.sp, lineHeight = 21.sp),
                                modifier = inset.padding(top = if (image) 6.dp else 0.dp),
                            )
                        }
                        if (msg.reactions.isNotEmpty()) {
                            ReactionPills(
                                msg.id,
                                msg.reactions,
                                msg.reactionsSelf,
                                textColor,
                                inset.padding(top = 4.dp),
                                onOpen = { onShowReactions(msg) },
                            )
                        }
                        if (showMeta && !metaOnImage && !metaInVoice) {
                            Box(
                                Modifier
                                    .align(Alignment.End)
                                    .then(inset)
                                    .padding(top = 2.dp, bottom = if (image) 4.dp else 0.dp),
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
                        val actions = buildList {
                            if (m.status == SendStatus.FAILED || m.status == SendStatus.WAITING) {
                                add(MenuAction("Erneut senden") { onRetry(msg) })
                            }
                            if (m.status != null) add(MenuAction("Löschen") { onDiscard(msg) })
                            if (replyable) add(MenuAction("Antworten") { onReply(msg) })
                            if (msg.text.isNotEmpty()) add(MenuAction("Kopieren") { clipboard.setText(AnnotatedString(msg.text)) })
                        }
                        MessageMenu(
                            own = own,
                            mine = if (reactable) msg.reactionsSelf else null,
                            actions = actions,
                            onReact = { onReact(msg, it) },
                            onMoreReactions = { onMoreReactions(msg) },
                            onDismiss = { menu = false },
                        )
                    }
                }
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
