package org.sirplus.talk.call

import android.content.Context
import android.media.AudioAttributes
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import org.webrtc.IceCandidate
import org.webrtc.MediaConstraints
import org.webrtc.MediaStreamTrack
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RTCStatsReport
import org.webrtc.RtpTransceiver
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription
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
 */
class WebRtcMediaEngine(
    private val context: Context,
    private val reports: MediaReports,
    private val audioHost: CallAudioHost,
) : MediaEngine {
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

    // Video (docs/calls.md "Video"): not implemented yet, the camera stays off.
    override fun setCameraEnabled(enabled: Boolean) = Unit

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
        val f = PeerConnectionFactory.builder()
            .setOptions(PeerConnectionFactory.Options())
            .setAudioDeviceModule(module)
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
            PeerKind.PUBLISHER -> pc.addTransceiver(
                localTrack(f),
                RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.SEND_ONLY, listOf(STREAM_ID)),
            )
            // addTrack (not addTransceiver): reused when the other side sends the offer.
            PeerKind.MESH -> pc.addTrack(localTrack(f), listOf(STREAM_ID))
            // Created by the MCU's offer; answering without a track makes it recvonly.
            PeerKind.SUBSCRIBER -> Unit
        }
    }

    private fun dispose(peer: Peer) {
        peer.closed = true
        peer.queue.close()
        peer.job.cancel()
        peer.pc?.dispose()
        peer.pc = null
    }

    private fun releaseAll() {
        stopLevels()
        peers.values.forEach { dispose(it) }
        peers.clear()
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
            while (isActive) {
                delay(LEVEL_INTERVAL_MS)
                for (peer in peers.values.toList()) {
                    val pc = peer.pc ?: continue
                    pc.getStats { report -> onEngine { reportLevel(peer, report) } }
                }
            }
        }
    }

    private fun stopLevels() {
        levels?.cancel()
        levels = null
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
