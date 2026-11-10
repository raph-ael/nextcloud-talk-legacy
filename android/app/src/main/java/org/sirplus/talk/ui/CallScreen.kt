package org.sirplus.talk.ui

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.ColorMatrix
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import org.sirplus.talk.SessionManager
import org.sirplus.talk.call.AudioOutput
import org.sirplus.talk.call.AudioRoute
import org.sirplus.talk.call.AudioRouting
import org.sirplus.talk.call.CallNotifications
import uniffi.talk_ffi.CallParticipant
import uniffi.talk_ffi.CallState
import uniffi.talk_ffi.CallView
import uniffi.talk_ffi.EndReason

private val Decline = Color(0xFFE5483F)
private val Accept = Color(0xFF2FB25B)
private val Speaking = Color(0xFF6FD08C)
private val WashFilter = ColorFilter.colorMatrix(
    ColorMatrix().apply {
        setToSaturation(1.8f)
        timesAssign(ColorMatrix(floatArrayOf(0.6f, 0f, 0f, 0f, 0f, 0f, 0.6f, 0f, 0f, 0f, 0f, 0f, 0.6f, 0f, 0f, 0f, 0f, 0f, 1f, 0f)))
    },
)

/**
 * The call screen, Signal's layout: blurred avatar behind a 40 % dimming; ringing with
 * two round buttons (decline left, accept right); otherwise avatar, name, status and the
 * control bar (output · [video later] · mute · hang up). Groups show participant tiles.
 */
@Composable
fun CallScreen(
    call: CallView,
    routing: AudioRouting,
    sessions: SessionManager,
    onAccept: () -> Unit,
    onDecline: () -> Unit,
    onHangUp: () -> Unit,
    onMute: () -> Unit,
    onSpeaker: () -> Unit,
    onOutput: (AudioOutput) -> Unit,
    onMinimize: () -> Unit,
) {
    val state by sessions.state.collectAsStateWithLifecycle()
    val room = state.rooms.find { it.token == call.token }
    val dark = isSystemInDarkTheme()
    val avatar by produceState(room?.let { sessions.cachedAvatar(it.token, it.avatarVersion, dark) }, call.token, room?.avatarVersion) {
        if (room != null) value = sessions.avatar(room.token, room.avatarVersion, dark)
    }
    val title = if (call.isGroup) call.roomName else call.callerName.ifBlank { call.roomName }

    Box(Modifier.fillMaxSize().background(Color(0xFF1B1B1D))) {
        CallBackground(avatar, colorFor(title))
        if (call.state == CallState.INCOMING) {
            Incoming(call, title, room?.token, room?.avatarVersion.orEmpty(), sessions, onAccept, onDecline)
        } else {
            InCall(call, title, room?.token, room?.avatarVersion.orEmpty(), routing, sessions, onHangUp, onMute, onSpeaker, onOutput, onMinimize)
        }
    }
}

@Composable
private fun CallBackground(avatar: ImageBitmap?, fallback: Color) {
    // A few pixels of the avatar, scaled up and blurred: only its colors remain (no
    // giant initials), like Signal's call background.
    val wash = remember(avatar) {
        avatar?.let {
            android.graphics.Bitmap.createScaledBitmap(it.asAndroidBitmap(), 4, 4, true).asImageBitmap()
        }
    }
    if (wash != null) {
        Image(
            bitmap = wash,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            filterQuality = FilterQuality.Low,
            // Pastel server avatars would turn grey under the dimming: richer and darker.
            colorFilter = WashFilter,
            modifier = Modifier.fillMaxSize().then(if (Build.VERSION.SDK_INT >= 31) Modifier.blur(48.dp) else Modifier),
        )
    } else {
        // No bitmap avatar (initials): its color, darkened like the wash.
        Box(Modifier.fillMaxSize().background(fallback.copy(red = fallback.red * 0.6f, green = fallback.green * 0.6f, blue = fallback.blue * 0.6f)))
    }
    Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.4f)))
}

@Composable
private fun Header(title: String, subtitle: @Composable () -> Unit, token: String?, version: String, sessions: SessionManager, showAvatar: Boolean) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (showAvatar) {
            Avatar(title, 80.dp, token, version, sessions)
            Spacer(Modifier.height(16.dp))
        }
        Text(
            title,
            color = Color.White,
            fontSize = 28.sp,
            fontWeight = FontWeight.Medium,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(6.dp))
        subtitle()
    }
}

