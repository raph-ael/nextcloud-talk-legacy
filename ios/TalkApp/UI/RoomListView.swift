import SwiftUI

/// Conversation list with Signal's layout: favorites first, 48 pt avatars, two-line previews,
/// unread badges, relative times (port of Android `RoomListScreen.kt`).
struct RoomListView: View {
    @Bindable var store: SessionStore
    @State private var query = ""
    @State private var confirmLogout = false

    private var visible: [RoomView] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return store.rooms.filter { !$0.isArchived && (q.isEmpty || $0.name.lowercased().contains(q)) }
    }

    var body: some View {
        let rooms = visible
        let favorites = rooms.filter(\.isFavorite)
        let others = rooms.filter { !$0.isFavorite }
        // Relative times ("5 Min.") need a clock.
        TimelineView(.periodic(from: .now, by: 30)) { context in
            List {
                if !store.online {
                    OfflineBanner()
                        .listRowInsets(EdgeInsets())
                        .listRowSeparator(.hidden)
                }
                if !store.recordings.isEmpty {
                    RecordingBanners(notices: store.recordings, showRoom: true)
                        .listRowInsets(EdgeInsets())
                        .listRowSeparator(.hidden)
                        .listRowBackground(Talk.background)
                }
                if !favorites.isEmpty {
                    Section {
                        ForEach(favorites, id: \.token) { row($0, now: context.date) }
                    } header: {
                        SectionTitle("Favoriten")
                    }
                    if !others.isEmpty {
                        Section {
                            ForEach(others, id: \.token) { row($0, now: context.date) }
                        } header: {
                            SectionTitle("Chats")
                        }
                    }
                } else {
                    ForEach(others, id: \.token) { row($0, now: context.date) }
                }
                if rooms.isEmpty {
                    Text(query.isEmpty ? "Noch keine Chats" : "Keine Treffer")
                        .foregroundStyle(Talk.muted)
                        .frame(maxWidth: .infinity)
                        .padding(32)
                        .listRowSeparator(.hidden)
                }
            }
            .listStyle(.plain)
            .listSectionSpacing(.compact)
            .scrollContentBackground(.hidden)
            .background(Talk.background)
        }
        .refreshable { await store.refresh() }
        .searchable(text: $query, prompt: "Suchen")
        .navigationTitle("Chats")
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Menu {
                    Section {
                        Text(store.user)
                        Text(store.server.replacingOccurrences(of: "https://", with: ""))
                    }
                    Button("Abmelden", role: .destructive) { confirmLogout = true }
                } label: {
                    Avatar(name: store.user, size: 32)
                }
                .accessibilityLabel("Konto")
            }
        }
        .confirmationDialog("Abmelden?", isPresented: $confirmLogout, titleVisibility: .visible) {
            Button("Abmelden", role: .destructive) { store.logout() }
        } message: {
            Text("Ungesendete Nachrichten und der Offline-Speicher werden gelöscht.")
        }
    }

    private func row(_ room: RoomView, now: Date) -> some View {
        // A button instead of a NavigationLink: no disclosure chevron (Signal shows none);
        // the store's active token drives the navigation stack.
        Button { store.openRoom(room.token) } label: {
            RoomRow(room: room, now: now)
        }
        .buttonStyle(RowPressStyle())
        .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
        .listRowBackground(Talk.background)
        .listRowSeparator(.hidden)
        .accessibilityIdentifier("room:\(room.name)")
    }
}

private struct RowPressStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(configuration.isPressed ? Talk.chip : Color.clear)
    }
}

private struct SectionTitle: View {
    let title: String
    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title)
            .font(.system(size: 16, weight: .medium))
            .foregroundStyle(Talk.onSurface)
            .textCase(nil)
            .padding(.leading, 8)
    }
}

/// One conversation with Signal's metrics (48 pt avatar, two-line summary, 84 pt row).
private struct RoomRow: View {
    let room: RoomView
    let now: Date

    var body: some View {
        let unread = room.unreadMessages > 0
        HStack(spacing: 16) {
            Avatar(name: room.name, size: 48, token: room.token, version: room.avatarVersion)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(room.name)
                        .font(.system(size: 16))
                        .foregroundStyle(Talk.onSurface)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Text(Format.listTime(room.lastActivity, now: now))
                        .font(.system(size: 14, weight: unread ? .bold : .regular))
                        .foregroundStyle(unread ? Talk.onSurface : Talk.muted)
                        .lineLimit(1)
                }
                HStack(alignment: .top, spacing: 6) {
                    preview
                        .font(.system(size: 14))
                        .foregroundStyle(Talk.muted)
                        .lineLimit(2, reservesSpace: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    if room.hasCall {
                        Circle().fill(Talk.callDot).frame(width: 8, height: 8).padding(.top, 6)
                    }
                    if room.unreadMention { Badge(text: "@") }
                    if unread { Badge(text: room.unreadMessages > 99 ? "99+" : "\(room.unreadMessages)") }
                }
            }
        }
        .frame(minHeight: 84)
        .contentShape(Rectangle())
    }

    private var preview: Text {
        guard let last = room.lastMessage else { return Text("") }
        let text = Format.withAttachment(last.text, last.attachment)
        if text.isEmpty { return Text("Nachricht gelöscht") }
        if last.isSystem { return Text(text).italic() }
        if !room.isGroup { return Text(text) }
        let who = last.isOwn ? "Du" : String(last.author.split(separator: " ").first ?? "")
        return Text("\(who): ").fontWeight(.medium).foregroundColor(Talk.onSurface) + Text(text)
    }
}

private struct Badge: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(.white)
            .padding(.horizontal, 6)
            .frame(minWidth: 22, minHeight: 22)
            .background(Talk.accent, in: Capsule())
            .padding(.top, 2)
    }
}
