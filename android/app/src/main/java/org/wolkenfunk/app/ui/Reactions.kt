package org.wolkenfunk.app.ui

import android.view.ContextThemeWrapper
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionOnScreen
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.rememberNestedScrollInteropConnection
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import androidx.emoji2.emojipicker.EmojiPickerView
import org.wolkenfunk.app.SessionManager
import kotlin.math.roundToInt
import uniffi.talk_ffi.ReactionView

/** Signal's quick reactions (docs/chat.md „Reaktionen“). */
private val QUICK = listOf("❤️", "👍", "👎", "😂", "😮", "😢")

/** The six shown in the bar; an own reaction outside them takes the last place instead of 😢. */
fun quickReactions(mine: List<String>): List<String> {
    val other = mine.firstOrNull { it !in QUICK } ?: return QUICK
    return QUICK.dropLast(1) + other
}

/** Pills in display order: most used first, then by emoji (the FFI map has no order). */
fun sortedReactions(reactions: Map<String, Long>): List<Pair<String, Long>> =
    reactions.entries.sortedWith(compareByDescending<Map.Entry<String, Long>> { it.value }.thenBy { it.key })
        .map { it.key to it.value }

class MenuAction(val label: String, val run: () -> Unit)

/**
 * Long-press menu: the quick reaction bar floats above the bubble, the actions sit below it,
 * both in one popup so they never overlap and always close together. Must be placed inside
 * the box that has the bubble's bounds (the popup anchor). [mine] is null when the message
 * cannot be reacted to (no bar then).
 */
@Composable
fun MessageMenu(
    own: Boolean,
    mine: List<String>?,
    actions: List<MenuAction>,
    onReact: (String) -> Unit,
    onMoreReactions: () -> Unit,
    onDismiss: () -> Unit,
) {
    var anchor by remember { mutableStateOf<IntRect?>(null) }
    var window by remember { mutableStateOf(IntSize.Zero) }
    // Where the popup window really ended up (the window manager keeps it below the status
    // bar), so the anchor is mapped through screen coordinates.
    var origin by remember { mutableStateOf<IntOffset?>(null) }
    val host = LocalView.current
    // The popup covers the whole window; children are placed around the recorded anchor.
    val provider = remember {
        object : PopupPositionProvider {
            override fun calculatePosition(
                anchorBounds: IntRect,
                windowSize: IntSize,
                layoutDirection: LayoutDirection,
                popupContentSize: IntSize,
            ): IntOffset {
                anchor = anchorBounds
                window = windowSize
                return IntOffset.Zero
            }
        }
    }
    Popup(popupPositionProvider = provider, onDismissRequest = onDismiss, properties = PopupProperties(focusable = true)) {
        Layout(
            modifier = Modifier
                .fillMaxSize()
                .onGloballyPositioned { c ->
                    val p = c.positionOnScreen()
                    origin = IntOffset(p.x.roundToInt(), p.y.roundToInt())
                }
                .pointerInput(Unit) { detectTapGestures { onDismiss() } },
            content = {
                if (mine != null) {
                    ReactionBar(mine, onReact = { onReact(it); onDismiss() }, onMore = { onMoreReactions(); onDismiss() })
                }
                if (actions.isNotEmpty()) {
                    Surface(
                        shape = MenuDefaults.shape,
                        color = MenuDefaults.containerColor,
                        tonalElevation = MenuDefaults.TonalElevation,
                        shadowElevation = MenuDefaults.ShadowElevation,
                        modifier = Modifier.widthIn(min = 112.dp, max = 280.dp),
                    ) {
                        Column(Modifier.width(IntrinsicSize.Max).padding(vertical = 8.dp)) {
                            actions.forEach { a ->
                                DropdownMenuItem(text = { Text(a.label) }, onClick = { onDismiss(); a.run() })
                            }
                        }
                    }
                }
            },
        ) { measurables, constraints ->
            val loose = constraints.copy(minWidth = 0, minHeight = 0)
            val placeables = measurables.map { it.measure(loose) }
            val bar = if (mine != null) placeables.getOrNull(0) else null
            val menu = if (actions.isNotEmpty()) placeables.lastOrNull() else null
            layout(constraints.maxWidth, constraints.maxHeight) {
                val o = origin ?: return@layout
                val loc = IntArray(2).also { host.getLocationOnScreen(it) }
                val a = anchor?.translate(loc[0] - o.x, loc[1] - o.y) ?: return@layout
                val w = constraints.maxWidth
                val h = minOf(constraints.maxHeight, window.height.takeIf { it > 0 } ?: constraints.maxHeight)
                val gap = 8.dp.roundToPx()
                val margin = 8.dp.roundToPx()
                fun x(width: Int) = (if (own) a.right - width else a.left).coerceIn(margin, maxOf(margin, w - width - margin))
                var barBottom = Int.MIN_VALUE
                if (bar != null) {
                    val y = (a.top - gap - bar.height).coerceIn(margin, maxOf(margin, h - bar.height - margin))
                    bar.place(x(bar.width), y)
                    barBottom = y + bar.height
                }
                if (menu != null) {
                    var y = a.bottom + gap
                    if (y + menu.height > h - margin) y = h - margin - menu.height
                    y = maxOf(y, barBottom + gap, margin)
                    menu.place(x(menu.width), y)
                }
            }
        }
    }
}

