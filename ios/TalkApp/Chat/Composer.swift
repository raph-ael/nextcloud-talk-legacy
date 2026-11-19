import SwiftUI

/// Input row like Signal iOS: [+] [ text ] [camera] [mic] – the send button replaces camera
/// and mic once there is text. Voice: hold the mic (slide left cancels, up locks); a draft or
/// a locked recording replaces the whole row.
/// Editing (docs/chat.md "Bearbeiten und Löschen"): the edit bar takes the reply bar's place,
/// the field holds the message's text, send becomes a check mark; draft and reply stay put.
/// Mentions (docs/mentions.md): an `@` brings up the suggestions above the row; the picks
/// travel beside the text into the draft, the outbox and the edit.
struct Composer: View {
    let token: String
    @Binding var replyTo: MessageView?
    @Binding var editing: EditTarget?
    @Binding var panelOpen: Bool
    /// The suggestions are up (back closes them first).
    @Binding var suggesting: Bool
    /// Bumps when back closes the suggestions for this `@`.
    var closeSuggestions = 0
    let onSend: (String, [MentionPick]) -> Void
    /// Saves the edited text; the caller ends the mode on success.
    let onSaveEdit: (String, [MentionPick]) async -> Void
    let onOpenCamera: () -> Void
    let onVoiceSent: () -> Void

    @State private var text = ""
    @State private var picks: [MentionPick] = []
    @State private var selection = NSRange(location: 0, length: 0)
    @State private var caretSerial = 0
    @State private var saving = false
    @State private var drag: CGSize = .zero
    @State private var focused = false
    @State private var trigger: MentionText.Trigger?
    @State private var candidates: [MentionCandidate] = []
    @State private var askSerial = 0
    /// Closed with back for this `@` and search text: stays closed until the text changes.
    @State private var closedFor: MentionText.Trigger?

    private var store: SessionStore { SessionStore.shared }
    private var voice: VoiceManager { store.voice }

    var body: some View {
        let recording = voice.recording(for: token)
        let draft = recording == nil ? voice.draft(token) : nil
        VStack(spacing: 0) {
            if let e = editing {
                EditBar(message: e.msg) { editing = nil }
            } else if let r = replyTo, recording == nil {
                ReplyBar(reply: r) { replyTo = nil }
            }
            // One branch for the input row in both modes, so the field keeps its focus.
            if editing == nil, let rec = recording, rec.locked {
                LockedRecordingBar(rec: rec, onCancel: { voice.cancel() }) {
                    voice.sendRecording(replyTo: replyTo) { sent() }
                }
            } else if editing == nil, let draft {
                VoiceDraftBar(draft: draft, onDelete: { voice.discardDraft(token) }) {
                    voice.sendDraft(token, replyTo: replyTo) { sent() }
                }
            } else {
                inputRow(held: editing == nil ? recording : nil)
            }
        }
        .background(Talk.background)
        // Over the message list, which does not move (docs/mentions.md "Android und iOS").
        .overlay(alignment: .top) {
            if showList {
                // Its bottom on the top edge of the row.
                MentionSuggestions(candidates: candidates, token: token, onPick: accept)
                    .offset(y: -MentionSuggestions.height(candidates.count))
            }
        }
        .onAppear { if editing == nil { load(store.textDraft(token)) } }
        // The edited text never becomes the draft: the draft stays as it was and comes back.
        .onChange(of: TextDraft(text: text, mentions: picks)) { _, d in if editing == nil { store.setTextDraft(token, d) } }
        .onChange(of: editing) { _, e in
            saving = false
            if let e {
                load(TextDraft(text: e.source, mentions: e.mentions))
                panelOpen = false
                Task { @MainActor in focused = true }
            } else {
                load(store.textDraft(token))
            }
        }
        .onChange(of: focused) { _, f in if f { panelOpen = false } }
        .onChange(of: panelOpen) { _, open in if open { focused = false } }
        .onChange(of: text) { ask() }
        .onChange(of: selection) { ask() }
        .onChange(of: showList) { _, up in suggesting = up }
        .onChange(of: closeSuggestions) { closedFor = trigger }
        .onDisappear { suggesting = false }
    }

    /// Visible while there are suggestions and the field has the focus, unless back closed
    /// them for this search text.
    private var showList: Bool {
        !candidates.isEmpty && focused && trigger != nil && trigger != closedFor
    }

