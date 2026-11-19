import SwiftUI
import UIKit

/// Pages below "Einstellungen" (docs/settings.md), each a route on the navigation stack.
enum SettingsPage: Hashable {
    case appearance, notifications, about

    @MainActor @ViewBuilder
    func view(store: SessionStore) -> some View {
        switch self {
        case .appearance: AppearanceSettingsView()
        case .notifications: NotificationSettingsView(store: store)
        case .about: AboutView()
        }
    }
}

/// Grouped list like the system settings, on the shared settings colours (docs/groups.md).
private extension View {
    func settingsList(_ title: String) -> some View {
        listStyle(.insetGrouped)
            .listSectionSpacing(20)
            .scrollContentBackground(.hidden)
            .background(Talk.groupedBackground)
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Talk.groupedBackground, for: .navigationBar)
    }
}

/// Start page: the active account's avatar, name and server (nothing to edit), the other
/// accounts (docs/accounts.md), the rows, "Abmelden".
struct SettingsView: View {
    @Bindable var store: SessionStore
    @State private var confirmLogout = false

    var body: some View {
        List {
            Section {
                VStack(spacing: 0) {
                    PersonAvatar(name: store.user, userId: store.userId, size: 64)
                    Text(store.user)
                        .font(.system(size: 20, weight: .semibold))
                        .foregroundStyle(Talk.onSurface)
                        .multilineTextAlignment(.center)
                        .padding(.top, 10)
                    Text(Account.label(store.server))
                        .font(.system(size: 14))
                        .foregroundStyle(Talk.muted)
                        .multilineTextAlignment(.center)
                        .padding(.top, 2)
                }
                .frame(maxWidth: .infinity)
                .accessibilityElement(children: .combine)
            }
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 0, trailing: 16))

            accounts

            Section {
                row(.appearance, icon: "paintpalette", title: "Darstellung")
                row(.notifications, icon: "bell", title: "Benachrichtigungen",
                    value: NotificationPermission.shared.off ? "Aus" : nil)
                row(.about, icon: "info.circle", title: "Über Wolkenfunk")
            }

            Section {
                Button(role: .destructive) { confirmLogout = true } label: {
                    SettingsLabel(icon: "rectangle.portrait.and.arrow.right", text: "Abmelden", danger: true)
                }
                .accessibilityLabel("Abmelden")
                .accessibilityIdentifier("settings-logout")
                .listRowBackground(Talk.groupedRow)
            }
        }
        .settingsList("Einstellungen")
        // An alert, not a popover: the contract wants "Abmelden" and "Abbrechen" both shown.
        .alert("Abmelden?", isPresented: $confirmLogout) {
            Button("Abmelden", role: .destructive) { store.logout() }
            Button("Abbrechen", role: .cancel) {}
        } message: {
            Text("\(store.user) (\(Account.label(store.server))) wird auf diesem Gerät abgemeldet. Ungesendete Nachrichten und der Offline-Speicher dieses Kontos werden gelöscht.")
        }
    }

    /// "Konten": the other accounts (a tap switches, the settings stay open) and
    /// "Konto hinzufügen", also while the avatar still opens the settings directly.
    private var accounts: some View {
        let busy = CallController.shared.busy
        return Section {
            ForEach(store.accounts.filter { $0.id != store.activeId }) { a in
                Button { store.switchTo(a.id) } label: {
                    AccountRow(account: a, active: false, count: store.newChats(a.id), locked: busy)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("account:\(a.id)")
                .listRowBackground(Talk.groupedRow)
            }
            if store.accounts.count < maxAccounts {
                Button { store.adding = true } label: {
                    SettingsLabel(icon: "plus", text: "Konto hinzufügen")
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Konto hinzufügen")
                .accessibilityIdentifier("settings-add-account")
                .listRowBackground(Talk.groupedRow)
            }
        } header: {
            SettingsHeader("Konten")
        }
    }

    /// `value`: a state at the right ("Aus"), also in the label.
    private func row(_ page: SettingsPage, icon: String, title: String, value: String? = nil) -> some View {
        NavigationLink(value: Route.settingsPage(page)) {
            HStack {
                SettingsLabel(icon: icon, text: title)
                if let value {
                    Spacer(minLength: 8)
                    Text(value).font(.system(size: 17)).foregroundStyle(Talk.muted)
                }
            }
        }
        .accessibilityLabel(value.map { "\(title), \($0.lowercased())" } ?? title)
        .accessibilityIdentifier("settings:\(title)")
        .listRowBackground(Talk.groupedRow)
    }
}

/// "Darstellung": preview, design and accent colour; every change applies at once.
struct AppearanceSettingsView: View {
    @Bindable private var appearance = Appearance.shared

    var body: some View {
        List {
            Section { AppearancePreview() }
                .listRowBackground(Talk.background)

            Section {
                Picker("Design", selection: $appearance.theme) {
                    ForEach(Appearance.Theme.allCases) { Text($0.label).tag($0) }
                }
                .pickerStyle(.inline)
                .labelsHidden()
                .listRowBackground(Talk.groupedRow)
            } header: {
                SettingsHeader("Design")
            }

            Section {
                AccentGrid(selected: $appearance.accentID)
                    .padding(.vertical, 12)
                    .listRowBackground(Talk.groupedRow)
            } header: {
                SettingsHeader("Akzentfarbe")
            }

            Section {
                Toggle(isOn: $appearance.splitChats) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Gruppen und Direktchats trennen")
                        Text("Eigene Abschnitte statt „Chats“")
                            .font(.footnote)
                            .foregroundStyle(Talk.muted)
                    }
                }
                .accessibilityIdentifier("splitChats")
                .listRowBackground(Talk.groupedRow)
            } header: {
                SettingsHeader("Chatliste")
            }
        }
        .settingsList("Darstellung")
    }
}

