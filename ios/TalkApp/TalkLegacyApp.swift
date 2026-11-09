import SwiftUI
import UserNotifications

@main
struct TalkLegacyApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @Environment(\.scenePhase) private var scenePhase
    private var store: SessionStore { .shared }

    init() {
        // Before the store starts a session (it may schedule the outbox task right away).
        BackgroundSender.register()
        // Before any WebRTC track: audio is switched on by CallKit, not by WebRTC.
        WebRTCMediaEngine.prepare()
    }

    var body: some Scene {
        WindowGroup {
            ZStack {
                switch store.phase {
                case .loading:
                    Talk.background.ignoresSafeArea()
                case .loggedOut:
                    LoginView { result in try store.login(result) }
                case .ready:
                    RootView(store: store)
                }
                ToastOverlay()
            }
            .tint(Talk.accent)
        }
        .onChange(of: scenePhase) { _, phase in
            store.foreground = phase == .active
            switch phase {
            case .active:
                BackgroundSender.shared.enteredForeground()
                if let token = store.activeToken { store.markVisibleRead(token) }
            case .inactive:
                // Screen off, app switcher, incoming call: like Signal, a recording stops as draft.
                store.voice.interrupt()
            case .background:
                store.voice.interrupt()
                BackgroundSender.shared.enteredBackground()
            @unknown default:
                break
            }
        }
    }
}

/// Conversation list with the open chat pushed on top; the store's active token is the
/// single source of truth (notification taps open a chat the same way).
struct RootView: View {
    @Bindable var store: SessionStore

    private var path: Binding<[String]> {
        Binding(
            get: { store.activeToken.map { [$0] } ?? [] },
            set: { new in
                if let token = new.last { store.openRoom(token) } else { store.closeRoom() }
            }
        )
    }

    var body: some View {
        NavigationStack(path: path) {
            RoomListView(store: store)
                .navigationDestination(for: String.self) { token in
                    ChatView(store: store, token: token)
                }
        }
        .safeAreaInset(edge: .top, spacing: 0) { CallBar() }
    }
}

final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        _ = CallController.shared
        VoipPush.shared.register()
        #if DEBUG
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(2))
            CallController.shared.showMockIfRequested()
            CallController.shared.autoCallIfRequested()
        }
        #endif
        return true
    }

    /// In front: show the failure banner unless that chat is open.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        let token = notification.request.content.userInfo["token"] as? String
        let open = await MainActor.run { SessionStore.shared.activeToken }
        return token != nil && token == open ? [] : [.banner, .sound]
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        guard let token = response.notification.request.content.userInfo["token"] as? String else { return }
        await MainActor.run {
            let store = SessionStore.shared
            if store.activeToken != token {
                store.closeRoom()
                store.openRoom(token)
            }
        }
    }
}
