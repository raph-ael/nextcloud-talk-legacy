package org.sirplus.talk.ui

import android.Manifest
import android.content.ContentResolver
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import android.util.LruCache
import android.util.Size
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.sirplus.talk.media.decodeSampled

private const val RECENT_LIMIT = 60

/**
 * Signal's attachment keyboard: the latest photos on top (tap → send screen), the
 * attachment types below. Only "Galerie" works yet; the other types are placeholders.
 */
@Composable
fun AttachPanel(onImage: (Uri) -> Unit, onGallery: () -> Unit) {
    val context = LocalContext.current
    val soon = { what: String -> Toast.makeText(context, "$what kommt bald", Toast.LENGTH_SHORT).show() }
    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
        RecentPhotos(onImage, Modifier.weight(1f).fillMaxWidth())
        Row(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(start = 15.dp, end = 15.dp, top = 12.dp, bottom = 12.dp),
        ) {
            AttachButton(TalkIcons.Gallery, "Galerie", onGallery)
            AttachButton(TalkIcons.File, "Datei") { soon("Dateien senden") }
            AttachButton(TalkIcons.Poll, "Umfrage") { soon("Umfragen") }
            AttachButton(TalkIcons.Contact, "Kontakt") { soon("Kontakte senden") }
            AttachButton(TalkIcons.Location, "Standort") { soon("Standort senden") }
        }
    }
}

@Composable
private fun AttachButton(icon: ImageVector, label: String, onClick: () -> Unit) {
    val colors = LocalTalkColors.current
    Column(
        Modifier
            .width(88.dp)
            .clip(RoundedCornerShape(12.dp))
            .clickable(onClick = onClick)
            .padding(vertical = 4.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier
                .size(56.dp)
                .clip(RoundedCornerShape(18.dp))
                .background(colors.accent.copy(alpha = 0.12f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, null, tint = MaterialTheme.colorScheme.onSurface, modifier = Modifier.size(26.dp))
        }
        Spacer(Modifier.height(8.dp))
        Text(label, fontSize = 15.sp, color = MaterialTheme.colorScheme.onSurface, maxLines = 1)
    }
}

private fun mediaPermissions(): Array<String> = when {
    Build.VERSION.SDK_INT >= 34 ->
        arrayOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED)
    Build.VERSION.SDK_INT >= 33 -> arrayOf(Manifest.permission.READ_MEDIA_IMAGES)
    else -> arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
}

/** Full or (Android 14+) partial photo access. */
private fun hasMediaAccess(context: Context): Boolean =
    mediaPermissions().any { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }

@Composable
private fun RecentPhotos(onImage: (Uri) -> Unit, modifier: Modifier) {
    val context = LocalContext.current
    var granted by remember { mutableStateOf(hasMediaAccess(context)) }
    var reload by remember { mutableIntStateOf(0) }
    val request = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        granted = hasMediaAccess(context)
        reload++
    }
    // Access may change in the system settings while the app is in the background.
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        granted = hasMediaAccess(context)
        reload++
    }

    if (!granted) {
        Column(
            modifier.padding(24.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                "Erlaube den Zugriff auf deine Fotos, um die neuesten hier direkt zu senden.",
                textAlign = TextAlign.Center,
                color = LocalTalkColors.current.muted,
                fontSize = 15.sp,
            )
            TextButton(onClick = { request.launch(mediaPermissions()) }) { Text("Zugriff erlauben") }
        }
        return
    }

    val photos by produceState(emptyList<Uri>(), reload) {
        value = withContext(Dispatchers.IO) { runCatching { recentImages(context.contentResolver) }.getOrDefault(emptyList()) }
    }
    val cache = remember { LruCache<Uri, ImageBitmap>(RECENT_LIMIT) }
    BoxWithConstraints(modifier.padding(top = 12.dp)) {
        val tilePx = with(LocalDensity.current) { maxHeight.roundToPx() }
        LazyRow(
            Modifier.fillMaxHeight(),
            contentPadding = PaddingValues(horizontal = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            items(photos, key = { it.toString() }) { uri ->
                val thumb by produceState(cache.get(uri), uri) {
                    if (value == null) {
                        value = withContext(Dispatchers.IO) { thumbnail(context, uri, tilePx) }?.also { cache.put(uri, it) }
                    }
                }
                Box(
                    Modifier
                        .fillMaxHeight()
                        .aspectRatio(1f)
                        .clip(RoundedCornerShape(18.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .clickable { onImage(uri) },
                ) {
                    thumb?.let {
                        Image(it, null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                    }
                }
            }
        }
        if (photos.isEmpty()) {
            Text(
                "Keine Fotos gefunden",
                color = LocalTalkColors.current.muted,
                modifier = Modifier.align(Alignment.Center),
            )
        }
    }
}

private fun recentImages(resolver: ContentResolver): List<Uri> {
    val collection = MediaStore.Images.Media.EXTERNAL_CONTENT_URI
    val args = Bundle().apply {
        putString(ContentResolver.QUERY_ARG_SQL_SORT_ORDER, "${MediaStore.Images.Media.DATE_ADDED} DESC")
        putInt(ContentResolver.QUERY_ARG_LIMIT, RECENT_LIMIT) // honoured from Android 11 on
    }
    val out = ArrayList<Uri>()
    resolver.query(collection, arrayOf(MediaStore.Images.Media._ID), args, null)?.use { c ->
        val idCol = c.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
        while (out.size < RECENT_LIMIT && c.moveToNext()) out += ContentUris.withAppendedId(collection, c.getLong(idCol))
    }
    return out
}

private fun thumbnail(context: Context, uri: Uri, px: Int): ImageBitmap? = runCatching {
    if (Build.VERSION.SDK_INT >= 29) {
        context.contentResolver.loadThumbnail(uri, Size(px, px), null).asImageBitmap()
    } else {
        decodeSampled(context, uri, px)?.asImageBitmap()
    }
}.getOrNull()
