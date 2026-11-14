import Foundation
import os
import Security

/// The device's push key (PKCS#8 PEM) in the Keychain, one per account. It lives in the
/// keychain group shared with the Notification Service Extension (`TalkKeychainGroup`),
/// which decrypts chat pushes. Builds without that entitlement (simulator) keep it private.
enum PushKey {
    private static let logger = Logger(subsystem: "org.wolkenfunk.app", category: "push")

    nonisolated private static var group: String? {
        let g = Bundle.main.object(forInfoDictionaryKey: "TalkKeychainGroup") as? String ?? ""
        return g.hasPrefix(".") || g.isEmpty ? nil : g
    }

    nonisolated private static func query(group: String?) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "org.wolkenfunk.app.push",
            kSecAttrAccount as String: "device-key",
        ]
        if let group { q[kSecAttrAccessGroup as String] = group }
        return q
    }

    nonisolated static func load() -> String? {
        for g in [group, nil] {
            var q = query(group: g)
            q[kSecReturnData as String] = true
            q[kSecMatchLimit as String] = kSecMatchLimitOne
            var item: CFTypeRef?
            if SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess, let data = item as? Data {
                return String(data: data, encoding: .utf8)
            }
            if g == nil { break }
        }
        return nil
    }

    static func loadOrCreate() -> String? {
        if let key = load() { return key }
        guard let key = try? generatePushKey() else { return nil }
        for g in [group, nil] {
            var q = query(group: g)
            q[kSecValueData as String] = Data(key.utf8)
            q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let status = SecItemAdd(q as CFDictionary, nil)
            if status == errSecSuccess { return key }
            logger.error("device key not stored (\(status)) group=\(g != nil)")
            if g == nil { break }
        }
        return nil
    }

    static func delete() {
        SecItemDelete(query(group: group) as CFDictionary)
        SecItemDelete(query(group: nil) as CFDictionary)
    }
}
