import Foundation
import WebRTC

/// The core's `MediaEngine` on libwebrtc (docs/calls.md, rules 1–12 and "Video").
///
/// - Every command runs on the peer's own serial chain; an asynchronous step (offer, answer,
///   descriptions, candidates) finishes before the next command of that peer starts.
/// - Unknown or closed peer ids are ignored and never reported.
/// - One microphone track and one camera track are shared by all sending peers (publisher,
///   mesh). Every peer has an audio and a video transceiver from the start; the camera is
///   switched by setting the video sender's track, without renegotiation.
final class WebRTCMediaEngine: MediaEngine, @unchecked Sendable {
    /// Reports go here (weak: the session owns the engine through the core).
    weak var session: TalkSession?
    /// `startAudio` / `stopAudio`, on the main actor.
    var onStartAudio: ((String) -> Void)?
    var onStopAudio: ((String) -> Void)?
    /// The camera failed or was taken away (message for the user), on the main actor.
    var onCameraFailed: ((String) -> Void)?

    private let lock = NSLock()
    private var peers: [String: Peer] = [:]
    private var micEnabled = true
    private var localTrack: RTCAudioTrack?
    private var levelTimer: DispatchSourceTimer?
    private let levelQueue = DispatchQueue(label: "talk.call.levels", qos: .utility)
    /// Camera wanted by the core, and the open camera. Opened and closed on `cameraQueue`.
    private var cameraWanted = false
    private var camera: CallCamera?
    private let cameraQueue = DispatchQueue(label: "talk.call.camera", qos: .userInitiated)

    private static let factory: RTCPeerConnectionFactory = {
        RTCInitializeSSL()
        #if DEBUG && targetEnvironment(simulator)
        // No real microphone on the build host's simulator (see FakeAudioDevice).
        return RTCPeerConnectionFactory(
            encoderFactory: RTCDefaultVideoEncoderFactory(),
            decoderFactory: RTCDefaultVideoDecoderFactory(),
            audioDevice: FakeAudioDevice()
        )
        #else
        return RTCPeerConnectionFactory(
            encoderFactory: RTCDefaultVideoEncoderFactory(),
            decoderFactory: RTCDefaultVideoDecoderFactory()
        )
        #endif
    }()

    /// Must run before the first track exists: audio is driven by CallKit / `CallAudio`.
    static func prepare() {
        let s = RTCAudioSession.sharedInstance()
        s.useManualAudio = true
        s.isAudioEnabled = false
        _ = factory
    }

    // MARK: MediaEngine

    func createPeer(peerId: String, kind: PeerKind, iceServers: [CallIceServer]) {
        let peer: Peer
        lock.lock()
        if peers[peerId] != nil {
            lock.unlock()
            return
        }
        peer = Peer(id: peerId, kind: kind, engine: self)
        peers[peerId] = peer
        lock.unlock()
        peer.enqueue { [weak self] done in
            guard let self else { return done() }
            self.open(peer, iceServers: iceServers)
            done()
        }
        startLevels()
    }

    func createOffer(peerId: String, iceRestart: Bool) {
        withPeer(peerId) { peer, done in
            guard let pc = peer.pc else { return done() }
            let constraints = RTCMediaConstraints(
                mandatoryConstraints: iceRestart ? [kRTCMediaConstraintsIceRestart: kRTCMediaConstraintsValueTrue] : nil,
                optionalConstraints: nil
            )
            pc.offer(for: constraints) { [weak self] sdp, error in
                guard let self, let sdp else {
                    self?.fail(peer, "createOffer: \(error?.localizedDescription ?? "no sdp")")
                    return done()
                }
                self.setLocal(peer, pc, sdp, type: .offer, done: done)
            }
        }
    }

