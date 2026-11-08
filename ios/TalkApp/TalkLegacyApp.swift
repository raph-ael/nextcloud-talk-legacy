import SwiftUI

@main
struct TalkLegacyApp: App {
    @State private var store = AccountStore()

    var body: some Scene {
        WindowGroup {
            if store.session != nil {
                RoomListView(store: store)
            } else {
                LoginView { result in try store.login(result) }
            }
        }
    }
}

/// Holds the signed-in account and the state the core pushes to us.
/// Credentials live in the Keychain.
@MainActor
@Observable
final class AccountStore {
    private(set) var account: StoredAccount?
    private(set) var session: TalkSession?
    private(set) var rooms: [RoomView] = []
    private(set) var online = true
    @ObservationIgnored private let bridge = EventBridge()

    init() {
        bridge.store = self
        if let saved = Keychain.loadAccount() {
            start(saved)
        }
    }

    func login(_ result: LoginResult) throws {
        let saved = StoredAccount(server: result.server, loginName: result.loginName, appPassword: result.appPassword)
        Keychain.saveAccount(saved)
        start(saved)
    }

    func logout() {
        let s = session
        Keychain.deleteAccount()
        session = nil
        account = nil
        rooms = []
        Task { try? await s?.logout() }
    }

    func refresh() async {
        try? await session?.refreshRooms()
    }

    private func start(_ saved: StoredAccount) {
        guard let s = try? TalkSession(
            server: saved.server, user: saved.loginName, appPassword: saved.appPassword,
            platform: .ios, listener: bridge
        ) else { return }
        account = saved
        session = s
        Task {
            do {
                try await s.start()
            } catch CoreError.Unauthorized {
                self.logout()
            } catch {
                // Offline: the core retries in the background.
            }
        }
    }

    fileprivate func handle(_ event: CoreEvent) {
        switch event {
        case .roomsChanged(let rooms):
            self.rooms = rooms
        case .connection(let online):
            self.online = online
        case .authFailed:
            Keychain.deleteAccount()
            session?.shutdown()
            session = nil
            account = nil
            rooms = []
        case .messagesAdded, .messagesUpdated, .readStatus:
            break // chat view: next iteration
        }
    }
}

/// Receives core events on a Rust thread and forwards them to the main actor.
private final class EventBridge: CoreListener, @unchecked Sendable {
    weak var store: AccountStore?

    func onEvent(event: CoreEvent) {
        Task { @MainActor [weak self] in
            self?.store?.handle(event)
        }
    }
}
