package org.wolkenfunk.app.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/** Design tokens shared with desktop (styles.css) and iOS. */
@Immutable
data class TalkColors(
    val accent: Color,
    val bubbleOwn: Color,
    val bubbleOwnText: Color,
    val bubbleOther: Color,
    val bubbleOtherText: Color,
    val muted: Color,
    val chip: Color,
    val danger: Color,
    /** Floating surfaces (jump button), like --floating on desktop. */
    val floating: Color,
)

private val LightTalk = TalkColors(
    accent = Color(0xFF2C6BED),
    bubbleOwn = Color(0xFF2C6BED),
    bubbleOwnText = Color.White,
    bubbleOther = Color(0xFFE9E9EB),
    bubbleOtherText = Color(0xFF1B1B1D),
    muted = Color(0xFF6B6B70),
    chip = Color(0xFFF1F1F3),
    danger = Color(0xFFD6342C),
    floating = Color(0xFFFFFFFF),
)

private val DarkTalk = TalkColors(
    accent = Color(0xFF3B78F0),
    bubbleOwn = Color(0xFF3B78F0),
    bubbleOwnText = Color.White,
    bubbleOther = Color(0xFF2C2C2E),
    bubbleOtherText = Color(0xFFE9E9EB),
    muted = Color(0xFF9A9AA0),
    chip = Color(0xFF232325),
    danger = Color(0xFFFF6B63),
    floating = Color(0xFF2E2E2E),
)

/** Conversation-list typography (Signal: BodyLarge/BodyMedium with tighter line spacing). */
object ListType {
    val title = TextStyle(fontSize = 16.sp, lineHeight = 24.sp, letterSpacing = 0.5.sp, fontWeight = FontWeight.Normal)
    val body = TextStyle(fontSize = 14.sp, lineHeight = 20.sp, letterSpacing = 0.25.sp)
    val section = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, fontWeight = FontWeight.Medium)
}

val LocalTalkColors = staticCompositionLocalOf { LightTalk }

@Composable
fun TalkTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val talk = if (dark) DarkTalk else LightTalk
    val scheme = if (dark) {
        darkColorScheme(
            primary = talk.accent,
            onPrimary = Color.White,
            background = Color(0xFF121212),
            surface = Color(0xFF121212),
            surfaceContainer = Color(0xFF1B1B1C),
            surfaceVariant = Color(0xFF2C2C2E),
        )
    } else {
        lightColorScheme(
            primary = talk.accent,
            onPrimary = Color.White,
            background = Color.White,
            surface = Color.White,
            surfaceContainer = Color(0xFFF6F6F7),
            surfaceVariant = Color(0xFFE9E9EB),
        )
    }
    androidx.compose.runtime.CompositionLocalProvider(LocalTalkColors provides talk) {
        MaterialTheme(colorScheme = scheme, typography = Typography(), content = content)
    }
}