    func createAnswer(peerId: String) {
        withPeer(peerId) { peer, done in
            guard let pc = peer.pc else { return done() }
            pc.answer(for: RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: nil)) { [weak self] sdp, error in
                guard let self, let sdp else {
                    self?.fail(peer, "createAnswer: \(error?.localizedDescription ?? "no sdp")")
                    return done()
                }
                self.setLocal(peer, pc, sdp, type: .answer, done: done)
            }
        }
    }

    func setRemoteDescription(peerId: String, sdpType: SdpType, sdp: String) {
        withPeer(peerId) { [weak self] peer, done in
            guard let pc = peer.pc else { return done() }
            let desc = RTCSessionDescription(type: sdpType == .offer ? .offer : .answer, sdp: sdp)
            pc.setRemoteDescription(desc) { error in
                if let error {
                    self?.fail(peer, "setRemoteDescription: \(error.localizedDescription)")
                } else if sdpType == .offer, peer.kind == .mesh {
                    self?.adoptRemoteOffer(peer, pc)
                }
                done()
            }
        }
    }

    func addRemoteCandidate(peerId: String, candidate: CallIceCandidate) {
        withPeer(peerId) { [weak self] peer, done in
            guard let pc = peer.pc else { return done() }
            let c = RTCIceCandidate(sdp: candidate.candidate, sdpMLineIndex: candidate.sdpMLineIndex, sdpMid: candidate.sdpMid)
            pc.add(c) { error in
                // A candidate the MCU sent before the description, or one of an old
                // generation, is harmless; the core only needs to know about real failures.
                if let error { Log.call.info("candidate \(peer.id, privacy: .public): \(error.localizedDescription, privacy: .public)") }
                _ = self
                done()
            }
        }
    }

    func closePeer(peerId: String) {
        lock.lock()
        let peer = peers.removeValue(forKey: peerId)
        let empty = peers.isEmpty
        lock.unlock()
        guard let peer else { return }
        peer.closed = true
        peer.enqueue { done in
            peer.pc?.close()
            peer.pc = nil
            done()
        }
        DispatchQueue.main.async { CallVideo.shared.removeRemote(peerId) }
        if empty { stopLevels() }
    }

    func setMicrophoneEnabled(enabled: Bool) {
        lock.lock()
        micEnabled = enabled
        let track = localTrack
        lock.unlock()
        track?.isEnabled = enabled
    }

    /// Opens the camera and attaches its track to every sending peer, or detaches and closes
    /// it (the camera light goes off). Repeated by the core after each new sending peer.
    func setCameraEnabled(enabled: Bool) {
        lock.lock()
        cameraWanted = enabled
        lock.unlock()
        cameraQueue.async { [weak self] in self?.syncCamera() }
    }

    /// "Kamera wechseln": front ↔ back, same track.
    func switchCamera() {
        cameraQueue.async { [weak self] in
            guard let self else { return }
            self.lock.lock()
            let cam = self.camera
            self.lock.unlock()
            guard let cam else { return }
            if let error = cam.switchCamera() {
                self.cameraFailed(error)
            } else {
                let track = cam.track, front = cam.front
                DispatchQueue.main.async { CallVideo.shared.setLocal(track, front: front) }
            }
        }
    }

    func startAudio(callId: String) {
        DispatchQueue.main.async { [weak self] in self?.onStartAudio?(callId) }
    }

    func stopAudio(callId: String) {
        lock.lock()
        let rest = Array(peers.values)
        peers.removeAll()
        let track = localTrack
        localTrack = nil
        cameraWanted = false
        lock.unlock()
        cameraQueue.async { [weak self] in self?.syncCamera() }
        DispatchQueue.main.async { CallVideo.shared.clear() }
        // The core closes its peers before; anything left must not keep the microphone.
        for peer in rest {
            peer.closed = true
            peer.enqueue { done in
                peer.pc?.close()
                peer.pc = nil
                done()
            }
        }
        track?.isEnabled = false
        stopLevels()
        DispatchQueue.main.async { [weak self] in self?.onStopAudio?(callId) }
    }

    // MARK: Peers

    private func withPeer(_ id: String, _ op: @escaping (Peer, @escaping () -> Void) -> Void) {
        lock.lock()
        let peer = peers[id]
        lock.unlock()
        guard let peer else { return }
        peer.enqueue { done in
            if peer.closed { return done() }
            op(peer, done)
        }
    }

    private func microphoneTrack() -> RTCAudioTrack {
        lock.lock()
        defer { lock.unlock() }
        if let t = localTrack { return t }
        let constraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: nil)
        let source = Self.factory.audioSource(with: constraints)
        let track = Self.factory.audioTrack(with: source, trackId: "audio-\(UUID().uuidString.prefix(8))")
        track.isEnabled = micEnabled
        localTrack = track
        return track
    }

    private func open(_ peer: Peer, iceServers: [CallIceServer]) {
        let config = RTCConfiguration()
        config.iceServers = iceServers.compactMap { s in
            guard !s.urls.isEmpty else { return nil }
            return RTCIceServer(urlStrings: s.urls, username: s.username, credential: s.credential)
        }
        config.sdpSemantics = .unifiedPlan
        config.bundlePolicy = .maxBundle
        config.rtcpMuxPolicy = .require
        config.continualGatheringPolicy = .gatherContinually
        config.tcpCandidatePolicy = .enabled
        // Faster recovery after a network change.
        config.iceConnectionReceivingTimeout = 5000
        let constraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: ["DtlsSrtpKeyAgreement": "true"])
        guard let pc = Self.factory.peerConnection(with: config, constraints: constraints, delegate: peer) else {
            fail(peer, "peerConnection could not be created")
            return
        }
        peer.pc = pc
        switch peer.kind {
        case .publisher:
            addSendingTransceivers(pc, direction: .sendOnly)
        case .subscriber:
            // The MCU's offer brings the receiving transceivers itself; nothing to add.
            break
        case .mesh:
            addSendingTransceivers(pc, direction: .sendRecv)
        }
        Log.call.info("peer \(peer.id, privacy: .public) \(String(describing: peer.kind), privacy: .public) created")
    }

    /// Audio (microphone) and video (camera, if on) for a sending peer.
    private func addSendingTransceivers(_ pc: RTCPeerConnection, direction: RTCRtpTransceiverDirection) {
        let audio = RTCRtpTransceiverInit()
        audio.direction = direction
        audio.streamIds = ["talk"]
        pc.addTransceiver(with: microphoneTrack(), init: audio)
        let video = RTCRtpTransceiverInit()
        video.direction = direction
        video.streamIds = ["talk"]
        lock.lock()
        let cam = camera?.track
        lock.unlock()
        if let cam { pc.addTransceiver(with: cam, init: video) } else { pc.addTransceiver(of: .video, init: video) }
    }

    /// Mesh, the other side offered: libwebrtc never reuses transceivers made with
    /// `addTransceiver` for a remote offer, it creates receive-only ones. Those become
    /// send+receive with our tracks; ours stay unused and are stopped (they would add
    /// extra m-lines to a later offer).
    private func adoptRemoteOffer(_ peer: Peer, _ pc: RTCPeerConnection) {
        lock.lock()
        let cam = camera?.track
        lock.unlock()
        for t in pc.transceivers where !t.isStopped {
            if t.mid.isEmpty {
                t.stopInternal()
                continue
            }
            guard t.direction == .recvOnly else { continue }
            var error: NSError?
            t.setDirection(.sendRecv, error: &error)
            t.sender.streamIds = ["talk"]
            t.sender.track = t.mediaType == .audio ? microphoneTrack() : cam
        }
    }

    /// Mobile video quality: at most 600 kbit/s and 24 fps, balanced degradation.
    private func tuneVideoSenders(_ pc: RTCPeerConnection) {
        for t in pc.transceivers where t.mediaType == .video && !t.isStopped {
            guard t.direction == .sendOnly || t.direction == .sendRecv else { continue }
            let params = t.sender.parameters
            guard !params.encodings.isEmpty else { continue }
            for e in params.encodings {
                e.maxBitrateBps = 600_000
                e.maxFramerate = 24
            }
            params.degradationPreference = NSNumber(value: RTCDegradationPreference.balanced.rawValue)
            t.sender.parameters = params
        }
    }

    // MARK: Camera

    /// Brings the camera to `cameraWanted` (on `cameraQueue`).
    private func syncCamera() {
        lock.lock()
        let wanted = cameraWanted
        var cam = camera
        lock.unlock()
        if wanted {
            if cam == nil {
                let c = CallCamera(factory: Self.factory)
                if let error = c.start(front: true) {
                    cameraFailed(error)
                    return
                }
                c.onInterrupted = { [weak self] message in
                    self?.cameraQueue.async { self?.cameraFailed(message) }
                }
                lock.lock()
                camera = c
                lock.unlock()
                cam = c
                let front = c.front
                DispatchQueue.main.async { CallVideo.shared.setLocal(c.track, front: front) }
            }
            attachCamera(cam?.track)
        } else if let cam {
            lock.lock()
            camera = nil
            lock.unlock()
            attachCamera(nil)
            cam.stop()
            DispatchQueue.main.async { CallVideo.shared.setLocal(nil, front: true) }
        }
    }

    /// No camera, no permission, taken by another app: off, and the core is told.
    private func cameraFailed(_ message: String) {
        lock.lock()
        let wasWanted = cameraWanted
        cameraWanted = false
        let cam = camera
        camera = nil
        lock.unlock()
        Log.call.error("camera failed: \(message, privacy: .public)")
        attachCamera(nil)
        cam?.stop()
        DispatchQueue.main.async { CallVideo.shared.setLocal(nil, front: true) }
        guard wasWanted else { return }
        session?.mediaCameraState(on: false)
        DispatchQueue.main.async { [weak self] in self?.onCameraFailed?(message) }
    }

    /// Sets the track on the video sender of every publisher and mesh peer, in each peer's
    /// command order (after its `createPeer`).
    private func attachCamera(_ track: RTCVideoTrack?) {
        lock.lock()
        let sending = peers.values.filter { $0.kind != .subscriber }
        lock.unlock()
        for peer in sending {
            peer.enqueue { done in
                if let pc = peer.pc, !peer.closed {
                    for t in pc.transceivers where t.mediaType == .video && !t.isStopped {
                        guard t.direction == .sendOnly || t.direction == .sendRecv else { continue }
                        if t.sender.track !== track { t.sender.track = track }
                    }
                }
                done()
            }
        }
    }

    fileprivate func remoteVideo(_ peer: Peer, _ track: RTCVideoTrack) {
        guard !peer.closed else { return }
        let id = peer.id
        Log.call.info("peer \(id, privacy: .public): remote video track")
        DispatchQueue.main.async { CallVideo.shared.setRemote(id, track) }
    }

    private func setLocal(_ peer: Peer, _ pc: RTCPeerConnection, _ sdp: RTCSessionDescription, type: SdpType, done: @escaping () -> Void) {
        pc.setLocalDescription(sdp) { [weak self] error in
            guard let self else { return done() }
            if let error {
                self.fail(peer, "setLocalDescription: \(error.localizedDescription)")
            } else if !peer.closed {
                if peer.kind != .subscriber { self.tuneVideoSenders(pc) }
                self.session?.mediaLocalDescription(peerId: peer.id, sdpType: type, sdp: sdp.sdp)
            }
            done()
        }
    }

    private func fail(_ peer: Peer, _ message: String) {
        Log.call.error("peer \(peer.id, privacy: .public): \(message, privacy: .public)")
        guard !peer.closed else { return }
        session?.mediaPeerError(peerId: peer.id, message: message)
    }

    fileprivate func candidate(_ peer: Peer, _ c: RTCIceCandidate) {
        guard !peer.closed else { return }
        session?.mediaLocalCandidate(
            peerId: peer.id,
            candidate: CallIceCandidate(candidate: c.sdp, sdpMid: c.sdpMid, sdpMLineIndex: c.sdpMLineIndex)
        )
    }

    fileprivate func state(_ peer: Peer, _ state: PeerState) {
        guard !peer.closed else { return }
        Log.call.info("peer \(peer.id, privacy: .public) → \(String(describing: state), privacy: .public)")
        session?.mediaPeerState(peerId: peer.id, state: state)
    }

    // MARK: Levels

    private func startLevels() {
        lock.lock()
        defer { lock.unlock() }
        guard levelTimer == nil else { return }
        let t = DispatchSource.makeTimerSource(queue: levelQueue)
        t.schedule(deadline: .now() + .milliseconds(300), repeating: .milliseconds(300))
        t.setEventHandler { [weak self] in self?.pollLevels() }
        levelTimer = t
        t.resume()
    }

    private func stopLevels() {
        lock.lock()
        let t = levelTimer
        levelTimer = nil
        lock.unlock()
        t?.cancel()
    }

    private func pollLevels() {
        lock.lock()
        let list = Array(peers.values)
        lock.unlock()
        for peer in list {
            guard let pc = peer.pc, !peer.closed else { continue }
            pc.statistics { [weak self] report in
                guard let self, !peer.closed else { return }
                if let level = Self.level(report, sending: peer.kind == .publisher) {
                    self.session?.mediaAudioLevel(peerId: peer.id, level: level)
                }
            }
        }
    }

    /// Received level (inbound-rtp) or, for the publisher, the microphone level (media-source).
    private static func level(_ report: RTCStatisticsReport, sending: Bool) -> Float? {
        let wanted = sending ? "media-source" : "inbound-rtp"
        for stat in report.statistics.values where stat.type == wanted {
            let kind = (stat.values["kind"] as? String) ?? (stat.values["mediaType"] as? String)
            guard kind == nil || kind == "audio" else { continue }
            if let n = stat.values["audioLevel"] as? NSNumber {
                return min(1, max(0, n.floatValue))
            }
        }
        return nil
    }
}

