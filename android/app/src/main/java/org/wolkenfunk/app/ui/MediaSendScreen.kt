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
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.wolkenfunk.app.media.PickedFile
import org.wolkenfunk.app.media.decodeSampled
import org.wolkenfunk.app.media.describeFile

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

/**
 * „Datei senden“ (docs/attachments.md): the chosen files with name, size and ✕, a field for the
 * description and the send button. [send] copies and queues them and reports the ones it
 * could not read; those leave the list, the rest stays. Without a file the screen closes.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FileSendScreen(
    uris: List<Uri>,
    onClose: () -> Unit,
    send: (files: List<PickedFile>, caption: String, done: (unreadable: List<Uri>) -> Unit) -> Unit,
    onSent: () -> Unit,
) {
    val context = LocalContext.current
    val colors = LocalTalkColors.current
    var caption by rememberSaveable { mutableStateOf("") }
    var files by remember(uris) { mutableStateOf<List<PickedFile>?>(null) }
    var sending by remember { mutableStateOf(false) }
    LaunchedEffect(uris) {
        files = withContext(Dispatchers.IO) { uris.map { describeFile(context, it) } }
    }
    BackHandler(onBack = onClose)
    val list = files
    LaunchedEffect(list) { if (list != null && list.isEmpty()) onClose() }

    Column(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.surface)
            // Nothing below reacts while this screen is up.
            .pointerInput(Unit) { detectTapGestures {} },
    ) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = onClose) { Icon(TalkIcons.Close, "Schließen") } },
            title = {
                Text(
                    when (val n = list?.size ?: uris.size) {
                        1 -> "Datei senden"
                        else -> "$n Dateien senden"
                    },
                    fontSize = 18.sp,
                    modifier = Modifier.semantics { heading() },
                )
            },
        )
        LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
            items(list.orEmpty(), key = { it.uri.toString() }) { f ->
                Row(
                    Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(40.dp).clip(CircleShape).background(colors.chip), contentAlignment = Alignment.Center) {
                        Icon(TalkIcons.File, null, tint = MaterialTheme.colorScheme.onSurface, modifier = Modifier.size(22.dp))
                    }
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(
                            f.name,
                            color = MaterialTheme.colorScheme.onSurface,
                            fontSize = 16.sp,
                            maxLines = 1,
                            overflow = TextOverflow.MiddleEllipsis,
                        )
                        if (f.size >= 0) Text(fileSize(f.size), color = colors.muted, fontSize = 13.sp)
                    }
                    IconButton(onClick = { files = list.orEmpty() - f }, enabled = !sending) {
                        Icon(TalkIcons.Close, "Entfernen", tint = colors.muted, modifier = Modifier.size(20.dp))
                    }
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().navigationBarsPadding().imePadding().padding(horizontal = 12.dp, vertical = 10.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            BasicTextField(
                value = caption,
                onValueChange = { caption = it },
                textStyle = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, color = MaterialTheme.colorScheme.onSurface),
                cursorBrush = SolidColor(colors.accent),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                maxLines = 4,
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 44.dp)
                    .clip(RoundedCornerShape(20.dp))
                    .background(colors.chip)
                    .padding(horizontal = 16.dp, vertical = 11.dp),
                decorationBox = { inner ->
                    Box {
                        if (caption.isEmpty()) Text("Beschreibung hinzufügen", color = colors.muted, fontSize = 16.sp)
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
                    .clickable(enabled = !sending && !list.isNullOrEmpty(), onClickLabel = "Senden", role = Role.Button) {
                        val chosen = list.orEmpty()
                        sending = true
                        send(chosen, caption) { unreadable ->
                            sending = false
                            if (unreadable.isEmpty()) onSent() else files = files.orEmpty().filter { it.uri !in unreadable }
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                if (sending) {
                    // The files are being copied into the staging folder.
                    CircularProgressIndicator(Modifier.size(22.dp), color = Color.White, strokeWidth = 2.5.dp)
                } else {
                    Icon(TalkIcons.Send, "Senden", tint = Color.White, modifier = Modifier.size(22.dp))
                }
            }
        }
    }
}
