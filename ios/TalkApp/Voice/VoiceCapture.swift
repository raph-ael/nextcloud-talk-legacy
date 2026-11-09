import AVFoundation
import CallKit

/// Microphone → 16-bit mono PCM → the core's MP3 recorder, in blocks of about 100 ms.
/// The core writes every block to disk at once, so a crash or kill keeps what was recorded.
///
/// Like Signal, the recording stops (and becomes a draft) when audio is interrupted (call,
/// Siri, alarm), the input route goes away, an error occurs or one hour is reached.
final class VoiceCapture: @unchecked Sendable {
    /// Called on the main queue.
    var onStatus: ((RecordingStatus) -> Void)?
    /// Called on the main queue with a notice for the user (nil: silent).
    var onInterrupted: ((String?) -> Void)?

    private let recording: VoiceRecording
    private let engine = AVAudioEngine()
    private let queue = DispatchQueue(label: "talk.voice.capture", qos: .userInitiated)
    private var converter: AVAudioConverter?
    private var outFormat: AVAudioFormat?
    private var observers: [NSObjectProtocol] = []
    private var stopped = false
    private var fake: FakeMic?

    /// Sample rate the core recording was created with; input is converted to it.
    private let sampleRate: Double

    init(recording: VoiceRecording, sampleRate: Double) {
        self.recording = recording
        self.sampleRate = sampleRate
    }

    /// The hardware input rate, known before the core recording is created.
    static func inputSampleRate() -> Double {
        #if DEBUG
        if let path = ProcessInfo.processInfo.environment["TALK_FAKE_MIC"], !path.isEmpty,
           let file = try? AVAudioFile(forReading: URL(fileURLWithPath: path)) {
            return file.processingFormat.sampleRate
        }
        #endif
        let rate = AVAudioSession.sharedInstance().sampleRate
        return rate > 0 ? rate : 48_000
    }

    /// A phone call is running: the microphone belongs to it.
    static func callActive() -> Bool {
        CXCallObserver().calls.contains { !$0.hasEnded }
    }

    static func prepareSession() throws {
        let s = AVAudioSession.sharedInstance()
        try s.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker, .allowBluetoothHFP])
        try s.setActive(true)
    }

    func start() -> Bool {
        #if DEBUG
        if let path = ProcessInfo.processInfo.environment["TALK_FAKE_MIC"], !path.isEmpty {
            guard let fake = FakeMic(path: path, sampleRate: sampleRate, queue: queue, push: { [weak self] data in self?.push(data) })
            else { return false }
            self.fake = fake
            fake.start()
            observe()
            return true
        }
        #endif
        let input = engine.inputNode
        let inFormat = input.outputFormat(forBus: 0)
        guard inFormat.sampleRate > 0, inFormat.channelCount > 0,
              let out = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: sampleRate, channels: 1, interleaved: true),
              let conv = AVAudioConverter(from: inFormat, to: out) else { return false }
        outFormat = out
        converter = conv
        let block = AVAudioFrameCount(inFormat.sampleRate / 10)
        input.installTap(onBus: 0, bufferSize: block, format: inFormat) { [weak self] buffer, _ in
            self?.queue.async { self?.convert(buffer) }
        }
        do {
            engine.prepare()
            try engine.start()
        } catch {
            input.removeTap(onBus: 0)
            return false
        }
        observe()
        return true
    }

    func stop() {
        queue.sync {
            stopped = true
        }
        fake?.stop()
        if engine.isRunning {
            engine.inputNode.removeTap(onBus: 0)
            engine.stop()
        }
        observers.forEach(NotificationCenter.default.removeObserver)
        observers.removeAll()
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private func observe() {
        let nc = NotificationCenter.default
        observers.append(nc.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] n in
            guard let raw = n.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
                  AVAudioSession.InterruptionType(rawValue: raw) == .began else { return }
            self?.interrupt("Aufnahme unterbrochen – als Entwurf gespeichert.")
        })
        observers.append(nc.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { [weak self] n in
            guard let raw = n.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
                  AVAudioSession.RouteChangeReason(rawValue: raw) == .oldDeviceUnavailable else { return }
            self?.interrupt("Mikrofon getrennt – als Entwurf gespeichert.")
        })
        observers.append(nc.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification, object: nil, queue: .main) { [weak self] _ in
            self?.interrupt(nil)
        })
    }

    private func interrupt(_ notice: String?) {
        DispatchQueue.main.async { self.onInterrupted?(notice) }
    }

    /// On `queue`.
    private func convert(_ buffer: AVAudioPCMBuffer) {
        guard !stopped, let converter, let outFormat else { return }
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * outFormat.sampleRate / buffer.format.sampleRate) + 32
        guard let out = AVAudioPCMBuffer(pcmFormat: outFormat, frameCapacity: capacity) else { return }
        var fed = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if fed {
                status.pointee = .noDataNow
                return nil
            }
            fed = true
            status.pointee = .haveData
            return buffer
        }
        guard error == nil, out.frameLength > 0, let samples = out.int16ChannelData else {
            Log.voice.error("convert: \(error?.localizedDescription ?? "no frames", privacy: .public)")
            return
        }
        push(Data(bytes: samples[0], count: Int(out.frameLength) * 2))
    }

    /// On `queue`: little-endian Int16 mono (iOS is little-endian).
    private func push(_ pcm: Data) {
        guard !stopped else { return }
        do {
            let status = try recording.push(pcm: pcm)
            DispatchQueue.main.async { self.onStatus?(status) }
            if status.limitReached {
                stopped = true
                interrupt("Maximale Länge erreicht – als Entwurf gespeichert.")
            }
        } catch {
            Log.voice.error("push failed: \(error.localizedDescription, privacy: .public)")
            // Disk full or similar: what was written stays as draft.
            stopped = true
            interrupt("Aufnahme gestoppt: Speichern nicht möglich.")
        }
    }
}

