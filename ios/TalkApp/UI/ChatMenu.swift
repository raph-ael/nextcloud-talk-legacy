import SwiftUI

/// Chat menu on a row of the conversation list (docs/settings.md "Chat-Menü"), opened by a
/// long press as a context menu. Changes go to the core; errors come back as toasts.
/// Favorite and "Als gelesen" show at once (the core sends `RoomsChanged` first).
struct ChatMenu: View {
    let store: SessionStore
    let room: RoomView
    /// "Neue Kategorie …": the list asks for the name in an alert.
    let onNewCategory: (String) -> Void

    private var token: String { room.token }

    var body: some View {
        if room.unreadMessages > 0 {
            Button("Als gelesen markieren", systemImage: "checkmark.message") {
                store.menuChange { await $0.markRoomRead(token: token) }
            }
        }
        Button(room.isFavorite ? "Aus Favoriten entfernen" : "Zu Favoriten hinzufügen",
               systemImage: room.isFavorite ? "star.slash" : "star") {
            let on = !room.isFavorite
            store.menuChange { await $0.setFavorite(token: token, on: on) }
        }
        if store.categoriesSupported { categories }
        notifications
        if GroupInfoView.applies(room) {
            Button("Gruppeneinstellungen", systemImage: "gearshape") { store.openGroupSettings(token) }
        }
    }

    /// Own categories with a tick each (the menu stays open while ticking), then
    /// "Neue Kategorie …".
    private var categories: some View {
        Menu {
            let custom = store.customCategories()
            ForEach(custom, id: \.id) { c in
                Toggle(c.name, isOn: Binding(
                    get: { (store.room(token) ?? room).tagIds.contains(c.id) },
                    set: { on in toggle(c.id, on: on, custom: custom.map(\.id)) }
                ))
                .menuActionDismissBehavior(.disabled)
            }
            if !custom.isEmpty { Divider() }
            Button("Neue Kategorie …", systemImage: "plus") { onNewCategory(token) }
        } label: {
            Label("Kategorie", systemImage: "folder")
        }
    }

    private func toggle(_ id: String, on: Bool, custom: [String]) {
        // From the current list: the menu may stay open across several ticks.
        var ids = (store.room(token) ?? room).tagIds.filter { custom.contains($0) && $0 != id }
        if on { ids.append(id) }
        store.menuChange { await $0.setRoomCategories(token: token, ids: ids) }
    }

    /// 1 "Alle Nachrichten", 2 "Nur Erwähnungen" (not in one-to-one chats), 3 "Aus".
    private var notifications: some View {
        let direct = [1, 5, 6].contains(room.roomType)
        let levels: [(UInt8, String)] = direct
            ? [(1, "Alle Nachrichten"), (3, "Aus")]
            : [(1, "Alle Nachrichten"), (2, "Nur Erwähnungen"), (3, "Aus")]
        return Picker(selection: Binding(
            get: { room.notificationLevel },
            set: { level in store.menuChange { await $0.setNotificationLevel(token: token, level: level) } }
        )) {
            ForEach(levels, id: \.0) { Text($0.1).tag($0.0) }
        } label: {
            Label("Benachrichtigungen", systemImage: "bell")
        }
        .pickerStyle(.menu)
    }
}
