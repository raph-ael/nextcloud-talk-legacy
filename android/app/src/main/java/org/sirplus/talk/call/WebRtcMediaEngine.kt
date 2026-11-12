package org.sirplus.talk.call

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import androidx.core.content.ContextCompat
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import org.webrtc.Camera2Enumerator
import org.webrtc.CameraVideoCapturer
import org.webrtc.DefaultVideoDecoderFactory
import org.webrtc.DefaultVideoEncoderFactory
import org.webrtc.IceCandidate
import org.webrtc.MediaConstraints
import org.webrtc.MediaStreamTrack
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RTCStatsReport
import org.webrtc.RtpParameters
import org.webrtc.RtpSender
import org.webrtc.RtpTransceiver
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription
import org.webrtc.SurfaceTextureHelper
import org.webrtc.VideoSource
import org.webrtc.VideoTrack
import org.webrtc.audio.JavaAudioDeviceModule
import uniffi.talk_ffi.CallIceCandidate
import uniffi.talk_ffi.CallIceServer
import uniffi.talk_ffi.MediaEngine
import uniffi.talk_ffi.PeerKind
import uniffi.talk_ffi.PeerState
import uniffi.talk_ffi.SdpType
import java.util.concurrent.Executors
import kotlin.coroutines.resume

/** Where the engine's results go (the core's `media*` methods). */
interface MediaReports {
    fun localDescription(peerId: String, type: SdpType, sdp: String)
    fun localCandidate(peerId: String, candidate: CallIceCandidate)
    fun peerState(peerId: String, state: PeerState)
    fun peerError(peerId: String, message: String)
    fun audioLevel(peerId: String, level: Float)
    /** The camera stopped or could not be opened; [reason] is shown as a hint. */
    fun cameraFailed(reason: String)
}

/** Platform side of starting/stopping call audio (mode, routing, focus, service). */
interface CallAudioHost {
    fun startAudio(callId: String)
    fun stopAudio(callId: String)
}

/**
 * The core's [MediaEngine] on libwebrtc (docs/calls.md, rules 1–12).
 *
 * Every command is queued and returns at once. All WebRTC work happens on one engine
 * thread; each peer has its own queue that runs strictly in order (an operation waiting
 * for an SDP result holds back only its own peer). Closed or unknown peers are ignored
 * and never reported.
 *
 * Video (docs/calls.md "Video"): every publisher/mesh peer has a video sender from the
 * start; the camera track is attached to or removed from it (no renegotiation). Received
 * video goes to a [VideoFeed] per peer for the call screen.
 */
