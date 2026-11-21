package org.wolkenfunk.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.wolkenfunk.app.SessionManager

/**
 * Account whose avatars are drawn below (docs/accounts.md): `null` is the active one. The
 * call screen draws the call's account, the account menu each row's own.
 */
val LocalAvatarAccount = staticCompositionLocalOf<String?> { null }

/**
 * Initials avatar; with [token] the server avatar is shown once loaded (PNG/JPEG only).
 * „Notizen an mich“ gets [NotesAvatar] instead of the server's picture.
 */
@Composable
fun Avatar(
    name: String,
    size: Dp = 48.dp,
    token: String? = null,
    version: String = "",
    sessions: SessionManager? = null,
) {
    val dark = isDarkTheme()
    val account = LocalAvatarAccount.current
    if (token != null && sessions?.hasNotesAvatar(token, account) == true) {
        NotesAvatar(size)
        return
    }
    val cached = if (token != null) sessions?.cachedAvatar(token, version, dark, account) else null
    val image by produceState(cached, token, version, dark, account) {
        value = cached ?: if (token != null && sessions != null) sessions.avatar(token, version, dark, account) else null
    }
    val img = image
    if (img != null) {
        Image(
            bitmap = img,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.size(size).clip(CircleShape),
        )
    } else {
        Box(
            Modifier.size(size).clip(CircleShape).background(colorFor(name)),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                initials(name),
                color = Color.White,
                fontWeight = FontWeight.SemiBold,
                fontSize = (size.value * 0.38f).sp,
            )
        }
    }
}

/** „Notizen an mich“: a sheet with three lines, white on the accent colour. Nothing is loaded. */
@Composable
private fun NotesAvatar(size: Dp) {
    Canvas(Modifier.size(size).clip(CircleShape).background(LocalTalkColors.current.accent)) {
        // The icon's 24 grid is half the avatar wide and sits in the middle.
        val u = this.size.minDimension / 48f
        val o = this.size.minDimension / 4f
        fun at(x: Float, y: Float) = Offset(o + x * u, o + y * u)
        val w = 1.8f * u
        drawRoundRect(Color.White, at(5f, 3.5f), Size(14f * u, 17f * u), CornerRadius(2.5f * u), Stroke(w))
        for ((y, end) in listOf(8.5f to 15.5f, 12f to 15.5f, 15.5f to 12.5f)) {
            drawLine(Color.White, at(8.5f, y), at(end, y), w, StrokeCap.Round)
        }
    }
}
