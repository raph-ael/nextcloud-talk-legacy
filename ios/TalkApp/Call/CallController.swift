import AVFoundation
@preconcurrency import CallKit
import Observation
import UIKit

/// The one call of the app: shows the core's `CallView`, drives CallKit, tones and audio.
///
/// The core decides everything about the call (docs/calls.md); this class only maps its
/// states to CallKit and the UI. User actions go through CallKit (so the system call UI,
/// lock screen and Bluetooth buttons stay in sync) and from there to the core.
@MainActor
@Observable
final class CallController: NSObject {
    static let shared = CallController()

    /// The call as shown; after `ended` it stays ~1.5 s with its end reason.
    private(set) var call: CallView? {
        didSet { if (oldValue == nil) != (call == nil) { CallWindow.shared.sync() } }
    }
    /// The full-screen call view is shown (else only the in-app bar).
    var presented = false {
        didSet { if oldValue != presented { CallWindow.shared.sync() } }
    }

    /// CallKit is used on devices; the simulator has no working CallKit audio.
    @ObservationIgnored let useCallKit: Bool
    @ObservationIgnored private var provider: CXProvider?
    @ObservationIgnored private let controller = CXCallController()
    /// The CallKit call and its conversation (one call at a time).
    @ObservationIgnored private var uuid: UUID?
    @ObservationIgnored private var uuidToken: String?
    /// CallKit already knows the call is over (ended through an action).
    @ObservationIgnored private var endedLocally = false
    @ObservationIgnored private var reportedConnecting = false
    @ObservationIgnored private var reportedConnected = false
    @ObservationIgnored private var lastState: CallState?
    @ObservationIgnored private var closeTask: Task<Void, Never>?
    /// Debug: a mock call is shown, core events are ignored.
    @ObservationIgnored var mocking = false

    private var session: TalkSession? { SessionStore.shared.session }

    /// Debug builds: `TALK_CALL_SILENT=1` calls without ringing anybody (tests in shared rooms).
    private var silent: Bool {
        #if DEBUG
        return ProcessInfo.processInfo.environment["TALK_CALL_SILENT"] == "1"
        #else
        return false
        #endif
    }

    override private init() {
        #if targetEnvironment(simulator)
        useCallKit = ProcessInfo.processInfo.environment["TALK_CALLKIT"] == "1"
        #else
        useCallKit = true
        #endif
        super.init()
        CallAudio.shared.selfManaged = !useCallKit
        if useCallKit {
            let p = CXProvider(configuration: Self.configuration())
            p.setDelegate(self, queue: nil)
            provider = p
        }
    }

    private static func configuration() -> CXProviderConfiguration {
        let c = CXProviderConfiguration()
        c.maximumCallGroups = 1
        c.maximumCallsPerCallGroup = 1
        c.supportsVideo = false
        c.supportedHandleTypes = [.generic]
        c.includesCallsInRecents = true
        c.iconTemplateImageData = CallKitIcon.png()
        // ringtoneSound nil: the system ringtone.
        return c
    }

    // MARK: User actions

    /// Call button in a chat: calls the conversation or joins its running call.
    func start(token: String, name: String) {
        if let call, call.state != .ended {
            if call.token == token { presented = true } else { Toast.show("Es läuft bereits ein Anruf") }
            return
        }
        guard session != nil else { return }
        guard AVAudioApplication.shared.recordPermission != .denied else {
            Toast.show("Mikrofon in den Einstellungen erlauben, um anzurufen.")
            return
        }
        presented = true
        guard useCallKit else {
            CallAudio.shared.beginCall()
            Task { await self.startInCore(token, callId: nil) }
            return
        }
        Notifications.askOnce()
        let id = UUID()
        uuid = id
        uuidToken = token
        endedLocally = false
        let action = CXStartCallAction(call: id, handle: CXHandle(type: .generic, value: token))
        action.contactIdentifier = name
        controller.request(CXTransaction(action: action)) { error in
            guard let error else { return }
            Task { @MainActor in
                Log.call.error("start transaction: \(error.localizedDescription, privacy: .public)")
                self.clearCallKit()
                self.presented = false
                Toast.show("Anruf nicht möglich")
            }
        }
    }

