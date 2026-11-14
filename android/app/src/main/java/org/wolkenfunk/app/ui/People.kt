package org.wolkenfunk.app.ui

import android.graphics.Bitmap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.media.decodeSampled
import uniffi.talk_ffi.UserView
import java.io.ByteArrayOutputStream

/** User avatar from the server (Talk users only), initials until loaded or for guests. */
@Composable
fun UserAvatar(sessions: SessionManager, name: String, userId: String, size: Dp) {
    val dark = isSystemInDarkTheme()
    val image by produceState(if (userId.isEmpty()) null else sessions.cachedUserAvatar(userId, dark), userId, dark) {
        if (value == null && userId.isNotEmpty()) value = sessions.userAvatar(userId, dark)
    }
    val img = image
    if (img != null) {
        Image(img, null, contentScale = ContentScale.Crop, modifier = Modifier.size(size).clip(CircleShape))
    } else {
        Avatar(name, size)
    }
}

/** A person in a list: avatar 40, name, optional muted subline and something on the right. */
@Composable
fun PersonRow(
    sessions: SessionManager,
    name: String,
    userId: String,
    subline: String = "",
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
    trailing: @Composable () -> Unit = {},
) {
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = 64.dp)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 24.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        UserAvatar(sessions, name, userId, 40.dp)
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f)) {
            Text(name, style = ListType.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (subline.isNotEmpty()) {
                Text(subline, style = ListType.body, color = LocalTalkColors.current.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        trailing()
    }
}

/** A row whose icon sits in a filled accent circle („Neue Gruppe“, „Mitglieder hinzufügen“). */
@Composable
fun AccentActionRow(icon: ImageVector, label: String, onClick: () -> Unit) {
    val accent = LocalTalkColors.current.accent
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp).clickable(onClick = onClick).padding(horizontal = 24.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(40.dp).clip(CircleShape).background(accent), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = Color.White, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.width(16.dp))
        Text(label, style = ListType.title)
    }
}

/** Round check: an empty muted ring, or filled accent with a white tick. */
@Composable
fun RoundCheck(selected: Boolean) {
    val colors = LocalTalkColors.current
    if (selected) {
        Box(Modifier.size(24.dp).clip(CircleShape).background(colors.accent), contentAlignment = Alignment.Center) {
            Icon(Icons.Filled.Check, null, tint = Color.White, modifier = Modifier.size(16.dp))
        }
    } else {
        Box(Modifier.size(24.dp).border(2.dp, colors.muted, CircleShape))
    }
}

/** Chip of a selected person: avatar 24, first name, ×. */
@Composable
fun PersonChip(sessions: SessionManager, user: UserView, onRemove: () -> Unit) {
    val colors = LocalTalkColors.current
    Row(
        Modifier
            .clip(RoundedCornerShape(16.dp))
            .background(colors.chip)
            .clickable(onClick = onRemove)
            .semantics { contentDescription = "${user.name} entfernen" }
            .padding(start = 4.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        UserAvatar(sessions, user.name, user.id, 24.dp)
        Spacer(Modifier.width(6.dp))
        Text(user.name.substringBefore(' '), fontSize = 14.sp, maxLines = 1)
        Spacer(Modifier.width(4.dp))
        Icon(Icons.Filled.Close, null, tint = colors.muted, modifier = Modifier.size(16.dp))
    }
}

/** Placeholder rows while a list loads. */
@Composable
fun PlaceholderRows(count: Int = 3) {
    val shade = LocalTalkColors.current.chip
    Column {
        repeat(count) {
            Row(Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 24.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(40.dp).clip(CircleShape).background(shade))
                Spacer(Modifier.width(16.dp))
                Box(Modifier.width(160.dp).height(14.dp).clip(RoundedCornerShape(7.dp)).background(shade))
            }
        }
    }
}

/** A list that failed to load: the reason and „Erneut versuchen“. */
@Composable
fun RetryRow(text: String, onRetry: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(text, color = LocalTalkColors.current.muted)
        TextButton(onClick = onRetry) { Text("Erneut versuchen") }
    }
}

@Composable
fun SectionTitle(title: String, modifier: Modifier = Modifier) {
    Text(
        title,
        style = ListType.section,
        modifier = modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 8.dp),
    )
}

