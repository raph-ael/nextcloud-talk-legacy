package org.wolkenfunk.app.media

import android.content.ComponentName
import android.content.Context
import android.net.Uri
import androidx.core.content.ContextCompat
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import java.io.File
import kotlin.coroutines.resume

/** What the bubbles need to know about playback. */
data class PlayerUi(
    val key: String? = null,
    val playing: Boolean = false,
    val loading: Boolean = false,
    val positionMs: Long = 0,
    val durationMs: Long = 0,
    val speed: Float = 1f,
    val error: String? = null,
)

/**
 * App side of [VoicePlaybackService]: one voice message at a time, addressed by a key
 * (message or draft). Runs on the main thread.
 */
class VoicePlayer(private val context: Context, private val scope: CoroutineScope) {
    companion object {
        val SPEEDS = floatArrayOf(1f, 1.5f, 2f, 0.5f)
        private const val PREFS = "ui"
        private const val KEY_SPEED = "voice_speed"
    }

    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val _state = MutableStateFlow(PlayerUi(speed = prefs.getFloat(KEY_SPEED, 1f).takeIf { s -> SPEEDS.any { it == s } } ?: 1f))
    val state: StateFlow<PlayerUi> = _state

    /** Called when a message finished playing (for autoplay of the next one). */
    var onEnded: ((key: String) -> Unit)? = null

    private var controller: MediaController? = null
    private var ticker: Job? = null
    private var request = 0

    private suspend fun controller(): MediaController {
        controller?.let { return it }
        val token = SessionToken(context, ComponentName(context, VoicePlaybackService::class.java))
        val future = MediaController.Builder(context, token).buildAsync()
        val c = suspendCancellableCoroutine { cont ->
            future.addListener({ cont.resume(future.get()) }, ContextCompat.getMainExecutor(context))
        }
        c.addListener(object : Player.Listener {
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                _state.update { it.copy(playing = isPlaying, loading = false) }
                if (isPlaying) startTicker() else tick()
            }

            override fun onPlaybackStateChanged(playbackState: Int) {
                when (playbackState) {
                    Player.STATE_BUFFERING -> _state.update { it.copy(loading = true) }
                    Player.STATE_READY -> _state.update { it.copy(loading = false, durationMs = c.duration.coerceAtLeast(0)) }
                    Player.STATE_ENDED -> {
                        val key = _state.value.key
                        _state.update { it.copy(playing = false, positionMs = 0) }
                        c.seekTo(0)
                        c.pause()
                        if (key != null) onEnded?.invoke(key)
                    }
                    else -> Unit
                }
            }

            override fun onPlayerError(error: PlaybackException) {
                _state.update { it.copy(playing = false, loading = false, error = "Wiedergabe nicht möglich") }
            }
        })
        controller = c
        return c
    }

    private fun startTicker() {
        ticker?.cancel()
        ticker = scope.launch {
            while (true) {
                tick()
                delay(100)
            }
        }
    }

    private fun tick() {
        val c = controller ?: return
        if (!c.isPlaying) {
            ticker?.cancel()
            ticker = null
        }
        _state.update { it.copy(positionMs = c.currentPosition.coerceAtLeast(0)) }
    }

    /** Play/pause [key]; a different key starts that file (resolved lazily: download). */
    fun toggle(key: String, title: String, artist: String, resolve: suspend () -> String?) {
        val st = _state.value
        val c = controller
        if (st.key == key && c != null && c.mediaItemCount > 0) {
            if (c.isPlaying) c.pause() else c.play()
            return
        }
        play(key, title, artist, 0f, resolve)
    }

    fun play(key: String, title: String, artist: String, startFraction: Float, resolve: suspend () -> String?) {
        val my = ++request
        _state.update { it.copy(key = key, playing = false, loading = true, positionMs = 0, durationMs = 0, error = null) }
        scope.launch {
            val path = runCatching { resolve() }.getOrNull()
            if (my != request) return@launch
            if (path == null) {
                _state.update { it.copy(loading = false, error = "Nicht geladen – offline?") }
                return@launch
            }
            val c = controller()
            if (my != request) return@launch
            val item = MediaItem.Builder()
                .setMediaId(key)
                .setUri(Uri.fromFile(File(path)))
                .setMediaMetadata(MediaMetadata.Builder().setTitle(title).setArtist(artist).build())
                .build()
            c.setMediaItem(item)
            c.playbackParameters = PlaybackParameters(_state.value.speed)
            c.prepare()
            if (startFraction > 0f) {
                // Duration is known once prepared.
                waitReady(c)
                if (c.duration > 0) c.seekTo((c.duration * startFraction).toLong())
            }
            c.play()
        }
    }

    private suspend fun waitReady(c: MediaController) {
        if (c.playbackState == Player.STATE_READY) return
        suspendCancellableCoroutine { cont ->
            val l = object : Player.Listener {
                override fun onPlaybackStateChanged(playbackState: Int) {
                    if (playbackState == Player.STATE_READY || playbackState == Player.STATE_IDLE) {
                        c.removeListener(this)
                        if (cont.isActive) cont.resume(Unit)
                    }
                }
            }
            c.addListener(l)
            cont.invokeOnCancellation { c.removeListener(l) }
        }
    }

    /** Jump to [fraction] of [key] (starts it when another file is loaded). */
    fun seek(key: String, fraction: Float, title: String, artist: String, resolve: suspend () -> String?) {
        val f = fraction.coerceIn(0f, 1f)
        val c = controller
        if (_state.value.key == key && c != null && c.duration > 0) {
            c.seekTo((c.duration * f).toLong())
            _state.update { it.copy(positionMs = (c.duration * f).toLong()) }
            if (!c.isPlaying) c.play()
        } else {
            play(key, title, artist, f, resolve)
        }
    }

    fun cycleSpeed() {
        val i = SPEEDS.indexOfFirst { it == _state.value.speed }
        val speed = SPEEDS[(i + 1) % SPEEDS.size]
        prefs.edit().putFloat(KEY_SPEED, speed).apply()
        controller?.playbackParameters = PlaybackParameters(speed)
        _state.update { it.copy(speed = speed) }
    }

    /** Stops [key] (or whatever plays when null), e.g. before its draft is deleted. */
    fun stop(key: String? = null) {
        if (key != null && _state.value.key != key) return
        request++
        controller?.run {
            stop()
            clearMediaItems()
        }
        _state.update { it.copy(key = null, playing = false, loading = false, positionMs = 0) }
    }
}
