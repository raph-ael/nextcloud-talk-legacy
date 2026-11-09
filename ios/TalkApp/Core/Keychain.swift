import Foundation
import Security

struct StoredAccount: Codable, Equatable {
    let server: String
    let loginName: String
    let appPassword: String
}

/// Minimal Keychain wrapper for the single account (multi-account comes later).
enum Keychain {
    private static let service = "org.sirplus.talk.account"
    private static let key = "primary"

    private static var baseQuery: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
    }

    static func saveAccount(_ account: StoredAccount) {
        guard let data = try? JSONEncoder().encode(account) else { return }
        SecItemDelete(baseQuery as CFDictionary)
        var query = baseQuery
        query[kSecValueData as String] = data
        // Readable after first unlock so push handling can use it in the background.
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(query as CFDictionary, nil)
    }

    static func loadAccount() -> StoredAccount? {
        var query = baseQuery
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data else { return nil }
        return try? JSONDecoder().decode(StoredAccount.self, from: data)
    }

    static func deleteAccount() {
        SecItemDelete(baseQuery as CFDictionary)
    }
}
