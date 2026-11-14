package org.wolkenfunk.app.ui

import android.Manifest
import android.app.Activity
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import android.content.res.Configuration
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import uniffi.talk_ffi.CallScreen as SharedScreen
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
import androidx.compose.foundation.layout.BoxWithConstraints
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
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.ColorMatrix
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.call.AudioOutput
import org.wolkenfunk.app.call.AudioRoute
import org.wolkenfunk.app.call.AudioRouting
import org.wolkenfunk.app.call.CallController.Companion.isRunning
import org.wolkenfunk.app.call.CallController.Companion.isStarting
import org.wolkenfunk.app.call.CallNotifications
import org.wolkenfunk.app.call.LocalVideo
import org.wolkenfunk.app.call.VideoFeed
import org.wolkenfunk.app.call.VideoTextureView
import uniffi.talk_ffi.CallRecording
import uniffi.talk_ffi.CallParticipant
import uniffi.talk_ffi.CallState
import uniffi.talk_ffi.CallView
import uniffi.talk_ffi.EndReason

private val Decline = Color(0xFFE5483F)
private val Accept = Color(0xFF2FB25B)
private val Speaking = Color(0xFF6FD08C)
private val RecordRed = Color(0xFFFF4D4D)
private val WashFilter = ColorFilter.colorMatrix(
    ColorMatrix().apply {
        setToSaturation(1.8f)
        timesAssign(ColorMatrix(floatArrayOf(0.6f, 0f, 0f, 0f, 0f, 0f, 0.6f, 0f, 0f, 0f, 0f, 0f, 0.6f, 0f, 0f, 0f, 0f, 0f, 1f, 0f)))
    },
)

/**
 * The call screen, Signal's layout: blurred avatar behind a 40 % dimming; ringing with
 * two round buttons (decline left, accept right); otherwise avatar, name, status and the
 * control bar (output · camera · more · mute · hang up). Groups show participant tiles.
 * Video (docs/calls.md "Video"): in 1:1 the other side's video fills the screen, in groups
 * it replaces the avatar in the tile; our own preview sits bottom right.
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
    onCamera: (Boolean) -> Unit,
    onSpeaker: () -> Unit,
    onOutput: (AudioOutput) -> Unit,
    onMinimize: () -> Unit,
) {
    val feeds by sessions.callVideo.remote.collectAsState()
    val state by sessions.state.collectAsStateWithLifecycle()
    val room = state.rooms.find { it.token == call.token }
    val dark = isSystemInDarkTheme()
    val avatar by produceState(room?.let { sessions.cachedAvatar(it.token, it.avatarVersion, dark) }, call.token, room?.avatarVersion) {
        if (room != null) value = sessions.avatar(room.token, room.avatarVersion, dark)
    }
    val title = if (call.isGroup) call.roomName else call.callerName.ifBlank { call.roomName }

    // 1:1: the other side's video on the stage while it is on and frames arrive.
    val stageFeed = if (!call.isGroup && call.state != CallState.ENDED && call.state != CallState.INCOMING) {
        call.participants.firstOrNull { it.video }?.peerId?.let { feeds[it] }
    } else {
        null
    }
    val stage = stageFeed != null && feedActive(stageFeed)

    Box(Modifier.fillMaxSize().background(Color(0xFF1B1B1D))) {
        when {
            presenting(call) -> Unit // the presentation draws its own black stage
            stage -> Stage(stageFeed!!)
            else -> CallBackground(avatar, colorFor(title))
        }
        if (call.state == CallState.INCOMING) {
            Incoming(call, title, room?.token, room?.avatarVersion.orEmpty(), sessions, onAccept, onDecline)
        } else {
            InCall(call, title, room?.token, room?.avatarVersion.orEmpty(), routing, sessions, feeds, stage, onHangUp, onMute, onCamera, onSpeaker, onOutput, onMinimize)
        }
    }
}

/**
 * Someone shares their screen (docs/calls.md "Bildschirmfreigabe"): the presentation view
 * replaces the normal one, and the call screen may rotate (CallActivity).
 */
