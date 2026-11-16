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
        didSet {
            if (oldValue == nil) != (call == nil) { CallWindow.shared.sync() }
            syncOrientation()
        }
    }
    /// The full-screen call view is shown (else only the in-app bar).
    var presented = false {
        didSet {
            guard oldValue != presented else { return }
            CallWindow.shared.sync()
            applyCamera()
            syncOrientation()
        }
    }
    /// The call screen shows a shared screen: the app may turn to landscape
    /// (docs/calls.md "Bildschirmfreigabe", `AppDelegate`); everything else stays portrait.
    @ObservationIgnored private(set) var allowsLandscape = false

    /// CallKit is used on devices; the simulator has no working CallKit audio.
    @ObservationIgnored let useCallKit: Bool
    @ObservationIgnored private var provider: CXProvider?
    @ObservationIgnored private let controller = CXCallController()
    /// The CallKit call and its conversation (one call at a time).
    @ObservationIgnored private var uuid: UUID?
    @ObservationIgnored private var uuidToken: String?
    @ObservationIgnored private var uuidAccount: String?
    /// CallKit already knows the call is over (ended through an action).
    @ObservationIgnored private var endedLocally = false
    @ObservationIgnored private var reportedConnecting = false
    @ObservationIgnored private var reportedConnected = false
    @ObservationIgnored private var lastState: CallState?
    @ObservationIgnored private var closeTask: Task<Void, Never>?
    /// When CallKit started ringing for the current incoming call.
    @ObservationIgnored private var ringingSince: Date?
    /// The running recording of the current call was announced (toast, tone, VoiceOver).
    @ObservationIgnored private var announcedRecording = false
    /// Debug: a mock call is shown, core events are ignored.
    @ObservationIgnored var mocking = false
    /// The user switched the camera on (docs/calls.md "Video"). It runs only while the call
    /// screen is shown and the app is in the foreground; otherwise it is paused, which the
    /// others see as "video off".
    private(set) var wantCamera = false
    /// What the core was last told (`setCameraEnabled`).
    @ObservationIgnored private var sentCamera = false
    @ObservationIgnored private var foreground = true
    @ObservationIgnored private var reportedVideo = false
    @ObservationIgnored private var keepsScreenOn = false

    /// The account of the call (docs/accounts.md "Anrufe"): accept, decline, hang up, mute,
    /// camera and every media report go to its runtime, not to the active one.
    private(set) var account: String?

    private var session: TalkSession? {
        account.flatMap { SessionStore.shared.runtime($0)?.session } ?? SessionStore.shared.session
    }

    /// A call rings or runs (also one reported from a push the core has not taken over
    /// yet): switching accounts is locked.
    var busy: Bool { (call.map { $0.state != .ended } ?? false) || uuid != nil }

    /// "{Name} · {Server}" of the call's account, from two accounts on.
    var accountLine: String? {
        let store = SessionStore.shared
        guard store.accounts.count > 1 else { return nil }
        return store.runtime(account)?.account.line
    }

    /// The media engine reports to the session of the call.
    private func follow(_ id: String?) {
        account = id
        SessionStore.shared.callMedia.session = session
    }

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
        let nc = NotificationCenter.default
        nc.addObserver(forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.foreground = false
                self?.applyCamera()
            }
        }
        nc.addObserver(forName: UIApplication.willEnterForegroundNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.foreground = true
                self?.applyCamera()
            }
        }
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
        c.supportsVideo = true
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
        guard let active = SessionStore.shared.activeId else { return }
        guard AVAudioApplication.shared.recordPermission != .denied else {
            Toast.show("Mikrofon in den Einstellungen erlauben, um anzurufen.")
            return
        }
        follow(active)
        presented = true
        guard useCallKit else {
            CallAudio.shared.beginCall()
            Task { await self.startInCore(token, callId: nil) }
            return
        }
        let id = UUID()
        uuid = id
        uuidToken = token
        uuidAccount = active
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
            controller.request(CXTransaction(action: CXAnswerCallAction(call: uuid))) { error in
                guard let error else { return }
                Log.call.error("answer transaction: \(error.localizedDescription, privacy: .public)")
            }
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
            let id = account
            try await session?.acceptCall(token: token)
            // Accepting a call of another account switches there afterwards.
            if let id { SessionStore.shared.followCall(id) }
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
        #if DEBUG
        if mocking {
            self.call = nil
            presented = false
            return
        }
        #endif
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
            decline(call.token, session)
        } else {
            session.hangUp()
        }
    }

    /// Declines a ringing call. In a 1:1 the core then ends the call for the caller
    /// ("Abgelehnt"), a request of up to 10 s after `ended(declined)`; iOS suspends an app
    /// that PushKit woke right after the CallKit action, so the process is kept running
    /// until it is out (docs/calls.md "Wenn das System die App nach dem Ablehnen anhält").
    private func decline(_ token: String, _ session: TalkSession) {
        let done = holdBackground(name: "decline")
        Task {
            await session.declineCallAndWait(token: token, timeoutMs: 12_000)
            done()
        }
    }

    /// The system ended the ringing itself (CallKit's time limit) or would not show the call
    /// (focus, blocked): no decline, a missed call, ended in the core before iOS suspends
    /// the app (docs/calls.md "Wenn das System das Klingeln selbst beendet").
    private func miss(_ token: String, _ session: TalkSession) {
        let done = holdBackground(name: "miss")
        Task {
            await session.missCallAndWait(token: token, timeoutMs: 2_000)
            done()
        }
    }

    /// Keeps the process running after a CallKit action until the returned function is
    /// called (or iOS takes the time back), so the core's work that follows goes out.
    private func holdBackground(name: String) -> () -> Void {
        let app = UIApplication.shared
        var id = UIBackgroundTaskIdentifier.invalid
        let end = {
            guard id != .invalid else { return }
            app.endBackgroundTask(id)
            id = .invalid
        }
        id = app.beginBackgroundTask(withName: name) { end() }
        return { Task { @MainActor in end() } }
    }

    // MARK: Camera

    /// "Kamera einschalten" / "Kamera ausschalten". Asks for the camera permission the first time.
    func toggleCamera() {
        guard let call, call.state != .ended, call.state != .incoming else { return }
        #if DEBUG
        if mocking {
            wantCamera.toggle()
            CallVideo.shared.setLocal(wantCamera ? WebRTCMediaEngine.mockTrack(front: true) : nil, front: true)
            return
        }
        #endif
        if wantCamera {
            wantCamera = false
            applyCamera()
            return
        }
        #if !(DEBUG && targetEnvironment(simulator))
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            break
        case .notDetermined:
            Task {
                guard await AVCaptureDevice.requestAccess(for: .video) else {
                    Toast.show("Kamera in den Einstellungen erlauben, um sie im Anruf zu nutzen.")
                    return
                }
                guard self.call?.id == call.id else { return }
                self.wantCamera = true
                self.applyCamera()
            }
            return
        default:
            Toast.show("Kamera in den Einstellungen erlauben, um sie im Anruf zu nutzen.")
            return
        }
        #endif
        wantCamera = true
        applyCamera()
    }

    /// "Kamera wechseln" (front ↔ back) on the own preview.
    func switchCamera() {
        #if DEBUG
        if mocking {
            let front = !CallVideo.shared.localFront
            CallVideo.shared.setLocal(WebRTCMediaEngine.mockTrack(front: front), front: front)
            return
        }
        #endif
        SessionStore.shared.callMedia.switchCamera()
    }

    /// The engine could not open the camera or lost it: off, with the reason.
    func cameraFailed(_ message: String) {
        guard wantCamera else { return }
        wantCamera = false
        sentCamera = false
        applyCamera()
        Toast.show(message)
    }

    /// Tells the core whether the camera runs (wanted, call screen shown, app in the
    /// foreground) and follows up audio, screen lock and CallKit.
    private func applyCamera() {
        guard !mocking else { return }
        let active = call.map { $0.state != .ended && $0.state != .incoming } ?? false
        if !active { wantCamera = false }
        let run = wantCamera && presented && foreground
        if run != sentCamera, let session, call != nil {
            sentCamera = run
            Log.call.info("camera \(run ? "on" : "off", privacy: .public) (wanted \(self.wantCamera), shown \(self.presented), foreground \(self.foreground))")
            session.setCameraEnabled(on: run)
        }
        if active { CallAudio.shared.setVideo(wantCamera) }
        // The screen stays on while video runs (own, from the others or a shared screen).
        let screenOn = active && (wantCamera || call?.participants.contains { $0.video } == true
            || call?.screens.isEmpty == false)
        if screenOn != keepsScreenOn {
            keepsScreenOn = screenOn
            UIApplication.shared.isIdleTimerDisabled = screenOn
        }
        if active, wantCamera != reportedVideo, useCallKit, let uuid, let provider {
            reportedVideo = wantCamera
            let u = CXCallUpdate()
            u.hasVideo = wantCamera
            provider.reportCall(with: uuid, updated: u)
        }
    }

    /// Landscape only while the call screen shows a shared screen; back to portrait after.
    private func syncOrientation() {
        let allow = presented && call.map { !$0.screens.isEmpty && $0.state != .ended } == true
        guard allow != allowsLandscape else { return }
        allowsLandscape = allow
        Log.call.info("orientation: \(allow ? "landscape allowed" : "portrait", privacy: .public)")
        for scene in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }) {
            for w in scene.windows { w.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations() }
            if !allow { scene.requestGeometryUpdate(.iOS(interfaceOrientations: .portrait)) }
        }
    }

    // MARK: Recording

    /// "Audio aufnehmen" / "Video aufnehmen" (only with `canRecord`).
    func startRecording(video: Bool) {
        guard let session, call?.canRecord == true else { return }
        Task {
            do { try await session.startRecording(video: video) } catch {
                Log.call.error("startRecording: \(error.localizedDescription, privacy: .public)")
                Toast.show(Self.message(error, fallback: "Die Aufnahme konnte nicht gestartet werden"))
            }
        }
    }

    /// "Aufnahme beenden" (after the confirmation).
    func stopRecording() {
        guard let session else { return }
        Task {
            do { try await session.stopRecording() } catch {
                Log.call.error("stopRecording: \(error.localizedDescription, privacy: .public)")
                Toast.show(Self.message(error, fallback: "Die Aufnahme konnte nicht beendet werden"))
            }
        }
    }

    /// "Zustimmen und beitreten": the core joins the call it holds in `connecting`.
    func consentToRecording() {
        #if DEBUG
        if mocking, let c = call {
            var c2 = c
            c2.recordingConsentPending = false
            call = c2
            return
        }
        #endif
        session?.consentToRecording()
    }

    /// Toast, soft tone and VoiceOver, once per running recording (also when joining one).
    private func announceRecording(_ c: CallView, isNewCall: Bool) {
        if isNewCall { announcedRecording = false }
        guard RecordingText.isActive(c.recording) else {
            announcedRecording = false
            return
        }
        guard !announcedRecording, c.state == .connected || c.state == .reconnecting else { return }
        announcedRecording = true
        Log.call.info("recording announced (\(String(describing: c.recording), privacy: .public))")
        Toast.show(RecordingText.announcement)
        UIAccessibility.post(notification: .announcement, argument: RecordingText.announcement)
        // After the "connected" tone when both come together.
        Task {
            try? await Task.sleep(for: .milliseconds(400))
            CallTones.shared.play(.recording)
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

    func update(_ new: CallView?, account id: String) {
        if mocking { return }
        // `call: null` always comes from the call's account; another one's is no news.
        if new == nil, call != nil, let account, account != id { return }
        if let new, new.state != .ended, account != id || call?.id != new.id { follow(id) }
        guard let new else {
            #if DEBUG
            LogDump.write(sync: UIApplication.shared.applicationState == .background)
            #endif
            // `ended` came before; its screen closes by itself. Nothing else to show.
            if call?.state != .ended {
                call = nil
                presented = false
            }
            finishCallKit()
            wantCamera = false
            sentCamera = false
            applyCamera()
            return
        }
        if new.state != .ended { closeTask?.cancel() }
        let previous = lastState
        if previous != new.state {
            Log.call.info("call \(String(describing: new.state), privacy: .public) account=\(self.account ?? "-", privacy: .public) participants=\(new.participants.count) end=\(String(describing: new.endReason), privacy: .public)")
            #if DEBUG
            // In the background iOS suspends the app right after a call ends.
            LogDump.write(sync: UIApplication.shared.applicationState == .background)
            #endif
        }
        let isNewCall = call?.id != new.id
        if (call?.screens.map(\.peerId) ?? []) != new.screens.map(\.peerId) {
            Log.call.info("screens: \(new.screens.map(\.peerId).joined(separator: ","), privacy: .public)")
        }
        lastState = new.state
        call = new
        if isNewCall {
            reportedConnecting = false
            reportedConnected = false
            // Every call starts without camera, also when answering.
            wantCamera = false
            sentCamera = false
            reportedVideo = false
        }
        tones(previous: isNewCall ? nil : previous, new)
        announceRecording(new, isNewCall: isNewCall)
        callKit(new)
        applyCamera()
        switch new.state {
        case .incoming:
            if !useCallKit { presented = true }
            #if DEBUG
            if isNewCall, let delay = DebugEnv.sticky("TALK_AUTOANSWER").flatMap(Double.init) {
                let id = new.id
                Task {
                    try? await Task.sleep(for: .seconds(delay))
                    guard self.call?.id == id, self.call?.state == .incoming else { return }
                    Log.call.info("autoanswer state=\(UIApplication.shared.applicationState.rawValue)")
                    self.accept()
                    try? await Task.sleep(for: .seconds(1))
                    LogDump.write()
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
        uuidAccount = account
        ringingSince = Date()
        endedLocally = false
        provider.reportNewIncomingCall(with: id, update: update(c)) { [weak self] error in
            guard let error else { return }
            // Do not disturb, blocked or another call: this device does not ring. Focus and
            // block list are no decline: the caller keeps ringing, here it is missed.
            Task { @MainActor in
                Log.call.info("incoming not shown: \(error.localizedDescription, privacy: .public)")
                guard let self, self.uuid == id else { return }
                self.clearCallKit()
                guard let session = self.session else { return }
                if Self.filtered(error) { self.miss(c.token, session) } else { self.decline(c.token, session) }
            }
        }
    }

    /// PushKit: iOS requires a CallKit report for every VoIP push, right away. A push that
    /// cannot be read (or is no call, or its account is gone) shows and ends at once. A call
    /// push is reported with a new UUID that the core of its account takes over through
    /// `handlePush`.
    func reportPushed(_ message: PushMessage?, account raw: String?, completion: @escaping () -> Void) {
        guard let provider else { return completion() }
        // The store comes up here on a cold start, with the runtimes of all accounts.
        let rt = message == nil ? nil : SessionStore.shared.runtime(raw)
        let pushed = rt?.account.id
        var token: String?
        var name = "Wolkenfunk"
        if case .call(let t, _, let subject)? = message {
            token = t
            name = pushCallerName(subject: subject)
            if SessionStore.shared.accounts.count > 1, let line = rt?.account.line { name = "\(name) (\(line))" }
        }
        // notify_push was faster: report that call again (CallKit answers that it exists,
        // the push is accounted for). The same call: same token in the same account, or in
        // an account of the same server (two accounts in one room ring once).
        let known = token != nil && uuid != nil && uuidToken == token && Self.sameServer(uuidAccount, pushed)
        let adopt = token != nil && uuid == nil && rt != nil
        let id = known ? uuid! : UUID()
        if adopt {
            uuid = id
            uuidToken = token
            uuidAccount = pushed
            ringingSince = Date()
            endedLocally = false
            reportedConnecting = false
            reportedConnected = false
            if call == nil { follow(pushed) }
        }
        let ringing = uuidAccount
        provider.reportNewIncomingCall(with: id, update: Self.update(handle: token ?? "talk", name: name)) { [weak self] error in
            Task { @MainActor in
                completion()
                guard let self, !known else {
                    // The same call in a second account of that server: its core learns of
                    // the push and stays quiet.
                    if known, let message, let rt, rt.account.id != ringing {
                        _ = await rt.session.handlePush(message: message, callId: nil)
                    }
                    return
                }
                if let error {
                    // Do not disturb, blocked: this device does not ring.
                    Log.push.info("pushed call not shown: \(error.localizedDescription, privacy: .public)")
                    if self.uuid == id { self.clearCallKit() }
                    // Turned away by the system: "Verpasster Anruf" (docs/notifications.md "Anrufe").
                    if Self.filtered(error), let token, let rt {
                        let room = rt.rooms.first { $0.token == token }
                        let group = room.map { $0.roomType == 2 || $0.roomType == 3 } ?? false
                        let caller = message.map { m in
                            if case .call(_, _, let subject) = m { pushCallerName(subject: subject) } else { "" }
                        } ?? ""
                        let shown = group || caller.isEmpty ? (room?.name ?? caller) : caller
                        Notifications.missedCall(SessionStore.shared.origin(rt.account.id), token: token, name: shown)
                        // The core may ring for it already (notify_push) or would on the push:
                        // it learns of the push and ends the call here as missed, before iOS
                        // suspends the app (its own clock would run out only on the next wake).
                        // Its `CallMissed` falls within the ten minutes of the notification above.
                        let done = self.holdBackground(name: "miss")
                        if let message { _ = await rt.session.handlePush(message: message, callId: nil) }
                        await rt.session.missCallAndWait(token: token, timeoutMs: 2_000)
                        done()
                    }
                    return
                }
                guard let token, let message, adopt, let session = rt?.session else {
                    // Unreadable, no call, account gone or another call runs here: end it;
                    // the core still learns of the push (deletions, missed second call).
                    provider.reportCall(with: id, endedAt: nil, reason: token == nil ? .failed : .unanswered)
                    if self.uuid == id { self.clearCallKit() }
                    if let message, let rt {
                        _ = await rt.session.handlePush(message: message, callId: nil)
                    }
                    return
                }
                let rang = await session.handlePush(message: message, callId: id.uuidString)
                Log.push.info("handlePush rang=\(rang)")
                // `false`: nothing rings for it, e.g. the same call already rings in another
                // account. Unless this account's call is that token, end the reported call.
                if !rang, session.currentCall()?.token != token, self.uuid == id {
                    provider.reportCall(with: id, endedAt: nil, reason: .unanswered)
                    self.clearCallKit()
                }
            }
        }
    }

    /// CallKit would not show the call because of a focus or the block list.
    private static func filtered(_ error: Error) -> Bool {
        let e = error as NSError
        return e.domain == CXErrorDomainIncomingCall
            && (e.code == CXErrorCodeIncomingCallError.filteredByDoNotDisturb.rawValue
                || e.code == CXErrorCodeIncomingCallError.filteredByBlockList.rawValue)
    }

    private static func sameServer(_ a: String?, _ b: String?) -> Bool {
        if a == b { return true }
        let store = SessionStore.shared
        guard let x = store.runtime(a)?.account.server, let y = store.runtime(b)?.account.server else { return false }
        return x == y
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

    /// From two accounts on, the ringing names the account.
    private func update(_ c: CallView) -> CXCallUpdate {
        let name = c.isGroup || c.callerName.isEmpty ? c.roomName : c.callerName
        return Self.update(handle: c.token, name: accountLine.map { "\(name) (\($0))" } ?? name)
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
        ringingSince = nil
        uuidToken = nil
        uuidAccount = nil
        endedLocally = false
        reportedConnecting = false
        reportedConnected = false
    }

    /// Call errors reach Swift as `Server("call: <code>: <message>")` (no own case in the FFI yet).
    static func message(_ error: Error, fallback: String = "Anruf nicht möglich") -> String {
        if case CoreError.Server(let detail) = error, detail.hasPrefix("call: ") {
            let parts = detail.dropFirst(6).split(separator: ":", maxSplits: 1)
            if parts.count == 2 { return parts[1].trimmingCharacters(in: .whitespaces) }
        }
        if case CoreError.Network = error { return "Keine Verbindung" }
        return fallback
    }
}

#if DEBUG
extension CallController {
    /// `TALK_CALL_MOCK=dialing|ringing|connected|reconnecting|group|incoming|ended|consent`
    /// shows the call screen with a fake call (screenshots, layout checks). No media, no
    /// core. `TALK_CALL_MOCK_REC=off|startingAudio|audio|video|failed` sets its recording
    /// state, `TALK_CALL_MOCK_CAN_RECORD=1` shows the recording controls.
    /// `TALK_CALL_MOCK_VIDEO=1` gives the others a moving test picture (in a group every
    /// participant but the last two) and switches our camera on (own preview).
    /// `TALK_CALL_MOCK_SCREEN=1|2` lets the first one (or two) participants share a screen.
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
        let env = ProcessInfo.processInfo.environment
        let recording: CallRecording = switch env["TALK_CALL_MOCK_REC"] ?? "" {
        case "startingAudio": .startingAudio
        case "startingVideo": .startingVideo
        case "audio": .audio
        case "video": .video
        case "failed": .failed
        default: .off
        }
        let state: CallState = switch mode {
        case "consent": .connecting
        case "dialing": .dialing
        case "ringing": .ringing
        case "reconnecting": .reconnecting
        case "incoming": .incoming
        case "ended": .ended
        default: .connected
        }
        let video = env["TALK_CALL_MOCK_VIDEO"] == "1"
        let participants = group
            ? names.enumerated().map { i, n in
                CallParticipant(sessionId: "s\(i)", userId: "", name: n, muted: i == 2, speaking: i == 1, connected: i != 4,
                                video: video && i < 3, peerId: video && i < 3 ? "mock\(i)" : nil)
            }
            : [CallParticipant(sessionId: "s0", userId: "", name: room?.name ?? "Anna", muted: false, speaking: true, connected: true,
                               video: video, peerId: video ? "mock0" : nil)]
        if video {
            for p in participants {
                if let id = p.peerId { CallVideo.shared.setRemote(id, WebRTCMediaEngine.mockTrack(front: p.sessionId != "s1")) }
            }
            CallVideo.shared.setLocal(WebRTCMediaEngine.mockTrack(front: true), front: true)
            wantCamera = true
        }
        let screenCount = min(participants.count, Int(env["TALK_CALL_MOCK_SCREEN"] ?? "") ?? 0)
        let screens = participants.prefix(screenCount).enumerated().map { i, p in
            CallScreen(sessionId: p.sessionId, name: p.name, peerId: "mockscreen\(i)")
        }
        for s in screens { CallVideo.shared.setRemote(s.peerId, WebRTCMediaEngine.mockTrack(front: true, screen: true)) }
        let view = CallView(
            id: UUID().uuidString, token: room?.token ?? "mock", roomName: group ? "Projektgruppe" : (room?.name ?? "Anna Berg"),
            isGroup: group, direction: state == .incoming ? .incoming : .outgoing, state: state,
            callerName: room?.name ?? "Anna Berg",
            startedAt: state == .connected ? Int64(Date().timeIntervalSince1970 * 1000) - 192_000 : nil,
            muted: mode == "reconnecting", cameraOn: video, screens: screens, participants: participants,
            endReason: state == .ended ? .noAnswer : nil,
            recording: recording, canRecord: env["TALK_CALL_MOCK_CAN_RECORD"] == "1",
            recordingConsentPending: mode == "consent"
        )
        mocking = true
        call = view
        presented = env["TALK_CALL_MOCK_BAR"] != "1"
        if RecordingText.isActive(recording) && state == .connected {
            Toast.show(RecordingText.announcement)
        }
    }
}
#endif

// MARK: - CXProviderDelegate

extension CallController: CXProviderDelegate {
    nonisolated func providerDidReset(_ provider: CXProvider) {
        MainActor.assumeIsolated {
            Log.call.error("CallKit reset")
            #if DEBUG
            LogDump.write(sync: true)
            #endif
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
            Log.call.info("CallKit end action (ours: \(self.uuid == action.callUUID), state: \(String(describing: self.call?.state), privacy: .public))")
            #if DEBUG
            LogDump.write(sync: UIApplication.shared.applicationState == .background)
            #endif
            if uuid == action.callUUID {
                endedLocally = true
                // iOS ends an unanswered incoming call itself after 60 s. That is no decline
                // (in a 1:1 the caller would see "Abgelehnt"): missed, ended in the core now,
                // because iOS suspends the app right after this action and the core's own
                // clock would only run out on the next wake.
                if let call, call.state == .incoming, let since = ringingSince, Date().timeIntervalSince(since) >= 55 {
                    Log.call.info("CallKit ring timeout")
                    if let session { miss(call.token, session) }
                } else {
                    endInCore()
                }
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

/// CallKit's template icon (white on transparent): the Wolkenfunk glyph from the asset
/// catalog (`design/icon/build-icons.py ios`).
enum CallKitIcon {
    static func png() -> Data? { UIImage(named: "CallKitIcon")?.pngData() }
}
