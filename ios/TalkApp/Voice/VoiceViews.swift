import SwiftUI
import UIKit

// Voice message bubble. Layout follows Signal iOS (40 pt button, 32 pt waveform of 2 pt bars
// with ≥ 2 pt spacing and a position line, footer with duration, speed and message time).
// Own code; only the proportions are Signal's.
private let voiceWidth: CGFloat = 212
private let barWidth: CGFloat = 2
private let barMinGap: CGFloat = 1.5
private let barMaxHeight: CGFloat = 24
private let thumbHeight: CGFloat = 28

/// `n` bars 0…1 from the core's 100 levels (mean per bar).
private func bars(_ levels: Data?, _ n: Int) -> [CGFloat] {
    guard let levels, !levels.isEmpty else { return Array(repeating: 0, count: n) }
    let bytes = [UInt8](levels)
    return (0..<n).map { i in
        let a = i * bytes.count / n
        let b = max(a + 1, (i + 1) * bytes.count / n)
        let sum = bytes[a..<b].reduce(0) { $0 + Int($1) }
        return CGFloat(sum) / CGFloat(b - a) / 255
    }
}

/// Colors of one voice message, derived from its bubble.
struct VoicePalette {
    let circle: Color
    let icon: Color
    let played: Color
    let unplayed: Color
    let chip: Color

    static func make(own: Bool, text: Color, active: Bool, dark: Bool) -> VoicePalette {
        own
            ? VoicePalette(circle: .white.opacity(0.16), icon: .white, played: .white,
                           unplayed: .white.opacity(active ? 0.4 : 0.2), chip: .white.opacity(0.2))
            : VoicePalette(circle: dark ? .white.opacity(0.12) : .white, icon: text, played: text,
                           unplayed: text.opacity(dark ? 0.3 : 0.22), chip: text.opacity(0.08))
    }
}

/// Bars with the played part highlighted and a position line; tap or drag to seek.
struct Waveform: View {
    let levels: Data?
    let progress: Double
    let played: Color
    let unplayed: Color
    let onSeek: (Double) -> Void

    var body: some View {
        GeometryReader { geo in
            Canvas { ctx, size in
                let n = max(2, Int((size.width + barMinGap) / (barWidth + barMinGap)))
                let step = (size.width - barWidth) / CGFloat(n - 1)
                let mid = size.height / 2
                let thumbX = barWidth / 2 + CGFloat(min(1, max(0, progress))) * (size.width - barWidth)
                for (i, v) in bars(levels, n).enumerated() {
                    let x = barWidth / 2 + CGFloat(i) * step
                    let h = barWidth + v * (barMaxHeight - barWidth)
                    let rect = CGRect(x: x - barWidth / 2, y: mid - h / 2, width: barWidth, height: h)
                    ctx.fill(Path(roundedRect: rect, cornerRadius: barWidth / 2), with: .color(x <= thumbX ? played : unplayed))
                }
                let thumb = CGRect(x: thumbX - barWidth / 2, y: mid - thumbHeight / 2, width: barWidth, height: thumbHeight)
                ctx.fill(Path(roundedRect: thumb, cornerRadius: barWidth / 2), with: .color(played))
            }
            .contentShape(Rectangle())
            // A tap fails as soon as the finger moves, so a scroll starting here stays a scroll.
            .onTapGesture(coordinateSpace: .local) { p in onSeek(Double(p.x / max(1, geo.size.width))) }
            .modifier(HorizontalScrub { x in onSeek(Double(x / max(1, geo.size.width))) })
        }
        .frame(height: 32)
        .accessibilityHidden(true)
    }
}

/// Scrubbing by dragging sideways. A SwiftUI DragGesture would take every touch – vertical
/// drags then seek (and start playback) instead of scrolling the list – so this is a UIKit pan
/// that only begins for horizontal movement (iOS 18+; on iOS 17 tapping still seeks).
private struct HorizontalScrub: ViewModifier {
    let onChange: (CGFloat) -> Void

    func body(content: Content) -> some View {
        if #available(iOS 18.0, *) {
            content.gesture(ScrubRecognizer(onChange: onChange))
        } else {
            content
        }
    }
}

@available(iOS 18.0, *)
private struct ScrubRecognizer: UIGestureRecognizerRepresentable {
    let onChange: (CGFloat) -> Void

    func makeCoordinator(converter: CoordinateSpaceConverter) -> Coordinator { Coordinator() }

