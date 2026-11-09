import AVFoundation
import Observation
import UIKit

/// A recording in progress. `locked`: hands-free (slid up), else the finger holds the mic.
struct RecordingUi: Equatable {
    let token: String
    var durationMs: Int64 = 0
    /// Last levels 0…1, oldest first.
    var levels: [Float] = []
    var locked = false
    /// Close to the one-hour limit.
    var warn = false
}

/// Voice messages (port of Android `VoiceManager.kt`): Signal's hold / slide-to-cancel /
/// slide-up-to-lock recording, drafts that survive interruptions and restarts, playback.
///
/// Like Signal, a recording stops and stays as draft when the app leaves the foreground
/// (screen off, app switch, call), audio is interrupted or one hour is reached; the screen
/// stays on while recording.
@MainActor
@Observable
final class VoiceManager {
    static let levelBars = 40
    static let warnBeforeMs: Int64 = 5 * 60 * 1000
    /// Free space needed to start (an hour is ~29 MB).
    static let minFreeBytes: Int64 = 40 * 1024 * 1024
    /// Received voice messages up to this size are fetched right away (waveform).
    static let autoFetchBytes: Int64 = 15 * 1024 * 1024

    private(set) var recording: RecordingUi?
    /// Unsent recording per conversation (key absent: not loaded yet).
    private(set) var drafts: [String: VoiceDraft] = [:]
    @ObservationIgnored private var loadedDrafts = Set<String>()

    let player = VoicePlayer()

    @ObservationIgnored private weak var store: SessionStore?
    @ObservationIgnored private var core: VoiceRecording?
    @ObservationIgnored private var capture: VoiceCapture?
    @ObservationIgnored private let maxMs = Int64(voiceMaxDurationMs())
    @ObservationIgnored private var infos: [String: AudioInfo] = [:]

    init(store: SessionStore) {
        self.store = store
        player.onEnded = { [weak self] key in self?.playNext(after: key) }
    }

    private var session: TalkSession? { store?.session }

    func draft(_ token: String) -> VoiceDraft? { drafts[token] }

    // MARK: Recording

    /// Asks for the microphone (first time) – `true` once allowed.
    func ensurePermission() async -> Bool {
        switch AVAudioApplication.shared.recordPermission {
        case .granted: return true
        case .denied:
            Toast.show("Mikrofon in den Einstellungen erlauben, um Sprachnachrichten aufzunehmen.")
            return false
        default:
            return await AVAudioApplication.requestRecordPermission()
        }
    }

    var permissionGranted: Bool { AVAudioApplication.shared.recordPermission == .granted }

