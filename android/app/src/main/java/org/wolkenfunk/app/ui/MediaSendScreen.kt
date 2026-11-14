package org.wolkenfunk.app.ui

import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.wolkenfunk.app.media.decodeSampled

/** Full-screen preview of a photo before sending, with a caption field (Signal's media send screen). */
@Composable
fun MediaSendScreen(uri: Uri, roomName: String, onClose: () -> Unit, onSend: (caption: String) -> Unit) {
    val context = LocalContext.current
    val colors = LocalTalkColors.current
    var caption by rememberSaveable { mutableStateOf("") }
    val image by produceState<ImageBitmap?>(null, uri) {
        value = withContext(Dispatchers.IO) { runCatching { decodeSampled(context, uri, 2048)?.asImageBitmap() }.getOrNull() }
    }
    BackHandler(onBack = onClose)

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        val img = image
        if (img != null) {
            Image(img, null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
        } else {
            CircularProgressIndicator(Modifier.align(Alignment.Center), color = Color.White)
        }
        IconButton(onClick = onClose, modifier = Modifier.statusBarsPadding().padding(8.dp)) {
            Icon(TalkIcons.Close, "Schließen", tint = Color.White)
        }
        Column(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .background(Color.Black.copy(alpha = 0.45f))
                .navigationBarsPadding()
                .imePadding()
                .padding(horizontal = 12.dp, vertical = 10.dp),
        ) {
            Text(
                "An: $roomName",
                color = Color.White,
                fontSize = 13.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier
                    .padding(bottom = 10.dp)
                    .clip(RoundedCornerShape(12.dp))
                    .background(Color.White.copy(alpha = 0.18f))
                    .padding(horizontal = 10.dp, vertical = 4.dp),
            )
            Row(verticalAlignment = Alignment.Bottom) {
                BasicTextField(
                    value = caption,
                    onValueChange = { caption = it },
                    textStyle = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, color = Color.White),
                    cursorBrush = SolidColor(Color.White),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    maxLines = 4,
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 44.dp)
                        .clip(RoundedCornerShape(20.dp))
                        .background(Color.White.copy(alpha = 0.18f))
                        .padding(horizontal = 16.dp, vertical = 11.dp),
                    decorationBox = { inner ->
                        Box {
                            if (caption.isEmpty()) Text("Nachricht hinzufügen", color = Color.White.copy(alpha = 0.7f), fontSize = 16.sp)
                            inner()
                        }
                    },
                )
                Spacer(Modifier.width(12.dp))
                Box(
                    Modifier
                        .padding(bottom = 2.dp)
                        .size(40.dp)
                        .clip(CircleShape)
                        .background(colors.accent)
                        .clickable(enabled = img != null) { onSend(caption) },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(TalkIcons.Send, "Senden", tint = Color.White, modifier = Modifier.size(22.dp))
                }
            }
        }
    }
}