fun presenting(call: CallView): Boolean =
    call.screens.isNotEmpty() && call.state != CallState.ENDED && call.state != CallState.INCOMING

// --- video ----------------------------------------------------------------------

/** Frames arrived within the last 2 s. */
@Composable
private fun feedActive(feed: VideoFeed?): Boolean {
    val flow = remember(feed) { feed?.active ?: MutableStateFlow(false) }
    return flow.collectAsState().value
}

/** A video feed in a TextureView (fills the view, cropping what does not fit). */
@Composable
private fun VideoView(feed: VideoFeed, modifier: Modifier = Modifier, mirror: Boolean = false) {
    AndroidView(
        factory = { VideoTextureView(it) },
        update = {
            it.bind(feed)
            it.setMirror(mirror)
        },
        onRelease = { it.release() },
        modifier = modifier,
    )
}

/**
 * 1:1 video on the whole screen: cropped when its orientation matches the screen,
 * otherwise whole (landscape video on a portrait phone); scrims keep the text readable.
 */
@Composable
private fun Stage(feed: VideoFeed) {
    val aspect by feed.aspect.collectAsState()
    BoxWithConstraints(Modifier.fillMaxSize().background(Color.Black)) {
        val screenLandscape = maxWidth > maxHeight
        if (aspect > 0f && (aspect > 1f) != screenLandscape) {
            VideoView(feed, Modifier.align(Alignment.Center).fillMaxWidth().aspectRatio(aspect))
        } else {
            VideoView(feed, Modifier.fillMaxSize())
        }
        Box(
            Modifier.fillMaxWidth().height(180.dp).align(Alignment.TopCenter)
                .background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.55f), Color.Transparent))),
        )
        Box(
            Modifier.fillMaxWidth().height(200.dp).align(Alignment.BottomCenter)
                .background(Brush.verticalGradient(listOf(Color.Transparent, Color.Black.copy(alpha = 0.6f)))),
        )
    }
}

