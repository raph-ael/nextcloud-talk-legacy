import PhotosUI
import SwiftUI

/// One conversation (port of Android `ChatScreen.kt`): message list, input row, attachment
/// panel, camera, photo send screen and image viewer.
struct ChatView: View {
    @Bindable var store: SessionStore
    let token: String

    @State private var replyTo: MessageView?
    @State private var actionsFor: UiMessage?
    @State private var scrollTick = 0
    @State private var atBottom = true
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
                highlighted: actionsFor?.key,
                scrollToBottom: scrollTick,
                actions: MessageListActions(
                    onReply: { replyTo = $0 },
                    onLongPress: { actionsFor = $0 },
                    onRetry: { store.retry($0) },
                    onOpenImage: { id, img in viewer = ViewerItem(fileId: id, initial: img) },
                    onNearTop: { store.loadOlder(token) },
                    onBottomChanged: { bottom in
                        atBottom = bottom
                        if bottom { store.markVisibleRead(token) }
                    }
                )
            )
            .background(Talk.background)
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
                HStack(spacing: 10) {
                    Avatar(name: room?.name ?? "", size: 32, token: token, version: room?.avatarVersion ?? "")
                    Text(room?.name ?? "").font(.system(size: 17, weight: .semibold)).lineLimit(1)
                }
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("chat-title")
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
        .confirmationDialog("Nachricht", isPresented: Binding(get: { actionsFor != nil }, set: { if !$0 { actionsFor = nil } }),
                            titleVisibility: .hidden, presenting: actionsFor) { m in
            messageActions(m)
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

    @ViewBuilder
    private func messageActions(_ m: UiMessage) -> some View {
        let msg = m.msg
        if m.status == .failed || m.status == .waiting {
            Button("Erneut senden") { store.retry(msg) }
        }
        if msg.kind == .text && m.status == nil {
            Button("Antworten") { replyTo = msg }
        }
        if !msg.text.isEmpty {
            Button("Kopieren") { UIPasteboard.general.string = msg.text }
        }
        if m.status != nil {
            Button("Löschen", role: .destructive) { store.discard(msg) }
        }
    }
}
