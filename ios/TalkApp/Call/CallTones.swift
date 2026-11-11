import AVFoundation

/// Call tones, generated at runtime (no sound files): ringback 425 Hz 1 s on / 4 s off
/// (European style), a short rising "connected", a falling "ended" tone and a soft double
/// beep when the call starts being recorded.
///
/// Played on an own queue: preparing an audio player waits for the audio server and must
/// never block the main thread.
final class CallTones: @unchecked Sendable {
    static let shared = CallTones()

    private let queue = DispatchQueue(label: "talk.call.tones", qos: .userInitiated)
    private var player: AVAudioPlayer?
    private var playing: Tone?
    /// Debug simulator builds use a fake audio device; the host's audio is not touched.
    private let enabled: Bool = {
        #if DEBUG && targetEnvironment(simulator)
        return ProcessInfo.processInfo.environment["TALK_TONES"] == "1"
        #else
        return true
        #endif
    }()

    enum Tone {
        case ringback, connected, ended, recording
    }

    func play(_ tone: Tone) {
        guard enabled else { return }
        queue.async { self.start(tone) }
    }

    func stop(_ tone: Tone? = nil) {
        guard enabled else { return }
        queue.async { self.halt(tone) }
    }

    private func start(_ tone: Tone) {
        if playing == tone && tone == .ringback { return }
        halt(nil)
        guard let data = Self.wav(tone), let p = try? AVAudioPlayer(data: data, fileTypeHint: AVFileType.wav.rawValue) else { return }
        p.volume = tone == .ringback ? 0.35 : (tone == .recording ? 0.25 : 0.5)
        p.numberOfLoops = tone == .ringback ? -1 : 0
        p.prepareToPlay()
        p.play()
        player = p
        playing = tone
    }

    private func halt(_ tone: Tone?) {
        if let tone, tone != playing { return }
        player?.stop()
        player = nil
        playing = nil
    }

    // MARK: Synthesis

    private static let rate = 16_000.0

    /// (frequency, seconds) segments; frequency 0 is silence.
    private static func segments(_ tone: Tone) -> [(Double, Double)] {
        switch tone {
        case .ringback: return [(425, 1.0), (0, 4.0)]
        case .connected: return [(660, 0.09), (0, 0.03), (880, 0.12)]
        case .ended: return [(880, 0.09), (0, 0.03), (660, 0.09), (0, 0.03), (440, 0.16)]
        case .recording: return [(1000, 0.1), (0, 0.12), (1000, 0.1)]
        }
    }

    private static func wav(_ tone: Tone) -> Data? {
        var samples: [Int16] = []
        for (freq, secs) in segments(tone) {
            let n = Int(secs * rate)
            let fade = min(Int(0.008 * rate), n / 2)
            for i in 0..<n {
                guard freq > 0 else {
                    samples.append(0)
                    continue
                }
                // Short fades avoid clicks at the edges of each beep.
                let env = min(1, Double(min(i, n - 1 - i)) / Double(max(fade, 1)))
                let v = sin(2 * .pi * freq * Double(i) / rate) * env * 0.6
                samples.append(Int16(v * Double(Int16.max)))
            }
        }
        var data = Data()
        func u32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { data.append(contentsOf: $0) } }
        func u16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { data.append(contentsOf: $0) } }
        let bytes = UInt32(samples.count * 2)
        data.append(contentsOf: Array("RIFF".utf8)); u32(36 + bytes)
        data.append(contentsOf: Array("WAVE".utf8))
        data.append(contentsOf: Array("fmt ".utf8)); u32(16); u16(1); u16(1)
        u32(UInt32(rate)); u32(UInt32(rate) * 2); u16(2); u16(16)
        data.append(contentsOf: Array("data".utf8)); u32(bytes)
        samples.withUnsafeBytes { raw in
            for s in raw.bindMemory(to: Int16.self) { u16(UInt16(bitPattern: s)) }
        }
        return data
    }
}
