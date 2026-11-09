import AVKit
import SwiftUI
import UIKit

/// Call screens after Signal-iOS (patterns and sizes only): full screen in its own window
/// above the app, so camera or photo sheets never hide it.
@MainActor
final class CallWindow {
    static let shared = CallWindow()
    private var window: UIWindow?

    func sync() {
        let c = CallController.shared
        let show = c.presented && c.call != nil
        if show && window == nil {
            guard let scene = UIApplication.shared.connectedScenes
                .compactMap({ $0 as? UIWindowScene })
                .first(where: { $0.activationState == .foregroundActive }) ?? UIApplication.shared.connectedScenes.first as? UIWindowScene
            else { return }
            let w = UIWindow(windowScene: scene)
            w.windowLevel = .normal + 1
            let host = UIHostingController(rootView: CallScreen())
            host.view.backgroundColor = .clear
            w.rootViewController = host
            w.overrideUserInterfaceStyle = .dark
            w.makeKeyAndVisible()
            w.alpha = 0
            UIView.animate(withDuration: 0.2) { w.alpha = 1 }
            window = w
        } else if !show, let w = window {
            window = nil
            UIView.animate(withDuration: 0.2, animations: { w.alpha = 0 }) { _ in
                w.isHidden = true
            }
        }
    }
}

enum CallText {
    static func status(_ c: CallView) -> String? {
        switch c.state {
        case .dialing: return "Anrufen …"
        case .ringing: return c.isGroup ? "Warte auf andere …" : "Es klingelt …"
        case .incoming: return c.isGroup ? "Gruppenanruf" : "Talk-Anruf"
        case .connecting: return "Verbindung wird hergestellt …"
        case .connected: return nil
        case .reconnecting: return "Verbindung wird neu hergestellt …"
        case .ended: return ended(c.endReason)
        }
    }

    static func ended(_ r: EndReason?) -> String {
        switch r {
        case .noAnswer: return "Keine Antwort"
        case .missed: return "Verpasster Anruf"
        case .declined: return "Abgelehnt"
        case .failed(_, let message): return message.isEmpty ? "Anruf fehlgeschlagen" : message
        default: return "Anruf beendet"
        }
    }

    /// m:ss, from one hour h:mm:ss.
    static func clock(since startedAt: Int64, now: Date) -> String {
        let secs = max(0, Int(now.timeIntervalSince1970 - Double(startedAt) / 1000))
        let h = secs / 3600, m = (secs / 60) % 60, s = secs % 60
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
    }

    static func title(_ c: CallView) -> String {
        c.direction == .incoming && !c.isGroup && !c.callerName.isEmpty ? c.callerName : c.roomName
    }
}

/// Status line under the name: state text, the running timer or the blinking reconnect text.
struct CallStatusLine: View {
    let call: CallView
    @State private var dim = false

    var body: some View {
        Group {
            if call.state == .connected, let started = call.startedAt {
                TimelineView(.periodic(from: .now, by: 1)) { ctx in
                    Text(CallText.clock(since: started, now: ctx.date)).monospacedDigit()
                }
            } else if call.state == .reconnecting {
                Text(CallText.status(call) ?? "")
                    .opacity(dim ? 0.25 : 1)
                    .onAppear {
                        withAnimation(.easeInOut(duration: 0.7).repeatForever(autoreverses: true)) { dim = true }
                    }
            } else {
                Text(CallText.status(call) ?? "")
            }
        }
        .font(.system(size: 15))
        .foregroundStyle(.white.opacity(0.8))
        .accessibilityIdentifier("call-status")
    }
}

struct CallScreen: View {
    private var controller: CallController { .shared }

    var body: some View {
        ZStack {
            if let call = controller.call {
                if call.state == .incoming && !controller.useCallKit {
                    IncomingCallView(call: call)
                } else {
                    ActiveCallView(call: call)
                }
            }
        }
        .preferredColorScheme(.dark)
    }
}

/// Blurred conversation avatar, darkened (Signal: 40 %).
struct CallBackdrop: View {
    let call: CallView

    var body: some View {
        let room = SessionStore.shared.room(call.token)
        GeometryReader { geo in
            Avatar(name: call.roomName, size: max(geo.size.width, geo.size.height), token: call.token, version: room?.avatarVersion ?? "")
                .frame(width: geo.size.width, height: geo.size.height)
                .blur(radius: 50)
                .clipped()
        }
        .overlay(Color.black.opacity(0.4))
        .background(Color(hex: 0x1B1B1D))
        .ignoresSafeArea()
    }
}

struct ActiveCallView: View {
    let call: CallView
    private var controller: CallController { .shared }

