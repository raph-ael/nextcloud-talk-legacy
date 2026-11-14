import Observation
import SwiftUI
import UserNotifications

/// Local notifications: a failed send per conversation, missed calls, finished recordings
/// (taps open the chat) and the app badge.
enum Notifications {
    private static let askedKey = "notifications.asked"

    /// Asks once, the first time the user sends something.
    static func askOnce() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: askedKey) else { return }
        defaults.set(true, forKey: askedKey)
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { _, _ in }
    }

    static func sendFailed(token: String, room: String) {
        let content = UNMutableNotificationContent()
        content.title = "Nachricht nicht gesendet"
        content.body = room
        content.sound = .default
        content.userInfo = ["token": token]
        content.threadIdentifier = "send_failed.\(token)"
        let request = UNNotificationRequest(identifier: "send_failed.\(token)", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    /// A call that rang here unanswered, or came in during another call. Tap opens the chat.
    static func missedCall(token: String, name: String) {
        let content = UNMutableNotificationContent()
        content.title = "Verpasster Anruf"
        content.body = name
        content.sound = .default
        content.userInfo = ["token": token]
        content.threadIdentifier = "missed_call.\(token)"
        let id = "missed_call.\(token).\(Int(Date().timeIntervalSince1970))"
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: id, content: content, trigger: nil))
    }

    /// A finished recording (transcript, summary) while the app runs: quiet, tap opens the chat.
    static func recording(_ n: RecordingNotice) {
        let content = UNMutableNotificationContent()
        content.title = RecordingText.title(n)
        content.body = n.roomName
        content.sound = nil
        content.interruptionLevel = .passive
        content.userInfo = ["token": n.token]
        content.threadIdentifier = "recording.\(n.token)"
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "recording.\(n.id)", content: content, trigger: nil))
    }

    static func removeRecording(_ id: Int64) {
        UNUserNotificationCenter.current().removeDeliveredNotifications(withIdentifiers: ["recording.\(id)"])
    }

    static func setBadge(_ count: Int) {
        UNUserNotificationCenter.current().setBadgeCount(count) { _ in }
    }
}

/// Short message at the bottom of the screen (Android's Toast).
@MainActor
@Observable
final class Toast {
    static let shared = Toast()
    private(set) var text: String?
    @ObservationIgnored private var hide: Task<Void, Never>?

    static func show(_ text: String) {
        shared.present(text)
    }

    /// Own window above everything (sheets too), never taking touches.
    @ObservationIgnored private var window: UIWindow?

    private func present(_ text: String) {
        attach()
        withAnimation { self.text = text }
        hide?.cancel()
        hide = Task {
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled else { return }
            withAnimation { self.text = nil }
        }
    }
}

extension Toast {
    private final class PassThrough: UIWindow {
        override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? { nil }
    }

    fileprivate func attach() {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        guard let scene = scenes.first(where: { $0.activationState == .foregroundActive }) ?? scenes.first else { return }
        if let window, window.windowScene === scene { return }
        let w = PassThrough(windowScene: scene)
        Appearance.shared.style(w)
        w.windowLevel = UIWindow.Level(rawValue: UIWindow.Level.normal.rawValue + 1)
        let host = UIHostingController(rootView: ToastOverlay())
        host.view.backgroundColor = .clear
        w.rootViewController = host
        w.isHidden = false
        window = w
    }
}

struct ToastOverlay: View {
    let toast = Toast.shared

    var body: some View {
        VStack {
            Spacer()
            if let text = toast.text {
                Text(text)
                    .font(.subheadline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .background(.black.opacity(0.8), in: Capsule())
                    .padding(.bottom, 90)
                    .transition(.opacity)
                    .accessibilityIdentifier("toast")
            }
        }
        .allowsHitTesting(false)
    }
}
