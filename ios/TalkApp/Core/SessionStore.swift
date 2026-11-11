import Foundation
import Network
import SwiftUI
import UIKit

enum SendStatus: Equatable {
    case sending, waiting, failed
}

/// A message as shown in the UI. Outbox messages (not yet delivered) carry a status, file
/// messages also their local file, upload progress (0…1, negative when unknown) and error.
struct UiMessage: Identifiable, Equatable {
    var msg: MessageView
    var status: SendStatus?
    var localFile: String?
    var progress: Float = -1
    var error: SendError?

    var key: String { status != nil ? "ref:\(msg.referenceId)" : "id:\(msg.id)" }
    var id: String { key }
}

struct ChatState: Equatable {
    var messages: [UiMessage] = []
    var hasMore = true
    var oldestId: Int64?
    var lastCommonRead: Int64 = 0
    var loading = true
    var loadingOlder = false
}

enum SessionPhase {
    case loading, loggedOut, ready
}

/// App-wide state holder (port of Android `SessionManager.kt`): owns the Rust session,
/// turns core events into observable state and exposes the user actions.
@MainActor
@Observable
final class SessionStore {
    static let shared = SessionStore()

    private(set) var phase: SessionPhase = .loading
    private(set) var server = ""
    private(set) var user = ""
    private(set) var rooms: [RoomView] = []
    private(set) var chats: [String: ChatState] = [:]
    private(set) var online = true
    private(set) var activeToken: String?
    /// Finished call recordings (and transcripts, summaries) waiting for "teilen"/"verwerfen".
    private(set) var recordings: [RecordingNotice] = []
    /// Notices whose share/dismiss request is running.
    private(set) var recordingBusy = Set<Int64>()
    /// Scene in the foreground (marking read, failure notifications).
    var foreground = true

    @ObservationIgnored private(set) var session: TalkSession?
    @ObservationIgnored private let bridge = EventBridge()
    @ObservationIgnored private var notifiedFailures = Set<String>()
    /// Recording notices already known; nil until the first `recordingsChanged` of a session
    /// (the starting point: banners, but no notifications).
    @ObservationIgnored private var notifiedRecordings: Set<Int64>?
    @ObservationIgnored private let pathMonitor = NWPathMonitor()
    @ObservationIgnored private var lastPathSatisfied = true
    @ObservationIgnored private var lastPathKey = ""
    /// WebRTC for calls; reports go back to the session.
    @ObservationIgnored let callMedia = WebRTCMediaEngine()
    /// Unsent text per conversation while the app runs.
    @ObservationIgnored var textDrafts: [String: String] = [:]

    @ObservationIgnored let media = MediaCache()
    @ObservationIgnored private(set) lazy var voice = VoiceManager(store: self)

    private init() {
        bridge.store = self
        // Core logs before the first session.
        CoreLog.install()
        if let dev = Self.devAccount() {
            start(dev)
        } else if let saved = Keychain.loadAccount() {
            start(saved)
        } else {
            phase = .loggedOut
        }
        // Network back or switched (Wi-Fi ↔ cellular): retry waiting messages right away,
        // reconnect the call's signaling and restart ICE.
        pathMonitor.pathUpdateHandler = { [weak self] path in
            let ok = path.status == .satisfied
            let key = path.availableInterfaces.map { "\($0.type)" }.joined(separator: ",")
            Task { @MainActor in
                guard let self else { return }
                let switched = ok && self.lastPathSatisfied && key != self.lastPathKey && !self.lastPathKey.isEmpty
                if (ok && !self.lastPathSatisfied) || switched { self.session?.networkChanged() }
                self.lastPathSatisfied = ok
                if ok { self.lastPathKey = key }
            }
        }
        pathMonitor.start(queue: DispatchQueue(label: "talk.path"))
        cleanStaging()
    }

    // MARK: - Session

    nonisolated static var dataDir: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        var dir = base.appendingPathComponent("talk", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        // Cache and outbox are rebuilt from the server; no need to put them into backups.
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? dir.setResourceValues(values)
        return dir
    }

