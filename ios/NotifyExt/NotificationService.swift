import UserNotifications

/// Chat pushes arrive encrypted with an account's device key ("Neue Nachricht" as
/// placeholder); this extension decrypts them with the core (docs/notifications.md). The key
/// that decrypts names the account (docs/accounts.md "Push"). Sender and text come from
/// `pushChatParts`; the extension does not know the conversation list, so the title stays
/// the push's first line ("Bob", in groups "Bob in Projekt"). From two accounts on the
/// subtitle names the account. Every message adds one to the shared icon number, plays the
/// standard sound and carries the actions "Antworten" and "Als gelesen markieren". Taps open
/// the conversation (`account` and `token` in userInfo, like the app's own notifications).
/// Unreadable pushes keep the placeholder.
final class NotificationService: UNNotificationServiceExtension {
    /// Must match `Notifications.chatCategory` in the app, which registers the actions.
    static let chatCategory = "chat"

    private var handler: ((UNNotificationContent) -> Void)?
    private var content: UNMutableNotificationContent?

    override func didReceive(_ request: UNNotificationRequest, withContentHandler handler: @escaping (UNNotificationContent) -> Void) {
        self.handler = handler
        let content = (request.content.mutableCopy() as? UNMutableNotificationContent) ?? UNMutableNotificationContent()
        self.content = content
        content.sound = .default
        if let subject = request.content.userInfo["subject"] as? String,
           let found = decryptPushFor(keys: DeviceKeys.decryptKeys(), subject: subject) {
            let accounts = DeviceKeys.all()
            let line = accounts.count > 1 ? accounts.first { $0.accountId == found.accountId }?.line : nil
            switch found.message {
            case .chat(let token, let nid, let text), .call(let token, let nid, let text):
                let parts = pushChatParts(subject: text, roomName: "")
                content.title = parts.sender
                content.body = parts.text.isEmpty ? "Neue Nachricht" : parts.text
                content.subtitle = line ?? ""
                content.threadIdentifier = "\(found.accountId).\(token)"
                content.userInfo["account"] = found.accountId
                content.userInfo["token"] = token
                content.userInfo["nid"] = nid
                if case .chat = found.message {
                    content.categoryIdentifier = Self.chatCategory
                    content.badge = NSNumber(value: SharedBadge.increment())
                }
            case .other(_, let text) where !text.isEmpty:
                // Not Talk (also "Push testen"): a plain notification, a tap opens the app.
                content.title = "Wolkenfunk"
                content.body = text
                content.subtitle = line ?? ""
                content.userInfo["account"] = found.accountId
            default:
                break
            }
        }
        handler(content)
        self.handler = nil
    }

    override func serviceExtensionTimeWillExpire() {
        if let handler, let content { handler(content) }
        handler = nil
    }
}
