import SwiftUI

/// Bars drawn for a voice message (Signal draws a similar count).
private let barCount = 36

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

/// Bars with the played part highlighted; tap or drag to seek.
struct Waveform: View {
    let levels: Data?
    let progress: Double
    let color: Color
    let onSeek: (Double) -> Void

    var body: some View {
        let values = bars(levels, barCount)
        GeometryReader { geo in
            let gap: CGFloat = 2
            let w = max(1, (geo.size.width - gap * CGFloat(barCount - 1)) / CGFloat(barCount))
            Canvas { ctx, size in
                for (i, v) in values.enumerated() {
                    let h = 4 + v * (size.height - 4)
                    let rect = CGRect(x: CGFloat(i) * (w + gap), y: (size.height - h) / 2, width: w, height: h)
                    let played = (Double(i) + 0.5) / Double(barCount) <= progress
                    ctx.fill(Path(roundedRect: rect, cornerRadius: w / 2), with: .color(played ? color : color.opacity(0.35)))
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onEnded { onSeek(Double($0.location.x / max(1, geo.size.width))) }
            )
            .simultaneousGesture(
                DragGesture(minimumDistance: 6)
                    .onChanged { onSeek(Double($0.location.x / max(1, geo.size.width))) }
            )
        }
        .frame(height: 28)
        .accessibilityHidden(true)
    }
}

struct PlayButton: View {
    let playing: Bool
    let loading: Bool
    let background: Color
    let tint: Color
    let upload: Float?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack {
                Circle().fill(background)
                if let upload {
                    Circle().stroke(tint.opacity(0.2), lineWidth: 2.5).frame(width: 34, height: 34)
                    Circle().trim(from: 0, to: CGFloat(max(0.03, upload)))
                        .stroke(tint, style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                        .frame(width: 34, height: 34)
                } else if loading {
                    ProgressView().tint(tint)
                }
                Image(systemName: playing ? "pause.fill" : "play.fill")
                    .font(.system(size: 17))
                    .foregroundStyle(tint)
                    .opacity(loading && upload == nil ? 0 : 1)
            }
            .frame(width: 40, height: 40)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(playing ? "Pause" : "Abspielen")
    }
}

/// Speed chip: 1× → 1,5× → 2× → 0,5×.
struct SpeedChip: View {
    let speed: Float
    let color: Color
    let action: () -> Void

    var body: some View {
        let label = (speed == speed.rounded() ? "\(Int(speed))" : "\(speed)".replacingOccurrences(of: ".", with: ",")) + "×"
        Button(action: action) {
            Text(label)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(color)
                .padding(.horizontal, 6)
                .padding(.vertical, 1)
                .background(color.opacity(0.15), in: RoundedRectangle(cornerRadius: 9))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Tempo \(label)")
    }
}

/// A voice message (or any audio file) inside a bubble.
struct VoiceBubble: View {
    let m: UiMessage
    let textColor: Color
    let own: Bool

    @State private var info: AudioInfo?
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

        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                PlayButton(
                    playing: current && player.playing, loading: current && player.loading,
                    background: own ? .white : Talk.surface,
                    tint: own ? Talk.bubbleOwn : textColor,
                    upload: uploading ? m.progress : nil
                ) { voice.toggle(m) }
                VStack(alignment: .leading, spacing: 0) {
                    Waveform(levels: shownInfo?.waveform, progress: progress, color: textColor) { voice.seek(m, fraction: $0) }
                    HStack(spacing: 6) {
                        Text(Format.clock(active ? duration - position : duration))
                            .font(.system(size: 12).monospacedDigit())
                            .foregroundStyle(textColor.opacity(0.8))
                        if active {
                            SpeedChip(speed: player.speed, color: textColor) { player.cycleSpeed() }
                        }
                        if current, let error = player.error {
                            Text(error).font(.system(size: 12)).foregroundStyle(textColor.opacity(0.8)).lineLimit(1)
                        }
                    }
                    .frame(minHeight: 18)
                }
            }
            if !a.voiceMessage {
                Text(a.name).font(.system(size: 12)).foregroundStyle(textColor.opacity(0.8)).lineLimit(1)
            }
        }
        .frame(width: 240)
        .task(id: "\(key)|\(auto)") {
            guard shownInfo == nil, auto, let path = await voice.localFile(m) else { return }
            info = await voice.info(key, path: path)
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
            Waveform(levels: draft.waveform, progress: progress, color: Talk.onSurface) { voice.seekDraft(draft, fraction: $0) }
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