@Composable
private fun ReactionBar(mine: List<String>, onReact: (String) -> Unit, onMore: () -> Unit) {
    val colors = LocalTalkColors.current
    Surface(
        shape = RoundedCornerShape(28.dp),
        color = MenuDefaults.containerColor,
        tonalElevation = MenuDefaults.TonalElevation,
        shadowElevation = 6.dp,
    ) {
        Row(Modifier.padding(horizontal = 6.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            quickReactions(mine).forEach { emoji ->
                val active = emoji in mine
                Box(
                    Modifier
                        .size(44.dp)
                        .clip(CircleShape)
                        .background(if (active) colors.accent.copy(alpha = 0.22f) else Color.Transparent)
                        .clickable { onReact(emoji) }
                        .clearAndSetSemantics {
                            contentDescription = "Mit $emoji reagieren"
                            role = Role.Button
                            onClick { onReact(emoji); true }
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(emoji, fontSize = 26.sp)
                }
            }
            Box(
                Modifier
                    .padding(start = 2.dp)
                    .size(36.dp)
                    .clip(CircleShape)
                    .background(colors.chip)
                    .clickable(onClick = onMore)
                    .clearAndSetSemantics {
                        contentDescription = "Weitere Reaktionen"
                        role = Role.Button
                        onClick { onMore(); true }
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Filled.Add, null, tint = colors.muted, modifier = Modifier.size(22.dp))
            }
        }
    }
}

/** Reaction pills under the text; a tap opens „Wer hat reagiert“, a new pill pops in. */
@Composable
fun ReactionPills(
    messageId: Long,
    reactions: Map<String, Long>,
    mine: List<String>,
    color: Color,
    modifier: Modifier = Modifier,
    onOpen: () -> Unit,
) {
    // Pills present when the row first appeared don't animate; later ones do.
    val known = remember(messageId) { HashSet(reactions.keys) }
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        sortedReactions(reactions).forEach { (emoji, count) ->
            key(emoji) {
                val scale = remember { Animatable(if (emoji in known) 1f else 0.6f) }
                LaunchedEffect(Unit) { scale.animateTo(1f, tween(150)) }
                DisposableEffect(Unit) {
                    known += emoji
                    onDispose { known -= emoji }
                }
                val shape = RoundedCornerShape(10.dp)
                val isMine = emoji in mine
                Text(
                    if (count > 1) "$emoji $count" else emoji,
                    fontSize = 13.sp,
                    color = color,
                    modifier = Modifier
                        .graphicsLayer { scaleX = scale.value; scaleY = scale.value }
                        .clip(shape)
                        .background(color.copy(alpha = if (isMine) 0.25f else 0.1f))
                        .clickable(onClick = onOpen)
                        .clearAndSetSemantics {
                            contentDescription = "$emoji $count, Reaktionen anzeigen"
                            role = Role.Button
                            onClick { onOpen(); true }
                        }
                        .padding(horizontal = 6.dp, vertical = 1.dp),
                )
            }
        }
    }
}

/** ＋ in the bar: the full emoji picker in a bottom sheet. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReactionPickerSheet(onPick: (String) -> Unit, onDismiss: () -> Unit) {
    val dark = isDarkTheme()
    val background = MaterialTheme.colorScheme.surfaceContainerLow
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = background,
    ) {
        AndroidView(
            factory = { ctx ->
                val themed = ContextThemeWrapper(
                    ctx,
                    if (dark) android.R.style.Theme_Material_NoActionBar else android.R.style.Theme_Material_Light_NoActionBar,
                )
                EmojiPickerView(themed).apply {
                    setBackgroundColor(background.toArgb())
                    setOnEmojiPickedListener { onPick(it.emoji) }
                }
            },
            modifier = Modifier
                .fillMaxWidth()
                .height(420.dp)
                .nestedScroll(rememberNestedScrollInteropConnection()),
        )
    }
}

/** „Wer hat reagiert“: tabs „Alle · N“ and per emoji; the own row removes the reaction. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WhoReactedSheet(
    sessions: SessionManager,
    token: String,
    messageId: Long,
    onRemove: () -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = LocalTalkColors.current
    var list by remember(messageId) { mutableStateOf<List<ReactionView>?>(null) }
    var failed by remember(messageId) { mutableStateOf(false) }
    LaunchedEffect(messageId) {
        val loaded = sessions.reactions(token, messageId)
        if (loaded == null) failed = true else list = loaded
    }
    var tab by remember(messageId) { mutableIntStateOf(0) }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)) {
        val all = list
        when {
            failed -> Text(
                "Reaktionen konnten nicht geladen werden.",
                color = colors.muted,
                modifier = Modifier.fillMaxWidth().padding(24.dp),
            )
            all == null -> Box(Modifier.fillMaxWidth().height(160.dp), Alignment.Center) {
                CircularProgressIndicator(Modifier.size(24.dp))
            }
            else -> {
                val groups = all.groupBy { it.emoji }.entries
                    .sortedWith(compareByDescending<Map.Entry<String, List<ReactionView>>> { it.value.size }.thenBy { it.key })
                val tabs = listOf("Alle · ${all.size}" to all) + groups.map { "${it.key} ${it.value.size}" to it.value }
                val selected = tab.coerceIn(0, tabs.lastIndex)
                PrimaryScrollableTabRow(
                    selectedTabIndex = selected,
                    edgePadding = 12.dp,
                    containerColor = Color.Transparent,
                ) {
                    tabs.forEachIndexed { i, (label, _) ->
                        Tab(selected = i == selected, onClick = { tab = i }, text = { Text(label, fontSize = 15.sp) })
                    }
                }
                LazyColumn(Modifier.fillMaxWidth().heightIn(min = 160.dp), contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = 8.dp)) {
                    items(tabs[selected].second, key = { "${it.actorType}/${it.actorId}/${it.emoji}" }) { r ->
                        ReactorRow(sessions, r) {
                            onRemove()
                            val rest = all.filterNot { it.isOwn }
                            if (rest.isEmpty()) onDismiss() else list = rest
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ReactorRow(sessions: SessionManager, r: ReactionView, onRemove: () -> Unit) {
    val colors = LocalTalkColors.current
    Row(
        Modifier
            .fillMaxWidth()
            .then(if (r.isOwn) Modifier.clickable(onClick = onRemove) else Modifier)
            .padding(horizontal = 20.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        UserAvatar(sessions, r.name, if (r.actorType == "users") r.actorId else "", 40.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(if (r.isOwn) "Du" else r.name, fontSize = 16.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (r.isOwn) Text("Tippen zum Entfernen", fontSize = 13.sp, color = colors.muted)
        }
        Text(r.emoji, fontSize = 24.sp)
    }
}
