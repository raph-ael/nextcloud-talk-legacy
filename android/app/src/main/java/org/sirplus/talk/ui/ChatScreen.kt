package org.sirplus.talk.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
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
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.materialIcon
import androidx.compose.material.icons.materialPath
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.graphics.SolidColor
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
import org.sirplus.talk.SendStatus
import org.sirplus.talk.SessionManager
import org.sirplus.talk.UiMessage
import org.sirplus.talk.UiState
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
fun ChatScreen(sessions: SessionManager, state: UiState, room: RoomView) {
    val chat = state.chats[room.token]
    val messages = chat?.messages.orEmpty()
    val isGroup = room.roomType.toInt() !in setOf(1, 6)
    val lastCommonRead = maxOf(chat?.lastCommonRead ?: 0, room.lastCommonReadMessage)
    var replyTo by remember(room.token) { mutableStateOf<MessageView?>(null) }
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()

    // Reverse layout: index 0 is the newest message at the bottom.
    val reversed = remember(messages) { messages.asReversed() }

    LaunchedEffect(room.token, messages.size, room.unreadMessages) {
        if (listState.firstVisibleItemIndex <= 1) sessions.markVisibleRead(room.token)
    }
    LaunchedEffect(listState, room.token) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .collect { last -> if (last >= reversed.size - 8) sessions.loadOlder(room.token) }
    }
    // Keep the newest message visible when sending / receiving while at the bottom.
    LaunchedEffect(messages.lastOrNull()?.key) {
        if (listState.firstVisibleItemIndex <= 2) listState.animateScrollToItem(0)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = {
                    IconButton(onClick = { sessions.closeRoom() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück")
                    }
                },
                title = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Avatar(room.name, 36.dp, room.token, room.avatarVersion, sessions)
                        Spacer(Modifier.width(12.dp))
                        Text(room.name, maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 18.sp)
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().imePadding()) {
            if (!state.online) OfflineBanner()
            Box(Modifier.weight(1f)) {
                LazyColumn(
                    state = listState,
                    reverseLayout = true,
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 12.dp, vertical = 8.dp),
                ) {
                    itemsIndexed(reversed, key = { _, m -> m.key }) { i, m ->
                        // reversed: i+1 is older (previous), i-1 is newer (next)
                        MessageRow(
                            m,
                            prev = reversed.getOrNull(i + 1),
                            next = reversed.getOrNull(i - 1),
                            isGroup = isGroup,
                            read = m.msg.isOwn && m.status == null && m.msg.id <= lastCommonRead,
                            onReply = { replyTo = it },
                            onRetry = { sessions.retry(room.token, it) },
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
                )
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
    m: UiMessage,
    prev: UiMessage?,
    next: UiMessage?,
    isGroup: Boolean,
    read: Boolean,
    onReply: (MessageView) -> Unit,
    onRetry: (MessageView) -> Unit,
) {
    val colors = LocalTalkColors.current
    val msg = m.msg
    Column(Modifier.fillMaxWidth()) {
        if (prev == null || !sameDay(prev.msg.timestamp, msg.timestamp)) {
            Box(Modifier.fillMaxWidth().padding(top = 14.dp, bottom = 8.dp), Alignment.Center) {
                Text(
                    dayLabel(msg.timestamp),
                    fontSize = 12.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = colors.muted,
                    modifier = Modifier.clip(RoundedCornerShape(10.dp)).background(colors.chip).padding(horizontal = 10.dp, vertical = 3.dp),
                )
            }
        }
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
        val jumbo = msg.kind == MessageKind.TEXT && msg.replyTo == null && isJumboEmoji(msg.text)

        // Swipe right to reply (Signal gesture).
        val offset = remember { Animatable(0f) }
        val haptics = LocalHapticFeedback.current
        val scope = rememberCoroutineScope()
        var menu by remember { mutableStateOf(false) }
        val clipboard = LocalClipboardManager.current
        val replyable = msg.kind == MessageKind.TEXT && m.status == null
        val threshold = 160f

        Row(
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
            horizontalArrangement = if (own) Arrangement.End else Arrangement.Start,
            verticalAlignment = Alignment.Bottom,
        ) {
            if (offset.value > 8f) {
                Icon(
                    ReplyIcon,
                    null,
                    tint = colors.muted,
                    modifier = Modifier.align(Alignment.CenterVertically).graphicsLayer { alpha = (offset.value / threshold).coerceAtMost(1f) },
                )
            }
            if (isGroup && !own) {
                Box(Modifier.width(36.dp)) { if (last) Avatar(msg.author, 28.dp) }
            }
            Box(Modifier.offset { IntOffset(offset.value.roundToInt(), 0) }) {
                Column(
                    Modifier
                        .widthIn(max = 300.dp)
                        .clip(shape)
                        .background(
                            when {
                                jumbo -> androidx.compose.ui.graphics.Color.Transparent
                                own -> colors.bubbleOwn
                                else -> colors.bubbleOther
                            },
                        )
                        .combinedClickable(
                            onClick = { if (m.status == SendStatus.FAILED) onRetry(msg) },
                            onLongClick = {
                                haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                menu = true
                            },
                        )
                        .graphicsLayer { alpha = if (m.status == SendStatus.FAILED) 0.6f else 1f }
                        .padding(horizontal = if (jumbo) 0.dp else 12.dp, vertical = 7.dp),
                ) {
                    val textColor = if (own && !jumbo) colors.bubbleOwnText else colors.bubbleOtherText
                    if (isGroup && !own && first) {
                        Text(msg.author, color = colorFor(msg.author), fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    }
                    msg.replyTo?.let { r ->
                        Column(
                            Modifier
                                .padding(top = 2.dp, bottom = 6.dp)
                                .clip(RoundedCornerShape(6.dp))
                                .background(textColor.copy(alpha = 0.1f))
                                .padding(horizontal = 8.dp, vertical = 4.dp),
                        ) {
                            Text(if (r.isOwn) "Du" else r.author, color = textColor, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                            Text(r.text.ifEmpty { "Nachricht gelöscht" }, color = textColor.copy(alpha = 0.85f), fontSize = 13.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        }
                    }
                    when {
                        msg.kind == MessageKind.DELETED ->
                            Text("Diese Nachricht wurde gelöscht.", color = textColor, fontStyle = FontStyle.Italic)
                        jumbo -> Text(msg.text, fontSize = 44.sp)
                        else -> Text(
                            remember(msg.text, msg.markdown, textColor) { renderMessage(msg.text, msg.markdown, textColor) },
                            color = textColor,
                            style = TextStyle(fontSize = 15.sp, lineHeight = 21.sp),
                        )
                    }
                    if (msg.reactions.isNotEmpty()) {
                        Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                            msg.reactions.forEach { (emoji, count) ->
                                Text(
                                    if (count > 1) "$emoji $count" else emoji,
                                    fontSize = 13.sp,
                                    color = textColor,
                                    modifier = Modifier.clip(RoundedCornerShape(10.dp)).background(textColor.copy(alpha = 0.1f)).padding(horizontal = 6.dp, vertical = 1.dp),
                                )
                            }
                        }
                    }
                    if (last || m.status != null) {
                        Row(
                            Modifier.align(Alignment.End).padding(top = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(3.dp),
                        ) {
                            val metaColor = if (jumbo) colors.muted else textColor.copy(alpha = 0.75f)
                            if (msg.edited) Text("bearbeitet ·", fontSize = 11.sp, color = metaColor)
                            Text(
                                when (m.status) {
                                    SendStatus.SENDING -> "Wird gesendet …"
                                    SendStatus.FAILED -> "Nicht gesendet · Tippen zum Wiederholen"
                                    null -> messageTime(msg.timestamp)
                                },
                                fontSize = 11.sp,
                                color = if (m.status == SendStatus.FAILED) colors.danger else metaColor,
                            )
                            if (own && m.status == null) Text(if (read) "✓✓" else "✓", fontSize = 11.sp, color = metaColor)
                        }
                    }
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    if (replyable) {
                        DropdownMenuItem(text = { Text("Antworten") }, onClick = { menu = false; onReply(msg) })
                    }
                    DropdownMenuItem(text = { Text("Kopieren") }, onClick = {
                        menu = false
                        clipboard.setText(AnnotatedString(msg.text))
                    })
                }
            }
        }
    }
}

@Composable
private fun Composer(
    token: String,
    replyTo: MessageView?,
    onCancelReply: () -> Unit,
    onSend: (String) -> Unit,
) {
    val colors = LocalTalkColors.current
    var text by rememberSaveable(token) { mutableStateOf("") }
    Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
        HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f))
        replyTo?.let { r ->
            Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(
                    Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(8.dp))
                        .background(colors.chip)
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                ) {
                    Text(if (r.isOwn) "Du" else r.author, color = colors.accent, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                    Text(r.text, maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 13.sp)
                }
                IconButton(onClick = onCancelReply) { Icon(Icons.Default.Close, "Antwort abbrechen") }
            }
        }
        Row(Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 8.dp), verticalAlignment = Alignment.Bottom) {
            BasicTextField(
                value = text,
                onValueChange = { text = it },
                textStyle = TextStyle(fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurface),
                cursorBrush = SolidColor(colors.accent),
                maxLines = 6,
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 44.dp)
                    .clip(RoundedCornerShape(22.dp))
                    .background(colors.chip),
                decorationBox = { inner ->
                    Box(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
                        if (text.isEmpty()) Text("Nachricht", color = colors.muted, fontSize = 16.sp)
                        inner()
                    }
                },
            )
            AnimatedVisibility(visible = text.isNotBlank(), enter = scaleIn(), exit = scaleOut()) {
                IconButton(
                    onClick = {
                        val value = text.trim()
                        if (value.isNotEmpty()) {
                            text = ""
                            onSend(value)
                        }
                    },
                    modifier = Modifier.padding(start = 8.dp).size(44.dp).clip(CircleShape).background(colors.accent),
                ) {
                    Icon(Icons.AutoMirrored.Filled.Send, "Senden", tint = androidx.compose.ui.graphics.Color.White)
                }
            }
        }
    }
}
