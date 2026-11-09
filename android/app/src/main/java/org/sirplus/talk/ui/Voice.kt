package org.sirplus.talk.ui

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
import org.sirplus.talk.RecordingUi
import org.sirplus.talk.SendStatus
import org.sirplus.talk.UiMessage
import org.sirplus.talk.VoiceManager
import org.sirplus.talk.media.PlayerUi
import uniffi.talk_ffi.AudioInfo
import uniffi.talk_ffi.VoiceDraft
import kotlin.math.roundToInt

/** Bars drawn for a voice message (Signal Android draws a similar count). */
private const val BARS = 36

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

/** Bars with the played part highlighted; tap or drag to seek. */
@Composable
private fun Waveform(levels: ByteArray?, progress: Float, color: Color, modifier: Modifier, onSeek: (Float) -> Unit) {
    val values = remember(levels) { bars(levels, BARS) }
    val seek by rememberUpdatedState(onSeek)
    Canvas(
        modifier
            .height(28.dp)
            .pointerInput(Unit) { detectTapGestures { seek(it.x / size.width) } }
            .pointerInput(Unit) {
                detectHorizontalDragGestures { change, _ -> seek(change.position.x / size.width) }
            },
    ) {
        val gap = 2.dp.toPx()
        val w = ((size.width - gap * (BARS - 1)) / BARS).coerceAtLeast(1f)
        val min = 4.dp.toPx()
        val max = size.height
        values.forEachIndexed { i, v ->
            val h = min + v * (max - min)
            val x = i * (w + gap)
            val played = (i + 0.5f) / BARS <= progress
            drawRoundRect(
                color = if (played) color else color.copy(alpha = 0.35f),
                topLeft = Offset(x, (size.height - h) / 2),
                size = Size(w, h),
                cornerRadius = CornerRadius(w / 2, w / 2),
            )
        }
    }
}

@Composable
private fun PlayButton(player: PlayerUi?, background: Color, tint: Color, upload: Float?, onClick: () -> Unit) {
    Box(
        Modifier.size(40.dp).clip(CircleShape).background(background).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        when {
            upload != null -> CircularProgressIndicator(
                progress = { upload.coerceAtLeast(0.03f) },
                modifier = Modifier.size(34.dp),
                color = tint,
                trackColor = tint.copy(alpha = 0.2f),
                strokeWidth = 2.5.dp,
            )
            player?.loading == true -> CircularProgressIndicator(Modifier.size(30.dp), color = tint, strokeWidth = 2.5.dp)
        }
        Icon(
            if (player?.playing == true) TalkIcons.Pause else TalkIcons.Play,
            if (player?.playing == true) "Pause" else "Abspielen",
            tint = tint,
            modifier = Modifier.size(22.dp),
        )
    }
}

/** Speed chip: 1× → 1,5× → 2× → 0,5×. */
@Composable
private fun SpeedChip(speed: Float, color: Color, onClick: () -> Unit) {
    val label = (if (speed % 1f == 0f) speed.toInt().toString() else speed.toString().replace('.', ',')) + "×"
    Text(
        label,
        color = color,
        fontSize = 11.sp,
        fontWeight = FontWeight.SemiBold,
        modifier = Modifier
            .clip(RoundedCornerShape(9.dp))
            .background(color.copy(alpha = 0.15f))
            .clickable(onClick = onClick)
            .padding(horizontal = 6.dp, vertical = 1.dp),
    )
}

/** A voice message (or any audio file) inside a bubble. */
@Composable
fun VoiceBubble(voice: VoiceManager, m: UiMessage, textColor: Color, own: Boolean, modifier: Modifier = Modifier) {
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
    Column(modifier.width(240.dp).semantics { contentDescription = "Sprachnachricht ${clock(duration)}" }) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            PlayButton(
                player,
                background = if (own) Color.White else MaterialTheme.colorScheme.surface,
                tint = if (own) LocalTalkColors.current.bubbleOwn else textColor,
                upload = if (uploading) m.progress else null,
                onClick = { voice.toggle(m) },
            )
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Waveform(info?.waveform, progress, textColor, Modifier.fillMaxWidth()) { voice.seek(m, it) }
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.heightIn(min = 18.dp)) {
                    Text(
                        clock(if (active) duration - position else duration),
                        color = textColor.copy(alpha = 0.8f),
                        fontSize = 12.sp,
                    )
                    if (active) {
                        Spacer(Modifier.width(6.dp))
                        SpeedChip(playerState.speed, textColor) { voice.player.cycleSpeed() }
                    }
                    player?.error?.let {
                        Spacer(Modifier.width(6.dp))
                        Text(it, color = textColor.copy(alpha = 0.8f), fontSize = 12.sp)
                    }
                }
            }
        }
        if (!a.voiceMessage) {
            Text(a.name, color = textColor.copy(alpha = 0.8f), fontSize = 12.sp, maxLines = 1)
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
        PlayButton(player, MaterialTheme.colorScheme.surface, text, null) { voice.toggleDraft(draft) }
        Spacer(Modifier.width(10.dp))
        Waveform(draft.waveform, progress, text, Modifier.weight(1f)) { voice.seekDraft(draft, it) }
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
