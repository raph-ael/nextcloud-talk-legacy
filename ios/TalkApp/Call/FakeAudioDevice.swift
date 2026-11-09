#if DEBUG
import AVFoundation
import WebRTC

/// Debug builds in the simulator: WebRTC's audio device without hardware. A build host
/// without anybody at the screen cannot grant the simulator microphone access, and the
/// voice-processing audio unit then aborts the app.
///
/// - Input: `TALK_FAKE_MIC` (any audio file, looped) or a beep (440 Hz, 0.5 s on / 0.5 s off).
/// - Output: pulled in real time and measured; the level is logged every second
///   ("playout level"), so tests can check that the other side is heard.
final class FakeAudioDevice: NSObject, RTCAudioDevice, @unchecked Sendable {
    private static let rate = 48_000.0
    private static let frames: UInt32 = 480 // 10 ms

    private var delegate: RTCAudioDeviceDelegate?
    private var input: [Int16] = []
    private var inputPos = 0
    private var thread: Thread?
    private let lock = NSLock()
    private var running = false
    private var recording = false
    private var playing = false
    private var sampleTime: Double = 0
    private var levelSum: Double = 0
    private var levelCount = 0
    private var maxLevel: Double = 0

    override init() {
        super.init()
        input = Self.loadInput()
    }

    private static func loadInput() -> [Int16] {
        if let path = ProcessInfo.processInfo.environment["TALK_FAKE_MIC"], !path.isEmpty,
           let samples = decode(path), !samples.isEmpty {
            Log.call.info("fake mic: \(path, privacy: .public)")
            return samples
        }
        let n = Int(rate)
        return (0..<n).map { i in
            i < n / 2 ? Int16(sin(2 * .pi * 440 * Double(i) / rate) * 8000) : 0
        }
    }

