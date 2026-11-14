package org.wolkenfunk.app.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.wolkenfunk.app.SessionManager

/** Initials avatar; with [token] the server avatar is shown once loaded (PNG/JPEG only). */
@Composable
fun Avatar(
    name: String,
    size: Dp = 48.dp,
    token: String? = null,
    version: String = "",
    sessions: SessionManager? = null,
) {
    val dark = isSystemInDarkTheme()
    val cached = if (token != null) sessions?.cachedAvatar(token, version, dark) else null
    val image by produceState(cached, token, version, dark) {
        value = cached ?: if (token != null && sessions != null) sessions.avatar(token, version, dark) else null
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
