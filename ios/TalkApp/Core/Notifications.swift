import Observation
import SwiftUI
import UserNotifications

/// Local notifications: a failed send per conversation, missed calls, finished recordings
/// (taps open the chat) and the app badge.
enum Notifications {
    /// Category of chat pushes (set by the extension): "Antworten", "Als gelesen markieren".
    static let chatCategory = "chat"
    static let replyAction = "reply"
    static let readAction = "read"

    /// At launch, before the first push can be acted on.
    static func registerCategories() {
        let reply = UNTextInputNotificationAction(
            identifier: replyAction, title: "Antworten", options: [],
            textInputButtonTitle: "Senden", textInputPlaceholder: "Nachricht")
        let read = UNNotificationAction(identifier: readAction, title: "Als gelesen markieren", options: [])
        let chat = UNNotificationCategory(identifier: chatCategory, actions: [reply, read], intentIdentifiers: [], options: [])
        UNUserNotificationCenter.current().setNotificationCategories([chat])
    }

    private static let askedKey = "notifications.asked.start"

    /// On the start with an account (also installations that asked only on the first send
    /// so far): asks once; iOS shows the question only while nothing is decided.
    static func askAtStart() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: askedKey) else { return }
        defaults.set(true, forKey: askedKey)
        Task {
            guard await UNUserNotificationCenter.current().notificationSettings().authorizationStatus == .notDetermined else { return }
            _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound])
            await NotificationPermission.shared.refresh()
        }
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

    /// A call that rang here unanswered, came in during another call, or was turned away by
    /// the system (focus, blocked). Tap opens the chat. The same call may be reported twice
    /// (by CallKit and later by the core): one per conversation within ten minutes.
    static func missedCall(_ o: Origin, token: String, name: String) {
        let content = content(o, token: token, title: "Verpasster Anruf", body: name)
        content.sound = .default
        let thread = "\(o.account).missed_call.\(token)"
        content.threadIdentifier = thread
        let id = "\(thread).\(Int(Date().timeIntervalSince1970))"
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { list in
            let recent = list.contains { $0.request.content.threadIdentifier == thread && $0.date.timeIntervalSinceNow > -600 }
            guard !recent else { return }
            center.add(UNNotificationRequest(identifier: id, content: content, trigger: nil))
        }
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

    /// Whether a delivered notification belongs to this account; `legacy`: the account taken
    /// over from the earlier version, whose notifications carry no or the legacy account.
    nonisolated static func belongs(_ n: UNNotification, to account: String, legacy: Bool) -> Bool {
        let a = n.request.content.userInfo["account"] as? String
        return a == account || (legacy && (a == nil || a == DeviceKeys.legacyId))
    }

    /// The chat notifications of pushes (they carry `nid`) of one account.
    nonisolated private static func chats(_ list: [UNNotification], account: String, legacy: Bool) -> [UNNotification] {
        list.filter { n in
            n.request.content.userInfo["nid"] != nil && n.request.content.userInfo["token"] is String
                && belongs(n, to: account, legacy: legacy)
        }
    }

    /// `noticesCleared`, "Als gelesen markieren", the chat opened: its notifications go.
    static func removeChats(account: String, legacy: Bool, tokens: Set<String>) {
        guard !tokens.isEmpty else { return }
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { list in
            let ids = chats(list, account: account, legacy: legacy)
                .filter { tokens.contains($0.request.content.userInfo["token"] as? String ?? "") }
                .map(\.request.identifier)
            if !ids.isEmpty { center.removeDeliveredNotifications(withIdentifiers: ids) }
        }
    }

    /// Back in front (deletions do not arrive reliably): notifications of chats without
    /// anything unread go. `unread`: the tokens with unread messages, from a fresh list.
    static func keepOnly(account: String, legacy: Bool, unread: Set<String>) {
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { list in
            let ids = chats(list, account: account, legacy: legacy)
                .filter { !unread.contains($0.request.content.userInfo["token"] as? String ?? "") }
                .map(\.request.identifier)
            if !ids.isEmpty { center.removeDeliveredNotifications(withIdentifiers: ids) }
        }
    }

    /// Sign-out: the account's notifications go, the others' stay.
    static func removeAll(account: String, legacy: Bool) {
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { list in
            let ids = list.filter { belongs($0, to: account, legacy: legacy) }.map(\.request.identifier)
            center.removeDeliveredNotifications(withIdentifiers: ids)
        }
    }

    /// The icon number, also for the extension, which counts on from it.
    static func setBadge(_ count: Int) {
        UNUserNotificationCenter.current().setBadgeCount(count) { _ in }
        SharedBadge.write(count)
    }
}

/// Whether notifications are allowed (docs/notifications.md "Erlaubnis und Einstellungen"),
/// read again whenever the app comes to the front (the user may have changed it).
@MainActor
@Observable
final class NotificationPermission {
    static let shared = NotificationPermission()
    private(set) var status: UNAuthorizationStatus?

    /// Off: denied, or not decided yet.
    var off: Bool { status == .denied || status == .notDetermined }

    func refresh() async {
        status = await UNUserNotificationCenter.current().notificationSettings().authorizationStatus
    }

    /// "Einschalten": asks while nothing is decided, else the system page of the app.
    func turnOn(open: (URL) -> Void) async {
        if status == .notDetermined {
            _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound])
            await refresh()
        } else if let url = URL(string: UIApplication.openNotificationSettingsURLString) {
            open(url)
        }
    }
}

/// Short message at the bottom of the screen (Android's Toast).
@MainActor
@Observable
final class Toast {
    static let shared = Toast()
    private(set) var text: String?
    /// Height of the keyboard, which lies above the toast's window: the notice goes above it
    /// (an error while editing). Followed from the start (`AppDelegate`), as the window comes later.
    private(set) var keyboard: CGFloat = 0
    @ObservationIgnored private var hide: Task<Void, Never>?
    @ObservationIgnored private var observer: NSObjectProtocol?

    private init() {
        observer = NotificationCenter.default.addObserver(
            forName: UIResponder.keyboardWillChangeFrameNotification, object: nil, queue: .main
        ) { n in
            guard let frame = n.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else { return }
            let height = max(0, UIScreen.main.bounds.height - frame.minY)
            MainActor.assumeIsolated { Toast.shared.keyboard = height }
        }
    }

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
        GeometryReader { geo in
            VStack {
                Spacer()
                if let text = toast.text {
                    Text(text)
                        .font(.subheadline)
                        .foregroundStyle(.white)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 10)
                        .background(.black.opacity(0.8), in: Capsule())
                        .padding(.bottom, max(0, toast.keyboard - geo.safeAreaInsets.bottom) + 90)
                        .transition(.opacity)
                        .accessibilityIdentifier("toast")
                }
            }
            .frame(maxWidth: .infinity)
        }
        .ignoresSafeArea(.keyboard)
        .allowsHitTesting(false)
    }
}
