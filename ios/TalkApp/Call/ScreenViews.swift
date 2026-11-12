import AVFoundation
import SwiftUI
import UIKit
import WebRTC

/// Presentation view (docs/calls.md "Bildschirmfreigabe"): a screen shared by somebody
/// else fills the stage, fitted and never cropped, zoomable with two fingers, movable and
/// double-tap zoomable. Portrait: participants (and further screens) in a row below, the
/// controls at the bottom. Landscape: the screen fills everything, the controls fade out
/// after 3 s and a tap brings them back.
struct ScreenCallView: View {
    let call: CallView
    /// The screen the user picked from the row; a newer share takes the stage again.
    @State private var picked: String?
    @State private var landscape = false
    @State private var chrome = true
    /// Restarts the 3 s until the controls fade out (landscape).
    @State private var hideRound = 0
    private var controller: CallController { .shared }
    private var video: CallVideo { .shared }

    private var screen: CallScreen {
        call.screens.first { $0.sessionId == picked } ?? call.screens[0]
    }

    var body: some View {
        GeometryReader { geo in
            let wide = geo.size.width > geo.size.height
            ZStack {
                Color.black.ignoresSafeArea()
                if wide { landscapeLayout } else { portraitLayout }
            }
            .onAppear { landscape = wide }
            .onChange(of: wide) { _, now in
                landscape = now
                withAnimation(.easeOut(duration: 0.2)) { chrome = true }
                hideRound += 1
            }
        }
        .onChange(of: call.screens.first?.sessionId) { _, _ in picked = nil }
        .task(id: hideRound) {
            guard landscape, chrome, !UIAccessibility.isVoiceOverRunning else { return }
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled, landscape else { return }
            withAnimation(.easeOut(duration: 0.3)) { chrome = false }
        }
    }

    // MARK: Portrait

    private var portraitLayout: some View {
        VStack(spacing: 0) {
            HStack(spacing: 4) {
                MinimizeButton()
                VStack(alignment: .leading, spacing: 1) {
                    Text(CallText.title(call))
                        .font(.system(size: 17, weight: .semibold))
                        .lineLimit(1)
                        .accessibilityIdentifier("call-title")
                    CallStatusLine(call: call)
                }
                .foregroundStyle(.white)
                Spacer(minLength: 8)
                RecordingIndicator(call: call)
            }
            .padding(.horizontal, 8)
            .padding(.trailing, 8)
            ScreenStageView(screen: screen) {}
                .overlay(alignment: .topLeading) { ScreenLabel(name: screen.name).padding(8) }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .padding(.top, 8)
            if !rowEmpty {
                row.padding(.top, 12)
            }
            CallControls(call: call)
                .padding(.top, 16)
                .padding(.bottom, 24)
        }
    }

    private var rowEmpty: Bool {
        call.screens.count < 2 && call.participants.isEmpty && !(controller.wantCamera && video.local != nil)
    }

