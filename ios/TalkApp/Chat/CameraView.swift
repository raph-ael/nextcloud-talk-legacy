import AVFoundation
import Observation
import SwiftUI

/// In-app camera (like Signal): tap to focus, pinch to zoom, flash off/auto/on,
/// front/back, gallery shortcut. A taken photo goes to the send screen.
struct CameraView: View {
    let onClose: () -> Void
    let onCaptured: (PreparedImage) -> Void
    let onGallery: () -> Void

    @State private var camera = CameraModel()
    @State private var flashEffect = false
    @State private var focusPoint: CGPoint?
    @State private var baseZoom: CGFloat = 1

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            if camera.available {
                CameraPreview(session: camera.session) { layer in camera.previewLayer = layer }
                    .ignoresSafeArea()
                    .gesture(
                        SpatialTapGesture().onEnded { tap in
                            focusPoint = tap.location
                            camera.focus(at: tap.location)
                            Task {
                                try? await Task.sleep(for: .seconds(1))
                                focusPoint = nil
                            }
                        }
                    )
                    .simultaneousGesture(
                        MagnifyGesture()
                            .onChanged { camera.zoom(baseZoom * $0.magnification) }
                            .onEnded { _ in baseZoom = camera.zoomFactor }
                    )
                if let p = focusPoint {
                    RoundedRectangle(cornerRadius: 4).stroke(.yellow, lineWidth: 1.5)
                        .frame(width: 70, height: 70).position(p).allowsHitTesting(false)
                }
            } else {
                Text(camera.denied ? "Kein Zugriff auf die Kamera – in den Einstellungen erlauben." : "Keine Kamera verfügbar")
                    .foregroundStyle(.white.opacity(0.8))
                    .multilineTextAlignment(.center)
                    .padding(32)
            }
            if flashEffect { Color.white.ignoresSafeArea().allowsHitTesting(false) }
            controls
        }
        .statusBarHidden()
        .task { await camera.start() }
        .onDisappear { camera.stop() }
    }

    private var controls: some View {
        VStack {
            HStack {
                Button(action: onClose) {
                    Image(systemName: "xmark").font(.system(size: 20, weight: .semibold)).frame(width: 44, height: 44)
                }
                .accessibilityLabel("Schließen")
                Spacer()
                Button { camera.cycleFlash() } label: {
                    Image(systemName: camera.flashIcon).font(.system(size: 20)).frame(width: 44, height: 44)
                }
                .accessibilityLabel("Blitz")
            }
            .padding(.horizontal, 12)
            Spacer()
            HStack {
                Button(action: onGallery) {
                    Image(systemName: "photo.on.rectangle").font(.system(size: 24)).frame(width: 56, height: 56)
                }
                .accessibilityLabel("Galerie")
                Spacer()
                Button(action: shoot) {
                    ZStack {
                        Circle().stroke(.white, lineWidth: 4).frame(width: 74, height: 74)
                        Circle().fill(.white).frame(width: 62, height: 62)
                    }
                }
                .accessibilityLabel("Foto aufnehmen")
                .disabled(!camera.available)
                Spacer()
                Button { camera.switchCamera() } label: {
                    Image(systemName: "arrow.triangle.2.circlepath.camera").font(.system(size: 24)).frame(width: 56, height: 56)
                }
                .accessibilityLabel("Kamera wechseln")
            }
            .padding(.horizontal, 28)
            .padding(.bottom, 24)
        }
        .foregroundStyle(.white)
    }

    private func shoot() {
        Haptics.light()
        withAnimation(.easeOut(duration: 0.08)) { flashEffect = true }
        Task {
            try? await Task.sleep(for: .milliseconds(90))
            withAnimation(.easeIn(duration: 0.2)) { flashEffect = false }
        }
        Task {
            guard let data = await camera.capture(),
                  let image = await Task.detached(operation: { ImagePrep.prepare(data: data) }).value else {
                Toast.show("Foto konnte nicht gespeichert werden")
                return
            }
            onCaptured(image)
        }
    }
}

