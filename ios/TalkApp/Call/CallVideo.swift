import AVFoundation
import Observation
import QuartzCore
import SwiftUI
import WebRTC

/// The local camera (docs/calls.md "Video"): capturer → source → one track that every
/// sending peer shares. Mobile quality: about 640×480 at 24 fps.
///
/// Used only on the engine's camera queue; `start` and `switchCamera` block until the
/// capturer runs.
final class CallCamera: @unchecked Sendable {
    let track: RTCVideoTrack
    /// The front camera is used (the own preview is mirrored).
    private(set) var front = true
    /// The camera stopped on its own (taken by another app): message for the user.
    var onInterrupted: ((String) -> Void)?

    private let source: RTCVideoSource
    private let capturer: RTCVideoCapturer
    private var observer: NSObjectProtocol?

    init(factory: RTCPeerConnectionFactory) {
        source = factory.videoSource()
        source.adaptOutputFormat(toWidth: 640, height: 480, fps: 24)
        track = factory.videoTrack(with: source, trackId: "video-\(UUID().uuidString.prefix(8))")
        #if DEBUG && targetEnvironment(simulator)
        capturer = FakeVideoCapturer(delegate: source)
        #else
        capturer = RTCCameraVideoCapturer(delegate: source)
        #endif
    }

    /// Opens the camera; returns a message for the user when that is not possible.
    func start(front: Bool) -> String? {
        self.front = front
        #if DEBUG && targetEnvironment(simulator)
        (capturer as? FakeVideoCapturer)?.start(front: front)
        return nil
        #else
        guard let cam = capturer as? RTCCameraVideoCapturer else { return "Kamera nicht verfügbar" }
        guard AVCaptureDevice.authorizationStatus(for: .video) == .authorized else {
            return "Kamera in den Einstellungen erlauben, um sie im Anruf zu nutzen."
        }
        // Explicitly the wide-angle camera of the wanted side: phones also list ultra-wide,
        // tele and depth cameras, which can deliver black frames.
        let wanted: AVCaptureDevice.Position = front ? .front : .back
        let devices = RTCCameraVideoCapturer.captureDevices().filter { $0.deviceType == .builtInWideAngleCamera }
        guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: wanted)
            ?? devices.first(where: { $0.position == wanted }) ?? devices.first else {
            return "Keine Kamera gefunden"
        }
        self.front = device.position != .back
        guard let format = Self.format(for: device, preferring: cam.preferredOutputPixelFormat()) else {
            return "Kamera nicht verfügbar"
        }
        let maxFps = format.videoSupportedFrameRateRanges.map(\.maxFrameRate).max() ?? 24
        let fps = Int(min(24, maxFps))
        let done = DispatchSemaphore(value: 0)
        var failure: Error?
        cam.startCapture(with: device, format: format, fps: fps) { error in
            failure = error
            done.signal()
        }
        if done.wait(timeout: .now() + 5) == .timedOut { return "Kamera nicht verfügbar" }
        if let failure {
            Log.call.error("camera: \(failure.localizedDescription, privacy: .public)")
            return "Kamera nicht verfügbar"
        }
        if observer == nil {
            observer = NotificationCenter.default.addObserver(
                forName: AVCaptureSession.wasInterruptedNotification, object: cam.captureSession, queue: nil
            ) { [weak self] n in
                let raw = (n.userInfo?[AVCaptureSessionInterruptionReasonKey] as? NSNumber)?.intValue
                // Background and split screen pause it (we switch it off ourselves then).
                guard raw.flatMap(AVCaptureSession.InterruptionReason.init(rawValue:)) == .videoDeviceInUseByAnotherClient else { return }
                self?.onInterrupted?("Die Kamera wird von einer anderen App verwendet")
            }
        }
        let dims = CMVideoFormatDescriptionGetDimensions(format.formatDescription)
        Log.call.info("camera on: \(self.front ? "front" : "back", privacy: .public) \(dims.width)x\(dims.height)@\(fps)")
        return nil
        #endif
    }

    func stop() {
        if let observer { NotificationCenter.default.removeObserver(observer) }
        observer = nil
        #if DEBUG && targetEnvironment(simulator)
        (capturer as? FakeVideoCapturer)?.stop()
        #else
        let done = DispatchSemaphore(value: 0)
        (capturer as? RTCCameraVideoCapturer)?.stopCapture { done.signal() }
        _ = done.wait(timeout: .now() + 3)
        #endif
        Log.call.info("camera off")
    }

    /// Front ↔ back; the track stays the same, so nothing changes for the peers.
    func switchCamera() -> String? {
        let next = !front
        stop()
        return start(front: next)
    }

    #if !(DEBUG && targetEnvironment(simulator))
    /// The format closest to 640×480 that delivers 24 fps, in the capturer's pixel format.
    private static func format(for device: AVCaptureDevice, preferring pixel: FourCharCode) -> AVCaptureDevice.Format? {
        let formats = RTCCameraVideoCapturer.supportedFormats(for: device)
        func cost(_ f: AVCaptureDevice.Format) -> Int {
            let d = CMVideoFormatDescriptionGetDimensions(f.formatDescription)
            var c = abs(Int(d.width) - 640) + abs(Int(d.height) - 480)
            if CMFormatDescriptionGetMediaSubType(f.formatDescription) != pixel { c += 1 }
            if (f.videoSupportedFrameRateRanges.map(\.maxFrameRate).max() ?? 0) < 24 { c += 10_000 }
            return c
        }
        return formats.min { cost($0) < cost($1) }
    }
    #endif
}