    /// Further screens (tap: onto the stage), our camera, the participants.
    private var row: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(call.screens.filter { $0.sessionId != screen.sessionId }, id: \.sessionId) { s in
                    ScreenThumb(screen: s) {
                        withAnimation(.easeOut(duration: 0.2)) { picked = s.sessionId }
                    }
                }
                if let own = video.local, controller.wantCamera {
                    LocalPreview(track: own, mirrored: video.localFront, size: CGSize(width: 84, height: 112))
                }
                ForEach(call.participants, id: \.sessionId) { p in
                    CallTile(participant: p, large: false, compact: true)
                        .frame(width: 84)
                }
            }
            .padding(.horizontal, 12)
        }
        .scrollBounceBehavior(.basedOnSize, axes: .horizontal)
        .frame(height: 112)
        .accessibilityIdentifier("call-screen-row")
    }

    // MARK: Landscape

    private var landscapeLayout: some View {
        ZStack {
            ScreenStageView(screen: screen) {
                withAnimation(.easeOut(duration: 0.2)) { chrome.toggle() }
                hideRound += 1
            }
            .ignoresSafeArea()
            if chrome {
                VStack(spacing: 0) {
                    HStack(spacing: 8) {
                        MinimizeButton()
                        ScreenLabel(name: screen.name)
                        Spacer(minLength: 8)
                        RecordingIndicator(call: call)
                        if call.screens.count > 1 { picker }
                    }
                    .padding(.horizontal, 8)
                    .background(alignment: .top) {
                        LinearGradient(colors: [.black.opacity(0.6), .clear], startPoint: .top, endPoint: .bottom)
                            .frame(height: 120)
                            .ignoresSafeArea()
                            .allowsHitTesting(false)
                    }
                    Spacer()
                    HStack(alignment: .bottom) {
                        Spacer()
                        CallControls(call: call)
                        Spacer()
                    }
                    .overlay(alignment: .bottomTrailing) {
                        if let own = video.local, controller.wantCamera {
                            LocalPreview(track: own, mirrored: video.localFront, size: CGSize(width: 72, height: 96))
                                .padding(.trailing, 8)
                        }
                    }
                    .padding(.bottom, 12)
                    .background(alignment: .bottom) {
                        LinearGradient(colors: [.clear, .black.opacity(0.6)], startPoint: .top, endPoint: .bottom)
                            .frame(height: 140)
                            .ignoresSafeArea()
                            .allowsHitTesting(false)
                    }
                    .simultaneousGesture(TapGesture().onEnded { hideRound += 1 })
                }
                .transition(.opacity)
            }
        }
    }

    /// Several screens in landscape (no row): choose which one is on the stage.
    private var picker: some View {
        Menu {
            ForEach(call.screens, id: \.sessionId) { s in
                Button {
                    picked = s.sessionId
                } label: {
                    if s.sessionId == screen.sessionId {
                        Label(s.name, systemImage: "checkmark")
                    } else {
                        Text(s.name)
                    }
                }
            }
        } label: {
            Image(systemName: "rectangle.on.rectangle")
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 44, height: 44)
        }
        .accessibilityLabel("Anderen Bildschirm zeigen")
        .accessibilityIdentifier("call-screen-picker")
        .simultaneousGesture(TapGesture().onEnded { hideRound += 1 })
    }
}

/// "Bildschirm von …" on the stage.
struct ScreenLabel: View {
    let name: String

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "rectangle.on.rectangle").font(.system(size: 12, weight: .semibold))
            Text("Bildschirm von \(name)").font(.system(size: 13, weight: .semibold)).lineLimit(1)
        }
        .foregroundStyle(.white)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(Color.black.opacity(0.5), in: Capsule())
        .allowsHitTesting(false)
        .accessibilityIdentifier("call-screen-label")
    }
}

/// The shared screen, or a spinner until its first frame arrives.
struct ScreenStageView: View {
    let screen: CallScreen
    let onTap: () -> Void
    private var video: CallVideo { .shared }

    var body: some View {
        ZStack {
            Color.black
            if let track = video.screenTrack(screen) {
                ScreenStage(track: track, onTap: onTap)
                    .accessibilityElement()
                    .accessibilityLabel("Bildschirm von \(screen.name)")
                    .accessibilityIdentifier("call-screen-stage")
            } else {
                VStack(spacing: 12) {
                    ProgressView().tint(.white)
                    Text("Bildschirm von \(screen.name) wird geladen …")
                        .font(.system(size: 14))
                        .foregroundStyle(.white.opacity(0.8))
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("call-screen-loading")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Rectangle())
                .onTapGesture(perform: onTap)
            }
        }
    }
}

/// A further screen in the row: tap puts it on the stage.
struct ScreenThumb: View {
    let screen: CallScreen
    let action: () -> Void
    private var video: CallVideo { .shared }

    var body: some View {
        Button(action: action) {
            ZStack(alignment: .bottomLeading) {
                Color.black
                if let track = video.screenTrack(screen) {
                    VideoView(track: track, fill: false).allowsHitTesting(false)
                } else {
                    ProgressView().tint(.white).frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                HStack(spacing: 4) {
                    Image(systemName: "rectangle.on.rectangle").font(.system(size: 10, weight: .semibold))
                    Text(screen.name).font(.system(size: 11, weight: .medium)).lineLimit(1)
                }
                .foregroundStyle(.white)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(.black.opacity(0.55), in: Capsule())
                .padding(4)
            }
            .frame(width: 150, height: 112)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.25), lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Bildschirm von \(screen.name) zeigen")
        .accessibilityIdentifier("call-screen-thumb")
    }
}

/// `ZoomableVideoView` for SwiftUI; a new track resets the zoom.
struct ScreenStage: UIViewRepresentable {
    let track: RTCVideoTrack
    let onTap: () -> Void

