import PhotosUI
import SwiftUI

/// Building blocks shared by "Neue Nachricht", "Mitglieder hinzufügen" and the group
/// settings (docs/groups.md).

/// Avatar 40, name and the muted subline below it.
struct PersonRow<Trailing: View>: View {
    let name: String
    var subline = ""
    var userId = ""
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(spacing: 14) {
            PersonAvatar(name: name, userId: userId, size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.system(size: 16))
                    .foregroundStyle(Talk.onSurface)
                    .lineLimit(1)
                if !subline.isEmpty {
                    Text(subline)
                        .font(.system(size: 13))
                        .foregroundStyle(Talk.muted)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 8)
            trailing()
        }
        .frame(minHeight: 52)
        .contentShape(Rectangle())
    }
}

extension PersonRow where Trailing == EmptyView {
    init(name: String, subline: String = "", userId: String = "") {
        self.init(name: name, subline: subline, userId: userId) { EmptyView() }
    }
}

/// Round check mark: an empty muted circle, or filled in accent with a white tick.
struct RoundCheck: View {
    let selected: Bool

    var body: some View {
        ZStack {
            if selected {
                Circle().fill(Talk.accent)
                Image(systemName: "checkmark")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(.white)
            } else {
                Circle().stroke(Talk.muted, lineWidth: 1.5)
            }
        }
        .frame(width: 24, height: 24)
        .animation(.easeOut(duration: 0.12), value: selected)
    }
}

/// Three placeholder rows while a list loads.
struct PlaceholderRows: View {
    var count = 3

    var body: some View {
        ForEach(0..<count, id: \.self) { i in
            HStack(spacing: 14) {
                Circle().fill(Talk.chip).frame(width: 40, height: 40)
                RoundedRectangle(cornerRadius: 4).fill(Talk.chip).frame(width: CGFloat(140 - i * 24), height: 12)
                Spacer()
            }
            .frame(height: 52)
            .accessibilityHidden(true)
        }
    }
}

/// "Keine Verbindung" / "Konnte nicht geladen werden" with "Erneut versuchen".
struct LoadFailed: View {
    let text: String
    let retry: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            Text(text).foregroundStyle(Talk.muted)
            Button("Erneut versuchen", action: retry)
                .font(.system(size: 15, weight: .medium))
                .accessibilityIdentifier("retry")
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 20)
    }
}

/// A round icon in accent ("Neue Gruppe", "Mitglieder hinzufügen").
struct AccentCircle: View {
    let icon: String

    var body: some View {
        Image(systemName: icon)
            .font(.system(size: 17, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: 40, height: 40)
            .background(Talk.accent, in: Circle())
    }
}

/// Users from `searchUsers`, fetched 250 ms after the last keystroke.
enum UserLoad: Equatable {
    case loading
    case failed(String)
    case done([UserView])
}

extension View {
    /// Loads `searchUsers(query, exclude)` into `state` whenever the query changes (debounced)
    /// or `retry` is bumped.
    func userSearch(_ query: String, exclude: String? = nil, retry: Int, into state: Binding<UserLoad>) -> some View {
        task(id: "\(query)|\(retry)") {
            if case .done = state.wrappedValue {} else { state.wrappedValue = .loading }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let store = SessionStore.shared
            do {
                let list = try await store.searchUsers(query, excluding: exclude)
                guard !Task.isCancelled else { return }
                state.wrappedValue = .done(list)
            } catch {
                guard !Task.isCancelled else { return }
                state.wrappedValue = .failed(store.online ? "Konnte nicht geladen werden" : "Keine Verbindung")
            }
        }
    }
}

/// Picked people as chips (avatar 24, first name, ×) that wrap.
struct UserChips: View {
    let users: [UserView]
    let onRemove: (UserView) -> Void