/** Our camera, mirrored for the front camera, with "Kamera wechseln". */
@Composable
private fun LocalPreview(local: LocalVideo, onSwitch: () -> Unit, modifier: Modifier = Modifier, width: Dp = 96.dp, height: Dp = 128.dp) {
    Box(
        modifier
            .size(width = width, height = height)
            .clip(RoundedCornerShape(12.dp))
            .background(Color.Black)
            .border(BorderStroke(1.dp, Color.White.copy(alpha = 0.25f)), RoundedCornerShape(12.dp)),
    ) {
        VideoView(local.feed, Modifier.fillMaxSize(), mirror = local.front)
        Box(
            Modifier.align(Alignment.BottomEnd).padding(if (height < 100.dp) 4.dp else 6.dp).size(if (height < 100.dp) 28.dp else 32.dp).clip(CircleShape)
                .background(Color.Black.copy(alpha = 0.45f)).clickable(onClick = onSwitch),
            contentAlignment = Alignment.Center,
        ) {
            Icon(TalkIcons.CameraSwitch, "Kamera wechseln", tint = Color.White, modifier = Modifier.size(if (height < 100.dp) 16.dp else 18.dp))
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
    feeds: Map<String, VideoFeed>,
    /** The other side's video fills the screen (1:1): no big avatar. */
    stage: Boolean,
    onHangUp: () -> Unit,
    onMute: () -> Unit,
    onCamera: (Boolean) -> Unit,
    onSpeaker: () -> Unit,
    onOutput: (AudioOutput) -> Unit,
    onMinimize: () -> Unit,
) {
    val local by sessions.callVideo.local.collectAsState()
    val openCamera = rememberCameraOpener { onCamera(true) }
    var picker by remember { mutableStateOf(false) }
    var more by remember { mutableStateOf(false) }
    var confirmStop by remember { mutableStateOf(false) }
    val tiles = call.isGroup && call.participants.isNotEmpty() && call.state != CallState.ENDED
    val bar: @Composable () -> Unit = {
        ControlBar(call, routing, onSpeaker = onSpeaker, onPicker = { picker = true }, onCamera = { on -> if (on) openCamera() else onCamera(false) }, onMore = { more = true }, onMute = onMute, onHangUp = onHangUp)
    }
    if (presenting(call)) {
        Presentation(call, title, sessions, feeds, local, onMinimize, bar)
    } else Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding()) {
            Box(Modifier.fillMaxWidth().height(56.dp), contentAlignment = Alignment.CenterStart) {
                IconButton(onClick = onMinimize) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück zu den Chats", tint = Color.White)
                }
                if (call.state != CallState.ENDED) RecordingIndicator(call, Modifier.align(Alignment.Center))
            }
            Spacer(Modifier.height(if (tiles || stage) 0.dp else 40.dp))
            Header(title, { CallStatus(call) }, token, version, sessions, showAvatar = !tiles && !stage)
            if (tiles) {
                Spacer(Modifier.height(16.dp))
                LazyVerticalGrid(
                    columns = GridCells.Fixed(if (call.participants.size <= 4) 2 else 3),
                    modifier = Modifier.weight(1f).padding(horizontal = 12.dp),
                    contentPadding = PaddingValues(bottom = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    items(call.participants, key = { it.sessionId }) { p ->
                        Tile(p, sessions, p.peerId?.takeIf { p.video }?.let { feeds[it] })
                    }
                }
            } else {
                Spacer(Modifier.weight(1f))
            }
            if (call.state != CallState.ENDED) {
                Box(Modifier.padding(top = 16.dp, bottom = 32.dp)) { bar() }
            } else {
                Spacer(Modifier.height(96.dp))
            }
        }
        val preview = local
        if (preview != null && call.cameraOn && call.state != CallState.ENDED) {
            LocalPreview(
                preview,
                onSwitch = { sessions.callVideo.switchCamera() },
                Modifier.align(Alignment.BottomEnd).navigationBarsPadding().padding(end = 16.dp, bottom = 112.dp),
            )
        }
    }
    if (picker) OutputSheet(routing, onDismiss = { picker = false }) { picker = false; onOutput(it) }
    // Rights can go away while the menu is open (e.g. no longer moderator).
    if (more && call.canRecord && call.state != CallState.ENDED) {
        MoreSheet(
            call.recording,
            onDismiss = { more = false },
            onRecord = { video -> more = false; sessions.startRecording(video) },
            onStop = { more = false; confirmStop = true },
        )
    }
    if (confirmStop && call.canRecord && call.state != CallState.ENDED) {
        AlertDialog(
            onDismissRequest = { confirmStop = false },
            title = { Text("Aufnahme beenden?") },
            text = { Text("Die Aufnahme wird gespeichert, du bekommst danach einen Hinweis.") },
            confirmButton = {
                TextButton(onClick = { confirmStop = false; sessions.stopRecording() }) { Text("Beenden") }
            },
            dismissButton = { TextButton(onClick = { confirmStop = false }) { Text("Abbrechen") } },
        )
    }
    ConsentDialog(call, sessions, onHangUp)
}

/** Output · camera · more · mute · hang up (docs/calls.md "Oberfläche"). */
@Composable
private fun ControlBar(
    call: CallView,
    routing: AudioRouting,
    onSpeaker: () -> Unit,
    onPicker: () -> Unit,
    onCamera: (Boolean) -> Unit,
    onMore: () -> Unit,
    onMute: () -> Unit,
    onHangUp: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth(),
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
        ) { if (routing.needsPicker) onPicker() else onSpeaker() }
        ControlButton(
            if (call.cameraOn) TalkIcons.Videocam else TalkIcons.VideocamOff,
            if (call.cameraOn) "Kamera ausschalten" else "Kamera einschalten",
            active = call.cameraOn,
        ) { onCamera(!call.cameraOn) }
        if (call.canRecord) ControlButton(TalkIcons.MoreHoriz, "Mehr", onClick = onMore)
        ControlButton(if (call.muted) TalkIcons.MicOff else TalkIcons.MicFilled, if (call.muted) "Stumm aus" else "Stumm", active = call.muted, onClick = onMute)
        ControlButton(TalkIcons.CallEnd, "Auflegen", background = Decline, onClick = onHangUp)
    }
}

