package org.sirplus.talk.call

import android.content.Context
import android.graphics.SurfaceTexture
import android.os.SystemClock
import android.view.TextureView
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.webrtc.EglBase
import org.webrtc.EglRenderer
import org.webrtc.GlRectDrawer
import org.webrtc.VideoFrame
import org.webrtc.VideoSink
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.abs

/** The EGL context shared by the codecs, the camera and every renderer (one per process). */
object CallEgl {
    val base: EglBase by lazy { EglBase.create() }
    val context: EglBase.Context get() = base.eglBaseContext
}

/**
 * One video stream (a peer's received track or our camera) as a stable sink for the UI:
 * renderers come and go independently of the track, which the engine owns and disposes.
 * [active]: frames arrived within the last 2 s (docs/calls.md: otherwise the avatar).
 */
class VideoFeed : VideoSink {
    private val sinks = CopyOnWriteArrayList<VideoSink>()
    @Volatile private var lastFrameAt = 0L

    private val _active = MutableStateFlow(false)
    val active: StateFlow<Boolean> = _active

    private val _aspect = MutableStateFlow(0f)
    /** Width / height of the (rotated) frames, 0 before the first one. */
    val aspect: StateFlow<Float> = _aspect

    fun add(sink: VideoSink) { sinks.addIfAbsent(sink) }
    fun remove(sink: VideoSink) { sinks.remove(sink) }

    override fun onFrame(frame: VideoFrame) {
        lastFrameAt = SystemClock.elapsedRealtime()
        if (!_active.value) _active.value = true
        val h = frame.rotatedHeight
        if (h > 0) {
            val a = frame.rotatedWidth.toFloat() / h
            if (abs(a - _aspect.value) > 0.01f) _aspect.value = a
        }
        for (s in sinks) s.onFrame(frame)
    }

    /** Called regularly by the engine: no frames for 2 s → inactive. */
    fun checkStale(now: Long = SystemClock.elapsedRealtime()) {
        if (_active.value && now - lastFrameAt > STALE_MS) _active.value = false
    }

    private companion object {
        const val STALE_MS = 2_000L
    }
}

/** Our camera while it runs: its feed and whether it is the front camera (mirrored preview). */
data class LocalVideo(val feed: VideoFeed, val front: Boolean)

/** What the call screen needs from the engine for video. */
interface CallVideo {
    /** Received video per MediaEngine peer id (`CallParticipant.peerId`). */
    val remote: StateFlow<Map<String, VideoFeed>>
    /** Our camera, `null` while it is off. */
    val local: StateFlow<LocalVideo?>
    /** Front ↔ back camera. */
    fun switchCamera()
}

/**
 * Renders a [VideoFeed] into a TextureView (clips, rounded corners and z-order like any
 * view, unlike SurfaceView). The frame is cropped to the view's aspect ratio (fill); for
 * letterboxing the caller sizes the view to [VideoFeed.aspect].
 */
class VideoTextureView(context: Context) : TextureView(context), TextureView.SurfaceTextureListener, VideoSink {
    private val renderer = EglRenderer("talk-video")
    private var feed: VideoFeed? = null
    private var released = false

    init {
        renderer.init(CallEgl.context, EglBase.CONFIG_PLAIN, GlRectDrawer())
        surfaceTextureListener = this
        isOpaque = false
    }

    fun bind(next: VideoFeed?) {
        if (next === feed || released) return
        feed?.remove(this)
        feed = next
        renderer.clearImage(0f, 0f, 0f, 0f)
        next?.add(this)
    }

    fun setMirror(mirror: Boolean) = renderer.setMirror(mirror)

    fun release() {
        if (released) return
        released = true
        feed?.remove(this)
        feed = null
        renderer.release()
    }

    override fun onFrame(frame: VideoFrame) = renderer.onFrame(frame)

    override fun onSurfaceTextureAvailable(surface: SurfaceTexture, width: Int, height: Int) {
        if (released) return
        if (height > 0) renderer.setLayoutAspectRatio(width.toFloat() / height)
        renderer.createEglSurface(surface)
    }

    override fun onSurfaceTextureSizeChanged(surface: SurfaceTexture, width: Int, height: Int) {
        if (height > 0) renderer.setLayoutAspectRatio(width.toFloat() / height)
    }

    override fun onSurfaceTextureDestroyed(surface: SurfaceTexture): Boolean {
        if (!released) {
            val done = CountDownLatch(1)
            renderer.releaseEglSurface { done.countDown() }
            done.await(1, TimeUnit.SECONDS)
        }
        return true
    }

    override fun onSurfaceTextureUpdated(surface: SurfaceTexture) = Unit
}
