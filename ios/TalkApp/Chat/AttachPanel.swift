import Photos
import PhotosUI
import SwiftUI

/// Attachment panel in place of the keyboard (Signal iOS): the newest photos in a strip,
/// then the sources. Picking a photo opens the send screen.
struct AttachPanel: View {
    let height: CGFloat
    let onImage: (PreparedImage) -> Void
    let onCamera: () -> Void

    @State private var assets: [PHAsset] = []
    @State private var status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
    @State private var picked: PhotosPickerItem?
    @State private var busy = false

    var body: some View {
        VStack(spacing: 16) {
            recentStrip
            HStack(spacing: 0) {
                PhotosPicker(selection: $picked, matching: .images, photoLibrary: .shared()) {
                    Source(icon: "photo.on.rectangle", label: "Galerie")
                }
                .accessibilityIdentifier("gallery")
                Button(action: onCamera) { Source(icon: "camera", label: "Kamera") }
                Button { Toast.show("Dateien senden kommt bald") } label: { Source(icon: "doc", label: "Datei") }
                Button { Toast.show("Umfragen kommen bald") } label: { Source(icon: "chart.bar", label: "Umfrage") }
                Button { Toast.show("Kontakte senden kommt bald") } label: { Source(icon: "person.crop.circle", label: "Kontakt") }
                Button { Toast.show("Standort senden kommt bald") } label: { Source(icon: "location", label: "Standort") }
            }
            .buttonStyle(.plain)
            Spacer(minLength: 0)
        }
        .padding(.top, 12)
        .frame(height: height)
        .frame(maxWidth: .infinity)
        .background(Talk.background)
        .overlay { if busy { ProgressView() } }
        .task { await load() }
        .onChange(of: picked) { _, item in
            guard let item else { return }
            picked = nil
            Task { await prepare { try? await item.loadTransferable(type: Data.self) } }
        }
    }

    @ViewBuilder
    private var recentStrip: some View {
        switch status {
        case .authorized, .limited:
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(spacing: 6) {
                    ForEach(assets, id: \.localIdentifier) { asset in
                        Thumbnail(asset: asset)
                            .accessibilityIdentifier("recent-photo")
                            .accessibilityAddTraits(.isButton)
                            .onTapGesture { Task { await prepare { await Self.data(of: asset) } } }
                    }
                    if status == .limited {
                        Button {
                            if let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene,
                               let root = scene.keyWindow?.rootViewController {
                                PHPhotoLibrary.shared().presentLimitedLibraryPicker(from: root)
                            }
                        } label: {
                            Image(systemName: "plus").font(.title2).foregroundStyle(Talk.muted)
                                .frame(width: 96, height: 96)
                                .background(Talk.chip, in: RoundedRectangle(cornerRadius: 10))
                        }
                    }
                }
                .padding(.horizontal, 12)
            }
            .frame(height: 96)
        case .notDetermined:
            Button("Zugriff auf Fotos erlauben") {
                Task {
                    status = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
                    await load()
                }
            }
            .frame(height: 96)
        default:
            Text("Kein Zugriff auf Fotos – in den Einstellungen erlauben.")
                .font(.footnote)
                .foregroundStyle(Talk.muted)
                .frame(height: 96)
        }
    }

    private func load() async {
        guard status == .authorized || status == .limited else { return }
        let options = PHFetchOptions()
        options.sortDescriptors = [NSSortDescriptor(key: "creationDate", ascending: false)]
        options.fetchLimit = 60
        let result = PHAsset.fetchAssets(with: .image, options: options)
        var list: [PHAsset] = []
        result.enumerateObjects { a, _, _ in list.append(a) }
        assets = list
    }

    private static func data(of asset: PHAsset) async -> Data? {
        await withCheckedContinuation { cont in
            let options = PHImageRequestOptions()
            options.isNetworkAccessAllowed = true
            options.deliveryMode = .highQualityFormat
            options.version = .current
            PHImageManager.default().requestImageDataAndOrientation(for: asset, options: options) { data, _, _, _ in
                cont.resume(returning: data)
            }
        }
    }

    private func prepare(_ load: @escaping () async -> Data?) async {
        busy = true
        defer { busy = false }
        guard let data = await load() else {
            Toast.show("Bild konnte nicht gelesen werden")
            return
        }
        guard let image = await Task.detached(operation: { ImagePrep.prepare(data: data) }).value else {
            Toast.show("Bild konnte nicht gelesen werden")
            return
        }
        onImage(image)
    }
}

private struct Source: View {
    let icon: String
    let label: String

    var body: some View {
        VStack(spacing: 6) {
            Image(systemName: icon)
                .font(.system(size: 20))
                .foregroundStyle(Talk.onSurface)
                .frame(width: 48, height: 48)
                .background(Talk.chip, in: Circle())
            Text(label).font(.system(size: 11)).foregroundStyle(Talk.muted)
        }
        .frame(maxWidth: .infinity)
    }
}

private struct Thumbnail: View {
    let asset: PHAsset
    @State private var image: UIImage?

    var body: some View {
        ZStack {
            Talk.chip
            if let image { Image(uiImage: image).resizable().scaledToFill() }
        }
        .frame(width: 96, height: 96)
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .contentShape(Rectangle())
        .task(id: asset.localIdentifier) {
            let options = PHImageRequestOptions()
            options.deliveryMode = .opportunistic
            options.isNetworkAccessAllowed = true
            let size = CGSize(width: 96 * UIScreen.main.scale, height: 96 * UIScreen.main.scale)
            for await img in Self.images(asset, size, options) { image = img }
        }
        .accessibilityLabel("Foto")
    }

    private static func images(_ asset: PHAsset, _ size: CGSize, _ options: PHImageRequestOptions) -> AsyncStream<UIImage> {
        AsyncStream { cont in
            let id = PHImageManager.default().requestImage(for: asset, targetSize: size, contentMode: .aspectFill, options: options) { img, info in
                if let img { cont.yield(img) }
                let degraded = (info?[PHImageResultIsDegradedKey] as? Bool) ?? false
                if !degraded { cont.finish() }
            }
            cont.onTermination = { _ in PHImageManager.default().cancelImageRequest(id) }
        }
    }
}
