import SwiftUI

@main
struct TalkLegacyApp: App {
    @State private var store = AccountStore()

    var body: some Scene {
        WindowGroup {
            if let session = store.session {
                RoomListView(session: session, accountName: store.account?.loginName ?? "") {
                    store.logout()
                }
            } else {
                LoginView { result in try store.login(result) }
            }
        }
    }
}

/// Holds the signed-in account. Credentials live in the Keychain.
@MainActor
@Observable
final class AccountStore {
    private(set) var account: StoredAccount?
    private(set) var session: TalkSession?

    init() {
        if let saved = Keychain.loadAccount() {
            account = saved
            session = try? Self.makeSession(saved)
        }
    }

    func login(_ result: LoginResult) throws {
        let saved = StoredAccount(server: result.server, loginName: result.loginName, appPassword: result.appPassword)
        session = try Self.makeSession(saved)
        account = saved
        Keychain.saveAccount(saved)
    }

    func logout() {
        // TODO: revoke the app password on the server (DELETE /ocs/v2.php/core/apppassword).
        Keychain.deleteAccount()
        session = nil
        account = nil
    }

    private static func makeSession(_ a: StoredAccount) throws -> TalkSession {
        try TalkSession(server: a.server, user: a.loginName, appPassword: a.appPassword, platform: .ios)
    }
}
