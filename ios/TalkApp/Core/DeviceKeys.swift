import Foundation
import os
import Security

/// The push keys (PKCS#8 PEM), one per account (docs/accounts.md "Push"). They live in the
/// keychain group shared with the Notification Service Extension (`TalkKeychainGroup`),
/// which decrypts chat pushes; builds without that entitlement (simulator) keep them
/// private. Each item carries its account id (`kSecAttrAccount` "account.<id>") and the
/// account line "{Name} · {Server}", so PushKit and the extension can tell the account
/// synchronously, without a session and without network.
///
/// The key of the earlier version (`device-key`) stays the fallback until the app has
/// taken its account over: PushKit and the extension may run before that.
enum DeviceKeys {
    private static let logger = Logger(subsystem: "org.wolkenfunk.app", category: "push")
    private static let service = "org.wolkenfunk.app.push"
    private static let legacyName = "device-key"
    private static let prefix = "account."
    /// Account id of the earlier version's key in `decryptKeys()`.
    static let legacyId = "legacy"

    /// One account's key and line.
    struct Entry {
        let accountId: String
        let keyPem: String
        let line: String
    }

    private struct Stored: Codable {
        let key: String
        let line: String
    }

    nonisolated private static var group: String? {
        let g = Bundle.main.object(forInfoDictionaryKey: "TalkKeychainGroup") as? String ?? ""
        return g.hasPrefix(".") || g.isEmpty ? nil : g
    }

    /// The shared group first, then the app's own (builds without the group).
    nonisolated private static var groups: [String?] { group == nil ? [nil] : [group, nil] }

    nonisolated private static func query(_ name: String?, group: String?) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
        ]
        if let name { q[kSecAttrAccount as String] = name }
        if let group { q[kSecAttrAccessGroup as String] = group }
        return q
    }

    // MARK: Reading (also in the extension)

    /// All account keys, in no particular order.
    nonisolated static func all() -> [Entry] {
        var found: [String: Entry] = [:]
        for g in groups {
            var q = query(nil, group: g)
            q[kSecReturnData as String] = true
            q[kSecReturnAttributes as String] = true
            q[kSecMatchLimit as String] = kSecMatchLimitAll
            var items: CFTypeRef?
            guard SecItemCopyMatching(q as CFDictionary, &items) == errSecSuccess,
                  let list = items as? [[String: Any]] else { continue }
            for item in list {
                guard let name = item[kSecAttrAccount as String] as? String, name.hasPrefix(prefix),
                      let data = item[kSecValueData as String] as? Data,
                      let stored = try? JSONDecoder().decode(Stored.self, from: data) else { continue }
                let id = String(name.dropFirst(prefix.count))
                if found[id] == nil { found[id] = Entry(accountId: id, keyPem: stored.key, line: stored.line) }
            }
        }
        return Array(found.values)
    }

    /// The earlier version's single key.
    nonisolated static func legacy() -> String? {
        for g in groups {
            var q = query(legacyName, group: g)
            q[kSecReturnData as String] = true
            q[kSecMatchLimit as String] = kSecMatchLimitOne
            var item: CFTypeRef?
            if SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess, let data = item as? Data {
                return String(data: data, encoding: .utf8)
            }
        }
        return nil
    }

    /// Everything that may decrypt a push: the accounts' keys, then the earlier version's
    /// key (as `legacyId`) while it is still there.
    nonisolated static func decryptKeys() -> [PushKey] {
        var keys = all().map { PushKey(accountId: $0.accountId, keyPem: $0.keyPem) }
        if let old = legacy() { keys.append(PushKey(accountId: legacyId, keyPem: old)) }
        return keys
    }

    // MARK: Writing (app only)

    private static func store(_ name: String, _ data: Data) -> Bool {
        for g in groups {
            SecItemDelete(query(name, group: g) as CFDictionary)
            var q = query(name, group: g)
            q[kSecValueData as String] = data
            // Readable after the first unlock, so a call can be decrypted while locked.
            q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let status = SecItemAdd(q as CFDictionary, nil)
            if status == errSecSuccess { return true }
            logger.error("push key not stored (\(status)) group=\(g != nil)")
        }
        return false
    }

    private static func write(_ id: String, key: String, line: String) -> Bool {
        guard let data = try? JSONEncoder().encode(Stored(key: key, line: line)) else { return false }
        return store(prefix + id, data)
    }

    static func key(for id: String) -> String? {
        all().first { $0.accountId == id }?.keyPem
    }

    /// The account's key; a new one if it has none yet.
    static func loadOrCreate(for id: String, line: String) -> String? {
        if let key = key(for: id) { return key }
        guard let key = try? generatePushKey(), write(id, key: key, line: line) else { return nil }
        return key
    }

    /// The account line changed (name refreshed after a start).
    static func setLine(_ line: String, for id: String) {
        guard let e = all().first(where: { $0.accountId == id }), e.line != line else { return }
        _ = write(id, key: e.keyPem, line: line)
    }

    /// Takeover: the earlier version's key becomes this account's, so the registration at
    /// the server stays valid. `true` when there was none or it was copied.
    static func adoptLegacy(for id: String, line: String) -> Bool {
        guard let old = legacy() else { return true }
        return write(id, key: old, line: line)
    }

    static func deleteLegacy() {
        for g in groups { SecItemDelete(query(legacyName, group: g) as CFDictionary) }
    }

    static func delete(for id: String) {
        for g in groups { SecItemDelete(query(prefix + id, group: g) as CFDictionary) }
    }
}