/// One PeerConnection with its command chain and state tracking.
private final class Peer: NSObject, RTCPeerConnectionDelegate, @unchecked Sendable {
    let id: String
    let kind: PeerKind
    weak var engine: WebRTCMediaEngine?
    var pc: RTCPeerConnection?
    /// Set as soon as `closePeer` arrives: nothing is reported afterwards.
    var closed = false

    private let lock = NSLock()
    private var queue: [(@escaping () -> Void) -> Void] = []
    private var running = false
    private var ice: RTCIceConnectionState = .new
    private var conn: RTCPeerConnectionState = .new
    private var reported: PeerState?

    init(id: String, kind: PeerKind, engine: WebRTCMediaEngine) {
        self.id = id
        self.kind = kind
        self.engine = engine
    }

    /// Runs `op` after every earlier command of this peer has called its `done`.
    func enqueue(_ op: @escaping (@escaping () -> Void) -> Void) {
        lock.lock()
        queue.append(op)
        let start = !running
        running = true
        lock.unlock()
        if start { DispatchQueue.global(qos: .userInitiated).async { self.next() } }
    }

    private func next() {
        lock.lock()
        guard !queue.isEmpty else {
            running = false
            lock.unlock()
            return
        }
        let op = queue.removeFirst()
        lock.unlock()
        var finished = false
        let flag = NSLock()
        op {
            flag.lock()
            let first = !finished
            finished = true
            flag.unlock()
            if first { DispatchQueue.global(qos: .userInitiated).async { self.next() } }
        }
    }