/** Search field in a rounded chip, like the room list's search. */
@Composable
fun NameSearchField(query: String, onChange: (String) -> Unit, modifier: Modifier = Modifier) {
    TextField(
        value = query,
        onValueChange = onChange,
        placeholder = { Text("Name suchen") },
        singleLine = true,
        leadingIcon = { Icon(Icons.Filled.Search, null) },
        trailingIcon = {
            if (query.isNotEmpty()) {
                androidx.compose.material3.IconButton(onClick = { onChange("") }) { Icon(Icons.Filled.Close, "Suche leeren") }
            }
        },
        shape = RoundedCornerShape(24.dp),
        colors = TextFieldDefaults.colors(
            focusedContainerColor = LocalTalkColors.current.chip,
            unfocusedContainerColor = LocalTalkColors.current.chip,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
    )
}

/** Result of [rememberUserSearch]: `users == null` while loading, [failed] after an error. */
class UserSearchState(val users: List<UserView>?, val failed: Boolean, val retry: () -> Unit)

/** `searchUsers` with 250 ms debounce (docs/groups.md „Neue Nachricht“). */
@Composable
fun rememberUserSearch(sessions: SessionManager, query: String, excludeToken: String? = null): UserSearchState {
    var attempt by remember { mutableIntStateOf(0) }
    var users by remember { mutableStateOf<List<UserView>?>(null) }
    var failed by remember { mutableStateOf(false) }
    LaunchedEffect(query, excludeToken, attempt) {
        users = null
        failed = false
        if (query.isNotEmpty()) delay(250)
        val found = sessions.searchUsers(query.trim(), excludeToken)
        failed = found == null
        users = found.orEmpty()
    }
    return UserSearchState(users, failed) { attempt++ }
}

/** A picked group photo: preview plus JPEG bytes for the core (which crops and scales). */
class PickedPhoto(val preview: ImageBitmap, val bytes: ByteArray)

/**
 * Photo picker for group pictures (images only). The image is decoded with its EXIF
 * rotation and re-encoded as JPEG, so formats the core cannot read (HEIC) work too.
 */
@Composable
fun rememberPhotoPicker(onPicked: (PickedPhoto) -> Unit): () -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val picked = withContext(Dispatchers.IO) {
                runCatching {
                    val bmp = decodeSampled(context, uri, 1024) ?: return@runCatching null
                    val out = ByteArrayOutputStream()
                    bmp.compress(Bitmap.CompressFormat.JPEG, 90, out)
                    PickedPhoto(bmp.asImageBitmap(), out.toByteArray())
                }.getOrNull()
            }
            if (picked != null) onPicked(picked)
            else android.widget.Toast.makeText(context, "Bild konnte nicht verwendet werden", android.widget.Toast.LENGTH_SHORT).show()
        }
    }
    return { launcher.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }
}

/** Large round group picture with a camera badge; shows [photo] when set. */
@Composable
fun GroupPhotoButton(photo: ImageBitmap?, fallback: @Composable () -> Unit, onClick: () -> Unit) {
    val colors = LocalTalkColors.current
    Box(Modifier.size(88.dp)) {
        Box(
            Modifier
                .size(80.dp)
                .clip(CircleShape)
                .clickable(onClick = onClick)
                .semantics { contentDescription = "Gruppenbild wählen" },
            contentAlignment = Alignment.Center,
        ) {
            if (photo != null) {
                Image(photo, null, contentScale = ContentScale.Crop, modifier = Modifier.size(80.dp).clip(CircleShape))
            } else {
                fallback()
            }
        }
        Box(
            Modifier
                .align(Alignment.BottomEnd)
                .size(32.dp)
                .clip(CircleShape)
                .background(MaterialTheme.colorScheme.surface)
                .padding(3.dp)
                .clip(CircleShape)
                .background(colors.chip),
            contentAlignment = Alignment.Center,
        ) {
            Icon(TalkIcons.Camera, null, modifier = Modifier.size(18.dp))
        }
    }
}

/** Empty group picture: the group icon on a soft circle. */
@Composable
fun EmptyGroupPhoto() {
    Box(Modifier.size(80.dp).clip(CircleShape).background(LocalTalkColors.current.chip), contentAlignment = Alignment.Center) {
        Icon(TalkIcons.Group, null, tint = LocalTalkColors.current.muted, modifier = Modifier.size(40.dp))
    }
}