// --- incoming -------------------------------------------------------------------

@Composable
private fun Incoming(
    call: CallView,
    title: String,
    token: String?,
    version: String,
    sessions: SessionManager,
    onAccept: () -> Unit,
    onDecline: () -> Unit,
) {
    Column(Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding()) {
        Spacer(Modifier.height(64.dp))
        Header(
            title,
            { StatusText(if (call.isGroup) "Gruppenanruf" else "Talk-Anruf") },
            token, version, sessions, showAvatar = true,
        )
        Spacer(Modifier.weight(1f))
        Row(
            Modifier.fillMaxWidth().padding(start = 64.dp, end = 64.dp, bottom = 64.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            RoundAction(TalkIcons.CallEnd, "Ablehnen", Decline, onDecline)
            RoundAction(TalkIcons.Call, "Annehmen", Accept, onAccept)
        }
    }
}

@Composable
private fun RoundAction(icon: ImageVector, label: String, color: Color, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            Modifier.size(78.dp).clip(CircleShape).background(color).clickable(onClick = onClick),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, label, tint = Color.White, modifier = Modifier.size(32.dp))
        }
        Spacer(Modifier.height(8.dp))
        Text(label, color = Color.White, fontSize = 14.sp)
    }
}

// --- dialing / connected ----------------------------------------------------------

@Composable
private fun InCall(
    call: CallView,
    title: String,
    token: String?,
    version: String,
    routing: AudioRouting,
    sessions: SessionManager,
    onHangUp: () -> Unit,
    onMute: () -> Unit,
    onSpeaker: () -> Unit,
    onOutput: (AudioOutput) -> Unit,
    onMinimize: () -> Unit,
) {
    var picker by remember { mutableStateOf(false) }
    val tiles = call.isGroup && call.participants.isNotEmpty() && call.state != CallState.ENDED
    Column(Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding()) {
        Row(Modifier.fillMaxWidth().height(56.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onMinimize) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück zu den Chats", tint = Color.White)
            }
        }
        Spacer(Modifier.height(if (tiles) 0.dp else 40.dp))
        Header(title, { CallStatus(call) }, token, version, sessions, showAvatar = !tiles)
        if (tiles) {
            Spacer(Modifier.height(16.dp))
            LazyVerticalGrid(
                columns = GridCells.Fixed(if (call.participants.size <= 4) 2 else 3),
                modifier = Modifier.weight(1f).padding(horizontal = 12.dp),
                contentPadding = PaddingValues(bottom = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(call.participants, key = { it.sessionId }) { p -> Tile(p, sessions) }
            }
        } else {
            Spacer(Modifier.weight(1f))
        }
        if (call.state != CallState.ENDED) {
            Row(
                Modifier.fillMaxWidth().padding(top = 16.dp, bottom = 32.dp),
                horizontalArrangement = Arrangement.spacedBy(20.dp, Alignment.CenterHorizontally),
            ) {
                val speakerOn = routing.current == AudioRoute.SPEAKER
                ControlButton(
                    when (routing.current) {
                        AudioRoute.BLUETOOTH -> TalkIcons.Bluetooth
                        AudioRoute.WIRED -> TalkIcons.Headset
                        else -> TalkIcons.Speaker
                    },
                    if (routing.needsPicker) "Audioausgabe" else "Lautsprecher",
                    active = speakerOn && !routing.needsPicker,
                ) { if (routing.needsPicker) picker = true else onSpeaker() }
                // Slot for the video button (video calls come later).
                ControlButton(if (call.muted) TalkIcons.MicOff else TalkIcons.MicFilled, if (call.muted) "Stumm aus" else "Stumm", active = call.muted, onClick = onMute)
                ControlButton(TalkIcons.CallEnd, "Auflegen", background = Decline, onClick = onHangUp)
            }
        } else {
            Spacer(Modifier.height(96.dp))
        }
    }
    if (picker) OutputSheet(routing, onDismiss = { picker = false }) { picker = false; onOutput(it) }
}

@Composable
private fun ControlButton(
    icon: ImageVector,
    label: String,
    active: Boolean = false,
    background: Color? = null,
    onClick: () -> Unit,
) {
    val bg = background ?: if (active) Color.White else Color.White.copy(alpha = 0.2f)
    val tint = if (background == null && active) Color.Black else Color.White
    Box(
        Modifier.size(48.dp).clip(CircleShape).background(bg).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, label, tint = tint, modifier = Modifier.size(28.dp))
    }
}