    nonisolated static var stagingDir: URL {
        let dir = dataDir.appendingPathComponent("staging", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// Debug builds only: `TALK_DEV_SERVER` / `TALK_DEV_USER` / `TALK_DEV_PASS` sign in
    /// without the browser (simulator tests). Never stored in the Keychain.
    private static func devAccount() -> StoredAccount? {
        #if DEBUG
        let env = ProcessInfo.processInfo.environment
        if let server = env["TALK_DEV_SERVER"], let user = env["TALK_DEV_USER"], let pass = env["TALK_DEV_PASS"],
           !server.isEmpty, !user.isEmpty, !pass.isEmpty {
            let account = StoredAccount(server: server, loginName: user, appPassword: pass)
            // Push tests: keep the account so a cold start from a VoIP push finds it.
            if env["TALK_DEV_SAVE"] == "1" { Keychain.saveAccount(account) }
            return account
        }
        #endif
        return nil
    }

    func login(_ result: LoginResult) throws {
        let saved = StoredAccount(server: result.server, loginName: result.loginName, appPassword: result.appPassword)
        Keychain.saveAccount(saved)
        start(saved)
    }

    private func start(_ saved: StoredAccount) {
        let s: TalkSession
        do {
            s = try TalkSession(
                server: saved.server, user: saved.loginName, appPassword: saved.appPassword,
                platform: .ios, listener: bridge, dataDir: Self.dataDir.path
            )
        } catch {
            phase = .loggedOut
            return
        }
        session = s
        callMedia.session = s
        callMedia.onStartAudio = { _ in CallAudio.shared.startAudio() }
        callMedia.onStopAudio = { _ in CallAudio.shared.stopAudio() }
        s.setMediaEngine(media: callMedia)
        server = saved.server
        user = saved.loginName
        // Cached conversation list: shown in the first frame, before any request.
        rooms = s.rooms()
        recordings = s.recordingNotices()
        notifiedRecordings = nil
        phase = .ready
        Task {
            do {
                try await s.start()
                Push.shared.update()
                #if DEBUG
                if ProcessInfo.processInfo.environment["TALK_DEV_LOGOUT"] == "1" { self.logout() }
                #endif
            } catch CoreError.Unauthorized {
                await self.signOut(revoke: false)
            } catch {
                // Offline start: the core keeps retrying in the background.
            }
        }
        BackgroundSender.shared.sessionStarted(s)
    }

    func logout() {
        Task { await signOut(revoke: true) }
    }

    private func signOut(revoke: Bool) async {
        guard let s = session else { return }
        voice.reset()
        s.hangUp()
        if revoke { await Push.shared.unregister(s) } else { PushKey.delete() }
        session = nil
        callMedia.session = nil
        if revoke { try? await s.logout() } else { s.shutdown() }
        Keychain.deleteAccount()
        // Unsent messages belong to this account: never deliver them under another one.
        try? FileManager.default.removeItem(at: Self.dataDir)
        notifiedFailures.removeAll()
        recordings = []
        recordingBusy = []
        notifiedRecordings = nil
        textDrafts.removeAll()
        media.clear()
        rooms = []
        chats = [:]
        activeToken = nil
        server = ""
        user = ""
        online = true
        phase = .loggedOut
        Notifications.setBadge(0)
    }

    func refresh() async {
        try? await session?.refreshRooms()
    }

    func room(_ token: String) -> RoomView? {
        rooms.first { $0.token == token }
    }

    // MARK: - Core events

    fileprivate func handle(_ event: CoreEvent) {
        switch event {
        case .roomsChanged(let rooms):
            self.rooms = rooms
            Notifications.setBadge(rooms.filter { !$0.isArchived }.reduce(0) { $0 + Int($1.unreadMessages) })
        case .messagesAdded(let token, let messages):
            media.adoptLocalPreviews(messages)
            updateChat(token) { $0.messages = Self.merge($0.messages, messages) }
        case .messagesUpdated(let token, let messages):
            let byId = Dictionary(messages.map { ($0.id, $0) }, uniquingKeysWith: { _, b in b })
            updateChat(token) { c in
                c.messages = c.messages.map { m in byId[m.msg.id].map { UiMessage(msg: $0) } ?? m }
            }
        case .readStatus(let token, let lastCommonRead):
            updateChat(token) { $0.lastCommonRead = max($0.lastCommonRead, lastCommonRead) }
        case .connection(let online):
            self.online = online
        case .authFailed:
            Task { await signOut(revoke: false) }
        case .callChanged(let call):
            CallController.shared.update(call)
        case .callMissed(let token, let roomName, let callerName, let isGroup):
            Notifications.missedCall(token: token, name: isGroup || callerName.isEmpty ? roomName : callerName)
        case .outboxChanged(let token, let items):
            updateChat(token) { $0.messages = Self.withPending($0.messages, items) }
            announceFailures(token, items)
        case .recordingsChanged(let items):
            recordings = items
            // The first one after the start is the starting point; only later ids are new.
            if let known = notifiedRecordings {
                for n in items where !known.contains(n.id) { Notifications.recording(n) }
            }
            notifiedRecordings = (notifiedRecordings ?? []).union(items.map(\.id))
        }
    }

    // MARK: - Recordings

    func recordingNotices(for token: String) -> [RecordingNotice] {
        recordings.filter { $0.token == token }
    }

    /// "In Chat teilen": the core posts the file into the conversation.
    func shareRecording(_ n: RecordingNotice) {
        recordingAction(n) { try await $0.shareRecording(id: n.id) }
    }

    /// "Verwerfen" / "OK".
    func dismissRecording(_ n: RecordingNotice) {
        recordingAction(n) { try await $0.dismissRecording(id: n.id) }
    }

    private func recordingAction(_ n: RecordingNotice, _ f: @escaping (TalkSession) async throws -> Void) {
        guard let session, !recordingBusy.contains(n.id) else { return }
        recordingBusy.insert(n.id)
        Task {
            do {
                try await f(session)
                Notifications.removeRecording(n.id)
            } catch {
                Log.call.error("recording notice \(n.id): \(error.localizedDescription, privacy: .public)")
                Toast.show(CallController.message(error, fallback: "Hat nicht geklappt"))
            }
            recordingBusy.remove(n.id)
        }
    }

    private func updateChat(_ token: String, _ f: (inout ChatState) -> Void) {
        guard var chat = chats[token] else { return }
        f(&chat)
        chats[token] = chat
    }

    /// Replaces the pending bubbles of a chat by the outbox state.
    private static func withPending(_ messages: [UiMessage], _ pending: [PendingMessage]) -> [UiMessage] {
        messages.filter { $0.status == nil } + pending.map(toUi)
    }

    private static func toUi(_ p: PendingMessage) -> UiMessage {
        let status: SendStatus
        switch p.state {
        case .sending: status = .sending
        case .waiting: status = .waiting
        case .failed: status = .failed
        }
        return UiMessage(msg: p.message, status: status, localFile: p.localPath, progress: p.progress, error: p.error)
    }

    private static func merge(_ existing: [UiMessage], _ incoming: [MessageView]) -> [UiMessage] {
        var byId: [Int64: UiMessage] = [:]
        var pending: [UiMessage] = []
        for m in existing {
            if m.status != nil { pending.append(m) } else { byId[m.msg.id] = m }
        }
        for m in incoming {
            byId[m.id] = UiMessage(msg: m)
            if !m.referenceId.isEmpty { pending.removeAll { $0.msg.referenceId == m.referenceId } }
        }
        return byId.values.sorted { $0.msg.id < $1.msg.id } + pending
    }

    /// Notification for messages that finally failed while the user was not looking.
    private func announceFailures(_ token: String, _ items: [PendingMessage]) {
        let fresh = items.filter { $0.state == .failed }.map(\.message.referenceId)
            .filter { notifiedFailures.insert($0).inserted }
        guard !fresh.isEmpty else { return }
        let visible = activeToken == token && foreground
        if !visible {
            Notifications.sendFailed(token: token, room: room(token)?.name ?? "Unterhaltung")
        }
    }

    // MARK: - Chat actions

    func openRoom(_ token: String) {
        guard let s = session, activeToken != token else { return }
        if let previous = activeToken {
            voice.interrupt()
            s.closeChat(token: previous)
        }
        // Cached messages and unsent ones (both survive restarts): shown in the first frame.
        let cached = s.cachedChat(token: token)
        var chat = chats[token] ?? ChatState()
        let messages = Self.merge(chat.messages, cached.messages)
        let older = cached.oldestId != nil && (chat.oldestId == nil || cached.oldestId! < chat.oldestId!)
        chat.loading = !messages.contains { $0.status == nil }
        chat.messages = Self.withPending(messages, s.pending(token: token))
        if older {
            chat.hasMore = cached.hasMore
            chat.oldestId = cached.oldestId
        }
        chat.lastCommonRead = max(chat.lastCommonRead, cached.lastCommonRead ?? 0)
        chats[token] = chat
        activeToken = token
        voice.loadDraft(token)
        Task {
            do {
                let page = try await s.openChat(token: token)
                let room = self.room(token)
                updateChat(token) { c in
                    // Outdated cache: drop what was shown (unsent messages stay).
                    let base = page.reset ? c.messages.filter { $0.status != nil } : c.messages
                    c.messages = Self.merge(base, page.messages)
                    c.hasMore = page.hasMore
                    c.oldestId = page.oldestId
                    c.lastCommonRead = max(page.lastCommonRead ?? 0, room?.lastCommonReadMessage ?? 0)
                    c.loading = false
                }
                markVisibleRead(token)
            } catch {
                updateChat(token) { $0.loading = false }
            }
        }
    }

    func closeRoom() {
        guard let token = activeToken else { return }
        voice.interrupt()
        session?.closeChat(token: token)
        activeToken = nil
    }

    func loadOlder(_ token: String) {
        guard let s = session, let chat = chats[token], let before = chat.oldestId,
              chat.hasMore, !chat.loadingOlder, !chat.loading else { return }
        updateChat(token) { $0.loadingOlder = true }
        Task {
            do {
                let page = try await s.loadOlder(token: token, beforeId: before)
                updateChat(token) { c in
                    c.messages = Self.merge(c.messages, page.messages)
                    c.hasMore = page.hasMore
                    c.oldestId = page.oldestId ?? c.oldestId
                    c.loadingOlder = false
                }
            } catch {
                updateChat(token) { $0.loadingOlder = false }
            }
        }
    }

    /// Queues a text message in the outbox; it shows up at once and is delivered reliably.
    func send(_ token: String, text: String, replyTo: MessageView?) {
        guard let s = session else { return }
        _ = s.enqueueText(token: token, text: text, replyTo: replyTo?.toReply, referenceId: newReferenceId())
        Notifications.askOnce()
        BackgroundSender.shared.queued()
    }

    /// Sends a prepared image with an optional caption. The outbox takes over the file; the
    /// bubble shows the local image until the server message replaces it without flicker.
    func sendImage(_ token: String, image: PreparedImage, caption: String, replyTo: MessageView?) {
        guard let s = session else { return }
        let ref = newReferenceId()
        media.setLocalPreview(ref, path: image.url.path)
        do {
            _ = try s.enqueueFile(
                token: token, localPath: image.url.path, name: image.name, mime: "image/jpeg",
                width: Int64(image.width), height: Int64(image.height),
                caption: caption.trimmingCharacters(in: .whitespacesAndNewlines),
                replyTo: replyTo?.toReply, referenceId: ref
            )
        } catch {
            media.removeLocalPreview(ref)
            try? FileManager.default.removeItem(at: image.url)
            Toast.show("Bild konnte nicht gespeichert werden")
            return
        }
        Notifications.askOnce()
        BackgroundSender.shared.queued()
    }

    /// Sends a failed message again (or a waiting one right now).
    func retry(_ msg: MessageView) {
        notifiedFailures.remove(msg.referenceId)
        session?.retrySend(referenceId: msg.referenceId)
        BackgroundSender.shared.queued()
    }

    /// Drops a pending or failed message.
    func discard(_ msg: MessageView) {
        media.removeLocalPreview(msg.referenceId)
        session?.discard(referenceId: msg.referenceId)
    }

    /// Marks the newest message read while the chat is visible and the app is in front.
    func markVisibleRead(_ token: String) {
        guard let s = session, foreground, activeToken == token, let room = room(token),
              let newest = chats[token]?.messages.last(where: { $0.status == nil })?.msg else { return }
        if room.unreadMessages > 0 || newest.id > room.lastReadMessage {
            Task { try? await s.markRead(token: token, messageId: newest.id) }
        }
    }

    /// Images prepared but never handed to the outbox (process died in between).
    private func cleanStaging() {
        let dir = Self.stagingDir
        DispatchQueue.global(qos: .utility).async {
            let cutoff = Date().addingTimeInterval(-3600)
            let files = (try? FileManager.default.contentsOfDirectory(
                at: dir, includingPropertiesForKeys: [.contentModificationDateKey])) ?? []
            for f in files {
                let date = (try? f.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
                if let date, date < cutoff { try? FileManager.default.removeItem(at: f) }
            }
        }
    }
}

/// Receives core events on a Rust thread and forwards them to the main actor.
private final class EventBridge: CoreListener, @unchecked Sendable {
    weak var store: SessionStore?

    func onEvent(event: CoreEvent) {
        Task { @MainActor [weak self] in
            self?.store?.handle(event)
        }
    }
}
