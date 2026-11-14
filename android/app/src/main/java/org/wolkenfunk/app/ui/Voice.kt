package org.wolkenfunk.app.ui

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.text.TextStyle
import androidx.compose.material3.ProvideTextStyle
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.wolkenfunk.app.RecordingUi
import org.wolkenfunk.app.SendStatus
import org.wolkenfunk.app.UiMessage
import org.wolkenfunk.app.VoiceManager
import org.wolkenfunk.app.media.PlayerUi
import uniffi.talk_ffi.AudioInfo
import uniffi.talk_ffi.VoiceDraft
import kotlin.math.roundToInt

/*
 * Voice message bubble. Layout and proportions follow Signal Android (measured on device):
 * content 212 dp wide; a 48 dp button cell (44 dp circle) next to a 32 dp waveform of 2 dp
 * bars with a 2 × 28 dp position line; below, a footer with duration, speed chip and the
 * message time. Own code and icons, only the measurements are Signal's.
 */
private val VoiceWidth = 212.dp
private val ButtonCell = 48.dp
private val BarWidth = 2.dp
private val BarMinGap = 1.2.dp
private val BarMaxHeight = 24.dp
private val ThumbHeight = 28.dp

/** 75 000 → "1:15", 3 723 000 → "1:02:03". */
fun clock(ms: Long): String {
    val total = ((ms + 500) / 1000).coerceAtLeast(0)
    val h = total / 3600
    val m = (total % 3600) / 60
    val s = total % 60
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
}

/** [n] bars 0..1 from the core's 100 levels (mean per bar). */
private fun bars(levels: ByteArray?, n: Int): FloatArray {
    if (levels == null || levels.isEmpty()) return FloatArray(n)
    return FloatArray(n) { i ->
        val a = i * levels.size / n
        val b = maxOf(a + 1, (i + 1) * levels.size / n)
        var sum = 0
        for (j in a until b) sum += levels[j].toInt() and 0xFF
        sum / (b - a) / 255f
    }
}

/** Colors of one voice message, derived from its bubble. */
private data class VoicePalette(
    val circle: Color,
    val icon: Color,
    val played: Color,
    val unplayed: Color,
    val chip: Color,
)

@Composable
private fun voicePalette(own: Boolean, textColor: Color, active: Boolean): VoicePalette {
    val dark = isDarkTheme()
    return if (own) {
        VoicePalette(
            circle = Color.White.copy(alpha = 0.16f),
            icon = Color.White,
            played = Color.White,
            unplayed = Color.White.copy(alpha = if (active) 0.4f else 0.2f),
            chip = Color.White.copy(alpha = 0.2f),
        )
    } else {
        VoicePalette(
            circle = if (dark) Color.White.copy(alpha = 0.12f) else Color.White,
            icon = textColor,
            played = textColor,
            unplayed = textColor.copy(alpha = if (dark) 0.3f else 0.22f),
            chip = textColor.copy(alpha = 0.08f),
        )
    }
}

/** Bars with the played part highlighted and a position line; tap or drag to seek. */
@Composable
private fun Waveform(
    levels: ByteArray?,
    progress: Float,
    played: Color,
    unplayed: Color,
    modifier: Modifier,
    onSeek: (Float) -> Unit,
) {
    val seek by rememberUpdatedState(onSeek)
    Canvas(
        modifier
            .height(ThumbHeight + 4.dp)
            .pointerInput(Unit) { detectTapGestures { seek(it.x / size.width) } }
            .pointerInput(Unit) {
                detectHorizontalDragGestures { change, _ -> seek(change.position.x / size.width) }
            },
    ) {
        val bar = BarWidth.toPx()
        val n = ((size.width + BarMinGap.toPx()) / (bar + BarMinGap.toPx())).toInt().coerceAtLeast(2)
        val step = (size.width - bar) / (n - 1)
        val values = bars(levels, n)
        val mid = size.height / 2
        val span = BarMaxHeight.toPx() - bar
        val thumbX = bar / 2 + progress.coerceIn(0f, 1f) * (size.width - bar)
        values.forEachIndexed { i, v ->
            val x = bar / 2 + i * step
            val half = v * span / 2
            drawLine(
                color = if (x <= thumbX) played else unplayed,
                start = Offset(x, mid - half),
                end = Offset(x, mid + half),
                strokeWidth = bar,
                cap = StrokeCap.Round,
            )
        }
        val t = ThumbHeight.toPx() / 2 - bar / 2
        drawLine(played, Offset(thumbX, mid - t), Offset(thumbX, mid + t), strokeWidth = bar, cap = StrokeCap.Round)
    }
}