    var body: some View {
        FlowLayout(spacing: 6) {
            ForEach(users, id: \.id) { u in
                Button { onRemove(u) } label: {
                    HStack(spacing: 6) {
                        PersonAvatar(name: u.name, userId: u.id, size: 24)
                        Text(String(u.name.split(separator: " ").first ?? Substring(u.name)))
                            .font(.system(size: 15))
                            .foregroundStyle(Talk.onSurface)
                            .lineLimit(1)
                        Image(systemName: "xmark")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(Talk.muted)
                    }
                    .padding(.leading, 3)
                    .padding(.trailing, 10)
                    .frame(height: 30)
                    .background(Talk.chip, in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("\(u.name) entfernen")
                .accessibilityAddTraits(.isButton)
                .accessibilityIdentifier("chip")
                .transition(.scale(scale: 0.8).combined(with: .opacity))
            }
        }
        .animation(.easeOut(duration: 0.15), value: users.map(\.id))
    }
}

/// Rows left to right, wrapping when the width is used up.
struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(proposal.width ?? .infinity, subviews)
        let width = rows.map(\.width).max() ?? 0
        let height = rows.reduce(0) { $0 + $1.height } + spacing * CGFloat(max(0, rows.count - 1))
        return CGSize(width: proposal.width ?? width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in arrange(bounds.width, subviews) {
            var x = bounds.minX
            for i in row.items {
                let size = subviews[i].sizeThatFits(.unspecified)
                subviews[i].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private func arrange(_ width: CGFloat, _ subviews: Subviews) -> [(items: [Int], width: CGFloat, height: CGFloat)] {
        var rows: [(items: [Int], width: CGFloat, height: CGFloat)] = []
        var current: (items: [Int], width: CGFloat, height: CGFloat) = ([], 0, 0)
        for (i, v) in subviews.enumerated() {
            let size = v.sizeThatFits(.unspecified)
            let needed = current.items.isEmpty ? size.width : current.width + spacing + size.width
            if needed > width && !current.items.isEmpty {
                rows.append(current)
                current = ([i], size.width, size.height)
            } else {
                current = (current.items + [i], needed, max(current.height, size.height))
            }
        }
        if !current.items.isEmpty { rows.append(current) }
        return rows
    }
}

/// A picked group photo: the preview and the bytes for the core (JPEG; the core cannot read
/// HEIC). The core crops and scales it, so 1024 px are plenty.
struct GroupPhoto: Equatable {
    let image: UIImage
    let jpeg: Data

    static func load(_ item: PhotosPickerItem) async -> GroupPhoto? {
        guard let data = try? await item.loadTransferable(type: Data.self) else { return nil }
        return await Task.detached {
            guard let image = ImageDecode.downsample(data, maxEdge: 1024),
                  let jpeg = image.jpegData(compressionQuality: 0.88) else { return nil }
            return GroupPhoto(image: image, jpeg: jpeg)
        }.value
    }
}

/// Large round avatar (80) that opens the photo picker; a camera badge marks it.
struct GroupPhotoPicker<Placeholder: View>: View {
    @Binding var item: PhotosPickerItem?
    let photo: UIImage?
    @ViewBuilder var placeholder: () -> Placeholder

    var body: some View {
        PhotosPicker(selection: $item, matching: .images) {
            ZStack(alignment: .bottomTrailing) {
                Group {
                    if let photo {
                        Image(uiImage: photo).resizable().scaledToFill()
                    } else {
                        placeholder()
                    }
                }
                .frame(width: 80, height: 80)
                .clipShape(Circle())
                Image(systemName: "camera.fill")
                    .font(.system(size: 13))
                    .foregroundStyle(Talk.onSurface)
                    .frame(width: 28, height: 28)
                    .background(Talk.floating, in: Circle())
                    .shadow(color: .black.opacity(0.15), radius: 3, y: 1)
                    .offset(x: 4, y: 4)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Gruppenbild wählen")
        .accessibilityIdentifier("group-photo")
    }
}
