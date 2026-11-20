import Foundation
import ImageIO
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
    /// A window into the history (docs/search.md "Chat an einer Nachricht öffnen"): newer
    /// messages exist below the list, `loadNewer(newestId)` fetches them.
    var hasNewer = false
    var newestId: Int64?
    var loadingNewer = false
    /// Live messages that came while the list is a window: shown once it reaches the end.
    var held: [MessageView] = []
    /// Opened at this message (search hit): the list positions there, not at the end.
    var openAt: Int64?
    /// The window to open with is on its way; the chat's own page stays hidden.
    var opening = false
    /// Messages that mention the user and were not seen yet, ascending: the "@" button
    /// (docs/mentions.md "„@“-Knopf im Chat").
    var mentions: [Int64] = []
    /// Seen once, never back into `mentions` while the chat is open.
    var seenMentions: Set<Int64> = []

    /// New messages from others that came while the list is a window ("Zum Ende springen").
    var heldUnread: Int { held.filter { !$0.isOwn && $0.kind != .system }.count }
}

/// What the input field of a chat holds while it is not sent: the text and the mentions
/// picked for it (docs/mentions.md "Entwurf").
struct TextDraft: Equatable {
    var text = ""
    var mentions: [MentionPick] = []
}

/// Scroll the open chat to a message and light it up (search hit, quote).
struct ChatFocus: Equatable {
    let token: String
    let key: String
    let serial: Int
}

/// "N ungelesene Nachrichten" of the open chat (docs/chat.md, "Ungelesen-Linie"): taken when
/// the chat opens, before anything moves the read marker, and kept until we send or leave.
/// Place and number follow from the loaded messages (`counted`), so a stale chat list or a
/// message arriving twice cannot make them wrong.
struct UnreadLine: Equatable {
    let token: String
    /// Messages after this id count unless they are ours or system messages.
    var from: Int64
    /// `unreadMessages` of the room at the snapshot (0 for a line made while open).
    var base: Int
    /// The server page is in and older pages are loaded up to the first unread (at most 10):
    /// the list may position.
    var settled = false

    func counts(_ m: UiMessage) -> Bool {
        m.status == nil && m.msg.id > from && !m.msg.isOwn && m.msg.kind != .system
    }

    /// The message the line goes above and the number on it; nil while none counts.
    func place(in messages: [UiMessage]) -> (row: UiMessage, count: Int)? {
        guard let first = messages.first(where: counts) else { return nil }
        return (first, max(base, messages.reduce(0) { $0 + (counts($1) ? 1 : 0) }))
    }
}

enum SessionPhase {
    case loading, loggedOut, ready
}

/// One account's running session (docs/accounts.md "Laufzeit"): a started `TalkSession`
/// with its own listener that knows the account id. Only the active one fills the UI; the
/// others deliver the badge, calls, notifications and `authFailed`.
@MainActor
final class AccountRuntime {
    fileprivate(set) var account: Account
    let session: TalkSession
    /// Avatars and previews of this account (tokens and file ids of servers overlap).
    let media: MediaCache
    fileprivate let bridge: EventBridge
    /// The last conversation list (inactive accounts: for the call screens).
    fileprivate(set) var rooms: [RoomView]
    /// docs/connection.md: a session starts as `connecting`, the event reports changes.
    fileprivate(set) var connection: ConnectionState
    fileprivate var notifiedFailures = Set<String>()
    /// Recording notices already known; nil until the first `recordingsChanged` of a session
    /// (the starting point: banners, but no notifications).
    fileprivate var notifiedRecordings: Set<Int64>?
    /// Push registration (`Push`), per account.
    var registering = false
    var registerAgain = false

    fileprivate init(account: Account, session: TalkSession, bridge: EventBridge) {
        self.account = account
        self.session = session
        self.bridge = bridge
        media = MediaCache(session: session)
        rooms = session.rooms()
        connection = session.connectionState()
    }
}

/// App-wide state holder (port of Android `SessionManager.kt`): owns one runtime per account
/// (docs/accounts.md), turns the active one's core events into observable state and exposes
/// the user actions. Everything below "Session" works on the active account.
@MainActor
@Observable
final class SessionStore {
    static let shared = SessionStore()

    private(set) var phase: SessionPhase = .loading
    private(set) var server = ""
    /// The active account's name (`displayName()`).
    private(set) var user = ""
    /// Our user id (own avatar in the settings).
    private(set) var userId = ""
    /// Started accounts in the order of sign-in.
    private(set) var accounts: [Account] = []
    private(set) var activeId: String?
    /// `badge()` per account, after every `RoomsChanged`.
    private(set) var badges: [String: BadgeView] = [:]
    /// The login screen for another account is shown ("Konto hinzufügen").
    var adding = false
    private(set) var rooms: [RoomView] = []
    /// Bumped on every `RoomsChanged`: a collapsed section leaves `rooms` equal, and
    /// Observation skips equal assignments, so the sections read this instead.
    private(set) var roomsEvents = 0
    private(set) var chats: [String: ChatState] = [:]
    /// The active account's connection (docs/connection.md); error texts read `.offline`.
    private(set) var connection: ConnectionState = .connecting
    /// What the header shows: `connection` once it held 500 ms, `online` at once.
    private(set) var connectionShown: ConnectionState = .online
    @ObservationIgnored private var connectionDelay: Task<Void, Never>?
    private(set) var activeToken: String?
    private(set) var unreadLine: UnreadLine?
    /// Bumped whenever the open chat is opened anew in place: its list starts over (position).
    private(set) var openSerial = 0
    /// The last jump to a message in the open chat (docs/search.md).
    private(set) var focus: ChatFocus?
    @ObservationIgnored private var focusSerial = 0
    /// The open chat's list shows its newest message; until then nothing is marked read
    /// (opening at the unread line must not read what is below it).
    @ObservationIgnored private var chatAtBottom = true
    /// The group settings of the active chat are pushed on top of it (docs/groups.md).
    private(set) var groupInfoOpen = false
    /// The open chat came from a tapped mention in this chat: back leads there
    /// (docs/mentions.md "Antippen").
    private(set) var chatBack: String?
    /// Pages pushed on the conversation list while no chat is open: the settings
    /// (docs/settings.md) and group settings opened from the chat menu. Opening a chat
    /// replaces them.
    var listPath: [Route] = []
    /// Bumped per conversation on `participantsChanged`: open member lists reload.
    private(set) var participantsVersion: [String: Int] = [:]
    /// Finished call recordings (and transcripts, summaries) waiting for "teilen"/"verwerfen".
    private(set) var recordings: [RecordingNotice] = []
    /// Notices whose share/dismiss request is running.
    private(set) var recordingBusy = Set<Int64>()
    /// Scene in the foreground (marking read, failure notifications).
    var foreground = true

    /// The active account's session.
    @ObservationIgnored private(set) var session: TalkSession?
    @ObservationIgnored private var runtimes: [String: AccountRuntime] = [:]
    /// `TALK_DEV_*` accounts: they run outside the list and write nothing into it.
    @ObservationIgnored private var devRun = false
    /// Groups we are leaving or deleting ourselves: no "nicht mehr in dieser Gruppe".
    @ObservationIgnored private var leaving = Set<String>()
    @ObservationIgnored private let pathMonitor = NWPathMonitor()
    @ObservationIgnored private var lastPathSatisfied = true
    @ObservationIgnored private var lastPathKey = ""
    /// WebRTC for calls, one per process, handed to every runtime; reports go to the session
    /// of the call (`CallController`).
    @ObservationIgnored let callMedia = WebRTCMediaEngine()
    /// Unsent text per account and conversation while the app runs (`textDraft`).
    @ObservationIgnored private var textDrafts: [String: TextDraft] = [:]
    @ObservationIgnored private let noMedia = MediaCache(session: nil)

    @ObservationIgnored private(set) lazy var voice = VoiceManager(store: self)

