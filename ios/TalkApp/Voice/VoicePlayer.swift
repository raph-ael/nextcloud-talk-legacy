import AVFoundation
import MediaPlayer
import Observation

/// One voice message at a time, addressed by a key (message or draft). Keeps playing with
/// the screen locked (background audio mode) and shows lock-screen controls.
@MainActor
@Observable
final class VoicePlayer: NSObject, AVAudioPlayerDelegate {
    static let speeds: [Float] = [1, 1.5, 2, 0.5]
    private static let speedKey = "voice_speed"

    private(set) var key: String?
    private(set) var playing = false
    private(set) var loading = false
    private(set) var positionMs: Int64 = 0
    private(set) var durationMs: Int64 = 0
    private(set) var speed: Float
    private(set) var error: String?

    /// A message finished playing (autoplay of the next one).
    @ObservationIgnored var onEnded: ((String) -> Void)?

    @ObservationIgnored private var player: AVAudioPlayer?
    @ObservationIgnored private var ticker: Timer?
    @ObservationIgnored private var request = 0
    @ObservationIgnored private var title = ""
    @ObservationIgnored private var artist = ""

    override init() {
        let saved = UserDefaults.standard.float(forKey: Self.speedKey)
        speed = Self.speeds.contains(saved) ? saved : 1
        super.init()
        let nc = NotificationCenter.default
        nc.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] n in
            guard let raw = n.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
                  AVAudioSession.InterruptionType(rawValue: raw) == .began else { return }
            MainActor.assumeIsolated { self?.pause() }
        }
        // Headphones unplugged: pause instead of continuing through the speaker.
        nc.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { [weak self] n in
            guard let raw = n.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
                  AVAudioSession.RouteChangeReason(rawValue: raw) == .oldDeviceUnavailable else { return }
            MainActor.assumeIsolated { self?.pause() }
        }
        setUpRemoteCommands()
    }

    func isCurrent(_ key: String) -> Bool { self.key == key }

    /// Play/pause `key`; a different key starts that file (resolved lazily, e.g. download).
    func toggle(_ key: String, title: String, artist: String, resolve: @escaping () async -> String?) {
        if self.key == key, let player {
            if player.isPlaying { pause() } else { resume() }
            return
        }
        play(key, title: title, artist: artist, at: 0, resolve: resolve)
    }

    /// Jump to `fraction` of `key` (starts it when another file is loaded).
    func seek(_ key: String, fraction: Double, title: String, artist: String, resolve: @escaping () async -> String?) {
        let f = min(1, max(0, fraction))
        if self.key == key, let player, player.duration > 0 {
            player.currentTime = player.duration * f
            positionMs = Int64(player.currentTime * 1000)
            if !player.isPlaying { resume() }
            updateNowPlaying()
        } else {
            play(key, title: title, artist: artist, at: f, resolve: resolve)
        }
    }

    func cycleSpeed() {
        let i = Self.speeds.firstIndex(of: speed) ?? 0
        speed = Self.speeds[(i + 1) % Self.speeds.count]
        UserDefaults.standard.set(speed, forKey: Self.speedKey)
        player?.rate = speed
        updateNowPlaying()
    }

    /// Stops `key` (or whatever plays when nil), e.g. before its draft is deleted.
    func stop(_ key: String? = nil) {
        if let key, self.key != key { return }
        request += 1
        player?.stop()
        player = nil
        stopTicker()
        self.key = nil
        playing = false
        loading = false
        positionMs = 0
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private func play(_ key: String, title: String, artist: String, at fraction: Double, resolve: @escaping () async -> String?) {
        player?.stop()
        player = nil
        stopTicker()
        request += 1
        let my = request
        self.key = key
        self.title = title
        self.artist = artist
        playing = false
        loading = true
        positionMs = 0
        durationMs = 0
        error = nil
        Task {
            let path = await resolve()
            guard my == self.request else { return }
            guard let path else {
                self.loading = false
                self.error = "Nicht geladen – offline?"
                return
            }
            do {
                try AVAudioSession.sharedInstance().setCategory(.playback, mode: .spokenAudio)
                try AVAudioSession.sharedInstance().setActive(true)
                let p = try Self.open(path)
                p.enableRate = true
                p.rate = self.speed
                p.delegate = self
                p.prepareToPlay()
                if fraction > 0 { p.currentTime = p.duration * fraction }
                self.player = p
                self.durationMs = Int64(p.duration * 1000)
                self.loading = false
                self.resume()
            } catch {
                Log.media.error("play \(path, privacy: .public): \(error.localizedDescription, privacy: .public)")
                self.loading = false
                self.error = "Wiedergabe nicht möglich"
            }
        }
    }

    /// Opens a file by its content, not its name: the official Talk apps send AAC/MP4 named
    /// `.mp3`. Opened by URL, AVAudioPlayer trusts the extension (even over a type hint) and
    /// rejects or misreads such files – 0.07 s instead of 12 s, silence. From bytes it probes.
    private static func open(_ path: String) throws -> AVAudioPlayer {
        let data = try Data(contentsOf: URL(fileURLWithPath: path), options: .mappedIfSafe)
        if let hint = fileType(data) { return try AVAudioPlayer(data: data, fileTypeHint: hint.rawValue) }
        return try AVAudioPlayer(data: data)
    }

    private static func fileType(_ data: Data) -> AVFileType? {
        guard data.count >= 12 else { return nil }
        let b = [UInt8](data.prefix(12))
        let ascii = { (r: Range<Int>) in String(decoding: b[r], as: UTF8.self) }
        if ascii(4..<8) == "ftyp" { return .mp4 }
        if ascii(0..<4) == "RIFF", ascii(8..<12) == "WAVE" { return .wav }
        if ascii(0..<4) == "caff" { return .caf }
        if ascii(0..<4) == "FORM" { return .aiff }
        if ascii(0..<3) == "ID3" { return .mp3 }
        // MPEG audio frame sync; layer bits 00 would be AAC in ADTS.
        if b[0] == 0xFF, b[1] & 0xE0 == 0xE0, b[1] & 0x06 != 0 { return .mp3 }
        return nil
    }

    private func resume() {
        guard let player else { return }
        try? AVAudioSession.sharedInstance().setActive(true)
        player.play()
        playing = true
        startTicker()
        updateNowPlaying()
    }

    private func pause() {
        guard let player, player.isPlaying else { return }
        player.pause()
        playing = false
        positionMs = Int64(player.currentTime * 1000)
        stopTicker()
        updateNowPlaying()
    }

    private func startTicker() {
        stopTicker()
        ticker = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let p = self.player else { return }
                self.positionMs = Int64(p.currentTime * 1000)
            }
        }
    }

    private func stopTicker() {
        ticker?.invalidate()
        ticker = nil
    }

    nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        Task { @MainActor in
            guard self.player === player else { return }
            let key = self.key
            self.playing = false
            self.positionMs = 0
            player.currentTime = 0
            self.stopTicker()
            self.updateNowPlaying()
            if let key { self.onEnded?(key) }
        }
    }

    // MARK: Lock screen

    private func updateNowPlaying() {
        guard let player else { return }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = [
            MPMediaItemPropertyTitle: title,
            MPMediaItemPropertyArtist: artist,
            MPMediaItemPropertyPlaybackDuration: player.duration,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: player.currentTime,
            MPNowPlayingInfoPropertyPlaybackRate: player.isPlaying ? Double(speed) : 0,
        ]
    }

    private func setUpRemoteCommands() {
        let c = MPRemoteCommandCenter.shared()
        c.playCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.resume() }
            return .success
        }
        c.pauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.pause() }
            return .success
        }
        c.togglePlayPauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let p = self.player else { return }
                if p.isPlaying { self.pause() } else { self.resume() }
            }
            return .success
        }
        c.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let e = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            MainActor.assumeIsolated {
                guard let self, let p = self.player else { return }
                p.currentTime = e.positionTime
                self.positionMs = Int64(e.positionTime * 1000)
                self.updateNowPlaying()
            }
            return .success
        }
    }
}