    func makeUIGestureRecognizer(context: Context) -> UIPanGestureRecognizer {
        let g = UIPanGestureRecognizer()
        g.delegate = context.coordinator
        return g
    }

    func handleUIGestureRecognizerAction(_ g: UIPanGestureRecognizer, context: Context) {
        switch g.state {
        case .began, .changed: onChange(context.converter.localLocation.x)
        default: break
        }
    }

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
            guard let pan = g as? UIPanGestureRecognizer else { return true }
            let v = pan.velocity(in: pan.view)
            return abs(v.x) > abs(v.y) * 1.5
        }
    }
}

/// Round play/pause button. A ring on the circle's edge shows upload progress or loading;
/// `download` marks a file that is not fetched yet.
struct PlayButton: View {
    let playing: Bool
    let loading: Bool
    let background: Color
    let tint: Color
    let upload: Float?
    var download = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack {
                Circle().fill(background)
                if let upload {
                    Circle().trim(from: 0, to: CGFloat(max(0.03, upload)))
                        .stroke(tint, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                        .padding(1)
                } else if loading {
                    LoadingRing(color: tint).padding(1)
                }
                Image(systemName: playing ? "pause.fill" : download ? "arrow.down" : "play.fill")
                    .font(.system(size: 17, weight: download && !playing ? .semibold : .regular))
                    .foregroundStyle(tint)
                    // play.fill sits left-heavy in its box; Signal centres the triangle optically.
                    .offset(x: playing || download ? 0 : 1.5)
            }
            .frame(width: 40, height: 40)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(playing ? "Pause" : "Abspielen")
    }
}

/// Indeterminate ring on the button's edge while a file loads.
private struct LoadingRing: View {
    let color: Color
    @State private var turn = false

    var body: some View {
        Circle().trim(from: 0, to: 0.3)
            .stroke(color, style: StrokeStyle(lineWidth: 2, lineCap: .round))
            .rotationEffect(.degrees(turn ? 360 : 0))
            .animation(.linear(duration: 0.9).repeatForever(autoreverses: false), value: turn)
            .onAppear { turn = true }
    }
}

/// Speed chip: 1× → 1,5× → 2× → 0,5×.
struct SpeedChip: View {
    let speed: Float
    let color: Color
    let background: Color
    let action: () -> Void

    var body: some View {
        let label = (speed == speed.rounded() ? "\(Int(speed))" : "\(speed)".replacingOccurrences(of: ".", with: ",")) + "×"
        Button(action: action) {
            Text(label)
                .font(.system(size: 12.5, weight: .bold))
                .foregroundStyle(color)
                .padding(.horizontal, 4)
                .frame(minWidth: 30, minHeight: 20)
                .background(background, in: RoundedRectangle(cornerRadius: 4))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Tempo \(label)")
    }
}

/// A voice message (or any audio file) inside a bubble: button + waveform, then a footer with
/// duration (remaining while playing), speed chip and `meta` (time, ticks) at the end.
struct VoiceBubble: View {
    let m: UiMessage
    let textColor: Color
    let metaColor: Color
    let own: Bool
    var meta: MessageMeta?

    @State private var info: AudioInfo?
    @Environment(\.colorScheme) private var scheme
    private var voice: VoiceManager { SessionStore.shared.voice }