    private static func decode(_ path: String) -> [Int16]? {
        guard let file = try? AVAudioFile(forReading: URL(fileURLWithPath: path)),
              let out = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: rate, channels: 1, interleaved: true),
              let conv = AVAudioConverter(from: file.processingFormat, to: out),
              let src = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: AVAudioFrameCount(file.length)),
              (try? file.read(into: src)) != nil else { return nil }
        let cap = AVAudioFrameCount(Double(src.frameLength) * rate / file.processingFormat.sampleRate) + 1024
        guard let dst = AVAudioPCMBuffer(pcmFormat: out, frameCapacity: cap) else { return nil }
        var fed = false
        conv.convert(to: dst, error: nil) { _, status in
            if fed {
                status.pointee = .endOfStream
                return nil
            }
            fed = true
            status.pointee = .haveData
            return src
        }
        guard let ch = dst.int16ChannelData else { return nil }
        return Array(UnsafeBufferPointer(start: ch[0], count: Int(dst.frameLength)))
    }

    // MARK: RTCAudioDevice

    var deviceInputSampleRate: Double { Self.rate }
    var inputIOBufferDuration: TimeInterval { 0.01 }
    var inputNumberOfChannels: Int { 1 }
    var inputLatency: TimeInterval { 0 }
    var deviceOutputSampleRate: Double { Self.rate }
    var outputIOBufferDuration: TimeInterval { 0.01 }
    var outputNumberOfChannels: Int { 1 }
    var outputLatency: TimeInterval { 0 }
    var isInitialized: Bool { delegate != nil }
    private(set) var isPlayoutInitialized = false
    private(set) var isRecordingInitialized = false
    var isPlaying: Bool { lock.withLock { playing } }
    var isRecording: Bool { lock.withLock { recording } }

    func initialize(with delegate: RTCAudioDeviceDelegate) -> Bool {
        self.delegate = delegate
        return true
    }

    func terminateDevice() -> Bool {
        lock.withLock {
            recording = false
            playing = false
        }
        stopThreadIfIdle()
        delegate = nil
        isPlayoutInitialized = false
        isRecordingInitialized = false
        return true
    }

    func initializePlayout() -> Bool {
        isPlayoutInitialized = true
        return true
    }

    func startPlayout() -> Bool {
        lock.withLock { playing = true }
        startThread()
        Log.call.info("fake audio: playout started")
        return true
    }

    func stopPlayout() -> Bool {
        lock.withLock { playing = false }
        stopThreadIfIdle()
        return true
    }

    func initializeRecording() -> Bool {
        isRecordingInitialized = true
        return true
    }

    func startRecording() -> Bool {
        lock.withLock { recording = true }
        startThread()
        Log.call.info("fake audio: recording started")
        return true
    }

    func stopRecording() -> Bool {
        lock.withLock { recording = false }
        stopThreadIfIdle()
        return true
    }

    // MARK: Clock

    private func startThread() {
        lock.lock()
        defer { lock.unlock() }
        guard !running else { return }
        running = true
        // One thread for all callbacks, as the delegate requires.
        let t = Thread { [weak self] in self?.loop() }
        t.name = "talk.fake-audio"
        t.qualityOfService = .userInteractive
        thread = t
        t.start()
    }

    private func stopThreadIfIdle() {
        lock.withLock {
            if !recording && !playing { running = false }
        }
    }

    private func loop() {
        let period = Double(Self.frames) / Self.rate
        var next = Date().timeIntervalSince1970
        var lastLog = next
        var playBuf = [Int16](repeating: 0, count: Int(Self.frames))
        var recBuf = [Int16](repeating: 0, count: Int(Self.frames))
        while true {
            let (run, rec, play) = lock.withLock { (running, recording, playing) }
            guard run, let delegate else { break }
            var ts = AudioTimeStamp()
            ts.mSampleTime = sampleTime
            ts.mHostTime = mach_absolute_time()
            ts.mFlags = [.sampleTimeValid, .hostTimeValid]
            var flags = AudioUnitRenderActionFlags()
            if rec {
                for i in 0..<recBuf.count {
                    recBuf[i] = input[inputPos]
                    inputPos = (inputPos + 1) % input.count
                }
                recBuf.withUnsafeMutableBytes { raw in
                    var abl = AudioBufferList(
                        mNumberBuffers: 1,
                        mBuffers: AudioBuffer(mNumberChannels: 1, mDataByteSize: UInt32(raw.count), mData: raw.baseAddress)
                    )
                    _ = delegate.deliverRecordedData(&flags, &ts, 1, Self.frames, &abl, nil, nil)
                }
            }
            if play {
                playBuf.withUnsafeMutableBytes { raw in
                    var abl = AudioBufferList(
                        mNumberBuffers: 1,
                        mBuffers: AudioBuffer(mNumberChannels: 1, mDataByteSize: UInt32(raw.count), mData: raw.baseAddress)
                    )
                    _ = delegate.getPlayoutData(&flags, &ts, 0, Self.frames, &abl)
                }
                var sum = 0.0
                for s in playBuf { sum += Double(s) * Double(s) }
                let rms = sqrt(sum / Double(playBuf.count)) / Double(Int16.max)
                levelSum += rms
                levelCount += 1
                maxLevel = max(maxLevel, rms)
            }
            sampleTime += Double(Self.frames)
            let now = Date().timeIntervalSince1970
            if play && now - lastLog >= 1 {
                let avg = levelCount > 0 ? levelSum / Double(levelCount) : 0
                Log.call.info("playout level avg=\(String(format: "%.4f", avg), privacy: .public) max=\(String(format: "%.4f", self.maxLevel), privacy: .public)")
                levelSum = 0
                levelCount = 0
                maxLevel = 0
                lastLog = now
            }
            next += period
            let wait = next - Date().timeIntervalSince1970
            if wait > 0 { Thread.sleep(forTimeInterval: wait) } else if wait < -0.2 { next = Date().timeIntervalSince1970 }
        }
        lock.withLock { thread = nil }
    }
}
#endif
