import SwiftUI

/// Full-screen photo before sending, with caption and the receiving conversation.
struct MediaSendView: View {
    let image: PreparedImage
    let roomName: String
    let onClose: () -> Void
    let onSend: (String) -> Void

    @State private var caption = ""
    @State private var shown: UIImage?

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            if let shown {
                Image(uiImage: shown).resizable().scaledToFit()
            } else {
                ProgressView().tint(.white)
            }
            VStack {
                HStack {
                    Button(action: onClose) {
                        Image(systemName: "xmark").font(.system(size: 20, weight: .semibold)).frame(width: 44, height: 44)
                    }
                    .accessibilityLabel("Schließen")
                    Spacer()
                }
                .padding(.horizontal, 12)
                Spacer()
                HStack(spacing: 10) {
                    TextField("Nachricht hinzufügen", text: $caption, axis: .vertical)
                        .lineLimit(1...4)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 9)
                        .background(.white.opacity(0.15), in: RoundedRectangle(cornerRadius: 20))
                        .accessibilityIdentifier("caption")
                    Button { onSend(caption) } label: {
                        Image(systemName: "arrow.up")
                            .font(.system(size: 18, weight: .semibold))
                            .frame(width: 44, height: 44)
                            .background(Talk.accent, in: Circle())
                    }
                    .accessibilityLabel("Senden")
                    .accessibilityIdentifier("send-media")
                }
                .padding(.horizontal, 12)
                Text("An: \(roomName)")
                    .font(.system(size: 13))
                    .padding(.horizontal, 12)
                    .padding(.vertical, 5)
                    .background(.white.opacity(0.15), in: Capsule())
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 14)
                    .padding(.bottom, 10)
            }
            .foregroundStyle(.white)
        }
        .preferredColorScheme(.dark)
        .task {
            let url = image.url
            shown = await Task.detached { ImageDecode.downsample(url: url, maxEdge: 2048) }.value
        }
    }
}

struct ViewerItem: Identifiable {
    let fileId: String
    let initial: UIImage?
    var id: String { fileId }
}

/// Black full-screen viewer: pinch zoom 1–6×, double tap 2.5×, pan; shows the chat preview
/// first, then the 2048 px version.
struct ImageViewer: View {
    let item: ViewerItem
    let onClose: () -> Void

    @State private var full: UIImage?
    @State private var scale: CGFloat = 1
    @State private var baseScale: CGFloat = 1
    @State private var offset: CGSize = .zero
    @State private var baseOffset: CGSize = .zero

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            if let img = full ?? item.initial {
                Image(uiImage: img)
                    .resizable()
                    .scaledToFit()
                    .scaleEffect(scale)
                    .offset(offset)
                    .gesture(
                        MagnifyGesture()
                            .onChanged { scale = min(6, max(1, baseScale * $0.magnification)) }
                            .onEnded { _ in
                                baseScale = scale
                                if scale == 1 { offset = .zero; baseOffset = .zero }
                            }
                            .simultaneously(with: DragGesture()
                                .onChanged { g in
                                    guard scale > 1 else { return }
                                    offset = CGSize(width: baseOffset.width + g.translation.width,
                                                    height: baseOffset.height + g.translation.height)
                                }
                                .onEnded { g in
                                    if scale <= 1 && g.translation.height > 120 { onClose() }
                                    baseOffset = offset
                                })
                    )
                    .onTapGesture(count: 2) {
                        withAnimation(.easeOut(duration: 0.2)) {
                            if scale > 1 { scale = 1; offset = .zero } else { scale = 2.5 }
                            baseScale = scale
                            baseOffset = offset
                        }
                    }
            } else {
                ProgressView().tint(.white)
            }
            VStack {
                HStack {
                    Button(action: onClose) {
                        Image(systemName: "xmark").font(.system(size: 20, weight: .semibold)).frame(width: 44, height: 44)
                    }
                    .foregroundStyle(.white)
                    .accessibilityLabel("Schließen")
                    Spacer()
                }
                .padding(.horizontal, 12)
                Spacer()
            }
        }
        .statusBarHidden()
        .task { full = await SessionStore.shared.media.fullImage(item.fileId) }
    }
}