    var body: some View {
        let room = SessionStore.shared.room(call.token)
        ZStack {
            Color(hex: 0x121212).ignoresSafeArea()
            if !call.isGroup { CallBackdrop(call: call) }
            VStack(spacing: 0) {
                HStack {
                    Button {
                        controller.presented = false
                    } label: {
                        Image(systemName: "chevron.down")
                            .font(.system(size: 20, weight: .semibold))
                            .foregroundStyle(.white)
                            .frame(width: 44, height: 44)
                    }
                    .accessibilityLabel("Zurück zum Chat")
                    .accessibilityIdentifier("call-minimize")
                    Spacer()
                }
                .padding(.horizontal, 8)
                VStack(spacing: 6) {
                    Text(CallText.title(call))
                        .font(.system(size: 26, weight: .semibold))
                        .lineLimit(2)
                        .multilineTextAlignment(.center)
                        .accessibilityIdentifier("call-title")
                    CallStatusLine(call: call)
                }
                .foregroundStyle(.white)
                .padding(.horizontal, 24)
                .padding(.top, 8)
                Spacer(minLength: 24)
                if call.isGroup {
                    CallTiles(call: call)
                } else {
                    Avatar(name: call.roomName, size: 160, token: call.token, version: room?.avatarVersion ?? "")
                        .overlay(Circle().stroke(Color.white.opacity(speaking ? 0.9 : 0), lineWidth: 3).padding(-6))
                        .animation(.easeOut(duration: 0.2), value: speaking)
                }
                Spacer(minLength: 24)
                CallControls(call: call)
                    .padding(.bottom, 24)
            }
        }
    }

    private var speaking: Bool {
        call.state == .connected && call.participants.contains { $0.speaking }
    }
}

/// Audio output · mute · (video, later) · hang up. 48 pt buttons, 20 pt apart.
struct CallControls: View {
    let call: CallView
    private var audio: CallAudio { .shared }
    private var controller: CallController { .shared }

    var body: some View {
        let ended = call.state == .ended
        HStack(spacing: 20) {
            AudioOutputButton()
            CallButton(
                symbol: call.muted ? "mic.slash.fill" : "mic.fill",
                active: call.muted,
                label: call.muted ? "Stummschaltung aufheben" : "Stummschalten",
                id: "call-mute"
            ) { controller.toggleMute() }
            // Video goes here later (same size), the bar keeps its layout.
            CallButton(symbol: "phone.down.fill", tint: Talk.danger, label: "Auflegen", id: "call-hangup") {
                controller.hangUp()
            }
        }
        .disabled(ended)
        .opacity(ended ? 0.5 : 1)
    }
}

struct CallButton: View {
    let symbol: String
    var active = false
    var tint: Color? = nil
    let label: String
    let id: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 20, weight: .medium))
                .foregroundStyle(active ? .black : .white)
                .frame(width: 48, height: 48)
                .background(tint ?? (active ? Color.white : Color.white.opacity(0.18)), in: Circle())
        }
        .accessibilityLabel(label)
        .accessibilityIdentifier(id)
    }
}

/// Earpiece ↔ speaker; with a headset or Bluetooth device the system route picker opens.
struct AudioOutputButton: View {
    private var audio: CallAudio { .shared }

    var body: some View {
        let symbol: String = switch audio.output {
        case .speaker: "speaker.wave.3.fill"
        case .receiver: "speaker.fill"
        case .external: "airpodspro"
        }
        let active = audio.output != .receiver
        ZStack {
            CallButton(symbol: symbol, active: active, label: "Audioausgabe", id: "call-audio") {
                audio.toggleSpeaker()
            }
            if audio.hasExternal {
                RoutePicker()
                    .frame(width: 48, height: 48)
                    .accessibilityLabel("Audioausgabe")
                    .accessibilityIdentifier("call-route-picker")
            }
        }
    }
}

/// The system's audio route picker, invisible over our button.
struct RoutePicker: UIViewRepresentable {
    func makeUIView(context: Context) -> AVRoutePickerView {
        let v = AVRoutePickerView()
        v.prioritizesVideoDevices = false
        v.tintColor = .clear
        v.activeTintColor = .clear
        v.backgroundColor = .clear
        return v
    }

    func updateUIView(_ uiView: AVRoutePickerView, context: Context) {}
}

/// Group call: one tile per participant (avatar, name, speaking frame, mute icon).
struct CallTiles: View {
    let call: CallView

    var body: some View {
        let list = call.participants
        if list.isEmpty {
            Image(systemName: "person.2.fill")
                .font(.system(size: 48))
                .foregroundStyle(.white.opacity(0.35))
        } else {
            let columns = list.count <= 1 ? 1 : (list.count <= 4 ? 2 : 3)
            ScrollView {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 10), count: columns), spacing: 10) {
                    ForEach(list, id: \.sessionId) { p in
                        CallTile(participant: p, large: columns == 1)
                    }
                }
                .padding(.horizontal, 12)
            }
            .scrollBounceBehavior(.basedOnSize)
        }
    }
}

