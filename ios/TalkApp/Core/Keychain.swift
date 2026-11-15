import Foundation
import Security

/// One signed-in account (docs/accounts.md "Kontoliste"), kept in UserDefaults. The app
/// password lives in the Keychain, the push key in the shared group (`DeviceKeys`).
struct Account: Codable, Identifiable, Equatable, Hashable {
    /// Random, local, never reused; in folder, key and notification names.
    let id: String
    let server: String
    let loginName: String
    var userId: String
    /// `displayName()` of the server, refreshed after every start.
    var name: String
    /// From `resolveAccount`: the same key means "already signed in".
    var key: String
    /// Data folder below Application Support: `talk` for the account taken over from an
    /// earlier version (never moved, the outbox keeps absolute paths), else `accounts/<id>`.
    let dir: String

    /// The address without `https://` or `http://`, wherever it is shown.
    var serverLabel: String { Self.label(server) }

    static func label(_ server: String) -> String {
        for scheme in ["https://", "http://"] where server.hasPrefix(scheme) {
            return String(server.dropFirst(scheme.count))
        }
        return server
    }

    /// "{Name} · {Server}": the account line of notifications.
    var line: String { "\(name) · \(serverLabel)" }

    static func newId() -> String {
        var bytes = [UInt8](repeating: 0, count: 8)
        if SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) != errSecSuccess {
            bytes = (0..<8).map { _ in UInt8.random(in: 0...255) }
        }
        return bytes.map { String(format: "%02x", $0) }.joined()
    }
}

/// The account list and the active account in UserDefaults.
enum AccountList {
    private static let listKey = "accounts.list"
    private static let activeKey = "accounts.active"
    /// The account of the earlier version was taken over (or there was none).
    private static let migratedKey = "accounts.migrated"

    static func load() -> [Account] {
        guard let data = UserDefaults.standard.data(forKey: listKey),
              let list = try? JSONDecoder().decode([Account].self, from: data) else { return [] }
        return list
    }

    static func save(_ list: [Account]) {
        guard let data = try? JSONEncoder().encode(list) else { return }
        UserDefaults.standard.set(data, forKey: listKey)
    }

    static var active: String? {
        get { UserDefaults.standard.string(forKey: activeKey) }
        set { UserDefaults.standard.set(newValue, forKey: activeKey) }
    }

    static var migrated: Bool {
        get { UserDefaults.standard.bool(forKey: migratedKey) }
        set { UserDefaults.standard.set(newValue, forKey: migratedKey) }
    }
}

/// What a Keychain read found. A failure (device locked since the restart) is not "no
/// account": nothing may be deleted then (docs/accounts.md "Lesefehler").
enum KeychainRead<T> {
    case found(T)
    case missing
    case failed(OSStatus)
}

/// The account of the earlier version (`primary`, the whole login as JSON).
struct StoredAccount: Codable, Equatable {
    let server: String
    let loginName: String
    let appPassword: String
}

/// App passwords in the Keychain, one item per account id.
enum Keychain {
    private static let service = "org.wolkenfunk.app.account"
    private static let legacyKey = "primary"

    private static func query(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }

    private static func read(_ account: String) -> KeychainRead<Data> {
        var q = query(account)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            guard let data = item as? Data else { return .failed(status) }
            return .found(data)
        case errSecItemNotFound:
            return .missing
        default:
            return .failed(status)
        }
    }

    /// `true` once stored.
    @discardableResult
    static func savePassword(_ password: String, for id: String) -> Bool {
        SecItemDelete(query(id) as CFDictionary)
        var q = query(id)
        q[kSecValueData as String] = Data(password.utf8)
        // Readable after the first unlock: pushes and the outbox work while locked.
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return SecItemAdd(q as CFDictionary, nil) == errSecSuccess
    }

    static func password(for id: String) -> KeychainRead<String> {
        switch read(id) {
        case .found(let data):
            guard let s = String(data: data, encoding: .utf8) else { return .failed(errSecDecode) }
            return .found(s)
        case .missing: return .missing
        case .failed(let s): return .failed(s)
        }
    }

    static func deletePassword(for id: String) {
        SecItemDelete(query(id) as CFDictionary)
    }

    /// The single account of the earlier version.
    static func legacyAccount() -> KeychainRead<StoredAccount> {
        switch read(legacyKey) {
        case .found(let data):
            guard let a = try? JSONDecoder().decode(StoredAccount.self, from: data) else { return .failed(errSecDecode) }
            return .found(a)
        case .missing: return .missing
        case .failed(let s): return .failed(s)
        }
    }

    static func deleteLegacy() {
        SecItemDelete(query(legacyKey) as CFDictionary)
    }

    // MARK: Sign-out without network

    private static let revokeKey = "revoke.pending"

    /// App passwords that could not be given back at sign-out (docs/notifications.md
    /// "Abmelden ohne Netz"); the next start tries again.
    static func pendingRevokes() -> [StoredAccount] {
        guard case .found(let data) = read(revokeKey),
              let list = try? JSONDecoder().decode([StoredAccount].self, from: data) else { return [] }
        return list
    }

    static func setPendingRevokes(_ list: [StoredAccount]) {
        SecItemDelete(query(revokeKey) as CFDictionary)
        guard !list.isEmpty, let data = try? JSONEncoder().encode(list) else { return }
        var q = query(revokeKey)
        q[kSecValueData as String] = data
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(q as CFDictionary, nil)
    }
}