class WebRtcMediaEngine(
    private val context: Context,
    private val reports: MediaReports,
    private val audioHost: CallAudioHost,
) : MediaEngine, CallVideo {
    private val thread = Executors.newSingleThreadExecutor { r -> Thread(r, "talk-media") }
    private val dispatcher = thread.asCoroutineDispatcher()
    private val scope = CoroutineScope(SupervisorJob() + dispatcher)

    // Owned by the engine thread.
    private var factory: PeerConnectionFactory? = null
    private var adm: JavaAudioDeviceModule? = null
    private var audioSource: org.webrtc.AudioSource? = null
    private var localTrack: org.webrtc.AudioTrack? = null
    private val peers = HashMap<String, Peer>()
    private var micEnabled = true
    private var levels: Job? = null

    // Camera (engine thread).
    private var capturer: CameraVideoCapturer? = null
    private var textureHelper: SurfaceTextureHelper? = null
    private var videoSource: VideoSource? = null
    private var videoTrack: VideoTrack? = null
    private var localFeed: VideoFeed? = null
    private var front = true

    private val _remote = MutableStateFlow<Map<String, VideoFeed>>(emptyMap())
    override val remote: StateFlow<Map<String, VideoFeed>> = _remote
    private val _local = MutableStateFlow<LocalVideo?>(null)
    override val local: StateFlow<LocalVideo?> = _local

    private inner class Peer(val id: String, val kind: PeerKind) {
        val job = SupervisorJob(scope.coroutineContext[Job])
        val queue = Channel<suspend () -> Unit>(Channel.UNLIMITED)
        var pc: PeerConnection? = null
        var closed = false
        var reported: PeerState? = null
        var connState: PeerConnection.PeerConnectionState? = null
        var iceState: PeerConnection.IceConnectionState? = null
        /** Remote candidates that arrived before the remote description. */
        val early = ArrayList<IceCandidate>()
        /** Our camera goes here (publisher, mesh). */
        var videoSender: RtpSender? = null
        /** Received video (subscriber, mesh) and its feed. */
        var remoteVideo: VideoTrack? = null
        var feed: VideoFeed? = null

        init {
            CoroutineScope(job + dispatcher).launch {
                for (op in queue) {
                    if (closed) break
                    try {
                        op()
                    } catch (e: kotlinx.coroutines.CancellationException) {
                        throw e
                    } catch (e: Throwable) {
                        Log.w(TAG, "peer $id: ${e.message}", e)
                        if (!closed) reports.peerError(id, e.message ?: e.javaClass.simpleName)
                    }
                }
            }
        }
    }

    // --- MediaEngine (any thread) -------------------------------------------------

    override fun createPeer(peerId: String, kind: PeerKind, iceServers: List<CallIceServer>) = onEngine {
        if (peers.containsKey(peerId)) return@onEngine
        val peer = Peer(peerId, kind)
        peers[peerId] = peer
        peer.enqueue { open(peer, iceServers) }
        startLevels()
    }

    override fun createOffer(peerId: String, iceRestart: Boolean) = withPeer(peerId) { peer ->
        val pc = peer.pc ?: return@withPeer
        if (iceRestart) pc.restartIce()
        val constraints = MediaConstraints().apply {
            if (iceRestart) mandatory += MediaConstraints.KeyValuePair("IceRestart", "true")
        }
        val sdp = pc.awaitCreate(offer = true, constraints)
        if (peer.closed) return@withPeer
        pc.awaitSetLocal(sdp)
        if (peer.closed) return@withPeer
        reports.localDescription(peer.id, SdpType.OFFER, sdp.description)
    }

    override fun createAnswer(peerId: String) = withPeer(peerId) { peer ->
        val pc = peer.pc ?: return@withPeer
        val sdp = pc.awaitCreate(offer = false, MediaConstraints())
        if (peer.closed) return@withPeer
        pc.awaitSetLocal(sdp)
        if (peer.closed) return@withPeer
        reports.localDescription(peer.id, SdpType.ANSWER, sdp.description)
    }

    override fun setRemoteDescription(peerId: String, sdpType: SdpType, sdp: String) = withPeer(peerId) { peer ->
        val pc = peer.pc ?: return@withPeer
        val type = if (sdpType == SdpType.OFFER) SessionDescription.Type.OFFER else SessionDescription.Type.ANSWER
        pc.awaitSetRemote(SessionDescription(type, sdp))
        if (peer.closed) return@withPeer
        if (peer.kind == PeerKind.MESH && sdpType == SdpType.OFFER) adoptOfferedVideo(peer, pc)
        for (c in peer.early) pc.addIceCandidate(c)
        peer.early.clear()
    }

    override fun addRemoteCandidate(peerId: String, candidate: CallIceCandidate) = withPeer(peerId) { peer ->
        val pc = peer.pc ?: return@withPeer
        val c = IceCandidate(candidate.sdpMid ?: "", candidate.sdpMLineIndex, candidate.candidate)
        if (pc.remoteDescription == null) peer.early += c
        else if (!pc.addIceCandidate(c)) Log.w(TAG, "peer ${peer.id}: candidate rejected")
    }

    override fun closePeer(peerId: String) = onEngine {
        val peer = peers.remove(peerId) ?: return@onEngine
        dispose(peer)
        if (peers.isEmpty()) stopLevels()
    }

    override fun setMicrophoneEnabled(enabled: Boolean) = onEngine {
        micEnabled = enabled
        localTrack?.setEnabled(enabled)
    }

    override fun setCameraEnabled(enabled: Boolean) = onEngine {
        if (enabled) {
            val track = videoTrack ?: openCamera() ?: return@onEngine
            for (peer in peers.values) attachCamera(peer, track)
        } else {
            closeCamera()
        }
    }

    override fun switchCamera() = onEngine {
        val c = capturer ?: return@onEngine
        // The first camera facing the other way (phones list extra lenses and depth
        // sensors; cycling through all of them would land on those).
        val enumerator = Camera2Enumerator(context)
        val target = enumerator.deviceNames.firstOrNull { enumerator.isFrontFacing(it) != front } ?: return@onEngine
        c.switchCamera(object : CameraVideoCapturer.CameraSwitchHandler {
            override fun onCameraSwitchDone(isFrontCamera: Boolean) = onEngine {
                front = isFrontCamera
                val feed = localFeed ?: return@onEngine
                if (capturer != null) _local.value = LocalVideo(feed, isFrontCamera)
            }

            override fun onCameraSwitchError(error: String) {
                Log.w(TAG, "camera switch: $error")
            }
        }, target)
    }

    override fun startAudio(callId: String) {
        // Platform audio (mode, focus, routing, service) first, then the first peer
        // creates the factory with the audio device in the right mode.
        audioHost.startAudio(callId)
    }

    override fun stopAudio(callId: String) {
        onEngine { releaseAll() }
        audioHost.stopAudio(callId)
    }

    // --- engine thread ------------------------------------------------------------

    private fun onEngine(block: () -> Unit) {
        scope.launch {
            try {
                block()
            } catch (e: Throwable) {
                Log.e(TAG, "engine", e)
            }
        }
    }

    private fun withPeer(peerId: String, op: suspend (Peer) -> Unit) = onEngine {
        val peer = peers[peerId] ?: return@onEngine
        peer.enqueue { op(peer) }
    }

    private fun Peer.enqueue(op: suspend () -> Unit) {
        queue.trySend(op)
    }

    private fun ensureFactory(): PeerConnectionFactory {
        factory?.let { return it }
        initialize(context)
        val prefs = CallPrefs(context)
        val hwAec = prefs.hardwareAec && JavaAudioDeviceModule.isBuiltInAcousticEchoCancelerSupported()
        val hwNs = prefs.hardwareAec && JavaAudioDeviceModule.isBuiltInNoiseSuppressorSupported()
        val module = JavaAudioDeviceModule.builder(context)
            .setUseHardwareAcousticEchoCanceler(hwAec)
            .setUseHardwareNoiseSuppressor(hwNs)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setAudioRecordErrorCallback(object : JavaAudioDeviceModule.AudioRecordErrorCallback {
                override fun onWebRtcAudioRecordInitError(msg: String) { Log.e(TAG, "record init: $msg") }
                override fun onWebRtcAudioRecordStartError(code: JavaAudioDeviceModule.AudioRecordStartErrorCode, msg: String) { Log.e(TAG, "record start $code: $msg") }
                override fun onWebRtcAudioRecordError(msg: String) { Log.e(TAG, "record: $msg") }
            })
            .setAudioTrackErrorCallback(object : JavaAudioDeviceModule.AudioTrackErrorCallback {
                override fun onWebRtcAudioTrackInitError(msg: String) { Log.e(TAG, "track init: $msg") }
                override fun onWebRtcAudioTrackStartError(code: JavaAudioDeviceModule.AudioTrackStartErrorCode, msg: String) { Log.e(TAG, "track start $code: $msg") }
                override fun onWebRtcAudioTrackError(msg: String) { Log.e(TAG, "track: $msg") }
            })
            .createAudioDeviceModule()
        Log.i(TAG, "audio device: hardware AEC=$hwAec NS=$hwNs")
        val egl = CallEgl.context
        val f = PeerConnectionFactory.builder()
            .setOptions(PeerConnectionFactory.Options())
            .setAudioDeviceModule(module)
            .setVideoEncoderFactory(DefaultVideoEncoderFactory(egl, true, true))
            .setVideoDecoderFactory(DefaultVideoDecoderFactory(egl))
            .createPeerConnectionFactory()
        adm = module
        factory = f
        return f
    }

    /** The microphone track shared by the publisher (and mesh peers). */
    private fun localTrack(f: PeerConnectionFactory): org.webrtc.AudioTrack {
        localTrack?.let { return it }
        // Software processing when the hardware one is off (WebRTC skips it otherwise).
        val constraints = MediaConstraints().apply {
            mandatory += MediaConstraints.KeyValuePair("googEchoCancellation", "true")
            mandatory += MediaConstraints.KeyValuePair("googNoiseSuppression", "true")
            mandatory += MediaConstraints.KeyValuePair("googAutoGainControl", "true")
            mandatory += MediaConstraints.KeyValuePair("googHighpassFilter", "true")
        }
        val source = f.createAudioSource(constraints)
        val track = f.createAudioTrack("talk-mic", source)
        track.setEnabled(micEnabled)
        audioSource = source
        localTrack = track
        return track
    }

    private fun open(peer: Peer, iceServers: List<CallIceServer>) {
        val f = ensureFactory()
        val config = PeerConnection.RTCConfiguration(
            iceServers.map { s ->
                PeerConnection.IceServer.builder(s.urls)
                    .setUsername(s.username ?: "")
                    .setPassword(s.credential ?: "")
                    .createIceServer()
            },
        ).apply {
            sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
            bundlePolicy = PeerConnection.BundlePolicy.MAXBUNDLE
            rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE
            continualGatheringPolicy = PeerConnection.ContinualGatheringPolicy.GATHER_CONTINUALLY
            tcpCandidatePolicy = PeerConnection.TcpCandidatePolicy.ENABLED
        }
        val pc = f.createPeerConnection(config, Observer(peer))
            ?: throw IllegalStateException("PeerConnection could not be created")
        peer.pc = pc
        when (peer.kind) {
            PeerKind.PUBLISHER -> {
                pc.addTransceiver(
                    localTrack(f),
                    RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.SEND_ONLY, listOf(STREAM_ID)),
                )
                peer.videoSender = addVideoTransceiver(pc, RtpTransceiver.RtpTransceiverDirection.SEND_ONLY).sender
            }
            PeerKind.MESH -> {
                // addTrack (not addTransceiver): reused when the other side sends the offer.
                pc.addTrack(localTrack(f), listOf(STREAM_ID))
                // Used when we offer; with their offer, adoptOfferedVideo takes theirs.
                peer.videoSender = addVideoTransceiver(pc, RtpTransceiver.RtpTransceiverDirection.SEND_RECV).sender
            }
            // Created by the MCU's offer (audio and video); answering without tracks makes them recvonly.
            PeerKind.SUBSCRIBER -> Unit
        }
        videoTrack?.let { attachCamera(peer, it) }
    }

    private fun addVideoTransceiver(pc: PeerConnection, direction: RtpTransceiver.RtpTransceiverDirection): RtpTransceiver {
        val encoding = RtpParameters.Encoding(null, true, null).apply {
            maxBitrateBps = VIDEO_MAX_BITRATE
            maxFramerate = VIDEO_FPS
        }
        return pc.addTransceiver(
            MediaStreamTrack.MediaType.MEDIA_TYPE_VIDEO,
            RtpTransceiver.RtpTransceiverInit(direction, listOf(STREAM_ID), listOf(encoding)),
        )
    }

    /**
     * Mesh peer that answers: the transceiver created by our video sender stays unused
     * (only addTrack ones are reused), so send on the one of their video m-line instead.
     * getTransceivers() disposes earlier wrappers; the sender reference is refreshed here.
     */
    private fun adoptOfferedVideo(peer: Peer, pc: PeerConnection) {
        val video = pc.transceivers.filter { it.mediaType == MediaStreamTrack.MediaType.MEDIA_TYPE_VIDEO && !it.isStopped }
        val offered = video.firstOrNull { it.mid != null } ?: return
        for (t in video) if (t !== offered && t.mid == null) t.stop()
        if (offered.direction != RtpTransceiver.RtpTransceiverDirection.SEND_RECV) {
            offered.setDirection(RtpTransceiver.RtpTransceiverDirection.SEND_RECV)
        }
        offered.sender.setStreams(listOf(STREAM_ID))
        peer.videoSender = offered.sender
        limitVideo(offered.sender)
        videoTrack?.let { attachCamera(peer, it) }
    }

    // --- camera -------------------------------------------------------------------

    /** Opens the front (or last used) camera; `null` and a hint when that fails. */
    private fun openCamera(): VideoTrack? {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            cameraFailed("Keine Berechtigung für die Kamera")
            return null
        }
        val f = ensureFactory()
        return try {
            val enumerator = Camera2Enumerator(context)
            val names = enumerator.deviceNames
            val name = names.firstOrNull { enumerator.isFrontFacing(it) == front }
                ?: names.firstOrNull()
                ?: run {
                    cameraFailed("Keine Kamera gefunden")
                    return null
                }
            front = enumerator.isFrontFacing(name)
            val c = enumerator.createCapturer(name, CameraEvents())
                ?: throw IllegalStateException("no capturer for $name")
            val helper = SurfaceTextureHelper.create("talk-camera", CallEgl.context)
            val source = f.createVideoSource(false)
            c.initialize(helper, context, source.capturerObserver)
            c.startCapture(VIDEO_WIDTH, VIDEO_HEIGHT, VIDEO_FPS)
            val track = f.createVideoTrack("talk-camera", source)
            val feed = VideoFeed()
            track.addSink(feed)
            capturer = c
            textureHelper = helper
            videoSource = source
            videoTrack = track
            localFeed = feed
            _local.value = LocalVideo(feed, front)
            Log.i(TAG, "camera open: $name (front $front)")
            track
        } catch (e: Exception) {
            Log.w(TAG, "camera", e)
            closeCamera()
            cameraFailed("Kamera nicht verfügbar")
            null
        }
    }

    private fun attachCamera(peer: Peer, track: VideoTrack) {
        val sender = peer.videoSender ?: return
        if (peer.closed) return
        try {
            if (sender.track() !== track) sender.setTrack(track, false)
            limitVideo(sender)
        } catch (e: IllegalStateException) {
            Log.w(TAG, "peer ${peer.id}: video sender", e)
        }
    }

    /** At most 600 kbit/s and 24 fps, resolution and frame rate degrade together. */
    private fun limitVideo(sender: RtpSender) {
        val params = sender.parameters
        params.degradationPreference = RtpParameters.DegradationPreference.BALANCED
        for (e in params.encodings) {
            e.maxBitrateBps = VIDEO_MAX_BITRATE
            e.maxFramerate = VIDEO_FPS
        }
        sender.setParameters(params)
    }

    private fun closeCamera() {
        for (peer in peers.values) {
            val sender = peer.videoSender ?: continue
            if (!peer.closed) runCatching { sender.setTrack(null, false) }
        }
        val c = capturer
        capturer = null
        _local.value = null
        if (c != null) {
            runCatching { c.stopCapture() }
            c.dispose()
        }
        videoTrack?.let { t -> localFeed?.let { t.removeSink(it) }; t.dispose() }
        videoTrack = null
        localFeed = null
        videoSource?.dispose()
        videoSource = null
        textureHelper?.dispose()
        textureHelper = null
    }

    /** The camera failed or went away: off, tell the core and show why. */
    private fun cameraFailed(reason: String) {
        Log.w(TAG, "camera failed: $reason")
        reports.cameraFailed(reason)
    }

    private inner class CameraEvents : CameraVideoCapturer.CameraEventsHandler {
        override fun onCameraError(error: String) = onEngine {
            if (capturer == null) return@onEngine
            closeCamera()
            val busy = error.contains("in use", ignoreCase = true) || error.contains("too many", ignoreCase = true) ||
                error.contains("disabled", ignoreCase = true)
            cameraFailed(if (busy) "Die Kamera wird gerade von einer anderen App verwendet" else "Kamera nicht verfügbar")
        }

        override fun onCameraDisconnected() = onEngine {
            if (capturer == null) return@onEngine
            closeCamera()
            cameraFailed("Die Kamera wurde von einer anderen App übernommen")
        }

        override fun onCameraFreezed(error: String) { Log.w(TAG, "camera frozen: $error") }
        override fun onCameraOpening(cameraName: String) = Unit
        override fun onFirstFrameAvailable() { Log.i(TAG, "camera: first frame") }
        override fun onCameraClosed() = Unit
    }

    private fun dispose(peer: Peer) {
        peer.closed = true
        peer.queue.close()
        peer.job.cancel()
        dropRemoteVideo(peer)
        peer.videoSender = null
        peer.pc?.dispose()
        peer.pc = null
    }

    private fun dropRemoteVideo(peer: Peer) {
        val feed = peer.feed ?: return
        runCatching { peer.remoteVideo?.removeSink(feed) }
        peer.remoteVideo = null
        peer.feed = null
        _remote.value = _remote.value - peer.id
    }

    private fun onRemoteVideo(peer: Peer, track: VideoTrack) {
        if (peer.closed || peer.remoteVideo === track) return
        dropRemoteVideo(peer)
        val feed = VideoFeed()
        track.addSink(feed)
        peer.remoteVideo = track
        peer.feed = feed
        _remote.value = _remote.value + (peer.id to feed)
        Log.i(TAG, "peer ${peer.id}: video track")
    }

    private fun releaseAll() {
        stopLevels()
        closeCamera()
        front = true
        peers.values.forEach { dispose(it) }
        peers.clear()
        _remote.value = emptyMap()
        localTrack?.dispose()
        localTrack = null
        audioSource?.dispose()
        audioSource = null
        factory?.dispose()
        factory = null
        adm?.release()
        adm = null
    }

    // --- states -------------------------------------------------------------------

    private inner class Observer(private val peer: Peer) : PeerConnection.Observer {
        override fun onConnectionChange(newState: PeerConnection.PeerConnectionState) = onEngine {
            peer.connState = newState
            reportState(peer)
        }

        override fun onIceConnectionChange(newState: PeerConnection.IceConnectionState) = onEngine {
            peer.iceState = newState
            reportState(peer)
        }

        override fun onIceCandidate(candidate: IceCandidate) = onEngine {
            if (peer.closed) return@onEngine
            reports.localCandidate(peer.id, CallIceCandidate(candidate.sdp, candidate.sdpMid, candidate.sdpMLineIndex))
        }

        override fun onTrack(transceiver: RtpTransceiver) {
            val track = transceiver.receiver.track() as? VideoTrack ?: return
            onEngine { onRemoteVideo(peer, track) }
        }

        override fun onSignalingChange(state: PeerConnection.SignalingState) = Unit
        override fun onIceConnectionReceivingChange(receiving: Boolean) = Unit
        override fun onIceGatheringChange(state: PeerConnection.IceGatheringState) = Unit
        override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>) = Unit
        override fun onAddStream(stream: org.webrtc.MediaStream) = Unit
        override fun onRemoveStream(stream: org.webrtc.MediaStream) = Unit
        override fun onDataChannel(channel: org.webrtc.DataChannel) = Unit
        override fun onRenegotiationNeeded() = Unit
    }

    /** connectionState is the source; `failed` from ICE alone counts as well. */
    private fun reportState(peer: Peer) {
        if (peer.closed) return
        val ice = peer.iceState
        val state = when {
            ice == PeerConnection.IceConnectionState.FAILED -> PeerState.FAILED
            peer.connState != null -> when (peer.connState!!) {
                PeerConnection.PeerConnectionState.NEW -> PeerState.NEW
                PeerConnection.PeerConnectionState.CONNECTING -> PeerState.CONNECTING
                PeerConnection.PeerConnectionState.CONNECTED -> PeerState.CONNECTED
                PeerConnection.PeerConnectionState.DISCONNECTED -> PeerState.DISCONNECTED
                PeerConnection.PeerConnectionState.FAILED -> PeerState.FAILED
                PeerConnection.PeerConnectionState.CLOSED -> PeerState.CLOSED
            }
            else -> when (ice) {
                null, PeerConnection.IceConnectionState.NEW -> PeerState.NEW
                PeerConnection.IceConnectionState.CHECKING -> PeerState.CONNECTING
                PeerConnection.IceConnectionState.CONNECTED,
                PeerConnection.IceConnectionState.COMPLETED -> PeerState.CONNECTED
                PeerConnection.IceConnectionState.DISCONNECTED -> PeerState.DISCONNECTED
                PeerConnection.IceConnectionState.FAILED -> PeerState.FAILED
                PeerConnection.IceConnectionState.CLOSED -> PeerState.CLOSED
            }
        }
        if (state == peer.reported) return
        peer.reported = state
        Log.i(TAG, "peer ${peer.id} (${peer.kind}): $state")
        reports.peerState(peer.id, state)
    }

    // --- audio levels -------------------------------------------------------------

    private fun startLevels() {
        if (levels?.isActive == true) return
        levels = scope.launch {
            var tick = 0
            while (isActive) {
                delay(LEVEL_INTERVAL_MS)
                if (++tick % VIDEO_STATS_TICKS == 0 && videoTrack != null) logVideoStats()
                for (peer in peers.values) peer.feed?.checkStale()
                localFeed?.checkStale()
                val logIn = tick % VIDEO_STATS_TICKS == 0
                for (peer in peers.values.toList()) {
                    val pc = peer.pc ?: continue
                    pc.getStats { report ->
                        if (logIn && peer.feed?.active?.value == true) logVideoIn(peer, report)
                        onEngine { reportLevel(peer, report) }
                    }
                }
            }
        }
    }

    private fun stopLevels() {
        levels?.cancel()
        levels = null
    }

    /** What the camera's encoder really does (resolution, fps, limits), for the log. */
    private fun logVideoStats() {
        val pc = peers.values.firstOrNull { it.kind == PeerKind.PUBLISHER || it.kind == PeerKind.MESH }?.pc ?: return
        pc.getStats { report ->
            val out = report.statsMap.values.firstOrNull { it.type == "outbound-rtp" && it.members["kind"] == "video" } ?: return@getStats
            val m = out.members
            if (m["frameWidth"] == null) return@getStats
            val camera = report.statsMap.values.firstOrNull { it.type == "media-source" && it.members["kind"] == "video" }?.members
            Log.i(
                TAG,
                "video out: ${m["frameWidth"]}x${m["frameHeight"]} ${m["framesPerSecond"]} fps " +
                    "(camera ${camera?.get("width")}x${camera?.get("height")} ${camera?.get("framesPerSecond")} fps), " +
                    "${m["encoderImplementation"]}, limit ${m["qualityLimitationReason"]}, " +
                    "target ${m["targetBitrate"]}",
            )
        }
    }

    /** What we receive and how it is decoded (e.g. a shared screen in 1080p), for the log. */
    private fun logVideoIn(peer: Peer, report: RTCStatsReport) {
        val m = report.statsMap.values.firstOrNull { it.type == "inbound-rtp" && it.members["kind"] == "video" }?.members ?: return
        if (m["frameWidth"] == null) return
        Log.i(
            TAG,
            "video in ${peer.id}: ${m["frameWidth"]}x${m["frameHeight"]} ${m["framesPerSecond"]} fps, " +
                "${m["decoderImplementation"]}, dropped ${m["framesDropped"]}",
        )
    }

    private fun reportLevel(peer: Peer, report: RTCStatsReport) {
        if (peer.closed) return
        val type = if (peer.kind == PeerKind.PUBLISHER) "media-source" else "inbound-rtp"
        val level = report.statsMap.values
            .filter { it.type == type && it.members["kind"] == "audio" }
            .mapNotNull { (it.members["audioLevel"] as? Number)?.toFloat() }
            .maxOrNull() ?: return
        val value = if (peer.kind == PeerKind.PUBLISHER && !micEnabled) 0f else level.coerceIn(0f, 1f)
        reports.audioLevel(peer.id, value)
    }

    companion object {
        private const val TAG = "TalkMedia"
        private const val STREAM_ID = "talk"
        private const val LEVEL_INTERVAL_MS = 300L
        private const val VIDEO_FPS = 24
        private const val VIDEO_STATS_TICKS = 17
        private const val VIDEO_MAX_BITRATE = 600_000

        /** Also on the A13 (32 bit): the encoder scales down by itself when needed. */
        private const val VIDEO_WIDTH = 640
        private const val VIDEO_HEIGHT = 480
        @Volatile private var initialized = false

        private fun initialize(context: Context) {
            if (initialized) return
            PeerConnectionFactory.initialize(
                PeerConnectionFactory.InitializationOptions.builder(context.applicationContext)
                    .createInitializationOptions(),
            )
            initialized = true
        }
    }
}

