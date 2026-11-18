package org.wolkenfunk.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.wolkenfunk.app.SessionManager
import uniffi.talk_ffi.SearchHit
import uniffi.talk_ffi.SearchResults
import java.text.Normalizer
import java.util.Locale

/** Hits of the search over all chats come in steps of this many (docs/search.md „Mehr“). */
const val SEARCH_PAGE = 50

/** The search in a chat walks through this many hits (docs/search.md „Mehr als 500“). */
const val CHAT_SEARCH_LIMIT = 500

/** Message hits start at two characters, spaces not counted. */
fun searchable(query: String): Boolean = query.count { !it.isWhitespace() } >= 2

/**
 * Search of the chat list (docs/search.md „Suche über alle Chats“). It lives above the
 * navigation: after a hit was opened, „Zurück“ finds text, hits and position as they were.
 */
@Stable
class RoomListSearch(active: Boolean = false, query: String = "") {
    var active by mutableStateOf(active)
    var query by mutableStateOf(query)
    var limit by mutableIntStateOf(SEARCH_PAGE)
    var results by mutableStateOf<SearchResults?>(null)
    /** Search text and limit [results] belong to. */
    var resultsFor by mutableStateOf<Pair<String, Int>?>(null)

    fun close() {
        active = false
        query = ""
        limit = SEARCH_PAGE
        results = null
        resultsFor = null
    }
}

@Composable
fun rememberRoomListSearch(): RoomListSearch = rememberSaveable(
    saver = listSaver(save = { listOf(it.active, it.query) }, restore = { RoomListSearch(it[0] as Boolean, it[1] as String) }),
) { RoomListSearch() }

/**
 * What the search compares, as the core does it: lowercase and without accents, „ß“ stays.
 * [starts] and [ends] lead from each folded character back to its place in the text.
 */
private class Folded(val text: String, val starts: IntArray, val ends: IntArray)

private fun isMark(codePoint: Int): Boolean = when (Character.getType(codePoint)) {
    Character.NON_SPACING_MARK.toInt(), Character.COMBINING_SPACING_MARK.toInt(), Character.ENCLOSING_MARK.toInt() -> true
    else -> false
}

private fun fold(text: String): Folded {
    val out = StringBuilder(text.length)
    var starts = IntArray(text.length + 8)
    var ends = IntArray(text.length + 8)
    fun put(c: Char, from: Int, to: Int) {
        if (out.length == starts.size) {
            starts = starts.copyOf(starts.size * 2)
            ends = ends.copyOf(ends.size * 2)
        }
        starts[out.length] = from
        ends[out.length] = to
        out.append(c)
    }
    var i = 0
    while (i < text.length) {
        val cp = text.codePointAt(i)
        val next = i + Character.charCount(cp)
        if (cp < 128) {
            put(Character.toLowerCase(cp.toChar()), i, next)
        } else {
            val plain = Normalizer.normalize(text.substring(i, next).lowercase(Locale.ROOT), Normalizer.Form.NFD)
            var j = 0
            while (j < plain.length) {
                val d = plain.codePointAt(j)
                val n = Character.charCount(d)
                if (!isMark(d)) for (k in j until j + n) put(plain[k], i, next)
                j += n
            }
        }
        i = next
    }
    return Folded(out.toString(), starts, ends)
}

/** The words of a search text, folded; empty below two characters. */
fun searchWords(query: String): List<String> =
    if (!searchable(query)) emptyList() else query.split(Regex("[\\s\\p{Z}]+")).map { fold(it).text }.filter { it.isNotEmpty() }.distinct()

/**
 * Puts [mark] behind every occurrence of [words] in [text] (docs/search.md „Hervorhebung“):
 * without case and accents, also inside a word. The text colour stays.
 */
fun markWords(text: AnnotatedString, words: List<String>, mark: Color): AnnotatedString {
    if (words.isEmpty() || text.isEmpty()) return text
    val folded = fold(text.text)
    val ranges = ArrayList<IntRange>()
    for (word in words) {
        var at = folded.text.indexOf(word)
        while (at >= 0) {
            ranges += folded.starts[at] until folded.ends[at + word.length - 1]
            at = folded.text.indexOf(word, at + word.length)
        }
    }
    if (ranges.isEmpty()) return text
    ranges.sortBy { it.first }
    return buildAnnotatedString {
        append(text)
        // Overlapping words become one stretch: the colour is translucent.
        var from = ranges[0].first
        var to = ranges[0].last
        for (r in ranges.drop(1)) {
            if (r.first <= to + 1) {
                to = maxOf(to, r.last)
            } else {
                addStyle(SpanStyle(background = mark), from, to + 1)
                from = r.first
                to = r.last
            }
        }
        addStyle(SpanStyle(background = mark), from, to + 1)
    }
}

/**
 * A message hit in the chat list (docs/search.md „Trefferzeile“): avatar and name of the
 * chat, the date as in the list, below the snippet with the search words bold.
 */
@Composable
fun SearchHitRow(hit: SearchHit, avatarVersion: String, sessions: SessionManager, now: Long, onClick: () -> Unit) {
    val colors = LocalTalkColors.current
    val onSurface = MaterialTheme.colorScheme.onSurface
    val snippet = remember(hit, onSurface) {
        buildAnnotatedString {
            if (hit.showActor) append(if (hit.isOwn) "Du: " else "${hit.actorName}: ")
            for (part in hit.snippet) {
                if (part.hit) {
                    withStyle(SpanStyle(fontWeight = FontWeight.Bold, color = onSurface)) { append(part.text) }
                } else {
                    append(part.text)
                }
            }
        }
    }
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 24.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Avatar(hit.roomName, 48.dp, hit.token, avatarVersion, sessions)
        Spacer(Modifier.width(20.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(hit.roomName, style = ListType.title, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                Spacer(Modifier.width(12.dp))
                Text(listTime(hit.timestamp, now), style = ListType.body, color = colors.muted, maxLines = 1)
            }
            Text(snippet, style = ListType.body, color = colors.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

/**
 * The bar of the search in a chat, in place of the input bar (docs/search.md „Leiste“): the
 * counter on the left, on the right the arrows to the older and to the newer hit.
 */
@Composable
fun ChatSearchBar(counter: String, older: Boolean, newer: Boolean, onOlder: () -> Unit, onNewer: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 20.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            counter,
            fontSize = 15.sp,
            color = LocalTalkColors.current.muted,
            maxLines = 1,
            modifier = Modifier.weight(1f).semantics { liveRegion = LiveRegionMode.Polite },
        )
        IconButton(onClick = onOlder, enabled = older) { Icon(Icons.Filled.KeyboardArrowUp, "Älterer Treffer") }
        IconButton(onClick = onNewer, enabled = newer) { Icon(Icons.Filled.KeyboardArrowDown, "Neuerer Treffer") }
    }
}