/// A received and an own bubble in the real bubble colours, the own one in the accent.
private struct AppearancePreview: View {
    var body: some View {
        VStack(spacing: 6) {
            bubble("Wie gefällt dir die neue Farbe?", own: false)
            bubble("Sieht gut aus!", own: true)
        }
        .padding(.vertical, 10)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Vorschau")
    }

    private func bubble(_ text: String, own: Bool) -> some View {
        HStack {
            if own { Spacer(minLength: 48) }
            Text(text)
                .foregroundStyle(own ? Talk.bubbleOwnText : Talk.bubbleOtherText)
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
                .background(own ? Talk.bubbleOwn : Talk.bubbleOther, in: RoundedRectangle(cornerRadius: 18))
            if !own { Spacer(minLength: 48) }
        }
    }
}

/// Round swatches, five per row; the chosen one has a white tick and a ring.
private struct AccentGrid: View {
    @Binding var selected: String
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: 5), spacing: 16) {
            ForEach(Accent.all) { a in
                let on = a.id == selected
                let color = Color(hex: scheme == .dark ? a.dark.accent : a.light.accent)
                Button { selected = a.id } label: {
                    ZStack {
                        Circle().fill(color).frame(width: 44, height: 44)
                        if on {
                            Circle().stroke(color, lineWidth: 2).frame(width: 50, height: 50)
                            Image(systemName: "checkmark")
                                .font(.system(size: 17, weight: .bold))
                                .foregroundStyle(.white)
                        }
                    }
                    .frame(width: 52, height: 52)
                    .contentShape(Circle())
                }
                .buttonStyle(.plain)
                // The state is in the label (docs/settings.md), not read twice as a trait.
                .accessibilityLabel(on ? "\(a.name), ausgewählt" : a.name)
                .accessibilityRemoveTraits(.isSelected)
                .accessibilityIdentifier("accent:\(a.id)")
            }
        }
    }
}

/// "Benachrichtigungen" (docs/notifications.md "Erlaubnis und Einstellungen"): the state
/// when they are off, with "Einschalten"; the system page of the app (single chats via the
/// chat menu); "Push testen" for the active account.
struct NotificationSettingsView: View {
    let store: SessionStore
    @Environment(\.openURL) private var openURL
    @State private var testing = false
    private var permission: NotificationPermission { .shared }

