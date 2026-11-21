import SwiftUI

/// Initials on a name color; the server avatar replaces them once loaded (rooms only).
/// "Notizen an mich" (room type 6) shows `NoteGlyph` on the accent colour instead of the
/// server's image, and nothing is fetched for it. Always: the server gives this room its 📝
/// emoji picture as a custom avatar, so `hasCustomAvatar` cannot tell it from an own one.
struct Avatar: View {
    let name: String
    var size: CGFloat
    var token: String? = nil
    var version: String = ""
    /// Another account than the active one (call of an inactive account).
    var account: String? = nil

    @Environment(\.colorScheme) private var scheme
    @State private var image: UIImage?

    private var media: MediaCache {
        account.map { SessionStore.shared.mediaCache(for: $0) } ?? SessionStore.shared.media
    }

    var body: some View {
        let dark = scheme == .dark
        let note = isNoteToSelf
        let shown = note ? nil : image ?? token.flatMap { media.cachedAvatar($0, version, dark: dark) }
        ZStack {
            if note {
                Talk.accent
                // The 24 grid is half the diameter, centred; stroke 1.8 in that grid.
                NoteGlyph()
                    .stroke(.white, style: StrokeStyle(lineWidth: 1.8 * size / 48, lineCap: .round, lineJoin: .round))
                    .frame(width: size / 2, height: size / 2)
            } else if let shown {
                Image(uiImage: shown).resizable().scaledToFill()
            } else {
                Format.color(for: name)
                Text(Format.initials(name))
                    .font(.system(size: size * 0.4, weight: .semibold))
                    .foregroundStyle(.white)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .task(id: "\(account ?? SessionStore.shared.activeId ?? "")|\(token ?? "")|\(version)|\(dark)|\(note)") {
            guard let token, !note else { return }
            image = await media.avatar(token, version, dark: dark)
        }
        .accessibilityHidden(true)
    }

    private var isNoteToSelf: Bool {
        guard let token else { return false }
        return SessionStore.shared.room(token, account: account)?.roomType == 6
    }
}

/// A note sheet with three lines, in lines only (24 grid; the same drawing on all platforms).
struct NoteGlyph: Shape {
    func path(in rect: CGRect) -> Path {
        let s = min(rect.width, rect.height) / 24
        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: rect.minX + x * s, y: rect.minY + y * s) }
        var path = Path(roundedRect: CGRect(origin: p(5, 3.5), size: CGSize(width: 14 * s, height: 17 * s)),
                        cornerRadius: 2.5 * s)
        for (y, end) in [(8.5, 15.5), (12, 15.5), (15.5, 12.5)] as [(CGFloat, CGFloat)] {
            path.move(to: p(8.5, y))
            path.addLine(to: p(end, y))
        }
        return path
    }
}
