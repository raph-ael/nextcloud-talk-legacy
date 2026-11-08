package org.sirplus.talk.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.sp

private val INLINE = Regex(
    "(`[^`\\n]+`)|(\\*\\*[^*\\n]+\\*\\*|__[^_\\n]+__)|(~~[^~\\n]+~~)|(\\*[^*\\s][^*\\n]*\\*|_[^_\\s][^_\\n]*_)" +
        "|(\\[[^\\]\\n]+\\]\\(https?://[^\\s)]+\\))|(https?://[^\\s<>\"']+[^\\s<>\"'.,;:!?)\\]])",
)

/**
 * Talk's Markdown subset → AnnotatedString (headings, lists, quotes, code, emphasis, links).
 * Plain text when [markdown] is false (links are still clickable).
 */
fun renderMessage(text: String, markdown: Boolean, linkColor: Color): AnnotatedString = buildAnnotatedString {
    val links = TextLinkStyles(SpanStyle(color = linkColor, textDecoration = TextDecoration.Underline))

    fun inline(s: String) {
        var last = 0
        for (m in INLINE.findAll(s)) {
            if (!markdown && m.groups[6] == null) continue
            append(s.substring(last, m.range.first))
            val tok = m.value
            when {
                m.groups[1] != null -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) { append(tok.drop(1).dropLast(1)) }
                m.groups[2] != null -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { inline(tok.drop(2).dropLast(2)) }
                m.groups[3] != null -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough)) { inline(tok.drop(2).dropLast(2)) }
                m.groups[4] != null -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { inline(tok.drop(1).dropLast(1)) }
                m.groups[5] != null -> {
                    val close = tok.indexOf("](")
                    withLink(LinkAnnotation.Url(tok.substring(close + 2, tok.length - 1), links)) { append(tok.substring(1, close)) }
                }
                else -> withLink(LinkAnnotation.Url(tok, links)) { append(tok) }
            }
            last = m.range.last + 1
        }
        append(s.substring(last))
    }

    if (!markdown) {
        inline(text)
        return@buildAnnotatedString
    }

    var inCode = false
    val lines = text.replace("\r\n", "\n").split('\n')
    lines.forEachIndexed { i, raw ->
        if (raw.trimStart().startsWith("```")) {
            inCode = !inCode
            return@forEachIndexed
        }
        if (inCode) {
            withStyle(SpanStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp)) { append(raw) }
        } else {
            val heading = Regex("^\\s{0,3}#{1,6}\\s+(.*)$").find(raw)
            val bullet = Regex("^\\s*[-*+]\\s+(.*)$").find(raw)
            val quote = Regex("^\\s*>\\s?(.*)$").find(raw)
            when {
                heading != null -> withStyle(SpanStyle(fontWeight = FontWeight.Bold, fontSize = 16.sp)) { inline(heading.groupValues[1]) }
                Regex("^\\s{0,3}([-*_])(\\s*\\1){2,}\\s*$").matches(raw) -> append("———")
                bullet != null -> { append("•  "); inline(bullet.groupValues[1]) }
                quote != null -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append("▎ "); inline(quote.groupValues[1]) }
                else -> inline(raw)
            }
        }
        if (i < lines.lastIndex) append('\n')
    }
}