struct CallTile: View {
    let participant: CallParticipant
    let large: Bool
    @State private var image: UIImage?
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let p = participant
        VStack(spacing: 8) {
            ZStack {
                if let image {
                    Image(uiImage: image).resizable().scaledToFill()
                } else {
                    Format.color(for: p.name)
                    Text(Format.initials(p.name))
                        .font(.system(size: (large ? 96 : 64) * 0.4, weight: .semibold))
                        .foregroundStyle(.white)
                }
            }
            .frame(width: large ? 96 : 64, height: large ? 96 : 64)
            .clipShape(Circle())
            HStack(spacing: 4) {
                if p.muted {
                    Image(systemName: "mic.slash.fill").font(.system(size: 12))
                }
                Text(p.name).font(.system(size: 14, weight: .medium)).lineLimit(1)
            }
            .foregroundStyle(.white)
            if !p.connected {
                Text("Verbinde …").font(.system(size: 12)).foregroundStyle(.white.opacity(0.7))
            }
        }
        .frame(maxWidth: .infinity)
        .frame(height: large ? 220 : 150)
        .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(Color.white, lineWidth: p.speaking && p.connected ? 3 : 0)
        )
        .opacity(p.connected ? 1 : 0.55)
        .animation(.easeOut(duration: 0.2), value: p.speaking)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("call-tile")
        .task(id: p.userId) {
            guard !p.userId.isEmpty else { return }
            image = await CallAvatars.shared.image(p.userId, dark: scheme == .dark)
        }
    }
}

/// Avatars of call participants (users, not rooms), cached while the app runs.
@MainActor
final class CallAvatars {
    static let shared = CallAvatars()
    private var cache: [String: UIImage] = [:]

    func image(_ userId: String, dark: Bool) async -> UIImage? {
        let key = "\(userId)|\(dark)"
        if let hit = cache[key] { return hit }
        guard let s = SessionStore.shared.session,
              let data = try? await s.userAvatar(userId: userId, dark: dark),
              let img = UIImage(data: data) else { return nil }
        cache[key] = img
        return img
    }
}

/// Incoming call without CallKit (simulator): CallKit shows its own screen on devices.
struct IncomingCallView: View {
    let call: CallView
    private var controller: CallController { .shared }

    var body: some View {
        let room = SessionStore.shared.room(call.token)
        ZStack {
            CallBackdrop(call: call)
            VStack(spacing: 12) {
                Spacer().frame(height: 60)
                Avatar(name: call.roomName, size: 80, token: call.token, version: room?.avatarVersion ?? "")
                Text(CallText.title(call))
                    .font(.system(size: 26, weight: .semibold))
                    .multilineTextAlignment(.center)
                    .accessibilityIdentifier("call-title")
                CallStatusLine(call: call)
                Spacer()
                HStack {
                    RoundAction(symbol: "phone.down.fill", color: Talk.danger, label: "Ablehnen", id: "call-decline") {
                        controller.hangUp()
                    }
                    Spacer()
                    RoundAction(symbol: "phone.fill", color: Talk.callDot, label: "Annehmen", id: "call-accept") {
                        controller.accept()
                    }
                }
                .padding(.horizontal, 48)
                .padding(.bottom, 48)
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 24)
        }
    }
}

private struct RoundAction: View {
    let symbol: String
    let color: Color
    let label: String
    let id: String
    let action: () -> Void

    var body: some View {
        VStack(spacing: 8) {
            Button(action: action) {
                Image(systemName: symbol)
                    .font(.system(size: 30, weight: .medium))
                    .foregroundStyle(.white)
                    .frame(width: 78, height: 78)
                    .background(color, in: Circle())
            }
            .accessibilityLabel(label)
            .accessibilityIdentifier(id)
            Text(label).font(.system(size: 14))
        }
    }
}

/// "Zurück zum Anruf · 03:12" above the app while the call screen is minimised.
struct CallBar: View {
    private var controller: CallController { .shared }

    var body: some View {
        if let call = controller.call, !controller.presented, call.state != .incoming || !controller.useCallKit {
            Button {
                controller.presented = true
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "phone.fill").font(.system(size: 12))
                    label(call).font(.system(size: 14, weight: .semibold)).monospacedDigit()
                }
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
                .background(Talk.callDot)
            }
            .accessibilityIdentifier("call-bar")
        }
    }

    @ViewBuilder
    private func label(_ call: CallView) -> some View {
        if call.state == .connected, let started = call.startedAt {
            TimelineView(.periodic(from: .now, by: 1)) { ctx in
                Text("Zurück zum Anruf · \(CallText.clock(since: started, now: ctx.date))")
            }
        } else {
            Text("Zurück zum Anruf · \(CallText.status(call) ?? "")")
        }
    }
}