// --- screen sharing (receive only) -------------------------------------------------

/**
 * Presentation view: the shared screen fitted on the stage, the participants as a row
 * below it (portrait). In landscape the screen takes everything and the controls fade
 * out after 3 s; a tap brings them back. Several screens: the newest one is shown, the
 * labels on the stage switch between them.
 */
@Composable
private fun Presentation(
    call: CallView,
    title: String,
    sessions: SessionManager,
    feeds: Map<String, VideoFeed>,
    local: LocalVideo?,
    onMinimize: () -> Unit,
    bar: @Composable () -> Unit,
) {
    val screens = call.screens
    val newest = screens.first().sessionId
    // A new share takes the stage; otherwise stay with the chosen one while it lasts.
    var chosen by remember(newest) { mutableStateOf(newest) }
    val screen = screens.firstOrNull { it.sessionId == chosen } ?: screens.first()
    val feed = feeds[screen.peerId]
    val landscape = LocalConfiguration.current.orientation == Configuration.ORIENTATION_LANDSCAPE
    // Landscape like a video player: the system bars only on a swipe from the edge.
    val view = LocalView.current
    DisposableEffect(landscape) {
        val window = (view.context as? Activity)?.window
        val insets = window?.let { WindowCompat.getInsetsController(it, view) }
        if (landscape && insets != null) {
            insets.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            insets.hide(WindowInsetsCompat.Type.systemBars())
        }
        onDispose { insets?.show(WindowInsetsCompat.Type.systemBars()) }
    }
    var controls by remember { mutableStateOf(true) }
    var touches by remember { mutableIntStateOf(0) }
    LaunchedEffect(landscape, controls, touches) {
        if (!landscape) controls = true
        else if (controls) {
            delay(CONTROLS_HIDE_MS)
            controls = false
        }
    }
    val onTap = { if (landscape) { controls = !controls; touches++ } }
    val labels: @Composable (Modifier) -> Unit = { m ->
        ScreenLabels(screens, screen.sessionId, m) { chosen = it; touches++ }
    }
    val top: @Composable () -> Unit = {
        Row(Modifier.fillMaxWidth().height(56.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onMinimize) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück zu den Chats", tint = Color.White)
            }
            Column(Modifier.weight(1f)) {
                Text(title, color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                CallStatusSmall(call)
            }
            RecordingIndicator(call, Modifier.padding(end = 12.dp))
        }
    }

    if (!landscape) {
        Column(Modifier.fillMaxSize().background(Color.Black).statusBarsPadding().navigationBarsPadding()) {
            top()
            Box(Modifier.weight(1f).fillMaxWidth()) {
                ScreenStage(screen.sessionId, screen.name, feed, Modifier.fillMaxSize(), onTap)
                labels(Modifier.align(Alignment.TopStart).padding(8.dp))
            }
            ParticipantRow(call, sessions, feeds, local, Modifier.padding(top = 12.dp))
            Box(Modifier.padding(top = 16.dp, bottom = 24.dp)) { bar() }
        }
    } else {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            ScreenStage(screen.sessionId, screen.name, feed, Modifier.fillMaxSize(), onTap)
            AnimatedVisibility(controls, Modifier.fillMaxSize(), enter = fadeIn(), exit = fadeOut()) {
                Box(Modifier.fillMaxSize()) {
                    Box(
                        Modifier.fillMaxWidth().align(Alignment.TopCenter)
                            .background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.6f), Color.Transparent))),
                    ) {
                        Column(Modifier.systemBarsPadding().padding(bottom = 16.dp)) {
                            top()
                            labels(Modifier.padding(horizontal = 12.dp))
                        }
                    }
                    Box(
                        Modifier.fillMaxWidth().align(Alignment.BottomCenter)
                            .background(Brush.verticalGradient(listOf(Color.Transparent, Color.Black.copy(alpha = 0.6f)))),
                    ) {
                        Box(Modifier.systemBarsPadding().padding(top = 24.dp, bottom = 16.dp)) { bar() }
                    }
                    if (local != null && call.cameraOn) {
                        LocalPreview(
                            local,
                            onSwitch = { sessions.callVideo.switchCamera() },
                            Modifier.align(Alignment.BottomEnd).systemBarsPadding().padding(end = 16.dp, bottom = 16.dp),
                            width = 72.dp, height = 96.dp,
                        )
                    }
                }
            }
        }
    }
}

