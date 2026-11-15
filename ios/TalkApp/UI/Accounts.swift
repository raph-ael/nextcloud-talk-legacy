import SwiftUI

/// The most accounts the menu offers to add to (docs/accounts.md "Kontomenü").
let maxAccounts = 5

/// A number in `accent` with white text, like the unread count in the chat list.
struct CountBadge: View {
    let count: Int
    var small = false

    var body: some View {
        Text(count > 99 ? "99+" : "\(count)")
            .font(.system(size: small ? 11 : 12, weight: .medium))
            .foregroundStyle(.white)
            .padding(.horizontal, small ? 4 : 6)
            .frame(minWidth: small ? 18 : 22, minHeight: small ? 18 : 22)
            .background(Talk.accent, in: Capsule())
    }
}

/// One account as in the account menu: avatar 36, name, server below it (muted); at the
/// right a tick for the active one, else its chats with something new.
struct AccountRow: View {
    let account: Account
    let active: Bool
    let count: Int
    /// Greyed out while a call rings or runs.
    let locked: Bool

    var body: some View {
        HStack(spacing: 12) {
            PersonAvatar(name: account.name, userId: account.userId, size: 36, account: account.id)
            VStack(alignment: .leading, spacing: 1) {
                Text(account.name)
                    .font(.system(size: 17))
                    .foregroundStyle(Talk.onSurface)
                    .lineLimit(1)
                Text(account.serverLabel)
                    .font(.system(size: 14))
                    .foregroundStyle(Talk.muted)
                    .lineLimit(1)
            }
            Spacer(minLength: 8)
            if active {
                Image(systemName: "checkmark")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(Talk.accent)
            } else if count > 0 {
                CountBadge(count: count)
            }
        }
        .frame(minHeight: 52)
        .opacity(locked ? 0.4 : 1)
        .contentShape(Rectangle())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
        .accessibilityAddTraits(.isButton)
    }

    private var label: String {
        var parts = ["\(account.name), \(account.serverLabel)"]
        if active {
            parts.append("aktiv")
        } else if count > 0 {
            parts.append(count == 1 ? "1 Chat mit Neuem" : "\(count) Chats mit Neuem")
        }
        if locked { parts.append("nicht verfügbar") }
        return parts.joined(separator: ", ")
    }
}

/// The account menu at the own avatar (two accounts and more), a sheet in half height:
/// the accounts in the order of sign-in, a line, "Konto hinzufügen", "Einstellungen".
struct AccountMenu: View {
    @Bindable var store: SessionStore
    /// After the sheet is gone: add an account or open the settings.
    let then: (AccountMenuAction) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let busy = CallController.shared.busy
        ScrollView {
            VStack(spacing: 0) {
                ForEach(store.accounts) { a in
                    let active = a.id == store.activeId
                    Button {
                        if active {
                            dismiss()
                        } else if store.switchTo(a.id) {
                            dismiss()
                        }
                    } label: {
                        AccountRow(account: a, active: active, count: store.newChats(a.id), locked: busy && !active)
                            .padding(.horizontal, 20)
                            .padding(.vertical, 4)
                    }
                    .buttonStyle(MenuRowStyle())
                    .accessibilityIdentifier("account:\(a.id)")
                }
                Divider().padding(.vertical, 6)
                if store.accounts.count < maxAccounts {
                    action("Konto hinzufügen", icon: "plus", id: "account-add") { then(.add) }
                }
                action("Einstellungen", icon: "gearshape", id: "account-settings") { then(.settings) }
            }
            .padding(.top, 20)
        }
        .background(Talk.background)
        .presentationDetents([.medium])
        .presentationDragIndicator(.visible)
    }

    private func action(_ title: String, icon: String, id: String, _ f: @escaping () -> Void) -> some View {
        Button {
            f()
            dismiss()
        } label: {
            SettingsLabel(icon: icon, text: title).padding(.horizontal, 24)
        }
        .buttonStyle(MenuRowStyle())
        .accessibilityLabel(title)
        .accessibilityIdentifier(id)
    }
}

enum AccountMenuAction {
    case add, settings
}

private struct MenuRowStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(configuration.isPressed ? Talk.chip : Color.clear)
    }
}
