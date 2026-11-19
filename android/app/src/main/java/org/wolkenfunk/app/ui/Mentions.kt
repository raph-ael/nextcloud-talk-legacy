package org.wolkenfunk.app.ui

import android.os.Build
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.wolkenfunk.app.SessionManager
import uniffi.talk_ffi.MentionCandidate
import uniffi.talk_ffi.MentionPick
import uniffi.talk_ffi.MentionView
import uniffi.talk_ffi.RoomView

// --- input field (docs/mentions.md „Vorschlagsliste“ and „Im Eingabefeld“) ------------------

/** The text after an „@“ is searched for up to this length. */
private const val QUERY_MAX = 40

/** The picks of an input field, kept with its draft. */
val PicksSaver = listSaver<List<MentionPick>, String>(
    save = { picks -> picks.flatMap { listOf(it.label, it.mentionId) } },
    restore = { saved -> saved.chunked(2).map { MentionPick(it[0], it[1]) } },
)

private fun letterOrDigitAt(text: String, index: Int): Boolean =
    index < text.length && Character.isLetterOrDigit(text.codePointAt(index))

/**
 * Where [text] mentions somebody, by the rule of the core (`wire_text`): `@{label}` of a
 * pick after the start, white space or one of `-.'@`, with no letter or digit behind it. The
 * longest name wins. Nothing is remembered but the picks: the text alone decides.
 */
fun fieldMentions(text: String, picks: List<MentionPick>): List<IntRange> {
    if (picks.isEmpty() || '@' !in text) return emptyList()
    val labels = picks.map { it.label }.filter { it.isNotEmpty() }.distinct().sortedByDescending { it.codePointCount(0, it.length) }
    val out = ArrayList<IntRange>()
    var i = 0
    // Whether a mention may begin here.
    var free = true
    while (i < text.length) {
        val c = text[i]
        if (c == '@' && free) {
            val label = labels.firstOrNull { text.startsWith(it, i + 1) && !letterOrDigitAt(text, i + 1 + it.length) }
            if (label != null) {
                out += i until i + 1 + label.length
                i += 1 + label.length
                free = false
                continue
            }
        }
        free = c.isWhitespace() || c in "-.'@"
        i++
    }
    return out
}

/** The „@“ the cursor types behind ([at]) and what stands between the two. */
data class MentionQuery(val at: Int, val text: String)

/**
 * What the suggestions search for: from the nearest „@“ before the cursor that stands at the
 * start or after white space, in the same line and at most [QUERY_MAX] characters away.
 * Nothing while the cursor stands in a finished mention or right behind it; further on the
 * text counts from the „@“ as always („@Lena Br“ finds Lena Brandt although „Lena“ is picked).
 */
fun mentionQuery(value: TextFieldValue, picks: List<MentionPick>): MentionQuery? {
    if (!value.selection.collapsed) return null
    val text = value.text
    val cursor = value.selection.start.coerceAtMost(text.length)
    if (fieldMentions(text, picks).any { cursor > it.first && cursor <= it.last + 1 }) return null
    var i = cursor - 1
    while (i >= 0 && cursor - i - 1 <= QUERY_MAX) {
        val c = text[i]
        if (c == '\n') return null
        if (c == '@' && (i == 0 || text[i - 1].isWhitespace())) return MentionQuery(i, text.substring(i + 1, cursor))
        i--
    }
    return null
}

/**
 * Takes a suggestion over: `@{label}` and a space replace everything from the „@“ to the
 * cursor (no second space when one follows), the cursor stands behind the space.
 */
fun acceptMention(value: TextFieldValue, query: MentionQuery, label: String): TextFieldValue {
    val text = value.text
    val cursor = value.selection.start.coerceIn(query.at, text.length)
    val space = if (cursor < text.length && text[cursor] == ' ') "" else " "
    return TextFieldValue(text.replaceRange(query.at, cursor, "@$label$space"), TextRange(query.at + label.length + 2))
}

/**
 * Deleting backwards with the cursor right behind a mention removes it as a whole, with its
 * „@“. [new] is what the field wants to become; any other change passes unchanged.
 */