    var body: some View {
        let a = m.msg.attachment!
        let key = voice.messageKey(m)
        let player = voice.player
        let current = player.isCurrent(key)
        let shownInfo = info ?? voice.cachedInfo(key)
        let duration = Int64(shownInfo?.durationMs ?? 0) > 0 ? Int64(shownInfo!.durationMs) : (current ? player.durationMs : 0)
        let position = current ? player.positionMs : 0
        let active = current && (player.playing || position > 0)
        let progress = duration > 0 ? min(1, max(0, Double(position) / Double(duration))) : 0
        let uploading = m.status == .sending && (0..<0.999).contains(m.progress)
        let auto = m.status != nil || a.size <= VoiceManager.autoFetchBytes || current
        let p = VoicePalette.make(own: own, text: textColor, active: active, dark: scheme == .dark)

        VStack(alignment: .leading, spacing: 0) {
            if !a.voiceMessage {
                Text(a.name).font(.system(size: 12)).foregroundStyle(textColor).lineLimit(1)
                    .truncationMode(.middle)
                    .padding(.bottom, 4)
            }
            HStack(spacing: 0) {
                PlayButton(
                    playing: current && player.playing, loading: current && player.loading,
                    background: p.circle, tint: p.icon,
                    upload: uploading ? m.progress : nil,
                    download: shownInfo == nil && !auto
                ) { voice.toggle(m) }
                Waveform(levels: shownInfo?.waveform, progress: progress, played: p.played, unplayed: p.unplayed) {
                    voice.seek(m, fraction: $0)
                }
                .padding(.leading, 12)
                .padding(.trailing, 8)
            }
            HStack(spacing: 0) {
                Text(Format.clock(active ? duration - position : duration))
                    .font(.system(size: 12).monospacedDigit())
                    .foregroundStyle(metaColor)
                if active {
                    SpeedChip(speed: player.speed, color: p.played, background: p.chip) { player.cycleSpeed() }
                        .padding(.leading, 16)
                }
                if current, let error = player.error {
                    Text(error).font(.system(size: 12)).foregroundStyle(metaColor).lineLimit(1)
                        .padding(.leading, 8)
                }
                Spacer(minLength: 8)
                if let meta { meta }
            }
            .frame(minHeight: 20)
            .padding(.leading, 4)
            .padding(.top, 4)
        }
        .frame(width: voiceWidth)
        .task(id: "\(key)|\(auto)") {
            guard shownInfo == nil, auto else { return }
            info = await voice.loadInfo(m)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Sprachnachricht \(Format.clock(duration))")
        .accessibilityIdentifier("voice-bubble")
    }
}

/// Blinking red dot of a running recording.
struct RecDot: View {
    @State private var dim = false

    var body: some View {
        Circle()
            .fill(Talk.danger)
            .frame(width: 10, height: 10)
            .opacity(dim ? 0.25 : 1)
            .onAppear {
                withAnimation(.easeInOut(duration: 0.6).repeatForever(autoreverses: true)) { dim = true }
            }
    }
}

struct LevelMeter: View {
    let levels: [Float]
    let color: Color

    var body: some View {
        Canvas { ctx, size in
            let w: CGFloat = 2, gap: CGFloat = 2
            let n = Int(size.width / (w + gap))
            let shown = levels.suffix(n)
            for (i, v) in shown.enumerated() {
                let h = 3 + CGFloat(v) * (size.height - 3)
                let x = size.width - CGFloat(shown.count - i) * (w + gap)
                ctx.fill(Path(roundedRect: CGRect(x: x, y: (size.height - h) / 2, width: w, height: h), cornerRadius: 1),
                         with: .color(color))
            }
        }
        .frame(height: 24)
    }
}

/// Inside the input row while the finger holds the mic: ● time … ‹ slide to cancel.
struct HeldRecording: View {
    let rec: RecordingUi
    let dragX: CGFloat

    var body: some View {
        HStack(spacing: 0) {
            RecDot()
            Text(Format.clock(rec.durationMs))
                .font(.system(size: 16).monospacedDigit())
                .foregroundStyle(Talk.onSurface)
                .padding(.leading, 8)
                .fixedSize()
                .accessibilityIdentifier("recording-held")
            ZStack(alignment: .trailing) {
                Color.clear
                Text("‹ Zum Abbrechen wischen")
                    .font(.system(size: 14))
                    .foregroundStyle(Talk.muted)
                    .lineLimit(1)
                    .fixedSize()
                    .offset(x: min(0, dragX))
                    .opacity(Double(min(1, max(0.2, 1 + dragX / 300))))
            }
            .clipped()
            .padding(.leading, 12)
        }
        .frame(minHeight: 40)
        .padding(.leading, 16)
        .padding(.trailing, 8)
    }
}

/// Floating hint above the mic while holding: slide up to lock.
struct LockHint: View {
    let lift: CGFloat

    var body: some View {
        VStack(spacing: 6) {
            Image(systemName: "lock").font(.system(size: 18))
            Image(systemName: "chevron.up").font(.system(size: 13, weight: .semibold))
        }
        .foregroundStyle(Talk.muted)
        .padding(.horizontal, 10)
        .padding(.vertical, 12)
        .background(Talk.surfaceVariant, in: RoundedRectangle(cornerRadius: 20))
        .offset(y: max(-100, min(0, lift)))
    }
}

/// Mic in the input row: press and hold to record, slide left to cancel, slide up to lock.
struct MicButton: View {
    let onPress: () -> Bool
    let onDrag: (CGSize) -> Void
    let onCancel: () -> Void
    let onLock: () -> Void
    let onRelease: () -> Void
    let cancelDistance: CGFloat

