import SwiftUI

/// Conversation list (first iteration: straight from the server; DB-first cache comes with the sync engine).
struct RoomListView: View {
    let session: TalkSession
    let accountName: String
    let onLogout: () -> Void

    @State private var rooms: [RoomSummary] = []
    @State private var loaded = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            List(rooms, id: \.token) { room in
                RoomRow(room: room)
                    .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
            }
            .listStyle(.plain)
            .navigationTitle("Chats")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Text(accountName)
                        Button("Abmelden", role: .destructive, action: onLogout)
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                }
            }
            .refreshable { await load() }
            .task { await load() }
            .overlay {
                if !loaded {
                    ProgressView()
                } else if rooms.isEmpty, error == nil {
                    ContentUnavailableView("Noch keine Chats", systemImage: "bubble.left.and.bubble.right")
                }
            }
            .safeAreaInset(edge: .bottom) {
                if let error {
                    Text(error)
                        .font(.footnote)
                        .padding(10)
                        .frame(maxWidth: .infinity)
                        .background(.red.opacity(0.15))
                }
            }
        }
    }

    private func load() async {
        do {
            rooms = try await session.rooms()
            error = nil
        } catch {
            self.error = "Aktualisieren fehlgeschlagen"
        }
        loaded = true
    }
}

struct RoomRow: View {
    let room: RoomSummary

    var body: some View {
        HStack(spacing: 12) {
            AvatarView(name: room.displayName)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline) {
                    Text(room.displayName)
                        .font(.body.weight(.semibold))
                        .lineLimit(1)
                    Spacer(minLength: 8)
                    Text(RelativeTime.short(room.lastActivity))
                        .font(.caption)
                        .foregroundStyle(room.unreadMessages > 0 ? Color.accentColor : .secondary)
                }
                HStack(alignment: .top) {
                    Text(preview)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                    Spacer(minLength: 8)
                    if room.unreadMessages > 0 {
                        Text(room.unreadMessages > 99 ? "99+" : "\(room.unreadMessages)")
                            .font(.caption2.weight(.bold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 7)
                            .padding(.vertical, 3)
                            .background(Color.accentColor, in: Capsule())
                    }
                }
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var preview: String {
        guard let text = room.lastMessagePreview else { return "" }
        // 1:1 chats: no author prefix, like Signal.
        if room.roomType == 1 || (room.lastMessageAuthor ?? "").isEmpty { return text }
        return "\(room.lastMessageAuthor!): \(text)"
    }
}

struct AvatarView: View {
    let name: String
    var size: CGFloat = 52

    private static let palette: [Color] = [
        .init(red: 0.85, green: 0.33, blue: 0.31), .init(red: 0.91, green: 0.55, blue: 0.20),
        .init(red: 0.30, green: 0.65, blue: 0.40), .init(red: 0.20, green: 0.55, blue: 0.80),
        .init(red: 0.45, green: 0.40, blue: 0.80), .init(red: 0.80, green: 0.35, blue: 0.60),
        .init(red: 0.25, green: 0.60, blue: 0.62), .init(red: 0.55, green: 0.50, blue: 0.40),
    ]

    var body: some View {
        Circle()
            .fill(color.gradient)
            .frame(width: size, height: size)
            .overlay {
                Text(initials)
                    .font(.system(size: size * 0.38, weight: .semibold, design: .rounded))
                    .foregroundStyle(.white)
            }
    }

    private var initials: String {
        let parts = name.split(whereSeparator: { $0 == " " || $0 == "." }).prefix(2)
        let letters = parts.compactMap(\.first).map { String($0) }.joined()
        return letters.isEmpty ? "?" : letters.uppercased()
    }

    private var color: Color {
        // Stable across launches (unlike String.hashValue).
        let sum = name.unicodeScalars.reduce(0) { ($0 &* 31 &+ Int($1.value)) & 0x7fffffff }
        return Self.palette[sum % Self.palette.count]
    }
}

enum RelativeTime {
    static func short(_ unix: Int64) -> String {
        guard unix > 0 else { return "" }
        let date = Date(timeIntervalSince1970: TimeInterval(unix))
        let cal = Calendar.current
        if cal.isDateInToday(date) {
            return date.formatted(date: .omitted, time: .shortened)
        }
        if cal.isDateInYesterday(date) {
            return "Gestern"
        }
        if let days = cal.dateComponents([.day], from: date, to: .now).day, days < 7 {
            return date.formatted(.dateTime.weekday(.abbreviated))
        }
        return date.formatted(.dateTime.day().month(.twoDigits))
    }
}