/// Counts arriving frames of a remote track (no drawing): "frames arrive" for the 2 s rule.
final class FrameWatch: NSObject, RTCVideoRenderer, @unchecked Sendable {
    private let lock = NSLock()
    private var last: CFTimeInterval = 0
    private var size = CGSize.zero

    var lastFrame: CFTimeInterval {
        lock.lock()
        defer { lock.unlock() }
        return last
    }

    var frameSize: CGSize {
        lock.lock()
        defer { lock.unlock() }
        return size
    }

    func setSize(_ size: CGSize) {
        lock.lock()
        self.size = size
        lock.unlock()
    }

    func renderFrame(_ frame: RTCVideoFrame?) {
        guard frame != nil else { return }
        let now = CACurrentMediaTime()
        lock.lock()
        last = now
        lock.unlock()
    }
}

/// Video tracks of the running call, for the UI: our camera and one track per remote peer.
@MainActor
@Observable
final class CallVideo {
    static let shared = CallVideo()

    /// Our camera (preview), while it is on.
    private(set) var local: RTCVideoTrack?
    private(set) var localFront = true
    /// Remote video per `MediaEngine` peer id.
    private(set) var remote: [String: RTCVideoTrack] = [:]
    /// Peers whose video delivered frames within the last 2 s.
    private(set) var live: Set<String> = []
    /// Peers whose video delivered at least one frame. A shared screen sends frames only
    /// when its content changes, so it stays on the stage without the 2 s rule.
    private(set) var seen: Set<String> = []

    @ObservationIgnored private var watches: [String: FrameWatch] = [:]
    @ObservationIgnored private var timer: Timer?

    /// The camera video to show for a participant: camera on (`video`) and frames arriving.
    func track(for p: CallParticipant) -> RTCVideoTrack? {
        guard p.video, let id = p.peerId, live.contains(id) else { return nil }
        return remote[id]
    }

    /// The video of a shared screen (`CallView.screens`), once it delivered a frame.
    func screenTrack(_ s: CallScreen) -> RTCVideoTrack? {
        guard seen.contains(s.peerId) else { return nil }
        return remote[s.peerId]
    }

    func setLocal(_ track: RTCVideoTrack?, front: Bool) {
        local = track
        localFront = front
    }

    func setRemote(_ peerId: String, _ track: RTCVideoTrack) {
        removeRemote(peerId)
        let w = FrameWatch()
        track.add(w)
        watches[peerId] = w
        remote[peerId] = track
        startTimer()
    }

    func removeRemote(_ peerId: String) {
        if let t = remote.removeValue(forKey: peerId), let w = watches.removeValue(forKey: peerId) {
            t.remove(w)
        }
        live.remove(peerId)
        seen.remove(peerId)
        if remote.isEmpty { stopTimer() }
    }

    func clear() {
        for id in Array(remote.keys) { removeRemote(id) }
        local = nil
    }

    private func startTimer() {
        guard timer == nil else { return }
        let t = Timer(timeInterval: 0.5, repeats: true) { _ in
            MainActor.assumeIsolated { CallVideo.shared.tick() }
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    private func stopTimer() {
        timer?.invalidate()
        timer = nil
    }

    private func tick() {
        let now = CACurrentMediaTime()
        var next = Set<String>()
        for (id, w) in watches where now - w.lastFrame < 2 { next.insert(id) }
        let started = next.subtracting(seen)
        if !started.isEmpty { seen.formUnion(started) }
        guard next != live else { return }
        for id in next.subtracting(live) {
            let s = watches[id]?.frameSize ?? .zero
            Log.call.info("video from \(id, privacy: .public): frames \(Int(s.width))x\(Int(s.height))")
        }
        for id in live.subtracting(next) {
            Log.call.info("video from \(id, privacy: .public): no frames")
        }
        live = next
    }
}

/// A video track drawn with Metal. `fill` crops to the frame, otherwise it fits.
struct VideoView: UIViewRepresentable {
    let track: RTCVideoTrack
    var fill = true

    final class Coordinator {
        var track: RTCVideoTrack?
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> RTCMTLVideoView {
        let v = RTCMTLVideoView(frame: .zero)
        v.videoContentMode = fill ? .scaleAspectFill : .scaleAspectFit
        v.clipsToBounds = true
        v.backgroundColor = .black
        return v
    }

    func updateUIView(_ v: RTCMTLVideoView, context: Context) {
        v.videoContentMode = fill ? .scaleAspectFill : .scaleAspectFit
        let c = context.coordinator
        guard c.track !== track else { return }
        c.track?.remove(v)
        track.add(v)
        c.track = track
    }

    static func dismantleUIView(_ v: RTCMTLVideoView, coordinator: Coordinator) {
        coordinator.track?.remove(v)
        coordinator.track = nil
    }
}