/** "Bildschirm von …"; with several screens one chip each, the shown one highlighted. */
@Composable
private fun ScreenLabels(screens: List<SharedScreen>, shown: String, modifier: Modifier, onChoose: (String) -> Unit) {
    Row(modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        for (s in screens) {
            val selected = s.sessionId == shown
            val several = screens.size > 1
            Text(
                "Bildschirm von ${s.name.ifBlank { "Gast" }}",
                color = if (selected && several) Color.Black else Color.White,
                fontSize = 13.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier
                    .clip(RoundedCornerShape(50))
                    .background(if (selected && several) Color.White else Color.Black.copy(alpha = 0.55f))
                    .then(if (several && !selected) Modifier.clickable { onChoose(s.sessionId) } else Modifier)
                    .padding(horizontal = 10.dp, vertical = 5.dp),
            )
        }
    }
}

/**
 * The shared screen, always fitted (never cropped). Two fingers zoom, one finger moves
 * the zoomed screen, a double tap zooms in at that point or back out.
 */
@Composable
private fun ScreenStage(key: String, name: String, feed: VideoFeed?, modifier: Modifier, onTap: () -> Unit) {
    val aspect by (feed?.aspect ?: remember { MutableStateFlow(0f) }).collectAsState()
    val active = feedActive(feed)
    BoxWithConstraints(modifier.clipToBounds()) {
        val boxW = constraints.maxWidth.toFloat()
        val boxH = constraints.maxHeight.toFloat()
        // Fitted size in px: the whole screen, letterboxed.
        val (fitW, fitH) = when {
            aspect <= 0f || boxH <= 0f -> boxW to boxH
            aspect > boxW / boxH -> boxW to boxW / aspect
            else -> boxH * aspect to boxH
        }
        var scale by remember(key, boxW, boxH) { mutableFloatStateOf(1f) }
        var offset by remember(key, boxW, boxH) { mutableStateOf(Offset.Zero) }
        val fit by rememberUpdatedState(Size(fitW, fitH))
        fun clamp(o: Offset, s: Float): Offset {
            val mx = ((fit.width * s - boxW) / 2f).coerceAtLeast(0f)
            val my = ((fit.height * s - boxH) / 2f).coerceAtLeast(0f)
            return Offset(o.x.coerceIn(-mx, mx), o.y.coerceIn(-my, my))
        }
        val center = Offset(boxW / 2f, boxH / 2f)
        Box(
            Modifier
                .fillMaxSize()
                .pointerInput(key) {
                    detectTapGestures(
                        onTap = { onTap() },
                        onDoubleTap = { tap ->
                            if (scale > 1f) {
                                scale = 1f
                                offset = Offset.Zero
                            } else {
                                scale = DOUBLE_TAP_ZOOM
                                offset = clamp((tap - center) * (1f - DOUBLE_TAP_ZOOM), DOUBLE_TAP_ZOOM)
                            }
                        },
                    )
                }
                .pointerInput(key) {
                    detectTransformGestures { centroid, pan, zoom, _ ->
                        val next = (scale * zoom).coerceIn(1f, MAX_ZOOM)
                        val z = next / scale
                        // Keep the point under the fingers in place while zooming.
                        offset = clamp((centroid - center) * (1f - z) + offset * z + pan, next)
                        scale = next
                    }
                },
        ) {
            if (feed != null && aspect > 0f) {
                val density = LocalDensity.current
                VideoView(
                    feed,
                    Modifier
                        .align(Alignment.Center)
                        .size(with(density) { fitW.toDp() }, with(density) { fitH.toDp() })
                        .graphicsLayer {
                            scaleX = scale
                            scaleY = scale
                            translationX = offset.x
                            translationY = offset.y
                        },
                )
            }
            if (!active) {
                Text(
                    "Bildschirm von ${name.ifBlank { "Gast" }} wird geladen …",
                    color = Color.White.copy(alpha = 0.8f),
                    fontSize = 15.sp,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.align(Alignment.Center).padding(24.dp),
                )
            }
        }
    }
}

