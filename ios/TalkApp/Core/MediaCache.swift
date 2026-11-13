import Foundation
import ImageIO
import UIKit
import UniformTypeIdentifiers

/// Decoded avatars and chat image previews (port of the caches in Android `SessionManager`).
/// Synchronous lookups let re-shown rows draw their first frame without flashing.
@MainActor
final class MediaCache {
    private let avatars: NSCache<NSString, UIImage> = {
        let c = NSCache<NSString, UIImage>()
        c.countLimit = 64
        return c
    }()
    /// Chat previews by file id, bounded by decoded bytes.
    private let previews: NSCache<NSString, UIImage> = {
        let c = NSCache<NSString, UIImage>()
        c.totalCostLimit = 48 * 1024 * 1024
        return c
    }()
    /// Width / height of previews seen in this process, so bubbles keep their size.
    private var aspects: [String: CGFloat] = [:]
    /// Decoded local images of pending sends, by reference id, until the server echo arrives.
    private var localPreviews: [String: UIImage] = [:]
    /// SVG avatars and failures: initials are drawn, no need to ask again.
    private var noAvatar = Set<String>()

    static let previewPx: UInt32 = 1024
    static let fullPx: UInt32 = 2048
    /// Longest edge a bubble shows, in pixels.
    static var bubbleEdge: CGFloat { 300 * UIScreen.main.scale }

    private var session: TalkSession? { SessionStore.shared.session }

    func clear() {
        avatars.removeAllObjects()
        previews.removeAllObjects()
        aspects.removeAll()
        localPreviews.removeAll()
        noAvatar.removeAll()
    }

    // MARK: Avatars

    private func avatarKey(_ token: String, _ version: String, _ dark: Bool) -> String { "\(token)|\(version)|\(dark)" }

    func cachedAvatar(_ token: String, _ version: String, dark: Bool) -> UIImage? {
        avatars.object(forKey: avatarKey(token, version, dark) as NSString)
    }

    /// Server-rendered conversation avatar; `nil` for SVG avatars (initials are drawn instead).
    func avatar(_ token: String, _ version: String, dark: Bool) async -> UIImage? {
        let key = avatarKey(token, version, dark)
        if let hit = avatars.object(forKey: key as NSString) { return hit }
        guard !noAvatar.contains(key), let s = session else { return nil }
        guard let data = try? await s.avatar(token: token, dark: dark) else { return nil }
        let image = await Task.detached { ImageDecode.downsample(data, maxEdge: 80 * 3) }.value
        guard let image else {
            noAvatar.insert(key)
            return nil
        }
        avatars.setObject(image, forKey: key as NSString)
        return image
    }

    // MARK: Chat previews

    func cachedPreview(_ fileId: String) -> UIImage? { previews.object(forKey: fileId as NSString) }

    func aspect(_ fileId: String) -> CGFloat? { aspects[fileId] }

    func preview(_ fileId: String) async -> UIImage? {
        if let hit = cachedPreview(fileId) { return hit }
        guard let s = session,
              let data = try? await s.filePreview(fileId: fileId, width: Self.previewPx, height: Self.previewPx)
        else { return nil }
        let edge = Self.bubbleEdge
        guard let image = await Task.detached(operation: { ImageDecode.downsample(data, maxEdge: edge) }).value
        else { return nil }
        store(image, for: fileId)
        return image
    }

    /// Large version for the full-screen viewer (not cached).
    func fullImage(_ fileId: String) async -> UIImage? {
        guard let s = session,
              let data = try? await s.filePreview(fileId: fileId, width: Self.fullPx, height: Self.fullPx)
        else { return nil }
        let edge = CGFloat(Self.fullPx)
        return await Task.detached { ImageDecode.downsample(data, maxEdge: edge) }.value
    }

    private func store(_ image: UIImage, for fileId: String) {
        let cost = Int(image.size.width * image.scale * image.size.height * image.scale * 4)
        previews.setObject(image, forKey: fileId as NSString, cost: cost)
        if image.size.height > 0 { aspects[fileId] = image.size.width / image.size.height }
    }

    // MARK: Pending sends

    func localPreview(_ referenceId: String) -> UIImage? { localPreviews[referenceId] }

    func setLocalPreview(_ referenceId: String, path: String) {
        if let image = ImageDecode.downsample(url: URL(fileURLWithPath: path), maxEdge: Self.bubbleEdge) {
            localPreviews[referenceId] = image
        }
    }

    func removeLocalPreview(_ referenceId: String) { localPreviews[referenceId] = nil }

    /// Decodes the outbox copy of a pending image (e.g. after an app restart).
    func loadLocalPreview(_ referenceId: String, path: String?) async -> UIImage? {
        if let hit = localPreviews[referenceId] { return hit }
        guard let path else { return nil }
        let edge = Self.bubbleEdge
        let image = await Task.detached { ImageDecode.downsample(url: URL(fileURLWithPath: path), maxEdge: edge) }.value
        if let image { localPreviews[referenceId] = image }
        return image
    }

    /// Hands the decoded local image to the server message so its bubble does not reload.
    func adoptLocalPreviews(_ messages: [MessageView]) {
        guard !localPreviews.isEmpty else { return }
        for m in messages {
            guard let image = localPreviews.removeValue(forKey: m.referenceId),
                  let id = m.attachment?.id, !id.isEmpty else { continue }
            store(image, for: id)
        }
    }
}

/// ImageIO thumbnails: decode straight to the shown size (EXIF orientation applied).
enum ImageDecode {
    nonisolated static func downsample(_ data: Data, maxEdge: CGFloat) -> UIImage? {
        guard let src = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary)
        else { return nil }
        return thumbnail(src, maxEdge)
    }

    nonisolated static func downsample(url: URL, maxEdge: CGFloat) -> UIImage? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary)
        else { return nil }
        return thumbnail(src, maxEdge)
    }

    nonisolated private static func thumbnail(_ src: CGImageSource, _ maxEdge: CGFloat) -> UIImage? {
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: max(1, Int(maxEdge)),
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { return nil }
        return UIImage(cgImage: cg)
    }
}

/// A photo ready for the outbox: downscaled, rotated, re-encoded (metadata such as GPS dropped).
struct PreparedImage: Identifiable {
    let url: URL
    let name: String
    let width: Int
    let height: Int
    var id: URL { url }
}

enum ImagePrep {
    static let maxEdge: CGFloat = 2048

    /// Downscales to 2048 px, applies the EXIF orientation and writes a JPEG (quality 0.85)
    /// into the staging folder, which the system does not clear (unlike caches).
    nonisolated static func prepare(data: Data) -> PreparedImage? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any]
        let w = props?[kCGImagePropertyPixelWidth] as? CGFloat ?? maxEdge
        let h = props?[kCGImagePropertyPixelHeight] as? CGFloat ?? maxEdge
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: Int(min(maxEdge, max(w, h))),
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { return nil }
        let url = SessionStore.stagingDir.appendingPathComponent(UUID().uuidString.lowercased() + ".jpg")
        guard let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil)
        else { return nil }
        CGImageDestinationAddImage(dest, cg, [kCGImageDestinationLossyCompressionQuality: 0.85] as CFDictionary)
        guard CGImageDestinationFinalize(dest) else { return nil }
        let stamp = Int(Date().timeIntervalSince1970)
        return PreparedImage(url: url, name: "IMG_\(stamp).jpg", width: cg.width, height: cg.height)
    }
}