    @discardableResult
    private func startInCore(_ token: String, callId: UUID?) async -> Bool {
        if AVAudioApplication.shared.recordPermission == .undetermined {
            _ = await AVAudioApplication.requestRecordPermission()
        }
        guard let session else { return false }
        do {
            // The CallKit UUID becomes the core's call id.
            try await session.startCall(token: token, silent: silent, callId: callId?.uuidString)
            return true
        } catch {
            Log.call.error("startCall: \(error.localizedDescription, privacy: .public)")
            if call == nil {
                presented = false
                CallAudio.shared.stopAudio()
            }
            Toast.show(Self.message(error))
            return false
        }
    }

    /// In-app incoming screen (without CallKit).
    func accept() {
        guard let call, call.state == .incoming else { return }
        if useCallKit, let uuid {
            controller.request(CXTransaction(action: CXAnswerCallAction(call: uuid))) { _ in }
            return
        }
        CallAudio.shared.beginCall()
        presented = true
        Task { await self.acceptInCore(call.token) }
    }

    private func acceptInCore(_ token: String) async -> Bool {
        if AVAudioApplication.shared.recordPermission == .undetermined {
            _ = await AVAudioApplication.requestRecordPermission()
        }
        do {
            try await session?.acceptCall(token: token)
            return true
        } catch {
            Log.call.error("acceptCall: \(error.localizedDescription, privacy: .public)")
            Toast.show(Self.message(error))
            return false
        }
    }

    /// Hang up, cancel dialing or decline.
    func hangUp() {
        guard let call else { return }
        if call.state == .ended {
            close()
            return
        }
        if useCallKit, let uuid {
            controller.request(CXTransaction(action: CXEndCallAction(call: uuid))) { [weak self] error in
                guard error != nil else { return }
                // CallKit lost the call: end it in the core anyway.
                Task { @MainActor in self?.endInCore() }
            }
            return
        }
        endInCore()
    }

    private func endInCore() {
        guard let call, let session else { return }
        if call.state == .incoming {
            session.declineCall(token: call.token)
        } else {
            session.hangUp()
        }
    }

    func toggleMute() {
        guard let call, call.state != .ended else { return }
        let muted = !call.muted
        if useCallKit, let uuid {
            controller.request(CXTransaction(action: CXSetMutedCallAction(call: uuid, muted: muted))) { [weak self] error in
                guard error != nil else { return }
                Task { @MainActor in self?.session?.setCallMuted(muted: muted) }
            }
        } else {
            session?.setCallMuted(muted: muted)
        }
    }

    /// Closes the ended call's screen now.
    func close() {
        closeTask?.cancel()
        closeTask = nil
        if call?.state == .ended {
            call = nil
            presented = false
        }
    }

    // MARK: Core events

    func update(_ new: CallView?) {
        if mocking { return }
        guard let new else {
            #if DEBUG
            LogDump.write()
            #endif
            // `ended` came before; its screen closes by itself. Nothing else to show.
            if call?.state != .ended {
                call = nil
                presented = false
            }
            finishCallKit()
            return
        }
        if new.state != .ended { closeTask?.cancel() }
        let previous = lastState
        if previous != new.state {
            Log.call.info("call \(String(describing: new.state), privacy: .public) participants=\(new.participants.count) end=\(String(describing: new.endReason), privacy: .public)")
            #if DEBUG
            LogDump.write()
            #endif
        }
        let isNewCall = call?.id != new.id
        lastState = new.state
        call = new
        if isNewCall {
            reportedConnecting = false
            reportedConnected = false
        }
        tones(previous: isNewCall ? nil : previous, new)
        callKit(new)
        switch new.state {
        case .incoming:
            if !useCallKit { presented = true }
            #if DEBUG
            if isNewCall, let delay = DebugEnv.sticky("TALK_AUTOANSWER").flatMap(Double.init) {
                let id = new.id
                Task {
                    try? await Task.sleep(for: .seconds(delay))
                    guard self.call?.id == id, self.call?.state == .incoming else { return }
                    Log.call.info("autoanswer")
                    self.accept()
                }
            }
            #endif
        case .connecting, .connected, .reconnecting:
            if previous == .incoming || isNewCall { presented = true }
        case .ended:
            ended(new)
        case .dialing, .ringing:
            break
        }
    }