@Composable
private fun StatusText(text: String, modifier: Modifier = Modifier) {
    Text(text, color = Color.White.copy(alpha = 0.85f), fontSize = 16.sp, modifier = modifier, textAlign = TextAlign.Center)
}

/** Status line from docs/calls.md. */
@Composable
private fun CallStatus(call: CallView) {
    when (call.state) {
        CallState.DIALING -> StatusText("Anrufen …")
        CallState.RINGING -> StatusText(if (call.isGroup) "Warte auf andere …" else "Es klingelt …")
        CallState.INCOMING -> StatusText(if (call.isGroup) "Gruppenanruf" else "Talk-Anruf")
        CallState.CONNECTING -> StatusText("Verbindung wird hergestellt …")
        CallState.CONNECTED -> StatusText(callDuration(call.startedAt))
        CallState.RECONNECTING -> {
            val blink by rememberInfiniteTransition(label = "reconnect").animateFloat(
                1f, 0.3f, infiniteRepeatable(tween(700), RepeatMode.Reverse), label = "blink",
            )
            StatusText("Verbindung wird neu hergestellt …", Modifier.alpha(blink))
        }
        CallState.ENDED -> StatusText(endText(call.endReason))
    }
}

@Composable
fun callDuration(startedAt: Long?): String {
    val now by produceState(System.currentTimeMillis(), startedAt) {
        while (true) {
            value = System.currentTimeMillis()
            delay(1000 - value % 1000)
        }
    }
    val sec = ((now - (startedAt ?: now)) / 1000).coerceAtLeast(0)
    val h = sec / 3600
    val m = (sec % 3600) / 60
    val s = sec % 60
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
}

private fun endText(reason: EndReason?): String = when (reason) {
    null, EndReason.Hangup, EndReason.RemoteHangup -> "Anruf beendet"
    EndReason.NoAnswer -> "Keine Antwort"
    EndReason.Declined -> "Abgelehnt"
    EndReason.AnsweredElsewhere -> "Auf einem anderen Gerät angenommen"
    EndReason.Missed -> "Verpasster Anruf"
    is EndReason.Failed -> reason.message.ifBlank { "Anruf fehlgeschlagen" }
}

// --- group tiles ----------------------------------------------------------------

@Composable
private fun Tile(p: CallParticipant, sessions: SessionManager) {
    val dark = isSystemInDarkTheme()
    val image by produceState(sessions.cachedUserAvatar(p.userId, dark), p.userId) {
        if (value == null && p.userId.isNotEmpty()) value = sessions.userAvatar(p.userId, dark)
    }
    val shape = RoundedCornerShape(16.dp)
    Box(
        Modifier
            .aspectRatio(1f)
            .clip(shape)
            .background(Color.White.copy(alpha = 0.1f))
            .then(if (p.speaking && !p.muted) Modifier.border(BorderStroke(3.dp, Speaking), shape) else Modifier)
            .alpha(if (p.connected) 1f else 0.5f),
    ) {
        Column(Modifier.align(Alignment.Center), horizontalAlignment = Alignment.CenterHorizontally) {
            TileAvatar(p.name, image, 64.dp)
            Spacer(Modifier.height(8.dp))
            Text(p.name, color = Color.White, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(horizontal = 8.dp))
            if (!p.connected) Text("Verbinde …", color = Color.White.copy(alpha = 0.7f), fontSize = 12.sp)
        }
        if (p.muted) {
            Icon(
                TalkIcons.MicOff, "Stumm", tint = Color.White,
                modifier = Modifier.align(Alignment.TopEnd).padding(8.dp).size(20.dp),
            )
        }
    }
}

@Composable
private fun TileAvatar(name: String, image: ImageBitmap?, size: Dp) {
    if (image != null) {
        Image(image, null, contentScale = ContentScale.Crop, modifier = Modifier.size(size).clip(CircleShape))
    } else {
        Avatar(name, size)
    }
}