    // MARK: State (rule 7)

    private func update() {
        let derived: PeerState
        if ice == .failed || conn == .failed {
            derived = .failed
        } else {
            switch conn {
            case .new: derived = Self.map(ice)
            case .connecting: derived = .connecting
            case .connected: derived = .connected
            case .disconnected: derived = .disconnected
            case .failed: derived = .failed
            case .closed: derived = .closed
            @unknown default: derived = Self.map(ice)
            }
        }
        lock.lock()
        let changed = reported != derived
        reported = derived
        lock.unlock()
        if changed { engine?.state(self, derived) }
    }

    private static func map(_ s: RTCIceConnectionState) -> PeerState {
        switch s {
        case .new: return .new
        case .checking: return .connecting
        case .connected, .completed: return .connected
        case .disconnected: return .disconnected
        case .failed: return .failed
        case .closed: return .closed
        case .count: return .new
        @unknown default: return .new
        }
    }

    // MARK: RTCPeerConnectionDelegate

    func peerConnection(_ peerConnection: RTCPeerConnection, didChange stateChanged: RTCSignalingState) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didAdd stream: RTCMediaStream) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didRemove stream: RTCMediaStream) {}
    func peerConnectionShouldNegotiate(_ peerConnection: RTCPeerConnection) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCIceGatheringState) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didRemove candidates: [RTCIceCandidate]) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didOpen dataChannel: RTCDataChannel) {}

    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCIceConnectionState) {
        ice = newState
        update()
    }

    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCPeerConnectionState) {
        conn = newState
        update()
    }

    func peerConnection(_ peerConnection: RTCPeerConnection, didGenerate candidate: RTCIceCandidate) {
        engine?.candidate(self, candidate)
    }

    func peerConnection(_ peerConnection: RTCPeerConnection, didAdd rtpReceiver: RTCRtpReceiver, streams mediaStreams: [RTCMediaStream]) {
        if let video = rtpReceiver.track as? RTCVideoTrack { engine?.remoteVideo(self, video) }
    }
}

#if DEBUG
extension WebRTCMediaEngine {
    nonisolated(unsafe) private static var mockCapturers: [FakeVideoCapturer] = []

    /// A moving test picture for `TALK_CALL_MOCK` (no call, no peers).
    static func mockTrack(front: Bool) -> RTCVideoTrack {
        let source = factory.videoSource()
        let capturer = FakeVideoCapturer(delegate: source)
        capturer.start(front: front)
        mockCapturers.append(capturer)
        return factory.videoTrack(with: source, trackId: "mock-\(mockCapturers.count)")
    }
}
#endif
