package org.wolkenfunk.app.ui

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.wolkenfunk.app.SendStatus
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.UiMessage
import uniffi.talk_ffi.FileAttachment

/** Width of photos in chat bubbles. */
val ChatImageWidth = 260.dp

fun FileAttachment.isImage() = mimetype.startsWith("image/") && !voiceMessage

/**
 * A photo inside a bubble. Starts from the synchronous caches (local send or earlier load)
 * so re-shown bubbles never flash; the size comes from known dimensions to avoid jumps.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ChatImage(
    sessions: SessionManager,
    m: UiMessage,
    shape: RoundedCornerShape,
    onOpen: (ImageBitmap?) -> Unit,
    onLongPress: () -> Unit,
    overlay: @Composable BoxScope.() -> Unit,
) {
    val a = m.msg.attachment ?: return
    val id = a.id
    val initial = if (id.isEmpty()) sessions.localPreview(m.msg.referenceId) else sessions.cachedPreview(id)
    val image by produceState(initial, id, m.msg.referenceId) {
        if (value == null) {
            value = if (id.isEmpty()) sessions.loadLocalPreview(m.msg.referenceId, m.localFile) else sessions.preview(id)
        }
    }
    val img = image
    val aspect = when {
        a.width > 0 && a.height > 0 -> a.width.toFloat() / a.height
        img != null -> img.width.toFloat() / img.height
        else -> sessions.previewAspect(id) ?: 1f
    }
    val height: Dp = (ChatImageWidth / aspect).coerceIn(120.dp, 340.dp)
    Box(
        Modifier
            .width(ChatImageWidth)
            .height(height)
            .clip(shape)
            .background(Color.Black.copy(alpha = 0.08f))
            .combinedClickable(onClick = { onOpen(img) }, onLongClick = onLongPress),
    ) {
        if (img != null) {
            Image(img, null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        }
        when {
            // Upload running: a real progress ring, like Signal.
            m.status == SendStatus.SENDING && m.progress in 0f..0.999f -> Box(
                Modifier.align(Alignment.Center).size(44.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.35f)),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator(
                    progress = { m.progress.coerceAtLeast(0.03f) },
                    modifier = Modifier.size(36.dp),
                    color = Color.White,
                    trackColor = Color.White.copy(alpha = 0.25f),
                    strokeWidth = 3.dp,
                )
            }
            m.status == SendStatus.SENDING || (m.status == null && img == null && a.previewAvailable) ->
                CircularProgressIndicator(
                    Modifier.align(Alignment.Center).size(36.dp),
                    color = Color.White,
                    strokeWidth = 3.dp,
                )
        }
        overlay()
    }
}

/** A non-image file or voice message: icon, name, size. Tapping opens it in the browser for now. */
@Composable
fun FileRow(a: FileAttachment, textColor: Color, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    Row(
        modifier
            .clip(RoundedCornerShape(10.dp))
            .clickable {
                if (a.link.isNotEmpty()) {
                    runCatching {
                        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(a.link)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                    }
                }
            }
            .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(40.dp).clip(CircleShape).background(textColor.copy(alpha = 0.12f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(if (a.voiceMessage) TalkIcons.Mic else TalkIcons.File, null, tint = textColor, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f, fill = false)) {
            Text(
                if (a.voiceMessage) "Sprachnachricht" else a.name,
                color = textColor,
                fontWeight = FontWeight.Medium,
                fontSize = 15.sp,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(fileSize(a.size), color = textColor.copy(alpha = 0.75f), fontSize = 12.sp)
        }
    }
}