    func makeUIView(context: Context) -> ZoomableVideoView {
        let v = ZoomableVideoView()
        v.onTap = onTap
        return v
    }

    func updateUIView(_ v: ZoomableVideoView, context: Context) {
        v.onTap = onTap
        v.setTrack(track)
    }

    static func dismantleUIView(_ v: ZoomableVideoView, coordinator: ()) {
        v.detach()
    }
}

/// A video fitted into its bounds inside a scroll view: pinch to zoom (up to 6×), pan while
/// zoomed, double tap zooms in at that point or back out. The video view is sized to the
/// frame's aspect ratio and centred, so no black bars can be panned around. libwebrtc
/// draws at the frame's own resolution, so zooming stays sharp up to the original pixels.
final class ZoomableVideoView: UIView, UIScrollViewDelegate, RTCVideoViewDelegate {
    var onTap: (() -> Void)?

    private let scroll = UIScrollView()
    private let video = RTCMTLVideoView(frame: .zero)
    private var track: RTCVideoTrack?
    private var videoSize = CGSize(width: 16, height: 9)

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .black
        scroll.delegate = self
        scroll.minimumZoomScale = 1
        scroll.maximumZoomScale = 6
        scroll.showsHorizontalScrollIndicator = false
        scroll.showsVerticalScrollIndicator = false
        scroll.contentInsetAdjustmentBehavior = .never
        scroll.decelerationRate = .fast
        scroll.bouncesZoom = true
        addSubview(scroll)
        video.videoContentMode = .scaleAspectFit
        video.backgroundColor = .black
        video.delegate = self
        scroll.addSubview(video)
        let double = UITapGestureRecognizer(target: self, action: #selector(doubleTapped(_:)))
        double.numberOfTapsRequired = 2
        scroll.addGestureRecognizer(double)
        let single = UITapGestureRecognizer(target: self, action: #selector(tapped))
        single.require(toFail: double)
        scroll.addGestureRecognizer(single)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func setTrack(_ t: RTCVideoTrack) {
        guard t !== track else { return }
        track?.remove(video)
        t.add(video)
        track = t
        relayout()
    }

    func detach() {
        track?.remove(video)
        track = nil
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard scroll.frame != bounds else { return }
        scroll.frame = bounds
        relayout()
    }

    /// Back to 1×, the video fitted and centred (new size, new frame format, rotation).
    private func relayout() {
        let b = scroll.bounds.size
        guard b.width > 0, b.height > 0, videoSize.width > 0, videoSize.height > 0 else { return }
        scroll.zoomScale = 1
        let s = min(b.width / videoSize.width, b.height / videoSize.height)
        let fit = CGSize(width: (videoSize.width * s).rounded(), height: (videoSize.height * s).rounded())
        video.frame = CGRect(origin: .zero, size: fit)
        scroll.contentSize = fit
        center()
        scroll.contentOffset = CGPoint(x: -scroll.contentInset.left, y: -scroll.contentInset.top)
    }

    private func center() {
        let b = scroll.bounds.size, c = scroll.contentSize
        let x = max(0, (b.width - c.width) / 2), y = max(0, (b.height - c.height) / 2)
        scroll.contentInset = UIEdgeInsets(top: y, left: x, bottom: y, right: x)
    }

    @objc private func tapped() { onTap?() }

    @objc private func doubleTapped(_ g: UITapGestureRecognizer) {
        if scroll.zoomScale > 1.01 {
            scroll.setZoomScale(1, animated: true)
            return
        }
        let p = g.location(in: video)
        let scale: CGFloat = 2.5
        let size = CGSize(width: scroll.bounds.width / scale, height: scroll.bounds.height / scale)
        scroll.zoom(to: CGRect(x: p.x - size.width / 2, y: p.y - size.height / 2, width: size.width, height: size.height), animated: true)
    }

    func viewForZooming(in scrollView: UIScrollView) -> UIView? { video }

    func scrollViewDidZoom(_ scrollView: UIScrollView) { center() }

    nonisolated func videoView(_ videoView: RTCVideoRenderer, didChangeVideoSize size: CGSize) {
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated { self?.sizeChanged(size) }
        }
    }

    private func sizeChanged(_ size: CGSize) {
        guard size.width > 0, size.height > 0, size != videoSize else { return }
        Log.call.info("screen size \(Int(size.width))x\(Int(size.height))")
        videoSize = size
        relayout()
    }
}