    var body: some View {
        List {
            if permission.off {
                Section {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Benachrichtigungen sind ausgeschaltet")
                            .font(.system(size: 17, weight: .semibold))
                            .foregroundStyle(Talk.onSurface)
                        Text("Neue Nachrichten und Anrufe bemerkst du dann nur bei geöffneter App.")
                            .font(.system(size: 15))
                            .foregroundStyle(Talk.muted)
                    }
                    .padding(.vertical, 4)
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("notifications-off")
                    .listRowBackground(Talk.groupedRow)
                    Button {
                        Task { await permission.turnOn { openURL($0) } }
                    } label: {
                        Text("Einschalten").font(.system(size: 17)).foregroundStyle(Talk.accent)
                    }
                    .accessibilityIdentifier("notifications-on")
                    .listRowBackground(Talk.groupedRow)
                }
            }

            Section {
                Button {
                    if let url = URL(string: UIApplication.openNotificationSettingsURLString) { openURL(url) }
                } label: {
                    HStack {
                        SettingsLabel(icon: "gear", text: "Systemeinstellungen öffnen")
                        Image(systemName: "arrow.up.forward").font(.system(size: 13)).foregroundStyle(Talk.muted)
                    }
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Systemeinstellungen öffnen")
                .accessibilityIdentifier("settings-system")
                .listRowBackground(Talk.groupedRow)
            } footer: {
                Text("Für einzelne Chats: Chat in der Liste lange gedrückt halten.")
                    .font(.system(size: 13))
                    .foregroundStyle(Talk.muted)
            }

            Section {
                if Push.proxyURL.isEmpty {
                    SettingsLabel(icon: "bell.badge", text: "Diese App-Version hat keinen Push")
                        .foregroundStyle(Talk.muted)
                        .accessibilityIdentifier("settings-test-push")
                        .listRowBackground(Talk.groupedRow)
                } else {
                    Button { test() } label: {
                        SettingsLabel(icon: "bell.badge", text: "Push testen")
                    }
                    .buttonStyle(.plain)
                    .disabled(testing)
                    .accessibilityLabel("Push testen")
                    .accessibilityIdentifier("settings-test-push")
                    .listRowBackground(Talk.groupedRow)
                }
            }
        }
        .settingsList("Benachrichtigungen")
        .task { await permission.refresh() }
    }

    private func test() {
        testing = true
        Task {
            let ok = await store.testPush()
            testing = false
            Toast.show(ok
                ? "Test angefordert. Die Benachrichtigung sollte in wenigen Sekunden erscheinen."
                : "Der Server hat den Test nicht angenommen.")
        }
    }
}

/// "Über Wolkenfunk": icon, versions, links, licence.
struct AboutView: View {
    var body: some View {
        List {
            Section {
                VStack(spacing: 0) {
                    if let icon = Self.appIcon {
                        Image(uiImage: icon)
                            .resizable()
                            .frame(width: 64, height: 64)
                            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                            .accessibilityHidden(true)
                    }
                    Text("Wolkenfunk")
                        .font(.system(size: 20, weight: .semibold))
                        .foregroundStyle(Talk.onSurface)
                        .padding(.top, 10)
                    Text("Version \(Self.version)")
                        .font(.system(size: 15))
                        .foregroundStyle(Talk.onSurface)
                        .padding(.top, 4)
                    Text("Kern \(coreVersion())")
                        .font(.system(size: 14))
                        .foregroundStyle(Talk.muted)
                        .padding(.top, 2)
                }
                .frame(maxWidth: .infinity)
                .accessibilityElement(children: .combine)
            }
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 0, trailing: 16))

            Section {
                link("Webseite", icon: "globe", url: "https://wolkenfunk.org")
                link("Quellcode", icon: "chevron.left.forwardslash.chevron.right",
                     url: "https://github.com/raph-ael/nextcloud-talk-legacy")
            } footer: {
                Text("Lizenz: AGPL-3.0")
                    .font(.system(size: 13))
                    .foregroundStyle(Talk.muted)
            }
        }
        .settingsList("Über Wolkenfunk")
    }

    private func link(_ title: String, icon: String, url: String) -> some View {
        Link(destination: URL(string: url)!) {
            HStack {
                SettingsLabel(icon: icon, text: title)
                Image(systemName: "arrow.up.forward").font(.system(size: 13)).foregroundStyle(Talk.muted)
            }
        }
        .accessibilityLabel(title)
        .listRowBackground(Talk.groupedRow)
    }

    static var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
    }

    /// The home screen icon (from the bundle's icon files).
    static var appIcon: UIImage? {
        guard let icons = Bundle.main.object(forInfoDictionaryKey: "CFBundleIcons") as? [String: Any],
              let primary = icons["CFBundlePrimaryIcon"] as? [String: Any],
              let files = primary["CFBundleIconFiles"] as? [String], let name = files.last
        else { return nil }
        return UIImage(named: name)
    }
}
