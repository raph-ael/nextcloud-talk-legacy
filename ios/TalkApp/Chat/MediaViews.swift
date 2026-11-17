import QuickLook
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

// MARK: - Files (docs/attachments.md "Datei senden", "Empfangene Datei öffnen")

/// A file picked in the system's file picker, read from its security-scoped URL when sent.
struct PickedFile: Identifiable, Equatable {
    let id = UUID()
    let url: URL
    let name: String
    let size: Int64
    let mime: String

    /// Name, size and MIME type from the system; nil when the file cannot be read.
    nonisolated static func read(_ url: URL) -> PickedFile? {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let v = try? url.resourceValues(forKeys: [.nameKey, .fileSizeKey, .contentTypeKey, .isDirectoryKey]),
              v.isDirectory != true,
              let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        try? handle.close()
        return PickedFile(
            url: url,
            name: v.name ?? url.lastPathComponent,
            size: Int64(v.fileSize ?? 0),
            mime: v.contentType?.preferredMIMEType ?? "application/octet-stream"
        )
    }
}

struct FileBatch: Identifiable {
    let id = UUID()
    let files: [PickedFile]
}

/// "Datei senden" / "3 Dateien senden": one row per file with ✕, the caption and the send
/// button (a spinner while the files are copied).
struct FileSendView: View {
    let onClose: () -> Void
    /// Copies and queues the files; the ids of those that could not be read (nothing sent).
    let onSend: ([PickedFile], String) async -> Set<UUID>

    @State private var files: [PickedFile]
    @State private var caption = ""
    @State private var busy = false

    init(files: [PickedFile], onClose: @escaping () -> Void, onSend: @escaping ([PickedFile], String) async -> Set<UUID>) {
        _files = State(initialValue: files)
        self.onClose = onClose
        self.onSend = onSend
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(files) { f in row(f) }
                    }
                    .padding(.vertical, 8)
                }
                HStack(spacing: 10) {
                    TextField("Beschreibung hinzufügen", text: $caption, axis: .vertical)
                        .lineLimit(1...4)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 9)
                        .background(Talk.chip, in: RoundedRectangle(cornerRadius: 20))
                        .accessibilityIdentifier("caption")
                    Button { send() } label: {
                        ZStack {
                            Circle().fill(Talk.accent)
                            if busy {
                                ProgressView().tint(.white)
                            } else {
                                Image(systemName: "arrow.up").font(.system(size: 18, weight: .semibold)).foregroundStyle(.white)
                            }
                        }
                        .frame(width: 44, height: 44)
                    }
                    .disabled(busy)
                    .accessibilityLabel("Senden")
                    .accessibilityIdentifier("send-files")
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
            }
            .background(Talk.background)
            .navigationTitle(files.count == 1 ? "Datei senden" : "\(files.count) Dateien senden")
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Talk.background, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(action: onClose) {
                        Image(systemName: "xmark").font(.system(size: 17, weight: .semibold)).foregroundStyle(Talk.onSurface)
                    }
                    .disabled(busy)
                    .accessibilityLabel("Schließen")
                }
            }
        }
        .interactiveDismissDisabled(busy)
        .onChange(of: files.isEmpty) { _, empty in if empty { onClose() } }
    }

    private func row(_ f: PickedFile) -> some View {
        HStack(spacing: 12) {
            Image(systemName: FileIcon.name(f.mime))
                .font(.system(size: 18))
                .frame(width: 40, height: 40)
                .background(Talk.chip, in: Circle())
            VStack(alignment: .leading, spacing: 1) {
                Text(f.name).font(.system(size: 15, weight: .medium)).lineLimit(1).truncationMode(.middle)
                Text(Format.fileSize(f.size)).font(.system(size: 12)).foregroundStyle(Talk.muted)
            }
            Spacer(minLength: 0)
            Button {
                withAnimation(.easeOut(duration: 0.15)) { files.removeAll { $0.id == f.id } }
            } label: {
                Image(systemName: "xmark").font(.system(size: 14, weight: .semibold)).foregroundStyle(Talk.muted)
                    .frame(width: 36, height: 36)
            }
            .disabled(busy)
            .accessibilityLabel("Entfernen")
            .accessibilityIdentifier("file-remove")
        }
        .foregroundStyle(Talk.onSurface)
        .padding(.horizontal, 16)
        .padding(.vertical, 6)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("file-send-row")
    }

    private func send() {
        guard !busy, !files.isEmpty else { return }
        busy = true
        Task {
            let failed = await onSend(files, caption)
            busy = false
            guard !failed.isEmpty else { return }
            Toast.show("Datei konnte nicht gelesen werden")
            files.removeAll { failed.contains($0.id) }
        }
    }
}

/// Loads received files once (spinner in the tile meanwhile) and shows them in QuickLook
/// under their real name, with the share button.
@MainActor
@Observable
final class FileOpener {
    static let shared = FileOpener()

    private(set) var loading: Set<String> = []
    var preview: URL?

    private func key(_ a: FileAttachment) -> String { "\(SessionStore.shared.activeId ?? "")|\(a.path)" }

    func isLoading(_ a: FileAttachment) -> Bool { loading.contains(key(a)) }

    func open(_ a: FileAttachment) {
        let k = key(a)
        guard !loading.contains(k), let s = SessionStore.shared.session else { return }
        loading.insert(k)
        Task {
            defer { loading.remove(k) }
            let local: String
            do {
                local = try await s.fetchFile(path: a.path, size: a.size)
            } catch {
                Log.media.error("open \(a.path, privacy: .public): \(error.localizedDescription, privacy: .public)")
                Toast.show("Datei konnte nicht geladen werden")
                return
            }
            let name = a.name
            guard let url = await Task.detached(operation: { Self.named(local, name) }).value else {
                Toast.show("Datei konnte nicht geladen werden")
                return
            }
            guard QLPreviewController.canPreview(url as NSURL) else {
                Toast.show("Keine App zum Öffnen von \(name)")
                return
            }
            preview = url
        }
    }

    /// The cached file under its real name, in a folder of its own (a hard link, else a copy);
    /// the one opened before goes.
    nonisolated private static func named(_ path: String, _ name: String) -> URL? {
        let fm = FileManager.default
        let root = fm.temporaryDirectory.appendingPathComponent("open", isDirectory: true)
        try? fm.removeItem(at: root)
        let dir = root.appendingPathComponent(UUID().uuidString, isDirectory: true)
        guard (try? fm.createDirectory(at: dir, withIntermediateDirectories: true)) != nil else { return nil }
        var safe = name.replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: ":", with: "_")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if safe.isEmpty || safe.hasPrefix(".") { safe = "Datei" + safe }
        let dest = dir.appendingPathComponent(safe)
        let src = URL(fileURLWithPath: path)
        if (try? fm.linkItem(at: src, to: dest)) != nil { return dest }
        return (try? fm.copyItem(at: src, to: dest)) != nil ? dest : nil
    }
}
