import SwiftUI

/// Initials on a name color; the server avatar replaces them once loaded (rooms only).
struct Avatar: View {
    let name: String
    var size: CGFloat
    var token: String? = nil
    var version: String = ""

    @Environment(\.colorScheme) private var scheme
    @State private var image: UIImage?

    private var store: SessionStore { SessionStore.shared }

    var body: some View {
        let dark = scheme == .dark
        let shown = image ?? token.flatMap { store.media.cachedAvatar($0, version, dark: dark) }
        ZStack {
            if let shown {
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
        .task(id: "\(token ?? "")|\(version)|\(dark)") {
            guard let token else { return }
            image = await store.media.avatar(token, version, dark: dark)
        }
        .accessibilityHidden(true)
    }
}