    private func ended(_ c: CallView) {
        CallAudio.shared.stopAudio()
        let quiet: Bool
        switch c.endReason {
        case .answeredElsewhere, .missed: quiet = true
        // The callee declined (1:1): the caller sees "Abgelehnt"; declined here stays quiet.
        case .declined: quiet = c.direction == .incoming
        default: quiet = c.direction == .incoming && previousWasIncoming(c)
        }
        if quiet {
            call = nil
            presented = false
            return
        }
        if case .failed(_, let message) = c.endReason, !message.isEmpty { Toast.show(message) }
        let id = c.id
        closeTask?.cancel()
        closeTask = Task {
            try? await Task.sleep(for: .milliseconds(1500))
            guard !Task.isCancelled, self.call?.id == id, self.call?.state == .ended else { return }
            self.call = nil
            self.presented = false
        }
    }

    /// An incoming call that never got answered here ends without a screen.
    private func previousWasIncoming(_ c: CallView) -> Bool {
        c.startedAt == nil && c.participants.isEmpty && !presented
    }

    private func tones(previous: CallState?, _ c: CallView) {
        let t = CallTones.shared
        if c.state == .ringing && !c.isGroup && c.direction == .outgoing {
            t.play(.ringback)
        } else {
            t.stop(.ringback)
        }
        if c.state == .connected && previous != .connected && previous != .reconnecting {
            t.play(.connected)
        }
        if c.state == .ended && previous != nil && previous != .incoming {
            if case .answeredElsewhere = c.endReason {} else { t.play(.ended) }
        }
    }

    // MARK: CallKit reports

    private func callKit(_ c: CallView) {
        guard useCallKit, let provider else { return }
        if c.state == .incoming && uuid == nil {
            reportIncoming(c)
            return
        }
        guard let uuid, uuidToken == c.token else { return }
        switch c.state {
        case .ringing, .connecting:
            if c.direction == .outgoing && !reportedConnecting {
                reportedConnecting = true
                provider.reportOutgoingCall(with: uuid, startedConnectingAt: nil)
            }
        case .connected:
            if c.direction == .outgoing && !reportedConnected {
                reportedConnected = true
                if !reportedConnecting { provider.reportOutgoingCall(with: uuid, startedConnectingAt: nil) }
                provider.reportOutgoingCall(with: uuid, connectedAt: nil)
            }
        case .ended:
            if !endedLocally {
                provider.reportCall(with: uuid, endedAt: nil, reason: Self.reason(c.endReason, c.direction))
            }
            clearCallKit()
        default:
            break
        }
    }

    private func reportIncoming(_ c: CallView) {
        guard let provider else { return }
        let id = UUID(uuidString: c.id) ?? UUID()
        uuid = id
        uuidToken = c.token
        endedLocally = false
        let update = Self.update(c)
        provider.reportNewIncomingCall(with: id, update: update) { [weak self] error in
            guard let error else { return }
            // Do not disturb, blocked or another call: this device does not ring.
            Task { @MainActor in
                Log.call.info("incoming not shown: \(error.localizedDescription, privacy: .public)")
                guard let self, self.uuid == id else { return }
                self.clearCallKit()
                self.session?.declineCall(token: c.token)
            }
        }
    }

    /// PushKit: iOS requires a CallKit report for every VoIP push, right away. A push that
    /// cannot be read (or is no call) shows and ends at once. A call push is reported with a
    /// new UUID that the core takes over through `handlePush`.
    func reportPushed(_ message: PushMessage?, completion: @escaping () -> Void) {
        guard let provider else { return completion() }
        var token: String?
        var name = "Talk"
        if case .call(let t, _, let subject)? = message {
            token = t
            name = pushCallerName(subject: subject)
        }
        // notify_push was faster: report that call again (CallKit answers that it exists,
        // the push is accounted for).
        let known = token != nil && uuid != nil && uuidToken == token
        let adopt = token != nil && uuid == nil
        let id = known ? uuid! : UUID()
        if adopt {
            uuid = id
            uuidToken = token
            endedLocally = false
            reportedConnecting = false
            reportedConnected = false
        }
        provider.reportNewIncomingCall(with: id, update: Self.update(handle: token ?? "talk", name: name)) { [weak self] error in
            Task { @MainActor in
                completion()
                guard let self, !known else { return }
                if let error {
                    // Do not disturb, blocked: this device does not ring.
                    Log.push.info("pushed call not shown: \(error.localizedDescription, privacy: .public)")
                    if self.uuid == id { self.clearCallKit() }
                    return
                }
                guard let token, let message, adopt, let session = self.session else {
                    // Unreadable, no call, or another call runs here: end it; the core still
                    // learns of the push (deletions, missed second call).
                    provider.reportCall(with: id, endedAt: nil, reason: token == nil ? .failed : .unanswered)
                    if self.uuid == id { self.clearCallKit() }
                    if let message, let session = self.session {
                        _ = await session.handlePush(message: message, callId: nil)
                    }
                    return
                }
                let rang = await session.handlePush(message: message, callId: id.uuidString)
                Log.push.info("handlePush rang=\(rang)")
                if !rang, session.currentCall()?.token != token, self.uuid == id {
                    provider.reportCall(with: id, endedAt: nil, reason: .unanswered)
                    self.clearCallKit()
                }
            }
        }
    }

