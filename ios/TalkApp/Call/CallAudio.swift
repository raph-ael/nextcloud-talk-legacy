import AVFoundation
import Observation
import UIKit
import WebRTC

/// Audio session of a call, like Signal:
/// - earpiece: `.playAndRecord` + `.voiceChat`, proximity sensor on
/// - speaker: `.playAndRecord` + `.videoChat` (+ speaker override)
/// - own camera on: `.videoChat`, speaker unless a headset is connected, no proximity sensor
/// - Bluetooth (HFP/A2DP), headphones and car audio through the system route picker.
///
/// WebRTC runs with `useManualAudio`: its audio unit starts only when the core has sent
/// `startAudio` **and** the system activated the session (CallKit `didActivate`, or we
/// ourselves without CallKit).
@MainActor
@Observable
final class CallAudio {
    static let shared = CallAudio()

    enum Output: Equatable {
        case receiver, speaker
        case external(String)
    }

    /// What is playing the call right now.
    private(set) var output: Output = .receiver
    /// A headset, Bluetooth device or car is available: the audio button opens the route picker.
    private(set) var hasExternal = false

    /// A call owns the audio session (voice messages must not touch it).
    private(set) var inCall = false {
        didSet { Self.sessionInUse = inCall }
    }
    /// `inCall` for code off the main actor (voice capture teardown).
    nonisolated(unsafe) static var sessionInUse = false

    @ObservationIgnored private var started = false
    @ObservationIgnored private var activated = false
    @ObservationIgnored private var wantSpeaker = false
    /// Our camera is on (docs/calls.md "Video", mobile).
    @ObservationIgnored private var video = false
    /// Without CallKit (simulator) the app activates the session itself.
    @ObservationIgnored var selfManaged = false
    @ObservationIgnored private var observers: [NSObjectProtocol] = []