    /// Starts recording for `token` (permission must be granted). `false`: not started.
    func start(_ token: String) -> Bool {
        guard core == nil, let s = session else { return false }
        let free = (try? SessionStore.dataDir.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey]))?
            .volumeAvailableCapacityForImportantUsage ?? Int64.max
        if free < Self.minFreeBytes {
            Toast.show("Zu wenig freier Speicher für eine Aufnahme.")
            return false
        }
        if VoiceCapture.callActive() {
            Toast.show("Während eines Anrufs kann nicht aufgenommen werden.")
            return false
        }
        player.stop()
        do {
            try VoiceCapture.prepareSession()
        } catch {
            Toast.show("Das Mikrofon lässt sich nicht öffnen.")
            return false
        }
        let rate = VoiceCapture.inputSampleRate()
        let rec: VoiceRecording
        do {
            rec = try s.startVoice(token: token, sampleRate: UInt32(rate))
        } catch {
            Toast.show("Aufnahme nicht möglich: \(error.localizedDescription)")
            return false
        }
        let cap = VoiceCapture(recording: rec, sampleRate: Double(UInt32(rate)))
        cap.onStatus = { [weak self] st in self?.status(st) }
        cap.onInterrupted = { [weak self] notice in
            Task { await self?.stopToDraft(notice: notice) }
        }
        guard cap.start() else {
            rec.cancel()
            Toast.show("Das Mikrofon lässt sich nicht öffnen.")
            return false
        }
        core = rec
        capture = cap
        recording = RecordingUi(token: token)
        Log.voice.info("recording at \(Int(rate)) Hz")
        UIApplication.shared.isIdleTimerDisabled = true
        Haptics.tap()
        return true
    }

    private func status(_ st: RecordingStatus) {
        guard var r = recording else { return }
        r.durationMs = Int64(st.durationMs)
        r.levels.append(st.level)
        if r.levels.count > Self.levelBars { r.levels.removeFirst(r.levels.count - Self.levelBars) }
        r.warn = r.durationMs >= maxMs - Self.warnBeforeMs
        recording = r
    }

    func lock() {
        recording?.locked = true
        Haptics.light()
    }

    private func endCapture() -> (VoiceRecording, VoiceCapture?, String)? {
        guard let rec = core, let token = recording?.token else { return nil }
        let cap = capture
        core = nil
        capture = nil
        UIApplication.shared.isIdleTimerDisabled = false
        return (rec, cap, token)
    }

    /// Ends the recording as the chat's draft. Returns it (`nil`: shorter than a second).
    @discardableResult
    func stopToDraft(notice: String? = nil) async -> VoiceDraft? {
        guard let (rec, cap, token) = endCapture() else { return nil }
        let draft: VoiceDraft? = await Task.detached {
            cap?.stop()
            do {
                return try rec.finish()
            } catch {
                Log.voice.error("finish failed: \(error.localizedDescription, privacy: .public)")
                return nil
            }
        }.value
        Log.voice.info("stopped: draft \(draft.map { "\($0.durationMs) ms" } ?? "none", privacy: .public)")
        recording = nil
        drafts[token] = draft
        loadedDrafts.insert(token)
        if let notice, draft != nil { Toast.show(notice) }
        return draft
    }

    /// Fire-and-forget `stopToDraft` (app left the foreground, chat closed).
    func interrupt() {
        if core != nil { Task { await stopToDraft() } }
    }

    func cancel() {
        guard let (rec, cap, _) = endCapture() else { return }
        recording = nil
        Haptics.light()
        Task.detached {
            cap?.stop()
            rec.cancel()
        }
    }

    /// Released finger or send button of the locked bar: stop and send.
    func sendRecording(replyTo: MessageView?, onSent: @escaping () -> Void) {
        guard let token = recording?.token else { return }
        Task {
            if await stopToDraft() == nil {
                Toast.show("Zum Aufnehmen gedrückt halten")
            } else {
                sendDraft(token, replyTo: replyTo, onSent: onSent)
            }
        }
    }

    func loadDraft(_ token: String) {
        guard recording?.token != token, let s = session else { return }
        Task.detached {
            let d = s.voiceDraft(token: token)
            await MainActor.run {
                self.drafts[token] = d
                self.loadedDrafts.insert(token)
            }
        }
    }

    func discardDraft(_ token: String) {
        if let d = drafts[token] { player.stop(draftKey(d)) }
        drafts[token] = nil
        guard let s = session else { return }
        Task.detached { s.discardVoiceDraft(token: token) }
    }

    func sendDraft(_ token: String, replyTo: MessageView?, onSent: @escaping () -> Void = {}) {
        guard let s = session else { return }
        if let d = drafts[token] { player.stop(draftKey(d)) }
        do {
            _ = try s.sendVoiceDraft(token: token, name: Format.voiceFileName(), replyTo: replyTo?.toReply, referenceId: newReferenceId())
        } catch {
            Toast.show("Senden nicht möglich: \(error.localizedDescription)")
            return
        }
        drafts[token] = nil
        Notifications.askOnce()
        BackgroundSender.shared.queued()
        onSent()
    }

    /// Sign-out: nothing of this account may keep running.
    func reset() {
        cancel()
        player.stop()
        drafts.removeAll()
        loadedDrafts.removeAll()
        infos.removeAll()
    }

    // MARK: Playback

    func draftKey(_ d: VoiceDraft) -> String { "draft:\(d.id)" }

    func messageKey(_ m: UiMessage) -> String { m.key }

    /// Local file of a message's audio: the outbox copy while pending, else downloaded once.
    func localFile(_ m: UiMessage) async -> String? {
        guard let a = m.msg.attachment else { return nil }
        if let local = m.localFile, m.status != nil || a.id.isEmpty { return local }
        guard let s = session else { return nil }
        do {
            return try await s.fetchFile(path: a.path, size: a.size)
        } catch {
            Log.media.error("fetch \(a.path, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    /// Duration and waveform (cached in memory; the core caches on disk).
    func info(_ key: String, path: String) async -> AudioInfo? {
        if let hit = infos[key] { return hit }
        guard let s = session else { return nil }
        do {
            let i = try await s.audioInfo(localPath: path)
            infos[key] = i
            return i
        } catch {
            Log.media.error("audio info \(path, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
    }

    func cachedInfo(_ key: String) -> AudioInfo? { infos[key] }

    private func author(_ m: UiMessage) -> String { m.msg.author.isEmpty || m.msg.isOwn ? "Du" : m.msg.author }

    func toggle(_ m: UiMessage) {
        player.toggle(messageKey(m), title: "Sprachnachricht", artist: author(m)) { [weak self] in await self?.localFile(m) }
    }

    func seek(_ m: UiMessage, fraction: Double) {
        player.seek(messageKey(m), fraction: fraction, title: "Sprachnachricht", artist: author(m)) { [weak self] in
            await self?.localFile(m)
        }
    }

    func toggleDraft(_ d: VoiceDraft) {
        let path = d.path
        player.toggle(draftKey(d), title: "Entwurf", artist: "Sprachnachricht") { path }
    }

    func seekDraft(_ d: VoiceDraft, fraction: Double) {
        let path = d.path
        player.seek(draftKey(d), fraction: fraction, title: "Entwurf", artist: "Sprachnachricht") { path }
    }

    /// After a voice message ends, the next newer one of the open chat follows.
    private func playNext(after key: String) {
        guard let store, let token = store.activeToken, let messages = store.chats[token]?.messages,
              let i = messages.firstIndex(where: { messageKey($0) == key }) else { return }
        guard let next = messages[(i + 1)...].first(where: { $0.status == nil && $0.msg.attachment?.voiceMessage == true })
        else { return }
        toggle(next)
    }
}