// --- suspending SDP helpers ---------------------------------------------------------

private open class SdpAdapter : SdpObserver {
    override fun onCreateSuccess(sdp: SessionDescription) = Unit
    override fun onSetSuccess() = Unit
    override fun onCreateFailure(error: String) = Unit
    override fun onSetFailure(error: String) = Unit
}

private class SdpException(message: String) : Exception(message)

private suspend fun PeerConnection.awaitCreate(offer: Boolean, constraints: MediaConstraints): SessionDescription =
    suspendCancellableCoroutine { cont ->
        val observer = object : SdpAdapter() {
            override fun onCreateSuccess(sdp: SessionDescription) {
                if (cont.isActive) cont.resume(sdp)
            }
            override fun onCreateFailure(error: String) {
                if (cont.isActive) cont.resumeWith(Result.failure(SdpException("create ${if (offer) "offer" else "answer"}: $error")))
            }
        }
        if (offer) createOffer(observer, constraints) else createAnswer(observer, constraints)
    }

private suspend fun PeerConnection.awaitSetLocal(sdp: SessionDescription) = suspendCancellableCoroutine { cont ->
    setLocalDescription(object : SdpAdapter() {
        override fun onSetSuccess() {
            if (cont.isActive) cont.resume(Unit)
        }
        override fun onSetFailure(error: String) {
            if (cont.isActive) cont.resumeWith(Result.failure(SdpException("set local: $error")))
        }
    }, sdp)
}

private suspend fun PeerConnection.awaitSetRemote(sdp: SessionDescription) = suspendCancellableCoroutine { cont ->
    setRemoteDescription(object : SdpAdapter() {
        override fun onSetSuccess() {
            if (cont.isActive) cont.resume(Unit)
        }
        override fun onSetFailure(error: String) {
            if (cont.isActive) cont.resumeWith(Result.failure(SdpException("set remote: $error")))
        }
    }, sdp)
}

@Suppress("unused")
private val audioType = MediaStreamTrack.MediaType.MEDIA_TYPE_AUDIO