    private static func update(handle: String, name: String) -> CXCallUpdate {
        let u = CXCallUpdate()
        u.remoteHandle = CXHandle(type: .generic, value: handle)
        u.localizedCallerName = name
        u.hasVideo = false
        u.supportsHolding = false
        u.supportsGrouping = false
        u.supportsUngrouping = false
        u.supportsDTMF = false
        return u
    }

    private static func update(_ c: CallView) -> CXCallUpdate {
        update(handle: c.token, name: c.isGroup || c.callerName.isEmpty ? c.roomName : c.callerName)
    }

    private static func reason(_ r: EndReason?, _ direction: CallDirection) -> CXCallEndedReason {
        switch r {
        case .remoteHangup, .hangup, .none: return .remoteEnded
        case .noAnswer, .missed: return .unanswered
        case .answeredElsewhere: return .answeredElsewhere
        // Outgoing: the other side declined; incoming: declined on another of our devices.
        case .declined: return direction == .outgoing ? .remoteEnded : .declinedElsewhere
        case .failed: return .failed
        }
    }

    private func finishCallKit() {
        if let uuid, let provider, !endedLocally {
            provider.reportCall(with: uuid, endedAt: nil, reason: .remoteEnded)
        }
        clearCallKit()
    }

    private func clearCallKit() {
        uuid = nil
        uuidToken = nil
        endedLocally = false
        reportedConnecting = false
        reportedConnected = false
    }

    /// Call errors reach Swift as `Server("call: <code>: <message>")` (no own case in the FFI yet).
    static func message(_ error: Error) -> String {
        if case CoreError.Server(let detail) = error, detail.hasPrefix("call: ") {
            let parts = detail.dropFirst(6).split(separator: ":", maxSplits: 1)
            if parts.count == 2 { return parts[1].trimmingCharacters(in: .whitespaces) }
        }
        if case CoreError.Network = error { return "Keine Verbindung" }
        return "Anruf nicht möglich"
    }
}

#if DEBUG
extension CallController {
    /// `TALK_CALL_MOCK=dialing|ringing|connected|reconnecting|group|incoming|ended` shows
    /// the call screen with a fake call (screenshots, layout checks). No media, no core.
    /// `TALK_AUTOCALL=<token>` calls that conversation after launch; `TALK_AUTOHANGUP=<s>`
    /// hangs up after that many seconds (automated call tests).
    func autoCallIfRequested() {
        let env = ProcessInfo.processInfo.environment
        guard let token = env["TALK_AUTOCALL"], !token.isEmpty else { return }
        Task {
            for _ in 0..<30 where SessionStore.shared.room(token) == nil {
                try? await Task.sleep(for: .seconds(1))
            }
            for _ in 0..<40 where call != nil {
                try? await Task.sleep(for: .seconds(1))
            }
            Log.call.info("autocall \(token, privacy: .public)")
            start(token: token, name: SessionStore.shared.room(token)?.name ?? "")
            if let s = env["TALK_AUTOHANGUP"].flatMap(Double.init) {
                try? await Task.sleep(for: .seconds(s))
                Log.call.info("autohangup")
                hangUp()
            }
        }
    }

