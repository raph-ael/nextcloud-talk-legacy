import Foundation
import Security

/// The number at the app icon, shared with the Notification Service Extension
/// (docs/notifications.md "Zähler am App-Symbol bei beendeter App"): the app writes the true
/// sum (`badge().messages` over all accounts) on every change, the extension adds one per
/// message and sets it. It lives in the keychain group of the push keys (`TalkKeychainGroup`);
/// builds without that group keep it private, so the extension then counts from zero.
enum SharedBadge {
    private static let service = "org.wolkenfunk.app.badge"
    private static let name = "count"

    nonisolated private static var group: String? {
        let g = Bundle.main.object(forInfoDictionaryKey: "TalkKeychainGroup") as? String ?? ""
        return g.hasPrefix(".") || g.isEmpty ? nil : g
    }

    nonisolated private static func query() -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: name,
        ]
        if let group { q[kSecAttrAccessGroup as String] = group }
        return q
    }

    nonisolated static func read() -> Int {
        var q = query()
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data, let s = String(data: data, encoding: .utf8) else { return 0 }
        return Int(s) ?? 0
    }

    nonisolated static func write(_ count: Int) {
        let data = Data(String(max(0, count)).utf8)
        let update = [kSecValueData as String: data]
        if SecItemUpdate(query() as CFDictionary, update as CFDictionary) == errSecItemNotFound {
            var q = query()
            q[kSecValueData as String] = data
            // Readable after the first unlock: pushes arrive while the phone is locked.
            q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            SecItemAdd(q as CFDictionary, nil)
        }
    }

    /// One more message: the new number for the icon.
    nonisolated static func increment() -> Int {
        let n = read() + 1
        write(n)
        return n
    }
}