/**
 * Round play/pause button in a [ButtonCell] (circle inset by 2 dp). A ring on the circle's edge
 * shows upload progress or loading; [download] marks a file that is not fetched yet.
 */
@Composable
private fun PlayButton(
    player: PlayerUi?,
    circle: Color,
    tint: Color,
    upload: Float?,
    download: Boolean = false,
    cell: Dp = ButtonCell,
    onClick: () -> Unit,
) {
    Box(Modifier.size(cell).padding(2.dp), contentAlignment = Alignment.Center) {
        Box(
            Modifier.fillMaxSize().clip(CircleShape).background(circle).clickable(onClick = onClick),
            contentAlignment = Alignment.Center,
        ) {
            val playing = player?.playing == true
            Icon(
                when {
                    playing -> TalkIcons.Pause
                    download && player == null -> TalkIcons.ArrowDown
                    else -> TalkIcons.Play
                },
                if (playing) "Pause" else "Abspielen",
                tint = tint,
                modifier = Modifier.size(22.dp),
            )
        }
        when {
            upload != null -> CircularProgressIndicator(
                progress = { upload.coerceAtLeast(0.03f) },
                modifier = Modifier.fillMaxSize(),
                color = tint,
                trackColor = Color.Transparent,
                strokeWidth = 2.dp,
            )
            player?.loading == true -> CircularProgressIndicator(Modifier.fillMaxSize(), color = tint, strokeWidth = 2.dp)
        }
    }
}

/** Speed chip: 1× → 1,5× → 2× → 0,5×. */
@Composable
private fun SpeedChip(speed: Float, color: Color, background: Color, onClick: () -> Unit) {
    val label = (if (speed % 1f == 0f) speed.toInt().toString() else speed.toString().replace('.', ',')) + "×"
    Box(
        Modifier
            .heightIn(min = 20.dp)
            .widthIn(min = 30.dp)
            .clip(RoundedCornerShape(4.dp))
            .background(background)
            .clickable(onClick = onClick)
            .padding(horizontal = 4.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = color, fontSize = 12.5.sp, fontWeight = FontWeight.Bold, lineHeight = 16.sp)
    }
}

/**
 * A voice message (or any audio file) inside a bubble: button + waveform, then a footer with
 * duration (remaining while playing), speed chip and [meta] (time, ticks) at the end.
 */