    func showMockIfRequested() {
        guard let mode = ProcessInfo.processInfo.environment["TALK_CALL_MOCK"], !mode.isEmpty else { return }
        let room = SessionStore.shared.rooms.first { !$0.isGroup } ?? SessionStore.shared.rooms.first
        let names = ["Anna Berg", "Bernd Krause", "Chiara Rossi", "Daniel Ohl", "Eva Lind"]
        let group = mode == "group"
        let state: CallState = switch mode {
        case "dialing": .dialing
        case "ringing": .ringing
        case "reconnecting": .reconnecting
        case "incoming": .incoming
        case "ended": .ended
        default: .connected
        }
        let participants = group
            ? names.enumerated().map { i, n in
                CallParticipant(sessionId: "s\(i)", userId: "", name: n, muted: i == 2, speaking: i == 1, connected: i != 4)
            }
            : [CallParticipant(sessionId: "s0", userId: "", name: room?.name ?? "Anna", muted: false, speaking: true, connected: true)]
        let view = CallView(
            id: UUID().uuidString, token: room?.token ?? "mock", roomName: group ? "Projektgruppe" : (room?.name ?? "Anna Berg"),
            isGroup: group, direction: state == .incoming ? .incoming : .outgoing, state: state,
            callerName: room?.name ?? "Anna Berg",
            startedAt: state == .connected ? Int64(Date().timeIntervalSince1970 * 1000) - 192_000 : nil,
            muted: mode == "reconnecting", participants: participants,
            endReason: state == .ended ? .noAnswer : nil
        )
        mocking = true
        call = view
        presented = ProcessInfo.processInfo.environment["TALK_CALL_MOCK_BAR"] != "1"
    }
}
#endif

// MARK: - CXProviderDelegate

extension CallController: CXProviderDelegate {
    nonisolated func providerDidReset(_ provider: CXProvider) {
        MainActor.assumeIsolated {
            Log.call.error("CallKit reset")
            session?.hangUp()
            clearCallKit()
            CallAudio.shared.stopAudio()
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXStartCallAction) {
        MainActor.assumeIsolated {
            let token = action.handle.value
            CallAudio.shared.beginCall()
            let update = CXCallUpdate()
            update.remoteHandle = action.handle
            update.localizedCallerName = action.contactIdentifier
            update.hasVideo = false
            update.supportsHolding = false
            update.supportsGrouping = false
            update.supportsUngrouping = false
            update.supportsDTMF = false
            Task {
                if await self.startInCore(token, callId: action.callUUID) {
                    action.fulfill()
                    provider.reportCall(with: action.callUUID, updated: update)
                } else {
                    action.fail()
                    if self.uuid == action.callUUID { self.clearCallKit() }
                }
            }
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXAnswerCallAction) {
        MainActor.assumeIsolated {
            guard let token = uuidToken, uuid == action.callUUID else {
                action.fail()
                return
            }
            CallAudio.shared.beginCall()
            presented = true
            Task {
                // Answered right after a push: the core may still be taking the call over.
                for _ in 0..<50 where self.call?.token != token || self.call?.state != .incoming {
                    try? await Task.sleep(for: .milliseconds(100))
                }
                if await self.acceptInCore(token) { action.fulfill() } else { action.fail() }
            }
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXEndCallAction) {
        MainActor.assumeIsolated {
            if uuid == action.callUUID {
                endedLocally = true
                endInCore()
            }
            action.fulfill()
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXSetMutedCallAction) {
        MainActor.assumeIsolated {
            session?.setCallMuted(muted: action.isMuted)
            action.fulfill()
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXSetHeldCallAction) {
        // No holding (another call ends ours, like Signal).
        action.fail()
    }

    nonisolated func provider(_ provider: CXProvider, didActivate audioSession: AVAudioSession) {
        MainActor.assumeIsolated { CallAudio.shared.didActivate(audioSession) }
    }

    nonisolated func provider(_ provider: CXProvider, didDeactivate audioSession: AVAudioSession) {
        MainActor.assumeIsolated { CallAudio.shared.didDeactivate(audioSession) }
    }
}

/// CallKit's template icon (white on transparent), drawn here: a speech bubble.
enum CallKitIcon {
    static func png() -> Data? {
        let size = CGSize(width: 40, height: 40)
        let format = UIGraphicsImageRendererFormat()
        format.scale = 3
        let img = UIGraphicsImageRenderer(size: size, format: format).image { _ in
            UIColor.white.setFill()
            let bubble = UIBezierPath(roundedRect: CGRect(x: 3, y: 5, width: 34, height: 24), cornerRadius: 10)
            bubble.fill()
            let tail = UIBezierPath()
            tail.move(to: CGPoint(x: 10, y: 26))
            tail.addLine(to: CGPoint(x: 8, y: 36))
            tail.addLine(to: CGPoint(x: 19, y: 28))
            tail.close()
            tail.fill()
        }
        return img.pngData()
    }
}