    private init() {
        let nc = NotificationCenter.default
        observers.append(nc.addObserver(forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main) { [weak self] n in
            let raw = n.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt
            MainActor.assumeIsolated { self?.routeChanged(raw.flatMap(AVAudioSession.RouteChangeReason.init(rawValue:))) }
        })
        observers.append(nc.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] n in
            let raw = n.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt
            MainActor.assumeIsolated { self?.interrupted(raw.flatMap(AVAudioSession.InterruptionType.init(rawValue:))) }
        })
        observers.append(nc.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.mediaReset() }
        })
    }

    // MARK: Call lifecycle

    /// A call begins (dialing or answering): voice messages stop, the category is set
    /// before CallKit activates the session.
    func beginCall() {
        guard !inCall else { return }
        SessionStore.shared.voice.stopForCall()
        inCall = true
        wantSpeaker = false
        configure()
        refreshRoute()
    }

    /// `startAudio` from the core.
    func startAudio() {
        if !inCall { beginCall() }
        started = true
        if selfManaged && !activated {
            do {
                try AVAudioSession.sharedInstance().setActive(true)
                didActivate(AVAudioSession.sharedInstance())
            } catch {
                Log.call.error("audio session: \(error.localizedDescription, privacy: .public)")
            }
        }
        apply()
    }

    /// `stopAudio` from the core, or the call is gone: release everything.
    func stopAudio() {
        started = false
        apply()
        if selfManaged && activated {
            didDeactivate(AVAudioSession.sharedInstance())
            try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        }
        endCall()
    }

    private func endCall() {
        guard inCall else { return }
        inCall = false
        wantSpeaker = false
        video = false
        UIDevice.current.isProximityMonitoringEnabled = false
        output = .receiver
    }

    /// CallKit `didActivate` (or our own activation).
    func didActivate(_ session: AVAudioSession) {
        RTCAudioSession.sharedInstance().audioSessionDidActivate(session)
        activated = true
        apply()
        applyOutput()
        Log.call.info("audio session activated")
    }

    /// CallKit `didDeactivate`.
    func didDeactivate(_ session: AVAudioSession) {
        RTCAudioSession.sharedInstance().audioSessionDidDeactivate(session)
        activated = false
        apply()
        Log.call.info("audio session deactivated")
    }

    private func apply() {
        let enabled = started && activated
        let rtc = RTCAudioSession.sharedInstance()
        if rtc.isAudioEnabled != enabled { rtc.isAudioEnabled = enabled }
    }

    // MARK: Routing

    /// Audio button without external devices: earpiece ↔ speaker.
    func toggleSpeaker() {
        wantSpeaker = output != .speaker
        configure()
        applyOutput()
    }

    /// Camera switched on or off: on goes to the speaker unless a headset is connected; off
    /// keeps the current output.
    func setVideo(_ on: Bool) {
        guard inCall, on != video else { return }
        video = on
        if on && Self.externalOutput(AVAudioSession.sharedInstance().currentRoute) == nil { wantSpeaker = true }
        configure()
        applyOutput()
    }

    private func configure() {
        let rtc = RTCAudioSession.sharedInstance()
        rtc.lockForConfiguration()
        defer { rtc.unlockForConfiguration() }
        do {
            try rtc.setCategory(.playAndRecord, with: [.allowBluetoothHFP, .allowBluetoothA2DP])
            try rtc.setMode(wantSpeaker || video ? .videoChat : .voiceChat)
        } catch {
            Log.call.error("audio category: \(error.localizedDescription, privacy: .public)")
        }
    }

    private func applyOutput() {
        let rtc = RTCAudioSession.sharedInstance()
        rtc.lockForConfiguration()
        defer { rtc.unlockForConfiguration() }
        let external = Self.externalOutput(AVAudioSession.sharedInstance().currentRoute) != nil
        do {
            try rtc.overrideOutputAudioPort(wantSpeaker && !external ? .speaker : .none)
        } catch {
            Log.call.error("audio override: \(error.localizedDescription, privacy: .public)")
        }
        refreshRoute()
    }

    private func routeChanged(_ reason: AVAudioSession.RouteChangeReason?) {
        guard inCall else { return }
        if reason == .newDeviceAvailable {
            // A headset was connected: it wins over the speaker, like on a phone call.
            wantSpeaker = false
            configure()
            applyOutput()
            return
        }
        refreshRoute()
    }

    private func refreshRoute() {
        let s = AVAudioSession.sharedInstance()
        let route = s.currentRoute
        if let ext = Self.externalOutput(route) {
            output = .external(ext)
        } else if route.outputs.contains(where: { $0.portType == .builtInSpeaker }) {
            output = .speaker
        } else {
            output = .receiver
        }
        let externalInputs: Set<AVAudioSession.Port> = [.bluetoothHFP, .headsetMic, .usbAudio, .carAudio]
        hasExternal = Self.externalOutput(route) != nil
            || (s.availableInputs ?? []).contains { externalInputs.contains($0.portType) }
        UIDevice.current.isProximityMonitoringEnabled = inCall && output == .receiver && !video
    }

    private static func externalOutput(_ route: AVAudioSessionRouteDescription) -> String? {
        let builtIn: Set<AVAudioSession.Port> = [.builtInReceiver, .builtInSpeaker]
        return route.outputs.first { !builtIn.contains($0.portType) }?.portName
    }

    // MARK: System events

    private func interrupted(_ type: AVAudioSession.InterruptionType?) {
        guard inCall, type == .ended else { return }
        // After Siri, an alarm or a GSM call the audio unit has to be started again.
        if selfManaged {
            try? AVAudioSession.sharedInstance().setActive(true)
        }
        let rtc = RTCAudioSession.sharedInstance()
        if started && activated {
            rtc.isAudioEnabled = false
            rtc.isAudioEnabled = true
        }
    }

    private func mediaReset() {
        guard inCall else { return }
        Log.call.error("media services were reset")
        // All audio objects are gone: configure again and restart the audio unit.
        configure()
        if selfManaged { try? AVAudioSession.sharedInstance().setActive(true) }
        let rtc = RTCAudioSession.sharedInstance()
        rtc.isAudioEnabled = false
        apply()
        applyOutput()
    }
}
