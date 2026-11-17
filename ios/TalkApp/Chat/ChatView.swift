import PhotosUI
import QuickLook
import SwiftUI

/// One conversation (port of Android `ChatScreen.kt`): message list, input row, attachment
/// panel, camera, photo send screen and image viewer.
struct ChatView: View {
    @Bindable var store: SessionStore
    let token: String

    @State private var replyTo: MessageView?
    /// docs/chat.md "Bearbeiten und Löschen": the message in the input field, the one asked about.
    @State private var editing: EditTarget?
    @State private var deleting: MessageView?
    /// Long-pressed message (menu overlay open) and the message whose reactions are listed.
    @State private var menuFor: MessageMenuTarget?
    @State private var reactionsFor: ReactionsTarget?
    @State private var scrollTick = 0
    @State private var atBottom = true
    @State private var showJump = false
    /// Floating date while scrolling (docs/chat.md, "Datum beim Scrollen").
    @State private var pillDate = ""
    @State private var pillShown = false
    @State private var pillHide: Task<Void, Never>?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var panelOpen = false
    @State private var camera = false
    @State private var sending: PreparedImage?
    @State private var sendingFromCamera = false
    @State private var viewer: ViewerItem?
    @State private var cameraGallery: PhotosPickerItem?
    @State private var cameraGalleryOpen = false
    /// docs/attachments.md: the system's file picker, the picked files, the place sheet.
    @State private var filePicker = false
    @State private var placeSheet = false
    @AppStorage("keyboard_height") private var keyboardHeight: Double = 300

