import UserNotifications

/// Chat pushes arrive encrypted with an account's device key ("Neue Nachricht" as
/// placeholder); this extension decrypts them with the core and shows the server's text.
/// The key that decrypts names the account (docs/accounts.md "Push"). Chat subjects come
/// as "Bob\nHallo" (groups: "Bob in Projekt\nHallo"): the first line becomes the title;
/// with two accounts or more the subtitle names the account. Taps open the conversation
/// (`account` and `token` in userInfo, like the app's own notifications). Unreadable pushes
/// keep the placeholder.
final class NotificationService: UNNotificationServiceExtension {
    private var handler: ((UNNotificationContent) -> Void)?
    private var content: UNMutableNotificationContent?

    override func didReceive(_ request: UNNotificationRequest, withContentHandler handler: @escaping (UNNotificationContent) -> Void) {
        self.handler = handler
        let content = (request.content.mutableCopy() as? UNMutableNotificationContent) ?? UNMutableNotificationContent()
        self.content = content
        if let subject = request.content.userInfo["subject"] as? String,
           let found = decryptPushFor(keys: DeviceKeys.decryptKeys(), subject: subject) {
            let accounts = DeviceKeys.all()
            let line = accounts.count > 1 ? accounts.first { $0.accountId == found.accountId }?.line : nil
            switch found.message {
            case .chat(let token, let nid, let text), .call(let token, let nid, let text):
                let lines = text.split(separator: "\n", maxSplits: 1).map(String.init)
                content.title = lines.count == 2 ? lines[0] : ""
                content.body = lines.last ?? text
                content.subtitle = line ?? ""
                content.threadIdentifier = "\(found.accountId).\(token)"
                content.userInfo["account"] = found.accountId
                content.userInfo["token"] = token
                content.userInfo["nid"] = nid
            case .other(_, let text) where !text.isEmpty:
                content.title = ""
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
