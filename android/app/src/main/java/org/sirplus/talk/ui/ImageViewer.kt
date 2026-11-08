package org.sirplus.talk.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import org.sirplus.talk.SessionManager

/** Full-screen image with pinch and double-tap zoom. Shows the chat preview until the large version is loaded. */
@Composable
fun ImageViewer(sessions: SessionManager, fileId: String, initial: ImageBitmap?, onClose: () -> Unit) {
    val image by produceState(initial, fileId) {
        if (fileId.isNotEmpty()) sessions.fullImage(fileId)?.let { value = it }
    }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    BackHandler(onBack = onClose)

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            .pointerInput(Unit) {
                detectTapGestures(onDoubleTap = { tap ->
                    if (scale > 1f) {
                        scale = 1f
                        offset = Offset.Zero
                    } else {
                        scale = 2.5f
                        offset = (Offset(size.width / 2f, size.height / 2f) - tap) * 1.5f
                    }
                })
            }
            .pointerInput(Unit) {
                detectTransformGestures { _, pan, zoom, _ ->
                    scale = (scale * zoom).coerceIn(1f, 6f)
                    offset = if (scale == 1f) Offset.Zero else offset + pan
                }
            },
    ) {
        val img = image
        if (img != null) {
            Image(
                img,
                null,
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        scaleX = scale
                        scaleY = scale
                        translationX = offset.x
                        translationY = offset.y
                    },
            )
        } else {
            CircularProgressIndicator(Modifier.align(Alignment.Center), color = Color.White)
        }
        IconButton(onClick = onClose, modifier = Modifier.statusBarsPadding().padding(8.dp)) {
            Icon(TalkIcons.Close, "Schließen", tint = Color.White)
        }
    }
}
