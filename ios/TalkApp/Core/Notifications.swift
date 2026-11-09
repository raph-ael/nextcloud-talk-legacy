import Observation
import SwiftUI
import UserNotifications

/// Local notifications: a failed send per conversation (tap opens it) and the app badge.
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

    private func present(_ text: String) {
        withAnimation { self.text = text }
        hide?.cancel()
        hide = Task {
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled else { return }
            withAnimation { self.text = nil }
        }
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
