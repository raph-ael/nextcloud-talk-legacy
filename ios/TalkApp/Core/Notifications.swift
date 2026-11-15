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

    /// Where a notification belongs (docs/accounts.md): the account id goes into the id,
    /// the thread and `userInfo`; `line` ("{Name} · {Server}", only from two accounts on)
    /// becomes the subtitle.
    struct Origin {
        let account: String
        let line: String?
    }

    private static func content(_ o: Origin, token: String, title: String, body: String) -> UNMutableNotificationContent {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        if let line = o.line { content.subtitle = line }
        content.userInfo = ["account": o.account, "token": token]
        return content
    }

    static func sendFailed(_ o: Origin, token: String, room: String) {
        let content = content(o, token: token, title: "Nachricht nicht gesendet", body: room)
        content.sound = .default
        content.threadIdentifier = "\(o.account).send_failed.\(token)"
        let request = UNNotificationRequest(identifier: "\(o.account).send_failed.\(token)", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    /// A call that rang here unanswered, or came in during another call. Tap opens the chat.
    static func missedCall(_ o: Origin, token: String, name: String) {
        let content = content(o, token: token, title: "Verpasster Anruf", body: name)
        content.sound = .default
        content.threadIdentifier = "\(o.account).missed_call.\(token)"
        let id = "\(o.account).missed_call.\(token).\(Int(Date().timeIntervalSince1970))"
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: id, content: content, trigger: nil))
    }

    /// A finished recording (transcript, summary) while the app runs: quiet, tap opens the chat.
    static func recording(_ o: Origin, _ n: RecordingNotice) {
        let content = content(o, token: n.token, title: RecordingText.title(n), body: n.roomName)
        content.sound = nil
        content.interruptionLevel = .passive
        content.threadIdentifier = "\(o.account).recording.\(n.token)"
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "\(o.account).recording.\(n.id)", content: content, trigger: nil))
    }

    static func removeRecording(account: String, _ id: Int64) {
        UNUserNotificationCenter.current().removeDeliveredNotifications(withIdentifiers: ["\(account).recording.\(id)"])
    }

    /// Sign-out: the account's notifications go, the others' stay.
    static func removeAll(account: String, legacy: Bool) {
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { list in
            let ids = list.filter { n in
                let a = n.request.content.userInfo["account"] as? String
                return a == account || (legacy && (a == nil || a == DeviceKeys.legacyId))
            }.map(\.request.identifier)
            center.removeDeliveredNotifications(withIdentifiers: ids)
        }
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
