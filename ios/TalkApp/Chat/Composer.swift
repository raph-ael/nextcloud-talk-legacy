import SwiftUI

/// Input row like Signal iOS: [+] [ text ] [camera] [mic] – the send button replaces camera
/// and mic once there is text. Voice: hold the mic (slide left cancels, up locks); a draft or
/// a locked recording replaces the whole row.
struct Composer: View {
    let token: String
    @Binding var replyTo: MessageView?
    @Binding var panelOpen: Bool
    let onSend: (String) -> Void
    let onOpenCamera: () -> Void
    let onVoiceSent: () -> Void

    @State private var text = ""
    @State private var drag: CGSize = .zero
    @FocusState private var focused: Bool

    private var store: SessionStore { SessionStore.shared }
    private var voice: VoiceManager { store.voice }

    var body: some View {
        let recording = voice.recording.flatMap { $0.token == token ? $0 : nil }
        let draft = recording == nil ? voice.draft(token) : nil
        VStack(spacing: 0) {
            if let r = replyTo, recording == nil {
                ReplyBar(reply: r) { replyTo = nil }
            }
            if let rec = recording, rec.locked {
                LockedRecordingBar(rec: rec, onCancel: { voice.cancel() }) {
                    voice.sendRecording(replyTo: replyTo) { sent() }
                }
            } else if let draft {
                VoiceDraftBar(draft: draft, onDelete: { voice.discardDraft(token) }) {
                    voice.sendDraft(token, replyTo: replyTo) { sent() }
                }
            } else {
                inputRow(held: recording)
            }
        }
        .background(Talk.background)
        .onAppear { text = store.textDrafts[token] ?? "" }
        .onChange(of: text) { _, t in store.textDrafts[token] = t }
        .onChange(of: focused) { _, f in if f { panelOpen = false } }
        .onChange(of: panelOpen) { _, open in if open { focused = false } }
    }

    private func sent() {
        replyTo = nil
        onVoiceSent()
    }

    @ViewBuilder
    private func inputRow(held: RecordingUi?) -> some View {
        let hasText = !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        HStack(alignment: .bottom, spacing: 6) {
            if let held {
                HeldRecording(rec: held, dragX: drag.width)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Talk.surfaceVariant, in: Capsule())
            } else {
                Button {
                    withAnimation(.easeOut(duration: 0.2)) { panelOpen.toggle() }
                } label: {
                    Image(systemName: "plus")
                        .font(.system(size: 20, weight: .medium))
                        .foregroundStyle(Talk.onSurface)
                        .rotationEffect(.degrees(panelOpen ? 45 : 0))
                        .frame(width: 40, height: 40)
                }
                .accessibilityLabel(panelOpen ? "Anhänge schließen" : "Anhang")
                .accessibilityIdentifier("attach")

                TextField("Nachricht", text: $text, axis: .vertical)
                    .lineLimit(1...5)
                    .font(.system(size: 16))
                    .textInputAutocapitalization(.sentences)
                    .focused($focused)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(Talk.surfaceVariant, in: RoundedRectangle(cornerRadius: 20))
                    .accessibilityIdentifier("composer")
            }
            if hasText && held == nil {
                Button(action: send) {
                    Image(systemName: "arrow.up")
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(.white)
                        .frame(width: 34, height: 34)
                        .background(Talk.accent, in: Circle())
                        .frame(width: 40, height: 40)
                }
                .accessibilityLabel("Senden")
                .accessibilityIdentifier("send")
            } else {
                if held == nil {
                    Button(action: onOpenCamera) {
                        Image(systemName: "camera")
                            .font(.system(size: 19))
                            .foregroundStyle(Talk.muted)
                            .frame(width: 36, height: 40)
                    }
                    .accessibilityLabel("Kamera")
                    .accessibilityIdentifier("camera")
                }
                MicButton(
                    onPress: press,
                    onDrag: { drag = $0 },
                    onCancel: { voice.cancel() },
                    onLock: { voice.lock() },
                    onRelease: { voice.sendRecording(replyTo: replyTo) { sent() } },
                    cancelDistance: UIScreen.main.bounds.width * 0.4
                )
                .overlay(alignment: .bottom) {
                    if held != nil {
                        LockHint(lift: drag.height)
                            .offset(y: -56)
                            .allowsHitTesting(false)
                    }
                }
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
    }

    /// Finger down on the mic: starts recording when the microphone is allowed.
    private func press() -> Bool {
        guard voice.permissionGranted else {
            Task { _ = await voice.ensurePermission() }
            return false
        }
        focused = false
        panelOpen = false
        return voice.start(token)
    }

    private func send() {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return }
        onSend(t)
        text = ""
    }
}

private struct ReplyBar: View {
    let reply: MessageView
    let onCancel: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            RoundedRectangle(cornerRadius: 2).fill(Talk.accent).frame(width: 3)
            VStack(alignment: .leading, spacing: 1) {
                Text(reply.isOwn ? "Du" : reply.author)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Talk.accent)
                Text(Format.withAttachment(reply.text, reply.attachment))
                    .font(.system(size: 13))
                    .foregroundStyle(Talk.muted)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            Button(action: onCancel) {
                Image(systemName: "xmark").font(.system(size: 14, weight: .semibold)).foregroundStyle(Talk.muted)
                    .frame(width: 32, height: 32)
            }
            .accessibilityLabel("Antwort abbrechen")
        }
        .frame(height: 44)
        .padding(.horizontal, 12)
        .padding(.top, 6)
    }
}
