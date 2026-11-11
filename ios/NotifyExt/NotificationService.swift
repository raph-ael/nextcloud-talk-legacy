import UserNotifications

/// Chat pushes arrive encrypted with this device's key ("Neue Nachricht" as placeholder);
/// this extension decrypts them with the core and shows the server's text. Chat subjects
/// come as "Bob\nHallo" (groups: "Bob in Projekt\nHallo"): the first line becomes the
/// title. Taps open the conversation (`token` in userInfo, like the app's own
/// notifications). Unreadable pushes keep the placeholder.
final class NotificationService: UNNotificationServiceExtension {
    private var handler: ((UNNotificationContent) -> Void)?
    private var content: UNMutableNotificationContent?

    override func didReceive(_ request: UNNotificationRequest, withContentHandler handler: @escaping (UNNotificationContent) -> Void) {
        self.handler = handler
        let content = (request.content.mutableCopy() as? UNMutableNotificationContent) ?? UNMutableNotificationContent()
        self.content = content
        if let subject = request.content.userInfo["subject"] as? String, let key = PushKey.load(),
           let message = try? decryptPush(deviceKeyPem: key, subject: subject) {
            switch message {
            case .chat(let token, let nid, let text), .call(let token, let nid, let text):
                let lines = text.split(separator: "\n", maxSplits: 1).map(String.init)
                content.title = lines.count == 2 ? lines[0] : ""
                content.body = lines.last ?? text
                content.threadIdentifier = token
                content.userInfo["token"] = token
                content.userInfo["nid"] = nid
            case .other(_, let text) where !text.isEmpty:
                content.title = ""
                content.body = text
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