    var body: some View {
        let room = store.room(token)
        let chat = store.chats[token]
        let messages = chat?.messages ?? []
        let lastCommonRead = max(chat?.lastCommonRead ?? 0, room?.lastCommonReadMessage ?? 0)
        let footer: MessageList.Footer = chat?.loadingOlder == true || chat?.loading == true
            ? .loading
            : (chat != nil && chat?.hasMore == false && !messages.isEmpty ? .start : .none)
        let line = store.unreadLine?.token == token ? store.unreadLine : nil
        let unread = line?.place(in: messages)
        let openAt: OpenAt = {
            guard store.activeToken == token, let chat, !chat.loading else { return .wait }
            guard let line else { return .bottom }
            // Only once the server page is in: the cache may not hold the new messages yet.
            guard line.settled else { return .wait }
            if let unread { return .row(unread.row.key) }
            return line.base > 0 ? .oldest : .bottom
        }()
        // Editing ends once the message was deleted meanwhile.
        let editedGone = editing.map { e in
            messages.contains { $0.status == nil && $0.msg.id == e.msg.id && $0.msg.kind == .deleted }
        } ?? false

        VStack(spacing: 0) {
            RecordingBanners(notices: store.recordingNotices(for: token))
            MessageList(
                messages: messages,
                isGroup: room?.isGroup ?? true,
                lastCommonRead: lastCommonRead,
                footer: footer,
                highlighted: menuFor?.m.key,
                unreadKey: unread?.row.key,
                unreadCount: unread?.count ?? 0,
                openAt: openAt,
                scrollToBottom: scrollTick,
                actions: MessageListActions(
                    onReply: { replyTo = $0 },
                    onLongPress: { openMenu($0, canReact: room?.canReact == true) },
                    onShowReactions: { reactionsFor = ReactionsTarget(token: token, messageId: $0.id) },
                    onRetry: { store.retry($0) },
                    onOpenImage: { id, img in viewer = ViewerItem(fileId: id, initial: img) },
                    onNearTop: { store.loadOlder(token) },
                    onBottomChanged: { bottom in
                        atBottom = bottom
                        store.setChatAtBottom(token, bottom)
                    },
                    onFarChanged: { showJump = $0 },
                    onScrollDate: { date in
                        pillHide?.cancel()
                        if let date { pillDate = date }
                        pillShown = date != nil
                    },
                    onScrollEnded: {
                        pillHide?.cancel()
                        pillHide = Task {
                            try? await Task.sleep(for: .milliseconds(400))
                            if !Task.isCancelled { pillShown = false }
                        }
                    }
                )
            )
            // Opened anew in place (a notification of this chat): a fresh list positions again.
            .id(store.openSerial)
            .background(Talk.background)
            .overlay(alignment: .top) {
                if pillShown {
                    DatePill(text: pillDate)
                        .padding(.top, 8)
                        .transition(.opacity)
                }
            }
            .animation(reduceMotion ? nil : .easeOut(duration: 0.1), value: pillShown)
            .overlay(alignment: .bottomTrailing) {
                if showJump {
                    JumpToBottomButton(unread: Int(room?.unreadMessages ?? 0)) { scrollTick += 1 }
                        .padding(.trailing, 14)
                        .padding(.bottom, 14)
                        .transition(.opacity.combined(with: .scale(scale: 0.6)))
                }
            }
            .animation(.easeOut(duration: 0.15), value: showJump)
            if room?.readOnly == true {
                Text("Diese Unterhaltung ist schreibgeschützt.")
                    .foregroundStyle(Talk.muted)
                    .frame(maxWidth: .infinity)
                    .padding(16)
            } else {
                Composer(
                    token: token,
                    replyTo: $replyTo,
                    editing: $editing,
                    panelOpen: $panelOpen,
                    onSend: { text in
                        store.send(token, text: text, replyTo: replyTo)
                        replyTo = nil
                        scrollTick += 1
                    },
                    onSaveEdit: saveEdit,
                    onOpenCamera: { camera = true },
                    onVoiceSent: {
                        store.clearUnreadLine(token)
                        scrollTick += 1
                    }
                )
                if panelOpen {
                    AttachPanel(
                        height: CGFloat(keyboardHeight),
                        onImage: { img in
                            sendingFromCamera = false
                            sending = img
                        },
                        onCamera: { camera = true },
                        onFile: { filePicker = true },
                        onPlace: { placeSheet = true }
                    )
                    .transition(.move(edge: .bottom))
                }
            }
        }
        .background(Talk.background)
        .ignoresSafeArea(.container, edges: panelOpen ? .bottom : [])
        .navigationBarTitleDisplayMode(.inline)
        // The back gesture first ends editing (docs/chat.md "Pro Plattform"): the system back
        // button is swapped for one that cancels, and a swipe from the edge does the same.
        .navigationBarBackButtonHidden(editing != nil)
        .simultaneousGesture(
            DragGesture(minimumDistance: 20).onEnded { g in
                if editing != nil, g.startLocation.x < Self.edgeWidth, g.translation.width > 60,
                   abs(g.translation.height) < g.translation.width {
                    editing = nil
                }
            },
            including: editing != nil ? .all : .subviews
        )
        .onChange(of: editedGone) { _, gone in if gone { editing = nil } }
        .onChange(of: store.activeToken) { _, t in if t != token { editing = nil } }
        .onChange(of: store.openSerial) {
            // The new list reports where it stands; what the old one showed goes.
            showJump = false
            pillShown = false
        }
        .confirmationDialog(
            "Nachricht löschen?",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible,
            presenting: deleting
        ) { m in
            Button("Löschen", role: .destructive) { store.deleteMessage(token, m.id) }
            Button("Abbrechen", role: .cancel) {}
        } message: { m in
            Text(Self.deleteQuestion(m))
        }
        .toolbar {
            if editing != nil {
                ToolbarItem(placement: .topBarLeading) {
                    Button { editing = nil } label: {
                        Image(systemName: "chevron.left")
                            .font(.system(size: 17, weight: .semibold))
                            .foregroundStyle(Talk.onSurface)
                    }
                    .accessibilityLabel("Zurück")
                    .accessibilityIdentifier("edit-back")
                }
            }
            ToolbarItem(placement: .principal) {
                // docs/connection.md "Chat": spinner beside the name (its place is kept, nothing
                // moves), "Keine Verbindung" under it once offline.
                let shown = store.connectionShown
                let title = HStack(spacing: 10) {
                    Avatar(name: room?.name ?? "", size: 32, token: token, version: room?.avatarVersion ?? "")
                    VStack(alignment: .leading, spacing: 0) {
                        HStack(spacing: 6) {
                            Text(room?.name ?? "").font(.system(size: 17, weight: .semibold)).lineLimit(1)
                            ConnectingSpinner()
                                .opacity(shown == .connecting ? 1 : 0)
                                .accessibilityHidden(shown != .connecting)
                        }
                        if shown == .offline {
                            Text("Keine Verbindung")
                                .font(.system(size: 12))
                                .foregroundStyle(Talk.muted)
                                .lineLimit(1)
                        }
                    }
                }
                if let room, GroupInfoView.applies(room) {
                    // Groups: the header opens the group settings (docs/groups.md).
                    Button { store.showGroupInfo(true) } label: { title.foregroundStyle(Talk.onSurface) }
                        .buttonStyle(.plain)
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel("Gruppeneinstellungen")
                        .accessibilityValue(room.name)
                        .accessibilityAddTraits(.isButton)
                        .accessibilityIdentifier("chat-title")
                } else {
                    title
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("chat-title")
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                let inCall = CallController.shared.call?.token == token
                Button {
                    CallController.shared.start(token: token, name: room?.name ?? "")
                } label: {
                    Image(systemName: room?.hasCall == true || inCall ? "phone.fill" : "phone")
                        .foregroundStyle(room?.hasCall == true || inCall ? Talk.callDot : Talk.accent)
                }
                .accessibilityLabel(room?.hasCall == true ? "Anruf beitreten" : "Anrufen")
                .accessibilityIdentifier("call-button")
            }
        }
        .toolbarBackground(Talk.background, for: .navigationBar)
        .onChange(of: messages.count) { _, _ in
            if atBottom { store.markVisibleRead(token) }
        }
        .onChange(of: room?.unreadMessages) { _, _ in
            if atBottom { store.markVisibleRead(token) }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { n in
            // The panel takes the keyboard's place at its height (it ignores the bottom safe area too).
            if let frame = n.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect, frame.height > 200 {
                keyboardHeight = Double(frame.height)
            }
        }
        .sheet(item: $reactionsFor) { WhoReactedSheet(target: $0) }
        .onDisappear {
            if menuFor != nil {
                MessageMenuWindow.shared.close()
                menuFor = nil
            }
        }
        .fullScreenCover(isPresented: $camera) {
            CameraView(
                onClose: { camera = false },
                onCaptured: { img in
                    sendingFromCamera = true
                    camera = false
                    sending = img
                },
                onGallery: { cameraGalleryOpen = true }
            )
            .photosPicker(isPresented: $cameraGalleryOpen, selection: $cameraGallery, matching: .images)
        }
        .onChange(of: cameraGallery) { _, item in
            guard let item else { return }
            cameraGallery = nil
            Task {
                guard let data = try? await item.loadTransferable(type: Data.self),
                      let img = await Task.detached(operation: { ImagePrep.prepare(data: data) }).value else {
                    Toast.show("Bild konnte nicht gelesen werden")
                    return
                }
                camera = false
                sendingFromCamera = false
                sending = img
            }
        }
        .fullScreenCover(item: $sending) { img in
            MediaSendView(
                image: img,
                roomName: room?.name ?? "",
                onClose: {
                    try? FileManager.default.removeItem(at: img.url)
                    sending = nil
                    if sendingFromCamera {
                        Task {
                            try? await Task.sleep(for: .milliseconds(350))
                            camera = true
                        }
                    }
                },
                onSend: { caption in
                    store.sendImage(token, image: img, caption: caption, replyTo: replyTo)
                    replyTo = nil
                    sending = nil
                    panelOpen = false
                    scrollTick += 1
                }
            )
        }
        .fullScreenCover(item: $viewer) { item in
            ImageViewer(item: item) { viewer = nil }
        }
        .modifier(AttachmentSheets(
            store: store, token: token, replyTo: $replyTo, panelOpen: $panelOpen,
            scrollTick: $scrollTick, filePicker: $filePicker, placeSheet: $placeSheet
        ))
        .onAppear { store.markVisibleRead(token) }
    }

    /// Long press: reaction bar (when allowed) above the message, the actions below.
    private func openMenu(_ t: MessageMenuTarget, canReact: Bool) {
        menuFor = t
        let msg = t.m.msg
        MessageMenuWindow.shared.show(
            MessageMenuOverlay(
                target: t,
                showBar: canReact && Reactions.reactable(t.m),
                actions: messageActions(t.m, room: store.room(token)),
                safe: MessageMenuWindow.shared.safeArea,
                onReact: { store.setReaction(token, msg.id, $0) },
                onClose: {
                    MessageMenuWindow.shared.close()
                    menuFor = nil
                }
            )
        )
    }

    /// Strip at the left edge where a drag is the back gesture, not "swipe to reply".
    static let edgeWidth: CGFloat = 24

    /// Worked out when the menu opens (docs/chat.md "Bearbeiten und Löschen"): edit and delete
    /// only for delivered messages, as the core's rights say; in the outbox "Löschen" discards.
    private func messageActions(_ m: UiMessage, room: RoomView?) -> [MenuAction] {
        let msg = m.msg
        let delivered = m.status == nil
        let canChat = room?.canChat == true
        let canEdit = delivered && msg.editable && canChat
            && (msg.editUntil.map { Int64(Date().timeIntervalSince1970) < $0 } ?? true)
        let canDelete = delivered && msg.deletable
            && (msg.isOwn ? canChat : room?.canDeleteOthers == true)
        var list: [MenuAction] = []
        if m.status == .failed || m.status == .waiting {
            list.append(MenuAction(id: "retry", label: "Erneut senden", icon: "arrow.clockwise") { store.retry(msg) })
        }
        if msg.kind == .text && delivered {
            list.append(MenuAction(id: "reply", label: "Antworten", icon: "arrowshape.turn.up.left") { replyTo = msg })
        }
        if canEdit {
            list.append(MenuAction(id: "edit", label: "Bearbeiten", icon: "pencil") { startEdit(msg) })
        }
        if let place = msg.location, msg.kind != .deleted {
            list.append(MenuAction(id: "copy", label: "Kopieren", icon: "doc.on.doc") {
                UIPasteboard.general.string = Places.copyText(place)
            })
        } else if !msg.text.isEmpty && msg.kind != .deleted {
            list.append(MenuAction(id: "copy", label: "Kopieren", icon: "doc.on.doc") { UIPasteboard.general.string = msg.text })
        }
        if !delivered {
            list.append(MenuAction(id: "delete", label: "Löschen", icon: "trash", destructive: true) { store.discard(msg) })
        } else if canDelete {
            list.append(MenuAction(id: "delete", label: "Löschen", icon: "trash", destructive: true) { deleting = msg })
        }
        return list
    }

    private func startEdit(_ msg: MessageView) {
        Task {
            let source = await store.editSource(token, msg.id) ?? msg.text
            editing = EditTarget(msg: msg, source: source)
        }
    }

    /// On an error the mode stays open with the text, and the toast says why.
    private func saveEdit(_ text: String) async {
        guard let e = editing else { return }
        if let error = await store.editMessage(token, e.msg.id, text) {
            Toast.show(error)
        } else if editing?.msg.id == e.msg.id {
            editing = nil
        }
    }

    static func deleteQuestion(_ m: MessageView) -> String {
        var text = m.isOwn
            ? "Sie wird für alle im Chat gelöscht."
            : "Die Nachricht von \(m.author) wird für alle im Chat gelöscht."
        if m.attachment != nil { text += " Die Datei selbst bleibt erhalten." }
        return text
    }
}

/// The date of the topmost visible message while scrolling: decoration only, so hidden from
/// VoiceOver (the day dividers are headings).
private struct DatePill: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(Talk.muted)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(Talk.floating, in: Capsule())
            .shadow(color: .black.opacity(0.18), radius: 4, y: 1)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

/// Round floating button bottom right of the message list (docs/chat.md, "Zum Ende springen"):
/// shown once the list is more than half its height away from the newest message.
private struct JumpToBottomButton: View {
    let unread: Int
    let action: () -> Void

    private var label: String {
        switch unread {
        case ..<1: "Zum Ende springen"
        case 1: "Zum Ende springen, 1 neue Nachricht"
        default: "Zum Ende springen, \(unread) neue Nachrichten"
        }
    }

    var body: some View {
        Button(action: action) {
            Image(systemName: "chevron.down")
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(Talk.muted)
                .frame(width: 40, height: 40)
                .background(Talk.floating, in: Circle())
                .shadow(color: .black.opacity(0.18), radius: 4, y: 1)
                .overlay(alignment: .top) {
                    if unread > 0 {
                        Text(unread > 99 ? "99+" : "\(unread)")
                            .font(.system(size: 11, weight: .bold))
                            .monospacedDigit()
                            .foregroundStyle(.white)
                            .padding(.horizontal, 5)
                            .frame(minWidth: 18, minHeight: 18)
                            .background(Talk.accent, in: Capsule())
                            .offset(y: -9)
                    }
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityIdentifier("jump-to-bottom")
    }
}

/// docs/attachments.md: the file picker, the "Datei senden" screen, the place sheet and the
/// QuickLook preview of received files.
private struct AttachmentSheets: ViewModifier {
    let store: SessionStore
    let token: String
    @Binding var replyTo: MessageView?
    @Binding var panelOpen: Bool
    @Binding var scrollTick: Int
    @Binding var filePicker: Bool
    @Binding var placeSheet: Bool
    @State private var files: FileBatch?

    func body(content: Content) -> some View {
        content
            .fileImporter(isPresented: $filePicker, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
                guard case .success(let urls) = result, !urls.isEmpty else { return }
                Task {
                    let read = await Task.detached { urls.map { PickedFile.read($0) } }.value
                    if read.contains(where: { $0 == nil }) { Toast.show("Datei konnte nicht gelesen werden") }
                    let ok = read.compactMap { $0 }
                    if !ok.isEmpty { files = FileBatch(files: ok) }
                }
            }
            .sheet(item: $files) { batch in
                FileSendView(
                    files: batch.files,
                    onClose: { files = nil },
                    onSend: { list, caption in
                        let failed = await store.sendFiles(token, files: list, caption: caption, replyTo: replyTo)
                        if failed.isEmpty {
                            replyTo = nil
                            files = nil
                            panelOpen = false
                            scrollTick += 1
                        }
                        return failed
                    }
                )
            }
            .sheet(isPresented: $placeSheet) {
                PlaceSendSheet(
                    onSend: { place in
                        store.sendLocation(token, place)
                        placeSheet = false
                        panelOpen = false
                        scrollTick += 1
                    },
                    onClose: { placeSheet = false }
                )
                .presentationDetents([.height(420)])
                .presentationDragIndicator(.visible)
            }
            .quickLookPreview(Binding(get: { FileOpener.shared.preview }, set: { FileOpener.shared.preview = $0 }))
    }
}