    private init() {
        // Core logs before the first session.
        CoreLog.install()
        callMedia.onStartAudio = { _ in CallAudio.shared.startAudio() }
        callMedia.onStopAudio = { _ in CallAudio.shared.stopAudio() }
        callMedia.onCameraFailed = { CallController.shared.cameraFailed($0) }
        // Started in the background (a push, a notification action): nothing is seen.
        foreground = UIApplication.shared.applicationState != .background
        if let devs = Self.devAccounts() {
            devRun = true
            for (a, password) in devs {
                if makeRuntime(a, password: password) != nil { accounts.append(a) }
            }
            if let first = accounts.first { activate(first.id) } else { phase = .loggedOut }
        } else {
            loadAccounts()
            retryRevokes()
        }
        Notifications.registerCategories()
        // Started before the first unlock (push after a restart): the Keychain could not be
        // read, the accounts start once it can.
        NotificationCenter.default.addObserver(
            forName: UIApplication.protectedDataDidBecomeAvailableNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !self.devRun else { return }
                self.loadAccounts()
            }
        }
        // Network back or switched (Wi-Fi ↔ cellular): retry waiting messages right away,
        // reconnect the call's signaling and restart ICE. No network at all: the header says
        // so at once (docs/connection.md), calls and the outbox stay untouched.
        pathMonitor.pathUpdateHandler = { [weak self] path in
            let ok = path.status == .satisfied
            let lost = path.status == .unsatisfied
            let key = path.availableInterfaces.map { "\($0.type)" }.joined(separator: ",")
            Task { @MainActor in
                guard let self else { return }
                let switched = ok && self.lastPathSatisfied && key != self.lastPathKey && !self.lastPathKey.isEmpty
                if (ok && !self.lastPathSatisfied) || switched {
                    for rt in self.allRuntimes { rt.session.networkChanged() }
                } else if lost {
                    for rt in self.allRuntimes { rt.session.networkLost() }
                }
                self.lastPathSatisfied = ok
                if ok { self.lastPathKey = key }
            }
        }
        pathMonitor.start(queue: DispatchQueue(label: "talk.path"))
        cleanStaging()
    }

    // MARK: - Session

    /// A data folder below Application Support (`talk`, `accounts/<id>`).
    nonisolated static func dataDir(_ dir: String) -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        var url = base.appendingPathComponent(dir, isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        // Cache and outbox are rebuilt from the server; no need to put them into backups.
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? url.setResourceValues(values)
        return url
    }

    /// The first account's folder: staging and the free-space check.
    nonisolated static var dataDir: URL { dataDir(legacyDir) }

    /// Data folder of the account taken over from the earlier version.
    nonisolated static let legacyDir = "talk"

    nonisolated static var stagingDir: URL {
        let dir = dataDir.appendingPathComponent("staging", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// Debug builds only: `TALK_DEV_SERVER` / `TALK_DEV_USER` / `TALK_DEV_PASS` (and the same
    /// with `_2` for a second account) sign in without the browser (simulator tests). Such a
    /// run starts only these accounts, never the stored ones, and writes nothing into the
    /// list. `TALK_DEV_SAVE=1` (push tests) stores them as ordinary accounts instead, so a
    /// cold start from a VoIP push finds them.
    private static func devAccounts() -> [(Account, String)]? {
        #if DEBUG
        let env = ProcessInfo.processInfo.environment
        var list: [(Account, String)] = []
        for (n, suffix) in ["", "_2"].enumerated() {
            guard let server = env["TALK_DEV_SERVER\(suffix)"], let user = env["TALK_DEV_USER\(suffix)"],
                  let pass = env["TALK_DEV_PASS\(suffix)"], !server.isEmpty, !user.isEmpty, !pass.isEmpty else { continue }
            let id = "de00000\(n + 1)"
            // Never `talk/`: on a device that is the stored first account's folder (outbox!).
            let a = Account(id: id, server: server, loginName: user, userId: user, name: user, key: "",
                            dir: "accounts/\(id)")
            list.append((a, pass))
        }
        guard !list.isEmpty else { return nil }
        if env["TALK_DEV_SAVE"] == "1" {
            var stored = AccountList.load()
            for (a, pass) in list where !stored.contains(where: { $0.server == a.server && $0.loginName == a.loginName }) {
                let id = Account.newId()
                let kept = Account(id: id, server: a.server, loginName: a.loginName, userId: a.userId,
                                   name: a.name, key: "", dir: "accounts/\(id)")
                if Keychain.savePassword(pass, for: kept.id) { stored.append(kept) }
            }
            AccountList.save(stored)
            AccountList.migrated = true
            return nil
        }
        return list
        #else
        return nil
        #endif
    }

    /// The stored accounts, after taking over the earlier version's one. Accounts whose
    /// password cannot be read now (locked since the restart) stay in the list and start
    /// on the next attempt; nothing is deleted for them.
    private func loadAccounts() {
        var list = AccountList.load()
        var unreadable = false
        if !AccountList.migrated || list.isEmpty {
            switch takeOver(&list) {
            case .failed: unreadable = true
            default: break
            }
        } else if list.contains(where: { $0.dir == Self.legacyDir }) {
            // The list written by the takeover was read back: only now the old items go.
            Keychain.deleteLegacy()
            if let a = list.first(where: { $0.dir == Self.legacyDir }), DeviceKeys.key(for: a.id) != nil {
                DeviceKeys.deleteLegacy()
            }
        }
        for a in list where runtimes[a.id] == nil {
            switch Keychain.password(for: a.id) {
            case .found(let password):
                _ = makeRuntime(a, password: password)
            case .missing:
                // Certainly gone: no account any more.
                Log.push.error("account \(a.id, privacy: .public) without password, removed")
                list.removeAll { $0.id == a.id }
                AccountList.save(list)
                DeviceKeys.delete(for: a.id)
            case .failed(let status):
                Log.push.error("account \(a.id, privacy: .public) unreadable (\(status)), later")
                unreadable = true
            }
        }
        // Push keys without an entry (an account whose list got lost) would name a missing
        // account for a push that its entry's key decrypts just as well.
        for e in DeviceKeys.all() where !list.contains(where: { $0.id == e.accountId }) {
            DeviceKeys.delete(for: e.accountId)
        }
        Push.dropFlags(keeping: Set(list.map(\.id)))
        accounts = list.filter { runtimes[$0.id] != nil }
        if let id = activeId, runtimes[id] != nil {
            phase = .ready
        } else if let id = AccountList.active.flatMap({ runtimes[$0] != nil ? $0 : nil }) ?? accounts.first?.id {
            activate(id)
        } else {
            // Nothing to show yet; the login only when there is certainly no account.
            phase = unreadable ? .loading : .loggedOut
        }
        updateAppBadge()
    }

    private enum TakeOver { case none, done, failed }

    /// First start with several accounts: the earlier version's account becomes the first
    /// entry, without a new login. It keeps its folder `talk/` and its push key. The old
    /// Keychain items go only on a later start that reads the new list back: UserDefaults
    /// may not reach the disk before iOS moves the container (update), and a lost list then
    /// simply takes over again.
    private func takeOver(_ list: inout [Account]) -> TakeOver {
        let old: StoredAccount
        switch Keychain.legacyAccount() {
        case .missing:
            AccountList.migrated = true
            return .none
        case .failed(let status):
            Log.push.error("takeover: account unreadable (\(status))")
            return .failed
        case .found(let a):
            old = a
        }
        let id = Account.newId()
        var a = Account(id: id, server: old.server, loginName: old.loginName, userId: old.loginName,
                        name: old.loginName, key: "", dir: Self.legacyDir)
        guard Keychain.savePassword(old.appPassword, for: id) else { return .failed }
        guard let rt = makeRuntime(a, password: old.appPassword) else {
            Keychain.deletePassword(for: id)
            return .failed
        }
        // From the cache, without network; the key follows after the first start.
        a.userId = rt.session.userId()
        a.name = rt.session.displayName()
        rt.account = a
        _ = DeviceKeys.adoptLegacy(for: id, line: a.line)
        list.insert(a, at: 0)
        AccountList.save(list)
        AccountList.active = id
        AccountList.migrated = true
        Push.adoptLegacyRegistration(for: id)
        Log.push.info("takeover: account \(id, privacy: .public)")
        return .done
    }

    /// Creates and starts the runtime of an account.
    private func makeRuntime(_ a: Account, password: String) -> AccountRuntime? {
        let bridge = EventBridge(account: a.id)
        bridge.store = self
        let s: TalkSession
        do {
            s = try TalkSession(
                server: a.server, user: a.loginName, appPassword: password,
                platform: .ios, listener: bridge, dataDir: Self.dataDir(a.dir).path
            )
        } catch {
            Log.push.error("session \(a.id, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return nil
        }
        s.setMediaEngine(media: callMedia)
        let rt = AccountRuntime(account: a, session: s, bridge: bridge)
        runtimes[a.id] = rt
        badges[a.id] = s.badge()
        Task { await self.run(rt, password: password) }
        BackgroundSender.shared.sessionStarted(s)
        return rt
    }

    private func run(_ rt: AccountRuntime, password: String) async {
        do {
            try await rt.session.start()
        } catch CoreError.Unauthorized {
            await expire(rt.account.id)
            return
        } catch {
            // Offline start: the core keeps retrying in the background.
        }
        guard runtimes[rt.account.id] === rt else { return }
        Push.shared.update(rt)
        await refreshIdentity(rt, password: password)
        #if DEBUG
        let env = ProcessInfo.processInfo.environment
        DevPushKeys.export(rt.account)
        if env["TALK_DEV_LOGOUT"] == "1", rt.account.id == activeId { logout() }
        // Device tests: sign out exactly this stored account (login name), like "Abmelden".
        if let who = env["TALK_DEV_REMOVE"], !who.isEmpty, who == rt.account.loginName {
            await signOut(rt.account.id, revoke: true)
        }
        #endif
    }

    /// Name after every start; the key of a taken-over account once the server is reachable.
    private func refreshIdentity(_ rt: AccountRuntime, password: String) async {
        var a = rt.account
        a.name = rt.session.displayName()
        a.userId = rt.session.userId()
        if a.key.isEmpty,
           let who = try? await resolveAccount(server: a.server, loginName: a.loginName, appPassword: password, platform: .ios) {
            a.key = who.key
            a.userId = who.userId
        }
        guard runtimes[a.id] === rt, a != rt.account else { return }
        update(a)
    }

    private func update(_ a: Account) {
        guard let rt = runtimes[a.id] else { return }
        rt.account = a
        if let i = accounts.firstIndex(where: { $0.id == a.id }) { accounts[i] = a }
        if a.id == activeId {
            user = a.name
            userId = a.userId
        }
        DeviceKeys.setLine(a.line, for: a.id)
        guard !devRun else { return }
        var list = AccountList.load()
        if let i = list.firstIndex(where: { $0.id == a.id }) {
            list[i] = a
            AccountList.save(list)
        }
    }

    /// Shows an account. Nothing reloads: the list comes from its runtime at once.
    private func activate(_ id: String) {
        guard let rt = runtimes[id] else { return }
        if activeId != id {
            if activeId != nil { leaveActive() }
            activeId = id
            session = rt.session
            let a = rt.account
            server = a.server
            user = a.name
            userId = a.userId
            rooms = rt.session.rooms()
            roomsEvents &+= 1
            recordings = rt.session.recordingNotices()
            connectionShown = .online
            setConnection(rt.connection)
            if !devRun { AccountList.active = id }
        }
        phase = .ready
    }

    /// The open chat closes, a recording becomes a draft; the settings stay open.
    private func leaveActive() {
        closeRoom()
        chats = [:]
        unreadLine = nil
        groupInfoOpen = false
        listPath.removeAll { if case .groupInfo = $0 { true } else { false } }
        participantsVersion = [:]
        leaving = []
        recordingBusy = []
    }

    /// Switch to another account (account menu, settings, notification). Locked while a
    /// call rings or runs.
    @discardableResult
    func switchTo(_ id: String) -> Bool {
        guard id != activeId else { return true }
        guard runtimes[id] != nil else { return false }
        if CallController.shared.busy {
            Toast.show("Während eines Anrufs nicht möglich")
            return false
        }
        activate(id)
        return true
    }

    /// Answering a call of another account switches there afterwards (docs/accounts.md).
    func followCall(_ id: String) {
        guard id != activeId, runtimes[id] != nil else { return }
        activate(id)
    }

    /// After the browser login: who signed in decides. The same account again only switches
    /// there; a failing `resolveAccount` keeps the login open and stores nothing.
    func login(_ result: LoginResult) async throws {
        let who = try await resolveAccount(
            server: result.server, loginName: result.loginName, appPassword: result.appPassword, platform: .ios
        )
        if let known = accounts.first(where: {
            $0.key == who.key || ($0.key.isEmpty && $0.server == result.server && $0.userId == who.userId)
        }) {
            let surplus = StoredAccount(server: result.server, loginName: result.loginName, appPassword: result.appPassword)
            Task { await self.revoke(surplus) }
            adding = false
            switchTo(known.id)
            Toast.show("Dieses Konto ist schon angemeldet")
            return
        }
        // In the list but not started (its password could not be read in this run): it gets
        // the new password and starts, with its folder and outbox; no second entry.
        if !devRun, let stale = AccountList.load().first(where: { a in
            runtimes[a.id] == nil
                && (a.key == who.key || (a.key.isEmpty && a.server == result.server && a.userId == who.userId))
        }) {
            guard Keychain.savePassword(result.appPassword, for: stale.id) else {
                throw CoreError.Server(detail: "Zugangsdaten konnten nicht gespeichert werden")
            }
            var a = stale
            a.key = who.key
            a.userId = who.userId
            a.name = who.displayName
            guard makeRuntime(a, password: result.appPassword) != nil else {
                throw CoreError.Server(detail: "Konto konnte nicht gestartet werden")
            }
            accounts = AccountList.load().filter { runtimes[$0.id] != nil || $0.id == a.id }
            update(a)
            adding = false
            if activeId == nil || !CallController.shared.busy { activate(a.id) }
            return
        }
        let id = Account.newId()
        let a = Account(id: id, server: result.server, loginName: result.loginName, userId: who.userId,
                        name: who.displayName, key: who.key, dir: "accounts/\(id)")
        guard Keychain.savePassword(result.appPassword, for: id) else {
            throw CoreError.Server(detail: "Zugangsdaten konnten nicht gespeichert werden")
        }
        guard makeRuntime(a, password: result.appPassword) != nil else {
            Keychain.deletePassword(for: id)
            throw CoreError.Server(detail: "Konto konnte nicht gestartet werden")
        }
        if !devRun {
            var list = AccountList.load()
            list.append(a)
            AccountList.save(list)
        }
        accounts.append(a)
        adding = false
        if activeId == nil || !CallController.shared.busy {
            activate(id)
        } else {
            Toast.show("Während eines Anrufs nicht möglich")
        }
    }

    /// "Abmelden": the active account only.
    func logout() {
        guard let id = activeId else { return }
        Task { await signOut(id, revoke: true) }
    }

    /// `authFailed`: this account goes, without asking and without `logout()` at the server.
    private func expire(_ id: String) async {
        guard let a = runtimes[id]?.account else { return }
        await signOut(id, revoke: false)
        Toast.show("\(a.name) (\(a.serverLabel)) wurde abgemeldet. Bitte neu anmelden.")
    }

    /// Removes one account: push, session, entry, password, push key, data folder. The
    /// others stay untouched; the first remaining becomes active.
    private func signOut(_ id: String, revoke: Bool) async {
        guard let rt = runtimes[id] else { return }
        let s = rt.session
        let wasActive = id == activeId
        if wasActive {
            voice.reset()
            closeRoom()
        }
        // Only this account's own call.
        if CallController.shared.account == id { s.hangUp() }
        // Read before it is deleted: given back to the server below, or on a later start.
        let password: String? = if case .found(let p) = Keychain.password(for: id) { p } else { nil }
        if revoke { await Push.shared.unregister(rt) } else { Push.shared.forget(rt) }
        runtimes[id] = nil
        rt.bridge.store = nil
        badges[id] = nil
        accounts.removeAll { $0.id == id }
        if revoke {
            do {
                try await s.logout()
            } catch CoreError.Unauthorized {
                // Already invalid at the server.
            } catch {
                // No network: the server would keep pushing to an account that is gone here.
                if let password, !devRun {
                    rememberRevoke(StoredAccount(server: rt.account.server, loginName: rt.account.loginName, appPassword: password))
                }
            }
        } else {
            s.shutdown()
        }
        Keychain.deletePassword(for: id)
        DeviceKeys.delete(for: id)
        if !devRun {
            AccountList.save(AccountList.load().filter { $0.id != id })
            // Not taken over again on the next start.
            if rt.account.dir == Self.legacyDir {
                Keychain.deleteLegacy()
                DeviceKeys.deleteLegacy()
            }
        }
        // Unsent messages belong to this account: never deliver them under another one.
        try? FileManager.default.removeItem(at: Self.dataDir(rt.account.dir))
        Notifications.removeAll(account: id, legacy: rt.account.dir == Self.legacyDir)
        CallAvatars.shared.forget(account: id)
        voice.forget(account: id)
        textDrafts = textDrafts.filter { !$0.key.hasPrefix("\(id)/") }
        updateAppBadge()
        guard wasActive else { return }
        activeId = nil
        session = nil
        chats = [:]
        rooms = []
        recordings = []
        recordingBusy = []
        unreadLine = nil
        groupInfoOpen = false
        participantsVersion = [:]
        leaving = []
        setConnection(.connecting)
        if let next = accounts.first {
            activate(next.id)
        } else {
            listPath = []
            server = ""
            user = ""
            userId = ""
            phase = .loggedOut
        }
    }

    // MARK: - Notifications (docs/notifications.md)

    /// Back in front: deletions do not arrive reliably, so the notifications of chats
    /// without anything unread go (after a fresh list) and the icon number is set anew.
    /// The permission is read again, and asked for once.
    func cameToFront() {
        Task { await NotificationPermission.shared.refresh() }
        if phase == .ready { Notifications.askAtStart() }
        for rt in allRuntimes {
            let id = rt.account.id
            let legacy = rt.account.dir == Self.legacyDir
            Task {
                // Offline: the old list could remove a notification that just came in.
                do { try await rt.session.refreshRooms() } catch { return }
                guard self.runtimes[id] === rt else { return }
                let unread = rt.session.rooms().filter { $0.unreadMessages > 0 || $0.unreadMention }.map(\.token)
                Notifications.keepOnly(account: id, legacy: legacy, unread: Set(unread))
                self.badges[id] = rt.session.badge()
                self.updateAppBadge()
            }
        }
    }

    /// "Antworten" in a notification: into the outbox of the notification's account,
    /// without making it active; also when iOS started the app in the background for it.
    func reply(account: String?, token: String, text: String) {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        guard let rt = runtime(account) else {
            Log.push.error("reply: account \(account ?? "-", privacy: .public) not running")
            return
        }
        _ = rt.session.enqueueText(token: token, text: text, mentions: [], replyTo: nil, referenceId: newReferenceId())
        BackgroundSender.shared.actionQueued(rt.session)
        Notifications.removeChats(account: rt.account.id, legacy: rt.account.dir == Self.legacyDir, tokens: [token])
    }

    /// "Als gelesen markieren" in a notification, in its account (the core asks the server
    /// for the newest message: after a cold start the list may not know it yet).
    func markReadFromNotification(account: String?, token: String) async {
        guard let rt = runtime(account) else {
            Log.push.error("mark read: account \(account ?? "-", privacy: .public) not running")
            return
        }
        if let error = await rt.session.markRoomRead(token: token) {
            Log.push.error("mark read: \(error, privacy: .public)")
        }
        Notifications.removeChats(account: rt.account.id, legacy: rt.account.dir == Self.legacyDir, tokens: [token])
        badges[rt.account.id] = rt.session.badge()
        updateAppBadge()
    }

    /// "Push testen": a test push to the active account's devices.
    func testPush() async -> Bool {
        guard let s = session else { return false }
        do {
            try await s.testPush()
            return true
        } catch {
            Log.push.error("test push: \(error.localizedDescription, privacy: .public)")
            return false
        }
    }

    // MARK: - Sign-out without network

    /// Gives an app password back; remembered for the next start when that fails.
    private func revoke(_ a: StoredAccount) async {
        do {
            try await revokeAppPassword(server: a.server, loginName: a.loginName, appPassword: a.appPassword, platform: .ios)
        } catch CoreError.Unauthorized {
        } catch {
            rememberRevoke(a)
        }
    }

    private func rememberRevoke(_ a: StoredAccount) {
        var list = Keychain.pendingRevokes()
        guard !list.contains(a) else { return }
        list.append(a)
        Keychain.setPendingRevokes(list)
        Log.push.info("revoke later (\(list.count) waiting)")
    }

    /// On start: the app passwords of earlier sign-outs without network.
    private func retryRevokes() {
        let waiting = Keychain.pendingRevokes()
        guard !waiting.isEmpty else { return }
        Task {
            var left: [StoredAccount] = []
            for a in waiting {
                do {
                    try await revokeAppPassword(server: a.server, loginName: a.loginName, appPassword: a.appPassword, platform: .ios)
                } catch CoreError.Unauthorized {
                } catch {
                    left.append(a)
                }
            }
            // Others may have been added meanwhile.
            let added = Keychain.pendingRevokes().filter { !waiting.contains($0) }
            Keychain.setPendingRevokes(left + added)
            Log.push.info("revoked \(waiting.count - left.count), \(left.count) still waiting")
        }
    }

    // MARK: - Accounts

    var allRuntimes: [AccountRuntime] { accounts.compactMap { runtimes[$0.id] } }

    /// The runtime a push, notification or call belongs to. `legacy` (the earlier version's
    /// key) and notifications without an account: the account taken over from it.
    func runtime(_ id: String?) -> AccountRuntime? {
        if let id, id != DeviceKeys.legacyId { return runtimes[id] }
        return allRuntimes.first { $0.account.dir == Self.legacyDir } ?? (accounts.count == 1 ? allRuntimes.first : nil)
    }

    var activeAccount: Account? { activeId.flatMap { runtimes[$0]?.account } }

    /// Chats with something new in an account (`badge().chats`).
    func newChats(_ id: String) -> Int { Int(badges[id]?.chats ?? 0) }

    /// The number at the own avatar: the other accounts' chats with something new.
    var otherNewChats: Int {
        accounts.filter { $0.id != activeId }.reduce(0) { $0 + newChats($1.id) }
    }

    /// A conversation of any account (call screens of an inactive account).
    func room(_ token: String, account: String?) -> RoomView? {
        guard let account, account != activeId else { return room(token) }
        return runtimes[account]?.rooms.first { $0.token == token }
    }

    /// Avatars and previews of the active account, or of another one.
    var media: MediaCache { activeId.flatMap { runtimes[$0]?.media } ?? noMedia }

    func mediaCache(for account: String?) -> MediaCache { runtime(account)?.media ?? media }

    /// Where a notification of this account belongs; the line from two accounts on.
    func origin(_ id: String) -> Notifications.Origin {
        Notifications.Origin(account: id, line: accounts.count > 1 ? runtimes[id]?.account.line : nil)
    }

    /// App badge: the sum of `badge().messages` over all accounts.
    private func updateAppBadge() {
        Notifications.setBadge(badges.values.reduce(0) { $0 + Int($1.messages) })
    }

    func textDraft(_ token: String) -> TextDraft { textDrafts["\(activeId ?? "")/\(token)"] ?? TextDraft() }

    func setTextDraft(_ token: String, _ draft: TextDraft) { textDrafts["\(activeId ?? "")/\(token)"] = draft }

    func refresh() async {
        try? await session?.refreshRooms()
    }

    func room(_ token: String) -> RoomView? {
        rooms.first { $0.token == token }
    }

    /// docs/connection.md "Regeln für alle": a state shows once it held 500 ms; online hides
    /// at once. While waiting the previous indicator stays (connecting → offline).
    private func setConnection(_ state: ConnectionState) {
        connection = state
        connectionDelay?.cancel()
        connectionDelay = nil
        if state == .online { connectionShown = .online; return }
        guard connectionShown != state else { return }
        connectionDelay = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(500))
            guard !Task.isCancelled, let self, self.connection == state else { return }
            self.connectionShown = state
        }
    }

    // MARK: - Core events

    /// Events of inactive accounts (docs/accounts.md "Im Hintergrund"): badge, calls,
    /// failed sends, recordings and `authFailed`; everything else is dropped.
    fileprivate func handle(_ event: CoreEvent, from id: String) {
        guard let rt = runtimes[id] else { return }
        let active = id == activeId
        switch event {
        case .roomsChanged(let rooms):
            rt.rooms = rooms
            badges[id] = rt.session.badge()
            updateAppBadge()
            if active { roomsChanged(rooms) }
        case .connection(let state):
            rt.connection = state
            if active { setConnection(state) }
        case .authFailed:
            Task { await expire(id) }
        case .callChanged(let call):
            CallController.shared.update(call, account: id)
        case .callMissed(let token, let roomName, let callerName, let isGroup):
            Notifications.missedCall(origin(id), token: token, name: isGroup || callerName.isEmpty ? roomName : callerName)
        case .outboxChanged(let token, let items):
            // A window shows no unsent messages: they belong at the end (docs/search.md).
            if active { updateChat(token) { $0.messages = Self.withPending($0.messages, $0.hasNewer ? [] : items) } }
            announceFailures(rt, token, items)
        case .recordingsChanged(let items):
            if active { recordings = items }
            // The first one after the start is the starting point; only later ids are new.
            if let known = rt.notifiedRecordings {
                for n in items where !known.contains(n.id) { Notifications.recording(origin(id), n) }
            }
            rt.notifiedRecordings = (rt.notifiedRecordings ?? []).union(items.map(\.id))
        case .messagesAdded(let token, let messages):
            guard active else { return }
            media.adoptLocalPreviews(messages)
            if token == activeToken {
                updateChat(token) { Self.addMentions(&$0, messages.filter(\.mentionsMe).map(\.id)) }
            }
            if chats[token]?.hasNewer == true {
                // A window: appended they would leave a gap. Kept for the end, counted in the button.
                if token == activeToken { countUnreadInWindow(token, messages) }
                updateChat(token) { c in
                    let ids = Set(messages.map(\.id))
                    c.held = (c.held.filter { !ids.contains($0.id) } + messages).sorted { $0.id < $1.id }
                }
                return
            }
            if token == activeToken { countUnread(token, messages) }
            updateChat(token) { $0.messages = Self.merge($0.messages, messages) }
        case .messagesUpdated(let token, let messages):
            guard active else { return }
            let byId = Dictionary(messages.map { ($0.id, $0) }, uniquingKeysWith: { _, b in b })
            updateChat(token) { c in
                c.messages = c.messages.map { m in byId[m.msg.id].map { UiMessage(msg: $0) } ?? m }
                c.held = c.held.map { byId[$0.id] ?? $0 }
                let deleted = c.mentions.filter { byId[$0]?.kind == .deleted }
                c.mentions.removeAll { deleted.contains($0) }
                c.seenMentions.formUnion(deleted)
            }
        case .messagesRemoved(let token, let ids):
            guard active else { return }
            let gone = Set(ids)
            updateChat(token) { c in
                c.messages.removeAll { $0.status == nil && gone.contains($0.msg.id) }
                c.held.removeAll { gone.contains($0.id) }
                c.mentions.removeAll { gone.contains($0) }
                c.seenMentions.formUnion(gone)
            }
        case .readStatus(let token, let lastCommonRead):
            guard active else { return }
            updateChat(token) { $0.lastCommonRead = max($0.lastCommonRead, lastCommonRead) }
        case .participantsChanged(let token):
            guard active else { return }
            participantsVersion[token, default: 0] += 1
        case .messageNotices:
            // iOS notifies from pushes (docs/notifications.md "Zwei Wege").
            break
        case .noticesCleared(let tokens):
            Notifications.removeChats(account: id, legacy: rt.account.dir == Self.legacyDir, tokens: Set(tokens))
        }
    }

    private func roomsChanged(_ rooms: [RoomView]) {
        let gone = activeToken.flatMap { t in
            rooms.contains { $0.token == t } ? nil : self.rooms.first { $0.token == t }
        }
        self.rooms = rooms
        roomsEvents &+= 1
        // Group settings from the chat menu of a group that is gone.
        if let i = listPath.firstIndex(where: { if case .groupInfo(let t) = $0 { !rooms.contains { $0.token == t } } else { false } }) {
            listPath.removeSubrange(i...)
        }
        if let gone {
            // Removed, deleted or left: chat and settings close (docs/groups.md).
            closeRoom()
            if leaving.remove(gone.token) == nil && (gone.roomType == 2 || gone.roomType == 3) {
                Toast.show("Du bist nicht mehr in dieser Gruppe")
            }
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
                if let id = self.activeId { Notifications.removeRecording(account: id, n.id) }
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
    private func announceFailures(_ rt: AccountRuntime, _ token: String, _ items: [PendingMessage]) {
        let fresh = items.filter { $0.state == .failed }.map(\.message.referenceId)
            .filter { rt.notifiedFailures.insert($0).inserted }
        guard !fresh.isEmpty else { return }
        let id = rt.account.id
        let visible = id == activeId && activeToken == token && foreground
        if !visible {
            let name = rt.rooms.first { $0.token == token }?.name ?? "Unterhaltung"
            Notifications.sendFailed(origin(id), token: token, room: name)
        }
    }

    // MARK: - Chat actions

    /// Opens a chat, at the end (or the unread line) or, from a search hit, at `messageId`
    /// (docs/search.md "Aus der Suche über alle Chats"): the window comes instead of the
    /// chat's own page, so the end does not flash up.
    func openRoom(_ token: String, at messageId: Int64? = nil) {
        guard let s = session, activeToken != token else { return }
        if let previous = activeToken {
            voice.interrupt()
            s.closeChat(token: previous)
            updateChat(previous) { $0.mentions = []; $0.seenMentions = [] }
        }
        // The snapshot comes first, always: the read marker moves as soon as anything is
        // marked read, and the list may not know the newest message yet. Nothing is read
        // until the list has opened (at the line or at the bottom) and says where it is.
        if let room = room(token), room.lastReadMessage > 0 {
            unreadLine = UnreadLine(token: token, from: room.lastReadMessage, base: Int(room.unreadMessages))
        } else {
            unreadLine = nil
        }
        chatAtBottom = false
        // Cached messages and unsent ones (both survive restarts): shown in the first frame.
        let cached = s.cachedChat(token: token)
        // A window left open last time is no base for the cached end.
        var chat = chats[token].flatMap { $0.hasNewer ? nil : $0 } ?? ChatState()
        chat.held = []
        // A chat starts with the unseen mentions behind the read marker (docs/mentions.md).
        chat.mentions = cached.unreadMentions.sorted()
        chat.seenMentions = []
        chat.openAt = messageId
        chat.opening = messageId != nil
        let messages = Self.merge(chat.messages, cached.messages)
        let older = cached.oldestId != nil && (chat.oldestId == nil || cached.oldestId! < chat.oldestId!)
        chat.loading = messageId != nil || !messages.contains { $0.status == nil }
        chat.messages = Self.withPending(messages, s.pending(token: token))
        if older {
            chat.hasMore = cached.hasMore
            chat.oldestId = cached.oldestId
        }
        chat.lastCommonRead = max(chat.lastCommonRead, cached.lastCommonRead ?? 0)
        chats[token] = chat
        activeToken = token
        groupInfoOpen = false
        chatBack = nil
        listPath = []
        voice.loadDraft(token)
        if let id = activeId {
            Notifications.removeChats(account: id, legacy: runtimes[id]?.account.dir == Self.legacyDir, tokens: [token])
        }
        if let messageId {
            // The line shows if it falls into the window; nothing is loaded towards it.
            settleUnreadLine(token)
            Task { await showAround(token, messageId, opening: true) }
        }
        Task {
            do {
                let page = try await s.openChat(token: token)
                let room = self.room(token)
                updateChat(token) { c in
                    c.lastCommonRead = max(c.lastCommonRead, page.lastCommonRead ?? 0, room?.lastCommonReadMessage ?? 0)
                    Self.addMentions(&c, page.unreadMentions)
                    // A window is shown (or on its way): the end comes back with "Zum Ende springen".
                    guard !c.hasNewer else { return }
                    // Outdated cache: drop what was shown (unsent messages stay).
                    let base = page.reset ? c.messages.filter { $0.status != nil } : c.messages
                    c.messages = Self.merge(base, page.messages)
                    c.hasMore = page.hasMore
                    c.oldestId = page.oldestId
                    if !c.opening { c.loading = false }
                }
                if messageId == nil { await loadToUnreadLine(token) }
                markVisibleRead(token)
            } catch {
                updateChat(token) { if !$0.opening { $0.loading = false } }
                settleUnreadLine(token)
            }
        }
    }

    // MARK: - Window into the history (docs/search.md "Chat an einer Nachricht öffnen")

    @ObservationIgnored private var aroundSerial = 0

    /// Scrolls the open chat to a message: at once when it is in the list, else the list
    /// becomes a window around it.
    func jump(_ token: String, to messageId: Int64) {
        guard activeToken == token, let chat = chats[token] else { return }
        if chat.messages.contains(where: { $0.status == nil && $0.msg.id == messageId }) {
            aroundSerial += 1
            setFocus(token, messageId)
        } else {
            Task { await showAround(token, messageId, opening: false) }
        }
    }

    private func setFocus(_ token: String, _ messageId: Int64) {
        focusSerial += 1
        focus = ChatFocus(token: token, key: "id:\(messageId)", serial: focusSerial)
    }

    /// `messagesAround` replaces the list. On an error the chat stays as it is; while opening
    /// it then shows its own page.
    private func showAround(_ token: String, _ messageId: Int64, opening: Bool) async {
        guard let s = session else { return }
        aroundSerial += 1
        let serial = aroundSerial
        do {
            let page = try await s.messagesAround(token: token, messageId: messageId)
            guard serial == aroundSerial, activeToken == token else { return }
            let pending = s.pending(token: token)
            updateChat(token) { c in
                if page.hasNewer {
                    c.messages = Self.merge([], page.messages)
                } else {
                    // The window reaches the end: an ordinary list again. Joined with a list
                    // that ends there too (never with an older window: that leaves a gap).
                    let base = c.hasNewer || c.opening ? [] : c.messages.filter { $0.status == nil }
                    c.messages = Self.withPending(Self.merge(base, page.messages), pending)
                }
                c.hasMore = page.hasMore
                c.oldestId = page.oldestId
                c.hasNewer = page.hasNewer
                c.newestId = page.newestId
                c.loadingNewer = false
                if !page.hasNewer { Self.reachEnd(&c, pending) }
                c.lastCommonRead = max(c.lastCommonRead, page.lastCommonRead ?? 0)
                c.loading = false
                c.opening = false
                Self.addMentions(&c, page.unreadMentions)
            }
            setFocus(token, messageId)
        } catch {
            guard serial == aroundSerial, activeToken == token else { return }
            if let e = error as? CoreError, case .Network = e {
                Toast.show("Keine Verbindung")
            } else {
                Toast.show("Nachricht nicht gefunden")
            }
            if opening {
                // The chat's own page (or the cache) is in the list already.
                updateChat(token) { c in
                    c.opening = false
                    c.openAt = nil
                    c.loading = false
                }
            }
        }
    }

    /// The window reached the end: messages held meanwhile follow (only those after the
    /// last of the list), then the unsent ones; from here on the chat runs as usual.
    private static func reachEnd(_ c: inout ChatState, _ pending: [PendingMessage]) {
        let last = c.messages.last(where: { $0.status == nil })?.msg.id ?? 0
        c.messages = withPending(merge(c.messages, c.held.filter { $0.id > last }), pending)
        c.held = []
        c.hasNewer = false
    }

    /// Next page below a window, from `newestId` of the last page fetched that way.
    func loadNewer(_ token: String) {
        Task { await fetchNewer(token) }
    }

    private func fetchNewer(_ token: String) async {
        guard let s = session, let chat = chats[token], chat.hasNewer, let after = chat.newestId,
              !chat.loadingNewer, !chat.loading else { return }
        let serial = aroundSerial
        updateChat(token) { $0.loadingNewer = true }
        do {
            let page = try await s.loadNewer(token: token, afterId: after)
            let pending = s.pending(token: token)
            updateChat(token) { c in
                c.loadingNewer = false
                // Replaced meanwhile (another jump, "Zum Ende springen").
                guard serial == aroundSerial, c.hasNewer, c.newestId == after else { return }
                c.messages = Self.merge(c.messages, page.messages)
                c.newestId = page.newestId ?? after
                Self.addMentions(&c, page.unreadMentions)
                if !page.hasNewer { Self.reachEnd(&c, pending) }
            }
            markVisibleRead(token)
        } catch {
            updateChat(token) { $0.loadingNewer = false }
        }
    }

    /// "Zum Ende springen" in a window, and sending from one: the cached end of the chat
    /// with the messages held meanwhile and the unsent ones.
    func showEnd(_ token: String) {
        guard let s = session, var chat = chats[token], chat.hasNewer else { return }
        aroundSerial += 1
        let cached = s.cachedChat(token: token)
        chat.messages = Self.withPending(Self.merge([], cached.messages + chat.held), s.pending(token: token))
        chat.hasMore = cached.hasMore
        chat.oldestId = cached.oldestId
        chat.hasNewer = false
        chat.newestId = nil
        chat.loadingNewer = false
        chat.held = []
        chats[token] = chat
    }

    /// New messages while the list is a window. Without a line one goes above the first of
    /// them; a line from the opening counts them by itself once they show.
    private func countUnreadInWindow(_ token: String, _ messages: [MessageView]) {
        guard unreadLine?.token != token,
              let first = messages.filter({ !$0.isOwn && $0.kind != .system }).map(\.id).min() else { return }
        unreadLine = UnreadLine(token: token, from: first - 1, base: 0, settled: true)
    }

    /// A notification of the chat that is open already was tapped (docs/notifications.md,
    /// "War der Chat schon offen"): like opening it anew, the list then stands at the unread
    /// line, without unread messages at the end.
    func reopenRoom(_ token: String) {
        guard activeToken == token else { return }
        closeRoom()
        openRoom(token)
        openSerial += 1
    }

    func closeRoom() {
        guard let token = activeToken else { return }
        voice.interrupt()
        session?.closeChat(token: token)
        updateChat(token) { $0.mentions = []; $0.seenMentions = [] }
        activeToken = nil
        unreadLine = nil
        chatAtBottom = true
        groupInfoOpen = false
        chatBack = nil
        focus = nil
        aroundSerial += 1
    }

    // MARK: - Unread line (docs/chat.md)

    /// More than a page unread: older pages until the first unread message is loaded (≤ 10).
    private func loadToUnreadLine(_ token: String) async {
        var pages = 0, waits = 0
        while pages < 10, waits < 100 {
            // Loaded messages all newer than the marker: earlier unread ones are still missing.
            guard let line = unreadLine, line.token == token, activeToken == token, line.base > 0,
                  let chat = chats[token], chat.hasMore, let oldest = chat.oldestId,
                  oldest > line.from else { break }
            if chat.loadingOlder {
                // A page the list asked for is on its way.
                waits += 1
                try? await Task.sleep(for: .milliseconds(100))
                continue
            }
            guard await fetchOlder(token) else { break }
            pages += 1
        }
        settleUnreadLine(token)
    }

    private func settleUnreadLine(_ token: String) {
        guard unreadLine?.token == token else { return }
        unreadLine?.settled = true
    }

    /// Gone once we send anything (text, file, voice) into the open chat.
    func clearUnreadLine(_ token: String) {
        // Sending from a window leads to the end, where the message shows (docs/search.md).
        showEnd(token)
        guard unreadLine?.token == token else { return }
        unreadLine = nil
    }

    /// The list reports whether it shows the newest message; at the bottom it is read.
    func setChatAtBottom(_ token: String, _ bottom: Bool) {
        guard activeToken == token else { return }
        chatAtBottom = bottom
        if bottom { markVisibleRead(token) }
    }

    /// New messages in the open chat. A shown line counts them by itself. Without one, a chat
    /// that is not at the bottom (or not in front) gets a new line above them; at the bottom
    /// they are read, and a snapshot that showed nothing is dropped.
    private func countUnread(_ token: String, _ messages: [MessageView]) {
        guard let chat = chats[token], let newest = chat.messages.last(where: { $0.status == nil })?.msg.id,
              messages.contains(where: { $0.id > newest && !$0.isOwn && $0.kind != .system }) else { return }
        if let line = unreadLine, line.token == token {
            // A shown line counts them; while opening (server page not in) nothing changes.
            guard line.settled, line.place(in: chat.messages) == nil else { return }
            // A snapshot without a line: seen at the bottom, they must not raise one.
            if chatAtBottom && foreground {
                unreadLine = nil
            } else {
                unreadLine = UnreadLine(token: token, from: newest, base: 0, settled: true)
            }
        } else if !chatAtBottom || !foreground {
            unreadLine = UnreadLine(token: token, from: newest, base: 0, settled: true)
        }
    }

    func showGroupInfo(_ open: Bool) {
        groupInfoOpen = open && activeToken != nil
    }

    func loadOlder(_ token: String) {
        Task { await fetchOlder(token) }
    }

    /// One older page; false when there is none or it failed.
    @discardableResult
    private func fetchOlder(_ token: String) async -> Bool {
        guard let s = session, let chat = chats[token], let before = chat.oldestId,
              chat.hasMore, !chat.loadingOlder, !chat.loading else { return false }
        updateChat(token) { $0.loadingOlder = true }
        do {
            let page = try await s.loadOlder(token: token, beforeId: before)
            updateChat(token) { c in
                c.messages = Self.merge(c.messages, page.messages)
                c.hasMore = page.hasMore
                c.oldestId = page.oldestId ?? c.oldestId
                c.loadingOlder = false
                Self.addMentions(&c, page.unreadMentions)
            }
            return true
        } catch {
            updateChat(token) { $0.loadingOlder = false }
            return false
        }
    }

    /// Queues a text message in the outbox; it shows up at once and is delivered reliably.
    func send(_ token: String, text: String, mentions: [MentionPick], replyTo: MessageView?) {
        guard let s = session else { return }
        clearUnreadLine(token)
        _ = s.enqueueText(token: token, text: text, mentions: mentions, replyTo: replyTo?.toReply, referenceId: newReferenceId())
        BackgroundSender.shared.queued()
    }

    /// Sends a prepared image with an optional caption. The outbox takes over the file; the
    /// bubble shows the local image until the server message replaces it without flicker.
    func sendImage(_ token: String, image: PreparedImage, caption: String, replyTo: MessageView?) {
        guard let s = session else { return }
        clearUnreadLine(token)
        let ref = newReferenceId()
        media.setLocalPreview(ref, path: image.url.path)
        do {
            _ = try s.enqueueFile(
                token: token, localPath: image.url.path, name: image.name, mime: "image/jpeg",
                width: Int64(image.width), height: Int64(image.height),
                caption: caption.trimmingCharacters(in: .whitespacesAndNewlines), mentions: [],
                replyTo: replyTo?.toReply, referenceId: ref
            )
        } catch {
            media.removeLocalPreview(ref)
            try? FileManager.default.removeItem(at: image.url)
            Toast.show("Bild konnte nicht gespeichert werden")
            return
        }
        BackgroundSender.shared.queued()
    }

    /// docs/attachments.md "Datei senden": copies every file unchanged into the staging
    /// folder, then queues them in order, the quote on the first and the caption on the
    /// last. When one cannot be read nothing is sent; the result is the ids of those.
    func sendFiles(_ token: String, files: [PickedFile], caption: String, replyTo: MessageView?) async -> Set<UUID> {
        guard let s = session else { return [] }
        let staged = await Task.detached { files.map { Self.stage($0) } }.value
        let failed = Set(zip(files, staged).filter { $0.1 == nil }.map(\.0.id))
        guard failed.isEmpty else {
            for f in staged.compactMap({ $0 }) { try? FileManager.default.removeItem(atPath: f.path) }
            return failed
        }
        clearUnreadLine(token)
        let text = caption.trimmingCharacters(in: .whitespacesAndNewlines)
        for (i, (f, st)) in zip(files, staged).enumerated() {
            guard let st else { continue }
            let ref = newReferenceId()
            if f.mime.hasPrefix("image/") { media.setLocalPreview(ref, path: st.path) }
            do {
                _ = try s.enqueueFile(
                    token: token, localPath: st.path, name: f.name, mime: f.mime,
                    width: st.width, height: st.height,
                    caption: i == files.count - 1 ? text : "", mentions: [],
                    replyTo: i == 0 ? replyTo?.toReply : nil, referenceId: ref
                )
            } catch {
                media.removeLocalPreview(ref)
                try? FileManager.default.removeItem(atPath: st.path)
                Toast.show("Datei konnte nicht gelesen werden")
            }
        }
        BackgroundSender.shared.queued()
        return []
    }

    private struct StagedFile {
        let path: String
        let width: Int64
        let height: Int64
    }

    /// A copy in the staging folder; images also get their size from the file header.
    nonisolated private static func stage(_ f: PickedFile) -> StagedFile? {
        let scoped = f.url.startAccessingSecurityScopedResource()
        defer { if scoped { f.url.stopAccessingSecurityScopedResource() } }
        let ext = (f.name as NSString).pathExtension
        let dest = stagingDir.appendingPathComponent(UUID().uuidString.lowercased() + (ext.isEmpty ? "" : ".\(ext)"))
        do {
            try FileManager.default.copyItem(at: f.url, to: dest)
        } catch {
            Log.media.error("stage file: \(error.localizedDescription, privacy: .public)")
            return nil
        }
        var w: Int64 = 0, h: Int64 = 0
        if f.mime.hasPrefix("image/"), let src = CGImageSourceCreateWithURL(dest as CFURL, nil),
           let p = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any] {
            w = (p[kCGImagePropertyPixelWidth] as? NSNumber)?.int64Value ?? 0
            h = (p[kCGImagePropertyPixelHeight] as? NSNumber)?.int64Value ?? 0
            // Turned by EXIF (5–8): shown the other way round.
            if let o = (p[kCGImagePropertyOrientation] as? NSNumber)?.intValue, (5...8).contains(o) { swap(&w, &h) }
        }
        return StagedFile(path: dest.path, width: w, height: h)
    }

    /// docs/attachments.md "Standort": queued like a text, never with a quote (Talk has none
    /// for places), so an open reply bar stays for the next message.
    func sendLocation(_ token: String, _ place: GeoLocation) {
        guard let s = session else { return }
        clearUnreadLine(token)
        _ = s.enqueueLocation(
            token: token, latitude: place.latitude, longitude: place.longitude, name: place.name,
            referenceId: newReferenceId()
        )
        BackgroundSender.shared.queued()
    }

    /// docs/attachments.md "Kontakt": the core builds the vCard from exactly what is in `card`
    /// and queues it like a file; it goes out with the quote.
    func sendContact(_ token: String, _ card: ContactCard, replyTo: MessageView?) {
        guard let s = session else { return }
        clearUnreadLine(token)
        do {
            _ = try s.enqueueContact(token: token, card: card, replyTo: replyTo?.toReply, referenceId: newReferenceId())
        } catch {
            Log.media.error("enqueue contact: \(error.localizedDescription, privacy: .public)")
            Toast.show("Kontakt konnte nicht gespeichert werden")
        }
        BackgroundSender.shared.queued()
    }

    /// Sends a failed message again (or a waiting one right now).
    func retry(_ msg: MessageView) {
        activeId.flatMap { runtimes[$0] }?.notifiedFailures.remove(msg.referenceId)
        session?.retrySend(referenceId: msg.referenceId)
        BackgroundSender.shared.queued()
    }

    /// Drops a pending or failed message.
    func discard(_ msg: MessageView) {
        media.removeLocalPreview(msg.referenceId)
        session?.discard(referenceId: msg.referenceId)
    }

    /// Sets the own reaction (docs/chat.md "Reaktionen"): the core replaces or takes it back
    /// like Signal, shows it at once and resets it on a failure; we only show the error.
    func setReaction(_ token: String, _ messageId: Int64, _ emoji: String?) {
        guard let s = session else { return }
        Task {
            if let error = await s.setReaction(token: token, messageId: messageId, emoji: emoji) {
                Toast.show(error)
            }
        }
    }

    // MARK: - Edit and delete (docs/chat.md "Bearbeiten und Löschen")

    /// The text for the input field when editing, with the mentions in it (docs/mentions.md
    /// "Bearbeiten"); nil: start from `MessageView.text` and `MessageView.mentions`.
    func editSource(_ token: String, _ messageId: Int64) async -> EditDraft? {
        await session?.editSource(token: token, messageId: messageId)
    }

    /// Saves the new text. The new version comes as `MessagesUpdated` before this returns;
    /// the result is the error text to show, nil on success.
    func editMessage(_ token: String, _ messageId: Int64, _ text: String, _ mentions: [MentionPick]) async -> String? {
        guard let s = session else { return nil }
        return await s.editMessage(token: token, messageId: messageId, text: text, mentions: mentions)
    }

    // MARK: - Mentions (docs/mentions.md)

    /// Who can be mentioned for the text after the `@`; answers from memory after the first call.
    func mentionCandidates(_ token: String, _ query: String) async -> [MentionCandidate] {
        guard let s = session else { return [] }
        return await s.mentionCandidates(token: token, query: query, limit: 50)
    }

    /// The 1:1 chat with a person, when there is one already.
    func existingDirect(_ userId: String) -> String? {
        session?.existingDirect(userId: userId)
    }

    /// A tapped mention leads into the 1:1 chat; back leads into the chat it came from.
    func openFromMention(_ token: String) {
        let from = activeToken
        openRoom(token)
        if let from, from != token { chatBack = from }
    }

    /// The 1:1 chat with a person, created after asking (docs/mentions.md "Antippen").
    func openDirectFromMention(_ userId: String) async {
        guard let s = session else { return }
        let r = await s.openDirect(userId: userId)
        guard let token = r.token else {
            Toast.show(r.error ?? "Hat nicht geklappt")
            return
        }
        openFromMention(token)
    }

    /// Seen at least half (or jumped to): out of the "@" button's list.
    func mentionsSeen(_ token: String, _ ids: [Int64]) {
        guard let chat = chats[token], chat.mentions.contains(where: { ids.contains($0) }) else { return }
        updateChat(token) { c in
            c.mentions.removeAll { ids.contains($0) }
            c.seenMentions.formUnion(ids)
        }
    }

    /// `unreadMentions` of every page and the new messages that mention the user, merged;
    /// a seen one does not come back.
    private static func addMentions(_ c: inout ChatState, _ ids: [Int64]) {
        let fresh = ids.filter { !c.seenMentions.contains($0) }
        guard !fresh.isEmpty else { return }
        c.mentions = Array(Set(c.mentions).union(fresh)).sorted()
    }

    /// Deletes a message for everybody; the bubble follows via `MessagesUpdated`.
    func deleteMessage(_ token: String, _ messageId: Int64) {
        guard let s = session else { return }
        Task {
            if let error = await s.deleteMessage(token: token, messageId: messageId) {
                Toast.show(error)
            }
        }
    }

    /// Who reacted to a message (own first, then newest).
    func reactions(_ token: String, _ messageId: Int64) async throws -> [ReactionView] {
        guard let s = session else { throw CancellationError() }
        return try await s.reactions(token: token, messageId: messageId)
    }

    // MARK: - Sections (docs/categories.md)

    /// "Favoriten", categories and "Chats" (or "Gruppen" and "Direktchats") from the core,
    /// already filtered; collapsed sections are empty. Reads `roomsEvents` and the setting,
    /// so a view using it redraws on every `RoomsChanged` (also a toggle) and when the
    /// setting changes.
    func sections(_ query: String) -> [SectionView] {
        _ = roomsEvents
        let split = Appearance.shared.splitChats
        return session?.roomSections(query: query, activeToken: activeToken, split: split) ?? []
    }

    /// "Sortierung" (docs/categories.md): `activity` or `alphabetical`. It lies with the
    /// account on the server; read with `roomsEvents`, another device may change it.
    var sortOrder: String {
        _ = roomsEvents
        return session?.sortOrder() ?? "activity"
    }

    /// The core shows the new order at once and takes it back when saving fails (toast).
    func setSortOrder(_ order: String) {
        guard let s = session else { return }
        Task { if let err = await s.setSortOrder(order: order) { Toast.show(err) } }
    }

    /// Opens or closes a section; the core shows it at once and saves it on the server
    /// (for all devices and Talk-Web). An error comes back as a toast.
    func toggleSection(_ id: String) {
        guard let s = session else { return }
        Task { if let err = await s.toggleSection(id: id) { Toast.show(err) } }
    }

    // MARK: - Chat menu (docs/settings.md)

    var categoriesSupported: Bool { session?.categoriesSupported() ?? false }

    /// Own categories in their order (read with `roomsEvents`, so menus stay current).
    func customCategories() -> [CategoryView] {
        _ = roomsEvents
        return session?.categories().filter { $0.kind == "custom" } ?? []
    }

    /// Runs a chat-menu change; the core's error text becomes a toast.
    func menuChange(_ f: @escaping (TalkSession) async -> String?) {
        guard let s = session else { return }
        Task { if let error = await f(s) { Toast.show(error) } }
    }

    func createCategory(_ name: String, with token: String) {
        guard let s = session else { return }
        Task {
            let r = await s.createCategory(name: name, tokens: [token])
            if let error = r.error { Toast.show(error) }
        }
    }

    /// "Gruppeneinstellungen" from the chat menu: pushed on the list, the chat stays closed.
    func openGroupSettings(_ token: String) {
        closeRoom()
        listPath = [.groupInfo(token)]
    }

    // MARK: - Groups (docs/groups.md)

    var canCreateGroups: Bool { session?.canCreateGroups() ?? false }

    func searchUsers(_ query: String, excluding token: String? = nil) async throws -> [UserView] {
        guard let s = session else { throw CancellationError() }
        return try await s.searchUsers(query: query, excludeToken: token)
    }

    func members(_ token: String) async throws -> [MemberView] {
        guard let s = session else { throw CancellationError() }
        return try await s.members(token: token)
    }

    func groupLink(_ token: String) -> String {
        session?.groupLink(token: token) ?? ""
    }

    /// The 1:1 chat with a user (created when missing), opened; `false` after a toast.
    func openDirect(_ userId: String) async -> Bool {
        guard let s = session else { return false }
        return open(await s.openDirect(userId: userId))
    }

    func createGroup(_ name: String, _ userIds: [String], photo: Data?) async -> Bool {
        guard let s = session else { return false }
        return open(await s.createGroup(name: name, userIds: userIds, photo: photo))
    }

    private func open(_ r: OpenResult) -> Bool {
        guard let token = r.token else {
            Toast.show(r.error ?? "Hat nicht geklappt")
            return false
        }
        openRoom(token)
        return true
    }

    /// Runs a group change; the core's error text becomes a toast. `true` on success.
    @discardableResult
    func groupChange(_ f: (TalkSession) async -> String?) async -> Bool {
        guard let s = session else { return false }
        if let error = await f(s) {
            Toast.show(error)
            return false
        }
        return true
    }

    /// Tapping "Gruppe verlassen": `true` → "Neuen Moderator wählen" first (the server's
    /// `canLeave` does not cover the last moderator).
    func leaveNeedsModerator(_ token: String) async throws -> Bool {
        guard let s = session else { throw CancellationError() }
        return try await s.leaveNeedsModerator(token: token)
    }

    /// Leave or delete: the chat closes quietly once the group is gone from the list.
    func leaveGroup(_ token: String, delete: Bool) async -> Bool {
        leaving.insert(token)
        let ok = await groupChange { delete ? await $0.deleteGroup(token: token) : await $0.leaveGroup(token: token) }
        if ok && activeToken == token { closeRoom() }
        leaving.remove(token)
        return ok
    }

    /// Marks the newest message read while the chat is visible and the app is in front.
    func markVisibleRead(_ token: String) {
        // In a window the read marker stays (docs/search.md).
        guard let s = session, foreground, chatAtBottom, activeToken == token, chats[token]?.hasNewer != true,
              let room = room(token),
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

/// Receives one account's core events on a Rust thread and forwards them to the main actor.
/// The events carry no account; only the listener knows whose they are.
fileprivate final class EventBridge: CoreListener, @unchecked Sendable {
    let account: String
    weak var store: SessionStore?

    init(account: String) {
        self.account = account
    }

    func onEvent(event: CoreEvent) {
        Task { @MainActor [weak self] in
            guard let self else { return }
            self.store?.handle(event, from: self.account)
        }
    }
}