// --- output picker --------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun OutputSheet(routing: AudioRouting, onDismiss: () -> Unit, onPick: (AudioOutput) -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(bottom = 24.dp)) {
            for (o in routing.outputs) {
                Row(
                    Modifier.fillMaxWidth().clickable { onPick(o) }.padding(horizontal = 24.dp, vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        when (o.route) {
                            AudioRoute.BLUETOOTH -> TalkIcons.Bluetooth
                            AudioRoute.WIRED -> TalkIcons.Headset
                            AudioRoute.SPEAKER -> TalkIcons.Speaker
                            AudioRoute.EARPIECE -> TalkIcons.Call
                        },
                        null,
                    )
                    Spacer(Modifier.width(20.dp))
                    Text(o.name, fontSize = 16.sp, modifier = Modifier.weight(1f))
                    if (o.id == routing.currentId) Icon(Icons.Default.Check, "Ausgewählt")
                }
            }
        }
    }
}

// --- in-app bar -----------------------------------------------------------------

/** "Zurück zum Anruf · 03:12" above the app while a call runs. */
@Composable
fun CallBar(call: CallView, onClick: () -> Unit) {
    val text = when (call.state) {
        CallState.CONNECTED -> "Zurück zum Anruf · ${callDuration(call.startedAt)}"
        CallState.INCOMING -> "Eingehender Anruf · ${call.callerName.ifBlank { call.roomName }}"
        CallState.ENDED -> endText(call.endReason)
        CallState.RECONNECTING -> "Zurück zum Anruf · Verbindung wird neu hergestellt …"
        else -> "Zurück zum Anruf"
    }
    Box(
        Modifier.fillMaxWidth().background(Accept).clickable(onClick = onClick).statusBarsPadding()
            .padding(vertical = 8.dp, horizontal = 16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

// --- permissions ----------------------------------------------------------------

/**
 * Returns `withPermissions(block)`: asks for the microphone (required) and, once, for
 * Bluetooth, phone state and notifications (optional), then runs [block] if the
 * microphone is allowed. On Android 14+ it points once to the full-screen setting.
 */
@Composable
fun rememberCallPermissions(): (() -> Unit) -> Unit {
    val context = LocalContext.current
    var pending by remember { mutableStateOf<(() -> Unit)?>(null) }
    var askFullScreen by remember { mutableStateOf(false) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        val block = pending
        pending = null
        if (granted(context, Manifest.permission.RECORD_AUDIO)) block?.invoke()
        else Toast.makeText(context, "Für Anrufe braucht die App das Mikrofon", Toast.LENGTH_LONG).show()
    }
    if (askFullScreen) {
        AlertDialog(
            onDismissRequest = { askFullScreen = false },
            title = { Text("Anrufe im Vollbild") },
            text = { Text("Damit eingehende Anrufe auch bei gesperrtem Bildschirm erscheinen, erlaube der App Vollbild-Benachrichtigungen.") },
            confirmButton = {
                TextButton(onClick = {
                    askFullScreen = false
                    if (Build.VERSION.SDK_INT >= 34) {
                        runCatching {
                            context.startActivity(
                                Intent(Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, Uri.parse("package:${context.packageName}")),
                            )
                        }
                    }
                }) { Text("Einstellungen") }
            },
            dismissButton = { TextButton(onClick = { askFullScreen = false }) { Text("Später") } },
        )
    }
    return { block ->
        val prefs = context.getSharedPreferences("ui", Context.MODE_PRIVATE)
        if (!CallNotifications.canUseFullScreen(context) && !prefs.getBoolean("asked_full_screen", false)) {
            prefs.edit().putBoolean("asked_full_screen", true).apply()
            askFullScreen = true
        }
        val askedOptional = prefs.getBoolean("asked_call_permissions", false)
        val wanted = buildList {
            add(Manifest.permission.RECORD_AUDIO)
            if (!askedOptional) {
                if (Build.VERSION.SDK_INT >= 31) add(Manifest.permission.BLUETOOTH_CONNECT)
                add(Manifest.permission.READ_PHONE_STATE)
                if (Build.VERSION.SDK_INT >= 33) add(Manifest.permission.POST_NOTIFICATIONS)
            }
        }.filterNot { granted(context, it) }
        if (wanted.isEmpty()) {
            block()
        } else {
            prefs.edit().putBoolean("asked_call_permissions", true).apply()
            pending = block
            launcher.launch(wanted.toTypedArray())
        }
    }
}

private fun granted(context: Context, permission: String) =
    ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