#if DEBUG
/// Debug builds: `TALK_FAKE_MIC=<audio file>` replaces the microphone (looped, real time).
private final class FakeMic: @unchecked Sendable {
    let sampleRate: Double
    private let file: AVAudioFile
    private let queue: DispatchQueue
    private let push: (Data) -> Void
    private var timer: DispatchSourceTimer?
    private let converter: AVAudioConverter
    private let outFormat: AVAudioFormat

    init?(path: String, sampleRate: Double, queue: DispatchQueue, push: @escaping (Data) -> Void) {
        guard let file = try? AVAudioFile(forReading: URL(fileURLWithPath: path)),
              let out = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: sampleRate, channels: 1, interleaved: true),
              let conv = AVAudioConverter(from: file.processingFormat, to: out) else { return nil }
        self.file = file
        self.queue = queue
        self.push = push
        self.converter = conv
        self.outFormat = out
        self.sampleRate = file.processingFormat.sampleRate
    }

    /// Wall-clock based: late timer ticks catch up, so the file is as long as the recording.
    private var started: UInt64 = 0
    private var delivered: Int64 = 0

    func start() {
        started = DispatchTime.now().uptimeNanoseconds
        let t = DispatchSource.makeTimerSource(queue: queue)
        t.schedule(deadline: .now(), repeating: .milliseconds(100))
        t.setEventHandler { [weak self] in self?.tick() }
        t.resume()
        timer = t
    }

    func stop() {
        timer?.cancel()
        timer = nil
    }

    private func tick() {
        let elapsed = Double(DispatchTime.now().uptimeNanoseconds - started) / 1e9
        let due = Int64(elapsed * sampleRate) - delivered
        guard due > 0 else { return }
        let frames = AVAudioFrameCount(min(due, Int64(sampleRate)))
        let outFrames = AVAudioFrameCount(Double(frames) * outFormat.sampleRate / sampleRate) + 64
        guard let input = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: frames),
              let out = AVAudioPCMBuffer(pcmFormat: outFormat, frameCapacity: outFrames) else { return }
        do {
            try file.read(into: input, frameCount: frames)
            // End of file: loop; what is missing follows on the next tick.
            if input.frameLength < frames { file.framePosition = 0 }
        } catch {
            file.framePosition = 0
            return
        }
        guard input.frameLength > 0 else { return }
        delivered += Int64(input.frameLength)
        var fed = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if fed {
                status.pointee = .noDataNow
                return nil
            }
            fed = true
            status.pointee = .haveData
            return input
        }
        guard error == nil, out.frameLength > 0, let samples = out.int16ChannelData else {
            Log.voice.error("fake mic convert: \(error?.localizedDescription ?? "no frames", privacy: .public) in=\(input.frameLength)")
            return
        }
        push(Data(bytes: samples[0], count: Int(out.frameLength) * 2))
    }
}
#endif
