import SwiftUI

/// Conversation list with Signal's layout: sections from the core (Favoriten, categories,
/// Chats), each collapsible, 48 pt avatars, two-line previews,
/// unread badges, relative times (port of Android `RoomListScreen.kt`).
struct RoomListView: View {
    @Bindable var store: SessionStore
    @State private var query = ""
    @State private var newMessage = false
    /// Chat for which "Neue Kategorie …" asks the name.
    @State private var newCategoryFor: String?
    @State private var categoryName = ""

    var body: some View {
        // Sections from the core (docs/categories.md): filtered, collapsed ones reduced.
        let sections = store.sections(query.trimmingCharacters(in: .whitespaces))
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
                ForEach(sections, id: \.id) { section in
                    if section.title.isEmpty {
                        rows(section, now: context.date)
                    } else {
                        Section {
                            rows(section, now: context.date)
                        } header: {
                            SectionTitle(section: section) { store.toggleSection(section.id) }
                        }
                    }
                }
                if sections.isEmpty {
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
                // Own avatar: opens the settings (docs/settings.md).
                Button { store.listPath = [.settings] } label: {
                    PersonAvatar(name: store.user, userId: store.userId, size: 32)
                }
                .accessibilityLabel("Einstellungen")
                .accessibilityIdentifier("settings")
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button { newMessage = true } label: { Image(systemName: "square.and.pencil") }
                    .accessibilityLabel("Neue Nachricht")
                    .accessibilityIdentifier("new-message")
            }
        }
        .sheet(isPresented: $newMessage) { NewMessageSheet(store: store) }
        .alert("Neue Kategorie", isPresented: Binding(
            get: { newCategoryFor != nil },
            set: { if !$0 { newCategoryFor = nil } }
        )) {
            TextField("Name", text: $categoryName)
            Button("Abbrechen", role: .cancel) {}
            Button("Anlegen") {
                let name = categoryName.trimmingCharacters(in: .whitespaces)
                if let token = newCategoryFor, !name.isEmpty { store.createCategory(name, with: token) }
            }
            .disabled(categoryName.trimmingCharacters(in: .whitespaces).isEmpty)
        }
    }

    /// A chat may stand in several sections, so the row ids carry the section.
    private func rows(_ section: SectionView, now: Date) -> some View {
        ForEach(section.rooms.map { (key: "\(section.id):\($0.token)", room: $0) }, id: \.key) { row($0.room, now: now) }
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
        // Chat menu (docs/settings.md); VoiceOver offers it as an action of the row.
        .contextMenu {
            ChatMenu(store: store, room: room) { token in
                categoryName = ""
                newCategoryFor = token
            }
        }
        .accessibilityIdentifier("room:\(room.name)")
    }
}

private struct RowPressStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(configuration.isPressed ? Talk.chip : Color.clear)
    }
}

/// Section head: the title with a small muted chevron that points down while open. The
/// whole head toggles the section (docs/categories.md); no counts, no lines.
private struct SectionTitle: View {
    let section: SectionView
    let onToggle: () -> Void
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Button(action: onToggle) {
            HStack(spacing: 6) {
                Text(section.title)
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(Talk.onSurface)
                    .lineLimit(1)
                Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .rotationEffect(.degrees(section.collapsed ? 0 : 90))
                    .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: section.collapsed)
                Spacer(minLength: 0)
            }
            .padding(.leading, 8)
            .frame(maxWidth: .infinity, minHeight: 32, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .textCase(nil)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
        .accessibilityValue(section.collapsed ? "eingeklappt" : "ausgeklappt")
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier("section:\(section.title)")
    }

    private var label: String {
        var l = section.kind == "custom" ? "\(section.title), Kategorie" : section.title
        let hidden = Int(section.total) - section.rooms.count
        if section.collapsed && hidden > 0 { l += ", \(hidden) \(hidden == 1 ? "Chat" : "Chats") ausgeblendet" }
        return l
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
                    HStack(alignment: .firstTextBaseline, spacing: 4) {
                        Text(room.name)
                            .font(.system(size: 16))
                            .foregroundStyle(Talk.onSurface)
                            .lineLimit(1)
                        if muted {
                            Image(systemName: "bell.slash.fill")
                                .font(.system(size: 12))
                                .foregroundStyle(Talk.muted)
                                .accessibilityLabel("stummgeschaltet")
                        }
                    }
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
                    if unread { Badge(text: room.unreadMessages > 99 ? "99+" : "\(room.unreadMessages)", quiet: quiet) }
                }
            }
        }
        .frame(minHeight: 84)
        .contentShape(Rectangle())
    }

    /// Notifications "Aus": crossed-out bell after the name.
    private var muted: Bool { room.notificationLevel == 3 }

    /// The count is grey at "Aus", and at "Nur Erwähnungen" unless we were mentioned.
    private var quiet: Bool { muted || (room.notificationLevel == 2 && !room.unreadMention) }

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
    /// Grey instead of the accent (muted chats).
    var quiet = false

    var body: some View {
        Text(text)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(.white)
            .padding(.horizontal, 6)
            .frame(minWidth: 22, minHeight: 22)
            .background(quiet ? Talk.muted : Talk.accent, in: Capsule())
            .padding(.top, 2)
    }
}