@MainActor
@Observable
final class CameraModel: NSObject, AVCapturePhotoCaptureDelegate {
    @ObservationIgnored let session = AVCaptureSession()
    @ObservationIgnored weak var previewLayer: AVCaptureVideoPreviewLayer?
    private(set) var available = false
    private(set) var denied = false
    private(set) var flash: AVCaptureDevice.FlashMode = .off
    private(set) var zoomFactor: CGFloat = 1
    @ObservationIgnored private let output = AVCapturePhotoOutput()
    @ObservationIgnored private var device: AVCaptureDevice?
    @ObservationIgnored private var position: AVCaptureDevice.Position = .back
    @ObservationIgnored private var pending: CheckedContinuation<Data?, Never>?
    @ObservationIgnored private let queue = DispatchQueue(label: "talk.camera")

    var flashIcon: String {
        switch flash {
        case .on: return "bolt.fill"
        case .auto: return "bolt.badge.a.fill"
        default: return "bolt.slash.fill"
        }
    }

    func start() async {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: break
        case .notDetermined:
            guard await AVCaptureDevice.requestAccess(for: .video) else { denied = true; return }
        default:
            denied = true
            return
        }
        guard configure(.back) else { return }
        available = true
        let s = session
        queue.async { s.startRunning() }
    }

    func stop() {
        let s = session
        queue.async { s.stopRunning() }
    }

    private func configure(_ pos: AVCaptureDevice.Position) -> Bool {
        guard let dev = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: pos),
              let input = try? AVCaptureDeviceInput(device: dev) else { return false }
        session.beginConfiguration()
        session.sessionPreset = .photo
        session.inputs.forEach(session.removeInput)
        guard session.canAddInput(input) else {
            session.commitConfiguration()
            return false
        }
        session.addInput(input)
        if !session.outputs.contains(output), session.canAddOutput(output) { session.addOutput(output) }
        session.commitConfiguration()
        device = dev
        position = pos
        zoomFactor = 1
        return true
    }

    func switchCamera() {
        _ = configure(position == .back ? .front : .back)
    }

    func cycleFlash() {
        flash = flash == .off ? .auto : flash == .auto ? .on : .off
    }

    func zoom(_ factor: CGFloat) {
        guard let device, (try? device.lockForConfiguration()) != nil else { return }
        let z = min(max(1, factor), min(device.activeFormat.videoMaxZoomFactor, 8))
        device.videoZoomFactor = z
        device.unlockForConfiguration()
        zoomFactor = z
    }

    func focus(at point: CGPoint) {
        guard let device, let layer = previewLayer, (try? device.lockForConfiguration()) != nil else { return }
        let p = layer.captureDevicePointConverted(fromLayerPoint: point)
        if device.isFocusPointOfInterestSupported {
            device.focusPointOfInterest = p
            device.focusMode = .autoFocus
        }
        if device.isExposurePointOfInterestSupported {
            device.exposurePointOfInterest = p
            device.exposureMode = .autoExpose
        }
        device.unlockForConfiguration()
    }

    func capture() async -> Data? {
        guard available, pending == nil else { return nil }
        let settings = AVCapturePhotoSettings()
        if output.supportedFlashModes.contains(flash) { settings.flashMode = flash }
        return await withCheckedContinuation { cont in
            pending = cont
            output.capturePhoto(with: settings, delegate: self)
        }
    }

    nonisolated func photoOutput(_ output: AVCapturePhotoOutput, didFinishProcessingPhoto photo: AVCapturePhoto, error: Error?) {
        let data = photo.fileDataRepresentation()
        Task { @MainActor in
            self.pending?.resume(returning: data)
            self.pending = nil
        }
    }
}

private struct CameraPreview: UIViewRepresentable {
    let session: AVCaptureSession
    let onLayer: (AVCaptureVideoPreviewLayer) -> Void

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var preview: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }
    }

    func makeUIView(context: Context) -> PreviewView {
        let v = PreviewView()
        v.preview.session = session
        v.preview.videoGravity = .resizeAspectFill
        onLayer(v.preview)
        return v
    }

    func updateUIView(_ view: PreviewView, context: Context) {}
}
