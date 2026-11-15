import SwiftUI
import UserNotifications

@main
struct TalkLegacyApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @Environment(\.scenePhase) private var scenePhase
    @Bindable private var store = SessionStore.shared
    private var appearance: Appearance { .shared }

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
                    LoginView { result in try await store.login(result) }
                case .ready:
                    RootView(store: store)
                        // "Konto hinzufügen" (docs/accounts.md): the login screen over the app;
                        // the other accounts keep running.
                        .fullScreenCover(isPresented: $store.adding) {
                            LoginView(adding: true, onCancel: { store.adding = false }) { result in
                                try await store.login(result)
                            }
                        }
                }
            }
            // Design and accent (docs/settings.md "Darstellung"); the windows get the same
            // style, so sheets and alerts follow at once.
            .tint(Talk.accent)
            .preferredColorScheme(appearance.colorScheme)
            .onAppear { appearance.applyToWindows() }
            // The first account just signed in: ask for notifications (docs/notifications.md).
            .onChange(of: store.phase) { _, phase in
                if phase == .ready && scenePhase == .active { Notifications.askAtStart() }
            }
        }
        .onChange(of: scenePhase) { _, phase in
            store.foreground = phase == .active
            switch phase {
            case .active:
                BackgroundSender.shared.enteredForeground()
                if let token = store.activeToken { store.markVisibleRead(token) }
                store.cameToFront()
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

/// Screens pushed on the conversation list.
enum Route: Hashable {
    case chat(String)
    case groupInfo(String)
    /// "Einstellungen" and its pages (docs/settings.md).
    case settings
    case settingsPage(SettingsPage)
}

/// Conversation list with the open chat (and its group settings) pushed on top; the store's
/// active token is the single source of truth (notification taps open a chat the same way).
/// Without an open chat the store's `listPath` holds the settings or group settings opened
/// from the chat menu.
struct RootView: View {
    @Bindable var store: SessionStore

    private var path: Binding<[Route]> {
        Binding(
            get: {
                guard let token = store.activeToken else { return store.listPath }
                return store.groupInfoOpen ? [.chat(token), .groupInfo(token)] : [.chat(token)]
            },
            set: { new in
                guard case .chat(let token)? = new.first else {
                    store.closeRoom()
                    store.listPath = new
                    return
                }
                store.openRoom(token)
                store.showGroupInfo(new.count > 1)
            }
        )
    }

    var body: some View {
        NavigationStack(path: path) {
            RoomListView(store: store)
                .navigationDestination(for: Route.self) { route in
                    switch route {
                    case .chat(let token): ChatView(store: store, token: token)
                    case .groupInfo(let token): GroupInfoView(store: store, token: token)
                    case .settings: SettingsView(store: store)
                    case .settingsPage(let page): page.view(store: store)
                    }
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
        // APNs token (background pushes); the push itself needs no permission.
        application.registerForRemoteNotifications()
        #if DEBUG
        DebugEnv.persist()
        Heartbeat.start()
        Task { @MainActor in
            await LogDump.deliveredNotifications()
            try? await Task.sleep(for: .seconds(2))
            CallController.shared.showMockIfRequested()
            CallController.shared.autoCallIfRequested()
        }
        #endif
        return true
    }

    /// Portrait, except while the call screen shows a shared screen (docs/calls.md
    /// "Bildschirmfreigabe"); the system rotation lock still applies.
    func application(_ application: UIApplication, supportedInterfaceOrientationsFor window: UIWindow?) -> UIInterfaceOrientationMask {
        CallController.shared.allowsLandscape ? .allButUpsideDown : .portrait
    }

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        Push.shared.apnsToken(deviceToken)
    }

    func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        Log.push.error("apns: \(error.localizedDescription, privacy: .public)")
    }

    /// Background push from the server (e.g. a call notification was deleted).
    nonisolated func application(
        _ application: UIApplication, didReceiveRemoteNotification userInfo: [AnyHashable: Any]
    ) async -> UIBackgroundFetchResult {
        await Push.handleBackground(userInfo)
        return .newData
    }

    /// Swiped away during a call: leave it properly so the others do not wait for us.
    func applicationWillTerminate(_ application: UIApplication) {
        let calls = SessionStore.shared.allRuntimes.map(\.session).filter { $0.currentCall() != nil }
        guard let session = calls.first else { return }
        let done = DispatchSemaphore(value: 0)
        Task.detached {
            await session.hangUpAndWait(timeoutMs: 2000)
            done.signal()
        }
        _ = done.wait(timeout: .now() + 2.5)
    }

    /// In front: show the banner unless exactly this chat is open in the active account.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        let info = notification.request.content.userInfo
        let token = info["token"] as? String
        let account = info["account"] as? String
        let open = await MainActor.run { () -> Bool in
            let store = SessionStore.shared
            guard let token, token == store.activeToken else { return false }
            return store.runtime(account)?.account.id == store.activeId
        }
        return open ? [] : [.banner, .list, .sound]
    }

    /// A tap switches into the notification's account and opens the chat (not during a call).
    /// "Antworten" and "Als gelesen markieren" act in the notification's account without
    /// switching; for them iOS may start the app in the background.
    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        guard let token = info["token"] as? String else { return }
        let account = info["account"] as? String
        switch response.actionIdentifier {
        case Notifications.replyAction:
            let text = (response as? UNTextInputNotificationResponse)?.userText ?? ""
            await MainActor.run { SessionStore.shared.reply(account: account, token: token, text: text) }
            return
        case Notifications.readAction:
            await SessionStore.shared.markReadFromNotification(account: account, token: token)
            return
        case UNNotificationDefaultActionIdentifier:
            break
        default:
            return
        }
        await MainActor.run {
            let store = SessionStore.shared
            guard let id = store.runtime(account)?.account.id, store.switchTo(id) else { return }
            if store.activeToken != token {
                store.closeRoom()
                store.openRoom(token)
            }
        }
    }
}