/** Below the shared screen (portrait): our camera, then everybody else as small tiles. */
@Composable
private fun ParticipantRow(call: CallView, sessions: SessionManager, feeds: Map<String, VideoFeed>, local: LocalVideo?, modifier: Modifier) {
    LazyRow(
        modifier.fillMaxWidth().height(ROW_TILE),
        contentPadding = PaddingValues(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (local != null && call.cameraOn) {
            item(key = "self") {
                LocalPreview(local, onSwitch = { sessions.callVideo.switchCamera() }, width = ROW_TILE, height = ROW_TILE)
            }
        }
        items(call.participants, key = { it.sessionId }) { p ->
            Tile(p, sessions, p.peerId?.takeIf { p.video }?.let { feeds[it] }, Modifier.size(ROW_TILE), compact = true)
        }
    }
}

@Composable
private fun CallStatusSmall(call: CallView) {
    val text = when (call.state) {
        CallState.CONNECTED -> callDuration(call.startedAt)
        CallState.RECONNECTING -> "Verbindung wird neu hergestellt …"
        else -> ""
    }
    if (text.isNotEmpty()) Text(text, color = Color.White.copy(alpha = 0.75f), fontSize = 13.sp, maxLines = 1)
}

private val ROW_TILE = 84.dp
private const val CONTROLS_HIDE_MS = 3_000L
private const val DOUBLE_TAP_ZOOM = 2.5f
private const val MAX_ZOOM = 6f

/** "Aufnahme läuft" with a gently pulsing red dot; also while starting / after a failure. */
@Composable
private fun RecordingIndicator(call: CallView, modifier: Modifier = Modifier) {
    val text = when {
        call.recording.isRunning() -> "Aufnahme läuft"
        call.recording.isStarting() -> "Aufnahme startet …"
        call.recording == CallRecording.FAILED && call.canRecord -> "Aufnahme fehlgeschlagen"
        else -> return
    }
    val running = call.recording.isRunning()
    val pulse by rememberInfiniteTransition(label = "rec").animateFloat(
        1f, 0.35f, infiniteRepeatable(tween(900), RepeatMode.Reverse), label = "recPulse",
    )
    Row(
        modifier
            .clip(RoundedCornerShape(50))
            .background(Color.Black.copy(alpha = 0.35f))
            .padding(horizontal = 12.dp, vertical = 6.dp)
            .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (call.recording != CallRecording.FAILED) {
            Box(
                Modifier.size(10.dp).alpha(if (running) pulse else 0.6f).clip(CircleShape)
                    .background(if (running) RecordRed else Color.White),
            )
            Spacer(Modifier.width(8.dp))
        }
        Text(text, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Medium)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MoreSheet(recording: CallRecording, onDismiss: () -> Unit, onRecord: (video: Boolean) -> Unit, onStop: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(bottom = 24.dp)) {
            if (recording.isRunning() || recording.isStarting()) {
                SheetRow(TalkIcons.RecordStop, "Aufnahme beenden", onStop)
            } else {
                SheetRow(TalkIcons.Record, "Audio aufnehmen") { onRecord(false) }
                HorizontalDivider(Modifier.padding(start = 68.dp))
                SheetRow(TalkIcons.Record, "Video aufnehmen") { onRecord(true) }
            }
        }
    }
}

@Composable
private fun SheetRow(icon: ImageVector, label: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 24.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null)
        Spacer(Modifier.width(20.dp))
        Text(label, fontSize = 16.sp)
    }
}