fun deleteWholeMention(old: TextFieldValue, new: TextFieldValue, picks: List<MentionPick>): TextFieldValue {
    if (picks.isEmpty() || !old.selection.collapsed || !new.selection.collapsed) return new
    val cursor = old.selection.start
    val removed = old.text.length - new.text.length
    if (removed < 1 || removed > cursor || new.selection.start != cursor - removed) return new
    if (new.text != old.text.removeRange(cursor - removed, cursor)) return new
    val mention = fieldMentions(old.text, picks).firstOrNull { it.last + 1 == cursor } ?: return new
    return TextFieldValue(old.text.removeRange(mention.first, cursor), TextRange(mention.first))
}

/** Puts [color] behind the mentions of an input field; weight and width of the text stay. */
data class MentionHighlight(val picks: List<MentionPick>, val color: Color) : VisualTransformation {
    override fun filter(text: AnnotatedString): TransformedText {
        val ranges = fieldMentions(text.text, picks)
        if (ranges.isEmpty()) return TransformedText(text, OffsetMapping.Identity)
        val marked = buildAnnotatedString {
            append(text)
            for (r in ranges) addStyle(SpanStyle(background = color), r.first, r.last + 1)
        }
        return TransformedText(marked, OffsetMapping.Identity)
    }
}

/**
 * „Zurück“ that comes before the keyboard closes (Android 13 and later; before, the keyboard
 * closes first as for every other „Zurück“ of the app).
 */
@Composable
fun BackBeforeKeyboard(enabled: Boolean, onBack: () -> Unit) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
        BackHandler(enabled, onBack)
        return
    }
    val current by rememberUpdatedState(onBack)
    val activity = LocalActivity.current
    DisposableEffect(enabled, activity) {
        if (!enabled || activity == null) return@DisposableEffect onDispose {}
        val callback = OnBackInvokedCallback { current() }
        activity.onBackInvokedDispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_OVERLAY, callback)
        onDispose { activity.onBackInvokedDispatcher.unregisterOnBackInvokedCallback(callback) }
    }
}

/**
 * The suggestions after an „@“ (docs/mentions.md „Android und iOS“): rows of 48 with a
 * picture of 32, at most four and a half of them, the best one on top.
 */