@Composable
fun VoiceBubble(
    voice: VoiceManager,
    m: UiMessage,
    textColor: Color,
    metaColor: Color,
    own: Boolean,
    modifier: Modifier = Modifier,
    meta: (@Composable () -> Unit)? = null,
) {
    val a = m.msg.attachment ?: return
    val key = voice.messageKey(m)
    val playerState by voice.player.state.collectAsStateWithLifecycle()
    val player = playerState.takeIf { it.key == key }
    var info by remember(key) { mutableStateOf<AudioInfo?>(voice.cachedInfo(key)) }
    val auto = m.status != null || a.size <= VoiceManager.AUTO_FETCH_BYTES || player != null
    LaunchedEffect(key, auto) {
        if (info == null && auto) {
            val path = voice.localFile(m) ?: return@LaunchedEffect
            info = voice.info(key, path)
        }
    }
    val duration = info?.durationMs?.toLong()?.takeIf { it > 0 } ?: player?.durationMs ?: 0L
    val position = player?.positionMs ?: 0L
    val active = player != null && (player.playing || position > 0)
    val progress = if (duration > 0) (position.toFloat() / duration).coerceIn(0f, 1f) else 0f
    val uploading = m.status == SendStatus.SENDING && m.progress in 0f..0.999f
    val p = voicePalette(own, textColor, active)
    Column(modifier.width(VoiceWidth).semantics { contentDescription = "Sprachnachricht ${clock(duration)}" }) {
        if (!a.voiceMessage) {
            Text(
                a.name,
                color = textColor,
                fontSize = 12.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(bottom = 4.dp),
            )
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            PlayButton(
                player,
                circle = p.circle,
                tint = p.icon,
                upload = if (uploading) m.progress else null,
                download = info == null && !auto,
                onClick = { voice.toggle(m) },
            )
            Waveform(
                info?.waveform,
                progress,
                p.played,
                p.unplayed,
                Modifier.weight(1f).padding(start = 11.dp, end = 8.dp),
            ) { voice.seek(m, it) }
        }
        // Compact line height: the theme's body style would make the footer 28 dp tall.
        ProvideTextStyle(TextStyle(lineHeight = 16.sp)) {
        Row(
            Modifier.fillMaxWidth().padding(start = 8.dp, top = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(clock(if (active) duration - position else duration), color = metaColor, fontSize = 12.sp)
            if (active) {
                Spacer(Modifier.width(16.dp))
                SpeedChip(playerState.speed, p.played, p.chip) { voice.player.cycleSpeed() }
            }
            player?.error?.let {
                Spacer(Modifier.width(8.dp))
                Text(it, color = metaColor, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.weight(1f).widthIn(min = 8.dp))
            meta?.invoke()
        }
        }
    }
}

/** Blinking red dot of a running recording. */
@Composable
private fun RecDot() {
    val t = rememberInfiniteTransition(label = "rec")
    val alpha by t.animateFloat(1f, 0.25f, infiniteRepeatable(tween(600), RepeatMode.Reverse), label = "dot")
    Box(Modifier.size(10.dp).alpha(alpha).clip(CircleShape).background(LocalTalkColors.current.danger))
}

@Composable
private fun LevelMeter(levels: List<Float>, color: Color, modifier: Modifier) {
    Canvas(modifier.height(24.dp)) {
        val w = 2.dp.toPx()
        val gap = 2.dp.toPx()
        val n = (size.width / (w + gap)).toInt()
        val shown = levels.takeLast(n)
        shown.forEachIndexed { i, v ->
            val h = 3.dp.toPx() + v * (size.height - 3.dp.toPx())
            val x = size.width - (shown.size - i) * (w + gap)
            drawRoundRect(color, Offset(x, (size.height - h) / 2), Size(w, h), CornerRadius(w / 2, w / 2))
        }
    }
}

/** Inside the pill while the finger holds the mic: ● time … ‹ slide to cancel. */
@Composable
fun HeldRecording(rec: RecordingUi, dragX: Float, modifier: Modifier = Modifier) {
    val colors = LocalTalkColors.current
    Row(
        modifier.heightIn(min = 44.dp).padding(start = 16.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RecDot()
        Spacer(Modifier.width(8.dp))
        Text(clock(rec.durationMs), fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurface, maxLines = 1)
        Spacer(Modifier.width(12.dp))
        Box(Modifier.weight(1f).clipToBounds(), contentAlignment = Alignment.CenterEnd) {
            Text(
                "‹ Zum Abbrechen wischen",
                color = colors.muted,
                fontSize = 14.sp,
                maxLines = 1,
                softWrap = false,
                modifier = Modifier
                    .offset { IntOffset(dragX.coerceAtMost(0f).roundToInt(), 0) }
                    .alpha((1f + dragX / 600f).coerceIn(0.2f, 1f)),
            )
        }
    }
}

/** Floating hint above the mic while holding: slide up to lock. */
@Composable
fun LockHint(modifier: Modifier = Modifier, lift: Float) {
    Column(
        modifier
            .offset { IntOffset(0, lift.coerceIn(-200f, 0f).roundToInt()) }
            .clip(RoundedCornerShape(20.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .padding(horizontal = 10.dp, vertical = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(TalkIcons.Lock, "Sperren", tint = LocalTalkColors.current.muted, modifier = Modifier.size(20.dp))
        Icon(TalkIcons.ArrowUp, null, tint = LocalTalkColors.current.muted, modifier = Modifier.size(16.dp))
    }
}

/** Mic in the pill: press and hold to record, slide left to cancel, slide up to lock. */
@Composable
fun MicButton(
    onPress: () -> Boolean,
    onDrag: (dx: Float, dy: Float) -> Unit,
    onCancel: () -> Unit,
    onLock: () -> Unit,
    onRelease: () -> Unit,
    cancelDistancePx: Float,
) {
    val density = LocalDensity.current
    val lockPx = with(density) { 110.dp.toPx() }
    val press by rememberUpdatedState(onPress)
    val drag by rememberUpdatedState(onDrag)
    val cancel by rememberUpdatedState(onCancel)
    val lock by rememberUpdatedState(onLock)
    val release by rememberUpdatedState(onRelease)
    val cancelAt by rememberUpdatedState(cancelDistancePx)
    Box(
        Modifier
            .height(44.dp)
            .width(36.dp)
            .padding(end = 12.dp)
            .semantics { contentDescription = "Sprachnachricht – gedrückt halten zum Aufnehmen" }
            .pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    if (!press()) return@awaitEachGesture
                    down.consume()
                    var done = false
                    while (!done) {
                        val event = awaitPointerEvent()
                        val change = event.changes.firstOrNull { it.id == down.id } ?: break
                        if (!change.pressed) break
                        val dx = change.position.x - down.position.x
                        val dy = change.position.y - down.position.y
                        drag(dx, dy)
                        change.consume()
                        when {
                            dx < -cancelAt -> {
                                cancel()
                                done = true
                            }
                            dy < -lockPx -> {
                                lock()
                                done = true
                            }
                        }
                    }
                    drag(0f, 0f)
                    if (!done) release()
                }
            },
        contentAlignment = Alignment.CenterEnd,
    ) {
        Icon(TalkIcons.Mic, null, tint = LocalTalkColors.current.muted, modifier = Modifier.size(24.dp))
    }
}

/** Hands-free recording (slid up): delete · ● time + level · send. */
@Composable
fun LockedRecordingBar(rec: RecordingUi, onCancel: () -> Unit, onSend: () -> Unit) {
    val colors = LocalTalkColors.current
    VoiceBarFrame(onDelete = onCancel, deleteLabel = "Aufnahme verwerfen", onSend = onSend) {
        RecDot()
        Spacer(Modifier.width(8.dp))
        Text(clock(rec.durationMs), fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurface)
        Spacer(Modifier.width(10.dp))
        LevelMeter(rec.levels, colors.muted, Modifier.weight(1f))
        if (rec.warn) {
            Spacer(Modifier.width(8.dp))
            val left = ((60 * 60 * 1000L - rec.durationMs + 59_999) / 60_000).coerceAtLeast(0)
            Text("Noch $left Min.", color = colors.danger, fontSize = 13.sp)
        }
    }
}

/** An unsent recording: delete · play + waveform + duration · send. */
@Composable
fun VoiceDraftBar(voice: VoiceManager, draft: VoiceDraft, onDelete: () -> Unit, onSend: () -> Unit) {
    val key = voice.draftKey(draft)
    val playerState by voice.player.state.collectAsStateWithLifecycle()
    val player = playerState.takeIf { it.key == key }
    val duration = draft.durationMs.toLong()
    val position = player?.positionMs ?: 0L
    val progress = if (duration > 0) (position.toFloat() / duration).coerceIn(0f, 1f) else 0f
    val text = MaterialTheme.colorScheme.onSurface
    VoiceBarFrame(onDelete = onDelete, deleteLabel = "Entwurf löschen", onSend = onSend, startPadding = 2.dp) {
        PlayButton(player, MaterialTheme.colorScheme.surface, text, null, cell = 44.dp) { voice.toggleDraft(draft) }
        Spacer(Modifier.width(10.dp))
        Waveform(draft.waveform, progress, text, text.copy(alpha = 0.3f), Modifier.weight(1f)) { voice.seekDraft(draft, it) }
        Spacer(Modifier.width(10.dp))
        Text(clock(if (player != null && position > 0) duration - position else duration), fontSize = 13.sp, color = text)
    }
}

@Composable
private fun VoiceBarFrame(
    onDelete: () -> Unit,
    deleteLabel: String,
    onSend: () -> Unit,
    startPadding: androidx.compose.ui.unit.Dp = 16.dp,
    content: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit,
) {
    val colors = LocalTalkColors.current
    Row(
        Modifier.fillMaxWidth().padding(start = 4.dp, end = 14.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = onDelete) { Icon(TalkIcons.Delete, deleteLabel, tint = colors.muted) }
        Row(
            Modifier
                .weight(1f)
                .heightIn(min = 44.dp)
                .clip(RoundedCornerShape(22.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .padding(start = startPadding, end = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            content = content,
        )
        Spacer(Modifier.width(12.dp))
        Box(
            Modifier.size(40.dp).clip(CircleShape).background(colors.accent).clickable(onClick = onSend),
            contentAlignment = Alignment.Center,
        ) {
            Icon(TalkIcons.Send, "Sprachnachricht senden", tint = Color.White, modifier = Modifier.size(22.dp))
        }
    }
}