/** The server wants consent to being recorded before we join (the core waits). */
@Composable
private fun ConsentDialog(call: CallView, sessions: SessionManager, onHangUp: () -> Unit) {
    // Hide right after the answer; the core clears the flag shortly after.
    var answered by remember(call.id) { mutableStateOf(false) }
    if (!call.recordingConsentPending || answered || call.state == CallState.ENDED) return
    AlertDialog(
        // Back or tapping outside does not decide: one of the two buttons.
        onDismissRequest = {},
        title = { Text("Dieser Anruf kann aufgezeichnet werden") },
        text = { Text("Wenn du beitrittst, kann deine Stimme in diesem Anruf aufgezeichnet werden.") },
        confirmButton = {
            TextButton(onClick = { answered = true; sessions.consentToRecording() }) { Text("Zustimmen und beitreten") }
        },
        dismissButton = { TextButton(onClick = { answered = true; onHangUp() }) { Text("Abbrechen") } },
    )
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
private fun Tile(p: CallParticipant, sessions: SessionManager, feed: VideoFeed?, modifier: Modifier = Modifier.aspectRatio(1f), compact: Boolean = false) {
    val dark = isSystemInDarkTheme()
    val image by produceState(sessions.cachedUserAvatar(p.userId, dark), p.userId) {
        if (value == null && p.userId.isNotEmpty()) value = sessions.userAvatar(p.userId, dark)
    }
    val shape = RoundedCornerShape(if (compact) 12.dp else 16.dp)
    Box(
        modifier
            .clip(shape)
            .background(Color.White.copy(alpha = 0.1f))
            .then(if (p.speaking && !p.muted) Modifier.border(BorderStroke(3.dp, Speaking), shape) else Modifier)
            .alpha(if (p.connected) 1f else 0.5f),
    ) {
        if (feed != null && feedActive(feed)) {
            VideoView(feed, Modifier.fillMaxSize())
            Text(
                p.name, color = Color.White, fontSize = if (compact) 11.sp else 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.align(Alignment.BottomStart).padding(if (compact) 4.dp else 8.dp)
                    .clip(RoundedCornerShape(6.dp)).background(Color.Black.copy(alpha = 0.45f))
                    .padding(horizontal = 6.dp, vertical = 2.dp),
            )
        } else {
            Column(Modifier.align(Alignment.Center), horizontalAlignment = Alignment.CenterHorizontally) {
                TileAvatar(p.name, image, if (compact) 40.dp else 64.dp)
                Spacer(Modifier.height(if (compact) 4.dp else 8.dp))
                Text(p.name, color = Color.White, fontSize = if (compact) 11.sp else 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(horizontal = if (compact) 4.dp else 8.dp))
                if (!p.connected && !compact) Text("Verbinde …", color = Color.White.copy(alpha = 0.7f), fontSize = 12.sp)
            }
        }
        if (p.muted) {
            Icon(
                TalkIcons.MicOff, "Stumm", tint = Color.White,
                modifier = Modifier.align(Alignment.TopEnd).padding(if (compact) 4.dp else 8.dp).size(if (compact) 16.dp else 20.dp),
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
    val rec = call.state != CallState.ENDED && call.recording.isRunning()
    Box(
        Modifier.fillMaxWidth().background(Accept).clickable(onClick = onClick).statusBarsPadding()
            .padding(vertical = 8.dp, horizontal = 16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (rec) {
                Text("● REC", color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.Bold, modifier = Modifier
                    .clip(RoundedCornerShape(4.dp)).background(RecordRed).padding(horizontal = 6.dp, vertical = 1.dp)
                    .semantics { contentDescription = "Aufnahme läuft" })
                Spacer(Modifier.width(8.dp))
            }
            Text(text, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
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
