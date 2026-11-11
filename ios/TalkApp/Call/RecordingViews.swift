import SwiftUI

/// Call recording (docs/calls.md, "Aufnahme"): the server's recording backend records,
/// the app only starts/stops it, shows that it runs and offers the result afterwards.
enum RecordingText {
    /// Ongoing recording (indicator shows a red dot).
    static func isActive(_ r: CallRecording) -> Bool { r == .audio || r == .video }

    static func isStarting(_ r: CallRecording) -> Bool { r == .startingAudio || r == .startingVideo }

    /// The call screen's indicator text, nil when nothing is shown.
    static func indicator(_ c: CallView) -> String? {
        switch c.recording {
        case .audio, .video: return "Aufnahme läuft"
        case .startingAudio, .startingVideo: return "Aufnahme startet …"
        case .failed: return c.canRecord ? "Aufnahme fehlgeschlagen" : nil
        case .off: return nil
        }
    }

    static let announcement = "Dieser Anruf wird aufgezeichnet"

    static func isFailure(_ k: RecordingNoticeKind) -> Bool {
        switch k {
        case .recordingFailed, .transcriptFailed, .summaryFailed: return true
        case .recording, .transcript, .summary: return false
        }
    }

    static func title(_ n: RecordingNotice) -> String {
        let head: String = switch n.kind {
        case .recording: "Aufnahme verfügbar"
        case .transcript: "Transkript verfügbar"
        case .summary: "Zusammenfassung verfügbar"
        case .recordingFailed: "Aufnahme fehlgeschlagen"
        case .transcriptFailed: "Transkript fehlgeschlagen"
        case .summaryFailed: "Zusammenfassung fehlgeschlagen"
        }
        return n.fileName.isEmpty ? head : "\(head) · \(n.fileName)"
    }

    static func symbol(_ k: RecordingNoticeKind) -> String {
        switch k {
        case .recording: "record.circle"
        case .transcript: "text.quote"
        case .summary: "list.bullet.rectangle"
        case .recordingFailed, .transcriptFailed, .summaryFailed: "exclamationmark.triangle"
        }
    }
}

/// Red dot that pulses gently while recording.
struct RecordingDot: View {
    var size: CGFloat = 8
    var pulsing = true
    @State private var dim = false

    var body: some View {
        Circle()
            .fill(Color(hex: 0xFF3B30))
            .frame(width: size, height: size)
            .opacity(pulsing && dim ? 0.35 : 1)
            .onAppear {
                guard pulsing else { return }
                withAnimation(.easeInOut(duration: 1).repeatForever(autoreverses: true)) { dim = true }
            }
    }
}

/// Top of the call screen: "● Aufnahme läuft" / "Aufnahme startet …" / failure.
struct RecordingIndicator: View {
    let call: CallView

    var body: some View {
        if let text = RecordingText.indicator(call) {
            HStack(spacing: 6) {
                if RecordingText.isActive(call.recording) {
                    RecordingDot()
                } else if call.recording == .failed {
                    Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 11))
                } else {
                    ProgressView().controlSize(.mini).tint(.white)
                }
                Text(text).font(.system(size: 13, weight: .semibold))
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(Color.black.opacity(0.35), in: Capsule())
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("call-recording")
        }
    }
}

/// Finished recordings, transcripts and summaries: in the conversation list (all, with the
/// conversation's name) and in the chat (only its own).
struct RecordingBanners: View {
    let notices: [RecordingNotice]
    var showRoom = false

    var body: some View {
        if !notices.isEmpty {
            VStack(spacing: 6) {
                ForEach(notices, id: \.id) { RecordingBanner(notice: $0, showRoom: showRoom) }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        }
    }
}

struct RecordingBanner: View {
    let notice: RecordingNotice
    let showRoom: Bool
    private var store: SessionStore { .shared }

    var body: some View {
        let failed = RecordingText.isFailure(notice.kind)
        let busy = store.recordingBusy.contains(notice.id)
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: RecordingText.symbol(notice.kind))
                    .font(.system(size: 16))
                    .foregroundStyle(failed ? Talk.danger : Talk.accent)
                    .frame(width: 20)
                VStack(alignment: .leading, spacing: 2) {
                    Text(RecordingText.title(notice))
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(Talk.onSurface)
                        .lineLimit(2)
                    if showRoom {
                        Text(notice.roomName)
                            .font(.system(size: 13))
                            .foregroundStyle(Talk.muted)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 0)
            }
            HStack(spacing: 8) {
                Spacer(minLength: 0)
                if busy { ProgressView().controlSize(.small) }
                if failed {
                    Button("OK") { store.dismissRecording(notice) }
                        .accessibilityValue(notice.fileName)
                        .accessibilityIdentifier("recording-ok")
                } else {
                    Button("Verwerfen") { store.dismissRecording(notice) }
                        .foregroundStyle(Talk.muted)
                        .accessibilityValue(notice.fileName)
                        .accessibilityIdentifier("recording-dismiss")
                    if notice.canShare {
                        Button("In Chat teilen") { store.shareRecording(notice) }
                            .fontWeight(.semibold)
                            .accessibilityValue(notice.fileName)
                            .accessibilityIdentifier("recording-share")
                    }
                }
            }
            .font(.system(size: 14))
            .buttonStyle(.borderless)
            .disabled(busy)
        }
        .padding(12)
        .background(Talk.chip, in: RoundedRectangle(cornerRadius: 12))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("recording-banner")
    }
}