    /// Text and picks into the field, the caret at the end.
    private func load(_ d: TextDraft) {
        text = d.text
        picks = d.mentions
        selection = NSRange(location: (d.text as NSString).length, length: 0)
        caretSerial += 1
    }

    /// On every change of text or caret, without waiting; answers for an older state are dropped.
    private func ask() {
        let t = MentionText.trigger(text, selection, picks)
        trigger = t
        askSerial += 1
        guard let t else {
            candidates = []
            return
        }
        let serial = askSerial
        Task {
            let found = await store.mentionCandidates(token, t.query)
            guard serial == askSerial else { return }
            candidates = found
        }
    }

    /// "Übernehmen" (docs/mentions.md): `@{label} ` in place of the search text, the pick joins.
    private func accept(_ c: MentionCandidate) {
        guard let t = trigger else { return }
        let (new, caret) = MentionText.accept(text, t, caret: selection.location, label: c.label)
        picks = MentionText.adding(MentionPick(label: c.label, mentionId: c.mentionId), to: picks)
        text = new
        selection = NSRange(location: caret, length: 0)
        caretSerial += 1
        // Closed for this "@" as after back, until the search text changes: "Lena " would
        // bring it up again when there is also "Lena Brandt" (docs/mentions.md "Übernehmen").
        closedFor = MentionText.trigger(new, selection, picks)
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
                // While editing only ＋ goes: the field keeps its identity and so its focus.
                if editing == nil {
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
                }
                field
                    .padding(.leading, editing != nil ? 4 : 0)
            }
            if let e = editing {
                saveButton(e)
            } else if hasText && held == nil {
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

    private var field: some View {
        ComposerField(text: $text, selection: $selection, focused: $focused, picks: picks, caretSerial: caretSerial)
            .background(Talk.surfaceVariant, in: RoundedRectangle(cornerRadius: 20))
    }

    /// Check mark: off while saving, while empty and while unchanged (both trimmed).
    private func saveButton(_ e: EditTarget) -> some View {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let can = !saving && !t.isEmpty && t != e.source.trimmingCharacters(in: .whitespacesAndNewlines)
        return Button {
            saving = true
            Task {
                await onSaveEdit(t, picks)
                saving = false
            }
        } label: {
            Image(systemName: "checkmark")
                .font(.system(size: 16, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 34, height: 34)
                .background(can ? Talk.accent : Talk.muted.opacity(0.4), in: Circle())
                .frame(width: 40, height: 40)
        }
        .disabled(!can)
        .accessibilityLabel("Änderung speichern")
        .accessibilityIdentifier("save-edit")
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
        onSend(t, picks)
        load(TextDraft())
    }
}

/// The message being edited, the text the field starts with and its mentions (`editSource()`).
struct EditTarget: Equatable {
    let msg: MessageView
    let source: String
    let mentions: [MentionPick]
}

/// Like the reply bar: pencil and "Nachricht bearbeiten" in the accent colour, the old text
/// (a file without caption: its name) below, ✕ cancels.
private struct EditBar: View {
    let message: MessageView
    let onCancel: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            RoundedRectangle(cornerRadius: 2).fill(Talk.accent).frame(width: 3)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 5) {
                    Image(systemName: "pencil").font(.system(size: 13, weight: .semibold)).accessibilityHidden(true)
                    Text("Nachricht bearbeiten").font(.system(size: 13, weight: .semibold))
                }
                .foregroundStyle(Talk.accent)
                Text(message.text.isEmpty ? message.attachment?.name ?? "" : message.text)
                    .font(.system(size: 13))
                    .foregroundStyle(Talk.muted)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            Button(action: onCancel) {
                Image(systemName: "xmark").font(.system(size: 14, weight: .semibold)).foregroundStyle(Talk.muted)
                    .frame(width: 32, height: 32)
            }
            .accessibilityLabel("Bearbeiten abbrechen")
            .accessibilityIdentifier("edit-cancel")
        }
        .frame(height: 44)
        .padding(.horizontal, 12)
        .padding(.top, 6)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Nachricht bearbeiten")
        .accessibilityIdentifier("edit-bar")
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
                Text(Format.withAttachment(reply.text, reply.attachment, reply.location))
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
