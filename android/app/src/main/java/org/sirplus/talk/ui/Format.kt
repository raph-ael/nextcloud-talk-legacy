package org.sirplus.talk.ui

import androidx.compose.ui.graphics.Color
import java.text.BreakIterator
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import uniffi.talk_ffi.FileAttachment

private val zone get() = ZoneId.systemDefault()
private val de = Locale.GERMANY
private val timeFmt = DateTimeFormatter.ofPattern("HH:mm", de)
private val weekdayFmt = DateTimeFormatter.ofPattern("EEE", de)
private val shortDateFmt = DateTimeFormatter.ofPattern("dd.MM.yy", de)
private val dayFmt = DateTimeFormatter.ofPattern("EEEE, d. MMMM", de)
private val dayYearFmt = DateTimeFormatter.ofPattern("EEEE, d. MMMM yyyy", de)

private fun date(unix: Long): LocalDate = Instant.ofEpochSecond(unix).atZone(zone).toLocalDate()
private fun daysAgo(unix: Long): Long = ChronoUnit.DAYS.between(date(unix), LocalDate.now(zone))

private val monthDayFmt = DateTimeFormatter.ofPattern("d. MMM", de)

/** Conversation list, like Signal: Jetzt · 5 Min. · 3 Std. · Fr. · 12. Okt. · 12.10.25 */
fun listTime(unix: Long, nowMillis: Long = System.currentTimeMillis()): String {
    if (unix <= 0) return ""
    val secs = nowMillis / 1000 - unix
    val dt = Instant.ofEpochSecond(unix).atZone(zone)
    return when {
        secs < 60 -> "Jetzt"
        secs < 3600 -> "${secs / 60} Min."
        secs < 24 * 3600 -> "${secs / 3600} Std."
        daysAgo(unix) < 7 -> weekdayFmt.format(dt)
        dt.year == LocalDate.now(zone).year -> monthDayFmt.format(dt)
        else -> shortDateFmt.format(dt)
    }
}

fun messageTime(unix: Long): String = timeFmt.format(Instant.ofEpochSecond(unix).atZone(zone))

fun dayLabel(unix: Long): String {
    val d = daysAgo(unix)
    val day = date(unix)
    return when {
        d == 0L -> "Heute"
        d == 1L -> "Gestern"
        day.year == LocalDate.now(zone).year -> dayFmt.format(day)
        else -> dayYearFmt.format(day)
    }
}

fun sameDay(a: Long, b: Long): Boolean = date(a) == date(b)

private val palette = listOf(
    Color(0xFFD0463B), Color(0xFFD9822B), Color(0xFF3D9A50), Color(0xFF2C7BD6),
    Color(0xFF6B5BD2), Color(0xFFC2408F), Color(0xFF2A9A9E), Color(0xFF8A7350),
)

/** Stable color per name (same hash as desktop and iOS). */
fun colorFor(name: String): Color {
    var h = 0
    name.codePoints().forEach { h = (h * 31 + it) and 0x7fffffff }
    return palette[h % palette.size]
}

fun initials(name: String): String {
    val letters = name.split(' ', '.').filter { it.isNotBlank() }.take(2)
        .joinToString("") { String(Character.toChars(it.codePointAt(0))) }
    return letters.uppercase().ifEmpty { "?" }
}

/** 1–3 emoji without other text are shown large (like Signal). */
fun isJumboEmoji(text: String): Boolean {
    val t = text.trim()
    if (t.isEmpty() || t.length > 40) return false
    val it = BreakIterator.getCharacterInstance().apply { setText(t) }
    var count = 0
    var start = it.first()
    var end = it.next()
    while (end != BreakIterator.DONE) {
        val g = t.substring(start, end)
        if (g.isNotBlank()) {
            val cp = g.codePointAt(0)
            val emoji = Character.getType(cp) == Character.OTHER_SYMBOL.toInt() ||
                Character.getType(cp) == Character.SURROGATE.toInt() || cp in 0x1F000..0x1FAFF || cp in 0x2600..0x27BF
            if (!emoji) return false
            count++
        }
        start = end
        end = it.next()
    }
    return count in 1..3
}

/** Short label for a file message: "📷 Foto", "🎤 Sprachnachricht", "📎 name.pdf". */
fun attachmentLabel(a: FileAttachment): String = when {
    a.voiceMessage -> "🎤 Sprachnachricht"
    a.mimetype.startsWith("image/") -> "📷 Foto"
    a.mimetype.startsWith("video/") -> "🎬 Video"
    else -> "📎 ${a.name}"
}

/** One-line preview of a message that may carry a file: label plus caption. */
fun withAttachment(text: String, a: FileAttachment?): String = when {
    a == null -> text
    text.isEmpty() -> attachmentLabel(a)
    else -> "${attachmentLabel(a)} · $text"
}

/** 1,2 MB style file size. */
fun fileSize(bytes: Long): String = when {
    bytes < 1024 -> "$bytes B"
    bytes < 1024 * 1024 -> "${bytes / 1024} KB"
    else -> String.format(de, "%.1f MB", bytes / (1024.0 * 1024.0))
}