    @State private var active = false
    @State private var done = false
    private let lockDistance: CGFloat = 110

    var body: some View {
        Image(systemName: "mic")
            .font(.system(size: 21))
            .foregroundStyle(Talk.muted)
            .frame(width: 40, height: 40)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0, coordinateSpace: .global)
                    .onChanged { g in
                        if !active && !done {
                            done = !onPress()
                            active = !done
                        }
                        guard active else { return }
                        onDrag(g.translation)
                        if g.translation.width < -cancelDistance {
                            active = false
                            done = true
                            onDrag(.zero)
                            onCancel()
                        } else if g.translation.height < -lockDistance {
                            active = false
                            done = true
                            onDrag(.zero)
                            onLock()
                        }
                    }
                    .onEnded { _ in
                        if active {
                            onDrag(.zero)
                            onRelease()
                        }
                        active = false
                        done = false
                    }
            )
            .accessibilityLabel("Sprachnachricht – gedrückt halten zum Aufnehmen")
            .accessibilityIdentifier("mic")
    }
}

/// Hands-free recording (slid up): delete · ● time + level · send.
struct LockedRecordingBar: View {
    let rec: RecordingUi
    let onCancel: () -> Void
    let onSend: () -> Void

    var body: some View {
        VoiceBarFrame(onDelete: onCancel, deleteLabel: "Aufnahme verwerfen", onSend: onSend) {
            RecDot()
            Text(Format.clock(rec.durationMs))
                .font(.system(size: 16).monospacedDigit())
                .foregroundStyle(Talk.onSurface)
                .padding(.leading, 8)
                .accessibilityIdentifier("recording-locked")
            LevelMeter(levels: rec.levels, color: Talk.muted).padding(.leading, 10)
            if rec.warn {
                let left = max(0, (60 * 60 * 1000 - rec.durationMs + 59_999) / 60_000)
                Text("Noch \(left) Min.").font(.system(size: 13)).foregroundStyle(Talk.danger).padding(.leading, 8)
            }
        }
    }
}

/// An unsent recording: delete · play + waveform + duration · send.
struct VoiceDraftBar: View {
    let draft: VoiceDraft
    let onDelete: () -> Void
    let onSend: () -> Void

    private var voice: VoiceManager { SessionStore.shared.voice }

    var body: some View {
        let key = voice.draftKey(draft)
        let player = voice.player
        let current = player.isCurrent(key)
        let duration = Int64(draft.durationMs)
        let position = current ? player.positionMs : 0
        let progress = duration > 0 ? min(1, max(0, Double(position) / Double(duration))) : 0
        VoiceBarFrame(onDelete: onDelete, deleteLabel: "Entwurf löschen", onSend: onSend, leading: 2) {
            PlayButton(playing: current && player.playing, loading: current && player.loading,
                       background: Talk.surface, tint: Talk.onSurface, upload: nil) { voice.toggleDraft(draft) }
            Waveform(levels: draft.waveform, progress: progress, played: Talk.onSurface,
                     unplayed: Talk.onSurface.opacity(0.3)) { voice.seekDraft(draft, fraction: $0) }
                .padding(.horizontal, 10)
            Text(Format.clock(current && position > 0 ? duration - position : duration))
                .font(.system(size: 13).monospacedDigit())
                .foregroundStyle(Talk.onSurface)
                .accessibilityIdentifier("voice-draft")
        }
    }
}

private struct VoiceBarFrame<Content: View>: View {
    let onDelete: () -> Void
    let deleteLabel: String
    let onSend: () -> Void
    var leading: CGFloat = 16
    @ViewBuilder let content: () -> Content

    var body: some View {
        HStack(spacing: 0) {
            Button(action: onDelete) {
                Image(systemName: "trash").font(.system(size: 19)).foregroundStyle(Talk.muted).frame(width: 44, height: 44)
            }
            .accessibilityLabel(deleteLabel)
            HStack(spacing: 0, content: content)
                .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                .padding(.leading, leading)
                .padding(.trailing, 14)
                .background(Talk.surfaceVariant, in: Capsule())
            Button(action: onSend) {
                Image(systemName: "arrow.up")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(.white)
                    .frame(width: 36, height: 36)
                    .background(Talk.accent, in: Circle())
            }
            .padding(.leading, 10)
            .accessibilityLabel("Sprachnachricht senden")
            .accessibilityIdentifier("voice-send")
        }
        .padding(.leading, 4)
        .padding(.trailing, 12)
        .padding(.vertical, 6)
    }
}
