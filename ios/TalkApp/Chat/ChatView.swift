import PhotosUI
import SwiftUI

/// One conversation (port of Android `ChatScreen.kt`): message list, input row, attachment
/// panel, camera, photo send screen and image viewer.
struct ChatView: View {
    @Bindable var store: SessionStore
    let token: String

    @State private var replyTo: MessageView?
    /// Long-pressed message (menu overlay open) and the message whose reactions are listed.
    @State private var menuFor: MessageMenuTarget?
    @State private var reactionsFor: ReactionsTarget?
    @State private var scrollTick = 0
    @State private var atBottom = true
    @State private var showJump = false
    @State private var panelOpen = false
    @State private var camera = false
    @State private var sending: PreparedImage?
    @State private var sendingFromCamera = false
    @State private var viewer: ViewerItem?
    @State private var cameraGallery: PhotosPickerItem?
    @State private var cameraGalleryOpen = false
    @AppStorage("keyboard_height") private var keyboardHeight: Double = 300

    var body: some View {
        let room = store.room(token)
        let chat = store.chats[token]
        let messages = chat?.messages ?? []
        let lastCommonRead = max(chat?.lastCommonRead ?? 0, room?.lastCommonReadMessage ?? 0)
        let footer: MessageList.Footer = chat?.loadingOlder == true || chat?.loading == true
            ? .loading
            : (chat != nil && chat?.hasMore == false && !messages.isEmpty ? .start : .none)

        VStack(spacing: 0) {
            if !store.online { OfflineBanner() }
            RecordingBanners(notices: store.recordingNotices(for: token))
            MessageList(
                messages: messages,
                isGroup: room?.isGroup ?? true,
                lastCommonRead: lastCommonRead,
                footer: footer,
                highlighted: menuFor?.m.key,
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
                        if bottom { store.markVisibleRead(token) }
                    },
                    onFarChanged: { showJump = $0 }
                )
            )
            .background(Talk.background)
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
                    panelOpen: $panelOpen,
                    onSend: { text in
                        store.send(token, text: text, replyTo: replyTo)
                        replyTo = nil
                        scrollTick += 1
                    },
                    onOpenCamera: { camera = true },
                    onVoiceSent: { scrollTick += 1 }
                )
                if panelOpen {
                    AttachPanel(
                        height: CGFloat(keyboardHeight),
                        onImage: { img in
                            sendingFromCamera = false
                            sending = img
                        },
                        onCamera: { camera = true }
                    )
                    .transition(.move(edge: .bottom))
                }
            }
        }
        .background(Talk.background)
        .ignoresSafeArea(.container, edges: panelOpen ? .bottom : [])
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                let title = HStack(spacing: 10) {
                    Avatar(name: room?.name ?? "", size: 32, token: token, version: room?.avatarVersion ?? "")
                    Text(room?.name ?? "").font(.system(size: 17, weight: .semibold)).lineLimit(1)
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
                actions: messageActions(t.m),
                safe: MessageMenuWindow.shared.safeArea,
                onReact: { store.setReaction(token, msg.id, $0) },
                onClose: {
                    MessageMenuWindow.shared.close()
                    menuFor = nil
                }
            )
        )
    }

    private func messageActions(_ m: UiMessage) -> [MenuAction] {
        let msg = m.msg
        var list: [MenuAction] = []
        if m.status == .failed || m.status == .waiting {
            list.append(MenuAction(id: "retry", label: "Erneut senden", icon: "arrow.clockwise") { store.retry(msg) })
        }
        if msg.kind == .text && m.status == nil {
            list.append(MenuAction(id: "reply", label: "Antworten", icon: "arrowshape.turn.up.left") { replyTo = msg })
        }
        if !msg.text.isEmpty && msg.kind != .deleted {
            list.append(MenuAction(id: "copy", label: "Kopieren", icon: "doc.on.doc") { UIPasteboard.general.string = msg.text })
        }
        if m.status != nil {
            list.append(MenuAction(id: "delete", label: "Löschen", icon: "trash", destructive: true) { store.discard(msg) })
        }
        return list
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
