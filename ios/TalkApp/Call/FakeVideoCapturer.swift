#if DEBUG
import CoreVideo
import QuartzCore
import WebRTC

/// Debug builds: a camera without hardware (the simulator has none). It delivers a moving
/// test picture at 24 fps in portrait (480×640), so tests can check sending and the preview:
/// moving bands, a square running left to right and a bright marker in the top-left corner
/// (top right when mirrored). The front camera is tinted blue, the back camera red.
final class FakeVideoCapturer: RTCVideoCapturer, @unchecked Sendable {
    private static let width = 480
    private static let height = 640

    private let queue = DispatchQueue(label: "talk.call.fake-camera", qos: .userInitiated)
    private var timer: DispatchSourceTimer?
    private var pool: CVPixelBufferPool?
    private var frame = 0
    private var front = true

    func start(front: Bool) {
        queue.sync {
            self.front = front
            guard timer == nil else { return }
            let t = DispatchSource.makeTimerSource(queue: queue)
            t.schedule(deadline: .now(), repeating: .milliseconds(1000 / 24))
            t.setEventHandler { [weak self] in self?.deliver() }
            timer = t
            t.resume()
        }
        Log.call.info("fake camera on (\(front ? "front" : "back", privacy: .public))")
    }

    func stop() {
        queue.sync {
            timer?.cancel()
            timer = nil
        }
    }

    private func deliver() {
        guard let buffer = makeBuffer() else { return }
        frame += 1
        let ns = Int64(CACurrentMediaTime() * 1_000_000_000)
        let f = RTCVideoFrame(buffer: RTCCVPixelBuffer(pixelBuffer: buffer), rotation: ._0, timeStampNs: ns)
        delegate?.capturer(self, didCapture: f)
    }

    private func makeBuffer() -> CVPixelBuffer? {
        let w = Self.width, h = Self.height
        if pool == nil {
            let attrs: [CFString: Any] = [
                kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                kCVPixelBufferWidthKey: w,
                kCVPixelBufferHeightKey: h,
                kCVPixelBufferIOSurfacePropertiesKey: [:] as CFDictionary,
            ]
            CVPixelBufferPoolCreate(nil, nil, attrs as CFDictionary, &pool)
        }
        guard let pool else { return nil }
        var out: CVPixelBuffer?
        CVPixelBufferPoolCreatePixelBuffer(nil, pool, &out)
        guard let buf = out else { return nil }
        CVPixelBufferLockBaseAddress(buf, [])
        defer { CVPixelBufferUnlockBaseAddress(buf, []) }
        guard let y = CVPixelBufferGetBaseAddressOfPlane(buf, 0),
              let uv = CVPixelBufferGetBaseAddressOfPlane(buf, 1) else { return nil }
        let yStride = CVPixelBufferGetBytesPerRowOfPlane(buf, 0)
        let uvStride = CVPixelBufferGetBytesPerRowOfPlane(buf, 1)
        let square = 120
        let sx = (frame * 6) % (w - square)
        let sy = h / 2 - square / 2
        for row in 0..<h {
            let line = y.advanced(by: row * yStride)
            let band = ((row + frame * 4) / 40) % 2 == 0
            memset(line, band ? 70 : 110, w)
            if row < 160 { memset(line, 235, 60) }
            if row >= sy && row < sy + square { memset(line.advanced(by: sx), 255, square) }
        }
        // Chroma: interleaved Cb/Cr per 2×2 block.
        let cb: UInt8 = front ? 170 : 100
        let cr: UInt8 = front ? 100 : 175
        for row in 0..<(h / 2) {
            let line = uv.advanced(by: row * uvStride).assumingMemoryBound(to: UInt8.self)
            for i in 0..<(w / 2) {
                line[2 * i] = cb
                line[2 * i + 1] = cr
            }
        }
        return buf
    }
}
#endif