@Composable
fun MentionSuggestions(sessions: SessionManager, room: RoomView, items: List<MentionCandidate>, onPick: (MentionCandidate) -> Unit) {
    val colors = LocalTalkColors.current
    val listState = rememberLazyListState()
    // Another search text: the best row is on top again.
    LaunchedEffect(items) { listState.scrollToItem(0) }
    Column(Modifier.fillMaxWidth().background(colors.floating)) {
        HorizontalDivider(thickness = Dp.Hairline, color = MaterialTheme.colorScheme.outlineVariant)
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = 216.dp), state = listState) {
            items(items) { c ->
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { onPick(c) }.padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    when (c.kind) {
                        "user" -> UserAvatar(sessions, c.label, c.userId.orEmpty(), 32.dp)
                        "all" -> Avatar(room.name, 32.dp, room.token, room.avatarVersion, sessions)
                        "group", "team" -> Box(Modifier.size(32.dp).clip(CircleShape).background(colors.chip), Alignment.Center) {
                            Icon(TalkIcons.Group, null, tint = colors.muted, modifier = Modifier.size(18.dp))
                        }
                        else -> Avatar(c.label, 32.dp)
                    }
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(c.label, fontSize = 15.sp, lineHeight = 20.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        if (c.subline.isNotEmpty()) {
                            Text(c.subline, fontSize = 12.sp, lineHeight = 16.sp, color = colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
        }
    }
}

// --- bubble (docs/mentions.md „In der Nachricht“) -------------------------------------------

/** Annotation of a mention that opens a chat; its item is the index in `MessageView.mentions`. */
private const val MENTION_TAG = "mention"

/**
 * Marks the mentions of a message in its rendered text: semibold on [mention], on
 * [mentionSelf] where somebody else means the user. Every occurrence of a label that no
 * letter or digit follows, the longest name first, not in code. Run before the search marks,
 * which win where both meet.
 */
fun markMentions(text: AnnotatedString, mentions: List<MentionView>, own: Boolean, mention: Color, mentionSelf: Color): AnnotatedString {
    if (mentions.isEmpty() || text.isEmpty()) return text
    val plain = text.text
    val code = text.spanStyles.filter { it.item.fontFamily == FontFamily.Monospace }
    val taken = ArrayList<IntRange>()
    return buildAnnotatedString {
        append(text)
        for ((index, m) in mentions.withIndex().sortedByDescending { it.value.label.length }) {
            if (m.label.length < 2) continue
            var at = plain.indexOf(m.label)
            while (at >= 0) {
                val end = at + m.label.length
                val free = !letterOrDigitAt(plain, end) && taken.none { at <= it.last && it.first < end } &&
                    code.none { at < it.end && it.start < end }
                if (free) {
                    taken += at until end
                    addStyle(SpanStyle(fontWeight = FontWeight.SemiBold, background = if (m.isSelf && !own) mentionSelf else mention), at, end)
                    // Only a person, and not the user, has a chat to open (docs/mentions.md „Antippen“).
                    if (m.kind == "user" && !m.isSelf) addStringAnnotation(MENTION_TAG, index.toString(), at, end)
                }
                at = plain.indexOf(m.label, if (free) end else at + 1)
            }
        }
    }
}

private fun mentionAt(text: AnnotatedString, layout: TextLayoutResult?, position: Offset): Int? {
    layout ?: return null
    // The layout may be one frame older than the text.
    val length = minOf(text.length, layout.layoutInput.text.length)
    return text.getStringAnnotations(MENTION_TAG, 0, length)
        .firstOrNull { a -> (a.start until minOf(a.end, length)).any { layout.getBoundingBox(it).contains(position) } }
        ?.item?.toIntOrNull()
}

/**
 * A tap on a mention [text] marks as a person calls [onTap] with its index. Nothing is
 * taken from the bubble around it: a long press stays its menu, and so does every tap beside
 * a mention. Screen readers get one action per person, „Chat mit {Name} öffnen“.
 */
fun Modifier.mentionTaps(text: AnnotatedString, labels: List<String>, layout: () -> TextLayoutResult?, onTap: (Int) -> Unit): Modifier {
    val marked = text.getStringAnnotations(MENTION_TAG, 0, text.length)
    if (marked.isEmpty()) return this
    val indices = marked.mapNotNull { it.item.toIntOrNull() }.distinct()
    return this
        .semantics {
            customActions = indices.map { i ->
                CustomAccessibilityAction("Chat mit ${labels.getOrNull(i).orEmpty().removePrefix("@")} öffnen") {
                    onTap(i)
                    true
                }
            }
        }
        .pointerInput(text) {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false)
                val index = mentionAt(text, layout(), down.position) ?: return@awaitEachGesture
                val up = withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) { waitForUpOrCancellation() }
                if (up != null) {
                    up.consume()
                    onTap(index)
                }
            }
        }
}

// --- „@“ button (docs/mentions.md „„@“-Knopf im Chat“) ---------------------------------------

/** The number on a round floating button, as on „Zum Ende springen“. */
@Composable
fun CountBadge(count: Int, modifier: Modifier = Modifier) {
    Box(
        modifier
            .offset(y = (-10).dp)
            .heightIn(min = 20.dp)
            .widthIn(min = 20.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(LocalTalkColors.current.accent)
            .clearAndSetSemantics {}
            .padding(horizontal = 5.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            if (count > 99) "99+" else count.toString(),
            color = Color.White,
            fontSize = 11.sp,
            lineHeight = 14.sp,
            fontWeight = FontWeight.Medium,
        )
    }
}

/**
 * Round button „@“ above „Zum Ende springen“ while mentions of the user are unseen; from two
 * on with their number. It stands 16 above the other button, whose badge reaches over its
 * edge, and keeps that place while the other one is away.
 */
@Composable
fun MentionJump(count: Int, onClick: () -> Unit, modifier: Modifier) {
    val colors = LocalTalkColors.current
    val label = if (count > 1) "Zur Erwähnung springen, $count Erwähnungen" else "Zur Erwähnung springen"
    AnimatedVisibility(
        visible = count > 0,
        // 14 below „Zum Ende springen“, its 40, and 16 between the two.
        modifier = modifier.padding(end = 14.dp, bottom = 70.dp),
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
                    .clickable(onClick = onClick)
                    .semantics(mergeDescendants = true) {
                        contentDescription = label
                        role = Role.Button
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text("@", color = colors.muted, fontSize = 20.sp, lineHeight = 24.sp, fontWeight = FontWeight.Medium, modifier = Modifier.clearAndSetSemantics {})
            }
            if (count > 1) CountBadge(count, Modifier.align(Alignment.TopCenter))
        }
    }
}
