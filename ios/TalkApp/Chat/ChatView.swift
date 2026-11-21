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
    @State private var contact: PickedContact?
    /// docs/search.md "Suche im Chat": the field in the header, its hits and the chosen one
    /// (0 = newest).
    @State private var chatSearch: MessageSearch?
    @State private var searchText = ""
    @State private var hitIndex = 0
    @State private var hitsFor = ""
    @State private var lastHit: Int64?
    @FocusState private var searchFocused: Bool
    /// The row lit up after a jump (docs/search.md "Aufleuchten").
    @State private var flashKey: String?
    @State private var flashOff: Task<Void, Never>?
    /// docs/mentions.md: the suggestions are up (back closes them first), and the person a
    /// tapped mention means while "Chat mit … beginnen?" asks.
    @State private var suggesting = false
    @State private var closeSuggestions = 0
    @State private var startChat: MentionView?
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
            if let target = chat.openAt { return .message("id:\(target)") }
            guard let line else { return .bottom }
            // Only once the server page is in: the cache may not hold the new messages yet.
            guard line.settled else { return .wait }
            if let unread { return .row(unread.row.key) }
            return line.base > 0 ? .oldest : .bottom
        }()
        // Editing ends once the message was deleted meanwhile.
        let window = chat?.hasNewer == true
        let searching = chatSearch != nil
        let editedGone = editing.map { e in
            messages.contains { $0.status == nil && $0.msg.id == e.msg.id && $0.msg.kind == .deleted }
        } ?? false

        VStack(spacing: 0) {
            RecordingBanners(notices: store.recordingNotices(for: token))
            MessageList(
                // Opening at a message: the chat's own page stays hidden until the window is in.
                messages: chat?.opening == true ? [] : messages,
                isGroup: room?.isGroup ?? true,
                lastCommonRead: lastCommonRead,
                footer: footer,
                highlighted: menuFor?.m.key,
                unreadKey: unread?.row.key,
                unreadCount: unread?.count ?? 0,
                openAt: openAt,
                window: window,
                focus: store.focus?.token == token ? store.focus : nil,
                flashKey: flashKey,
                marks: searching ? SearchText.words(searchText) : [],
                mentions: chat?.mentions ?? [],
                scrollToBottom: scrollTick,
                actions: MessageListActions(
                    onReply: { replyTo = $0 },
                    onLongPress: { openMenu($0, canReact: room?.canReact == true) },
                    onShowReactions: { reactionsFor = ReactionsTarget(token: token, messageId: $0.id) },
                    onRetry: { store.retry($0) },
                    onOpenImage: { id, img in viewer = ViewerItem(fileId: id, initial: img) },
                    onNearTop: { store.loadOlder(token) },
                    onNearBottom: { store.loadNewer(token) },
                    onQuote: { store.jump(token, to: $0) },
                    onMention: openMention,
                    onSeen: { store.mentionsSeen(token, $0) },
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
                // docs/mentions.md "„@“-Knopf im Chat": 16 above "Zum Ende springen", in a fixed
                // place also while that one is away; not while searching.
                VStack(spacing: 16) {
                    if let mentions = chat?.mentions, let oldest = mentions.first, !searching {
                        MentionJumpButton(count: mentions.count) {
                            store.jump(token, to: oldest)
                            store.mentionsSeen(token, [oldest])
                        }
                        .transition(.opacity.combined(with: .scale(scale: 0.6)))
                    }
                    // In a window it is always there and leads back to the end (docs/search.md).
                    if showJump || window {
                        let unread = Int(room?.unreadMessages ?? 0)
                        JumpToBottomButton(unread: window ? max(unread, chat?.heldUnread ?? 0) : unread) {
                            store.showEnd(token)
                            scrollTick += 1
                        }
                        .transition(.opacity.combined(with: .scale(scale: 0.6)))
                    } else {
                        Color.clear.frame(width: 40, height: 40).allowsHitTesting(false)
                    }
                }
                .padding(.trailing, 14)
                .padding(.bottom, 14)
            }
            .animation(.easeOut(duration: 0.15), value: showJump || window)
            .animation(.easeOut(duration: 0.15), value: chat?.mentions.isEmpty ?? true)
            bottomBar(room: room, searching: searching)
        }
        .background(Talk.background)
        .ignoresSafeArea(.container, edges: panelOpen ? .bottom : [])
        .navigationBarTitleDisplayMode(.inline)
        // The back gesture first ends editing (docs/chat.md "Pro Plattform"): the system back
        // button is swapped for one that cancels, and a swipe from the edge does the same.
        // The search closes the same way (docs/search.md "Kopf"): back first shows the header again.
        // The suggestions after an "@" close first (docs/mentions.md "Android und iOS").
        .navigationBarBackButtonHidden(editing != nil || searching || suggesting)
        .simultaneousGesture(
            DragGesture(minimumDistance: 20).onEnded { g in
                if editing != nil || searching || suggesting, g.startLocation.x < Self.edgeWidth, g.translation.width > 60,
                   abs(g.translation.height) < g.translation.width {
                    back()
                }
            },
            including: editing != nil || searching || suggesting ? .all : .subviews
        )
        .alert(
            startChat.map { "Chat mit \(MentionMarks.name($0)) beginnen?" } ?? "",
            isPresented: Binding(get: { startChat != nil }, set: { if !$0 { startChat = nil } }),
            presenting: startChat
        ) { m in
            Button("Abbrechen", role: .cancel) {}
            Button("Chat öffnen") { Task { await store.openDirectFromMention(m.id) } }
        }
        .onChange(of: editedGone) { _, gone in if gone { editing = nil } }
        .onChange(of: store.activeToken) { _, t in
            if t != token {
                editing = nil
                closeSearch()
            }
        }
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
        .toolbar { chatToolbar(room: room, searching: searching) }
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
            scrollTick: $scrollTick, filePicker: $filePicker, placeSheet: $placeSheet,
            contact: $contact
        ))
        .onAppear { store.markVisibleRead(token) }
    }

    /// The input row, or while searching the bar with the count and the arrows.
    private func bottomBar(room: RoomView?, searching: Bool) -> some View {
        VStack(spacing: 0) {
            if searching {
                ChatSearchBar(
                    results: chatSearch?.results,
                    selected: hitIndex,
                    onOlder: { selectHit(hitIndex + 1) },
                    onNewer: { selectHit(hitIndex - 1) }
                )
            } else if room?.readOnly == true {
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
                    suggesting: $suggesting,
                    closeSuggestions: closeSuggestions,
                    onSend: { text, mentions in
                        store.send(token, text: text, mentions: mentions, replyTo: replyTo)
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
                        onPlace: { placeSheet = true },
                        onContact: {
                            ContactPicker.present { picked in
                                if let picked { contact = picked } else { Toast.show("Kontakt konnte nicht gelesen werden") }
                            }
                        }
                    )
                    .transition(.move(edge: .bottom))
                }
            }
        }
        .onChange(of: searchText) { _, text in chatSearch?.set(text, session: store.session) }
        .onChange(of: chatSearch?.version) { _, _ in hitsChanged() }
        .onChange(of: store.focus) { _, f in
            guard let f, f.token == token else { return }
            flashKey = f.key
            flashOff?.cancel()
            flashOff = Task {
                try? await Task.sleep(for: .milliseconds(1500))
                if !Task.isCancelled { flashKey = nil }
            }
        }
    }

    @ToolbarContentBuilder
    private func chatToolbar(room: RoomView?, searching: Bool) -> some ToolbarContent {
        if searching {
            ToolbarItem(placement: .topBarLeading) {
                Button { closeSearch() } label: {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(Talk.onSurface)
                }
                .accessibilityLabel("Zurück")
                .accessibilityIdentifier("search-close")
            }
            ToolbarItem(placement: .principal) {
                TextField("Suchen", text: $searchText)
                    .textFieldStyle(.plain)
                    .font(.system(size: 17))
                    .submitLabel(.search)
                    .autocorrectionDisabled()
                    .focused($searchFocused)
                    .padding(.horizontal, 12)
                    // A principal item gets its ideal width only: the field takes the bar.
                    .frame(width: max(200, UIScreen.main.bounds.width - 110), height: 36)
                    .background(Talk.chip, in: Capsule())
                    .task {
                        // Once the field is in the bar, the keyboard opens (docs/search.md "Kopf").
                        try? await Task.sleep(for: .milliseconds(150))
                        searchFocused = true
                    }
                    .accessibilityIdentifier("chat-search-field")
            }
        } else if editing != nil || suggesting {
            ToolbarItem(placement: .topBarLeading) {
                Button { back() } label: {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(Talk.onSurface)
                }
                .accessibilityLabel("Zurück")
                .accessibilityIdentifier(editing != nil ? "edit-back" : "mention-back")
            }
        }
        if !searching { ToolbarItem(placement: .principal) {
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
        } }
        if !searching { ToolbarItem(placement: .topBarTrailing) {
            // docs/search.md "Einstieg": left of the call button.
            Button { openSearch() } label: { Image(systemName: "magnifyingglass") }
                // The edited text lives in the input row, which the search bar replaces.
                .disabled(editing != nil)
                .accessibilityLabel("Im Chat suchen")
                .accessibilityIdentifier("chat-search")
        } }
        if !searching, room?.canCall == true { ToolbarItem(placement: .topBarTrailing) {
            let inCall = CallController.shared.call?.token == token
            Button {
                CallController.shared.start(token: token, name: room?.name ?? "")
            } label: {
                Image(systemName: room?.hasCall == true || inCall ? "phone.fill" : "phone")
                    .foregroundStyle(room?.hasCall == true || inCall ? Talk.callDot : Talk.accent)
            }
            .accessibilityLabel(room?.hasCall == true ? "Anruf beitreten" : "Anrufen")
            .accessibilityIdentifier("call-button")
        } }
    }

    /// Back while something is open in the chat: the suggestions first, then editing, then the search.
    private func back() {
        if suggesting {
            closeSuggestions += 1
        } else if editing != nil {
            editing = nil
        } else {
            closeSearch()
        }
    }

    /// docs/mentions.md "Antippen": the chat with that person at once, or first the question
    /// when there is none yet (a stray tap creates no empty chat).
    private func openMention(_ m: MentionView) {
        guard MentionMarks.tappable(m) else { return }
        if let direct = store.existingDirect(m.id) {
            store.openFromMention(direct)
        } else {
            startChat = m
        }
    }

    // MARK: Search in the chat (docs/search.md)

    private func openSearch() {
        panelOpen = false
        replyTo = nil
        searchText = ""
        hitIndex = 0
        hitsFor = ""
        lastHit = nil
        chatSearch = MessageSearch(token: token, limit: 500)
    }

    /// The chat stays where it is; away from the end it shows "Zum Ende springen".
    private func closeSearch() {
        guard chatSearch != nil else { return }
        chatSearch?.stop()
        chatSearch = nil
        searchText = ""
        searchFocused = false
    }

    /// New hits: for a new text the newest is chosen and the chat jumps to it; the server's
    /// additions to the same text keep the chosen one.
    private func hitsChanged() {
        guard let search = chatSearch else { return }
        guard let r = search.results, !r.hits.isEmpty else {
            hitIndex = 0
            lastHit = nil
            hitsFor = search.query
            return
        }
        if hitsFor == search.query, let id = lastHit, let i = r.hits.firstIndex(where: { $0.messageId == id }) {
            hitIndex = i
            return
        }
        hitsFor = search.query
        selectHit(0)
    }

    private func selectHit(_ i: Int) {
        guard let hits = chatSearch?.results?.hits, hits.indices.contains(i) else { return }
        hitIndex = i
        lastHit = hits[i].messageId
        store.jump(token, to: hits[i].messageId)
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
            // Without a source: the shown text, each mention of the message a pick (docs/mentions.md).
            let source = await store.editSource(token, msg.id)
            editing = EditTarget(
                msg: msg,
                source: source?.text ?? msg.text,
                mentions: source?.mentions ?? msg.mentions.map { MentionPick(label: MentionMarks.name($0), mentionId: $0.id) }
            )
        }
    }

    /// On an error the mode stays open with the text, and the toast says why.
    private func saveEdit(_ text: String, _ mentions: [MentionPick]) async {
        guard let e = editing else { return }
        if let error = await store.editMessage(token, e.msg.id, text, mentions) {
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

/// docs/attachments.md: the file picker, the "Datei senden" screen, the place sheet, the
/// "Kontakt senden" sheet and the QuickLook preview of received files and contacts.
private struct AttachmentSheets: ViewModifier {
    let store: SessionStore
    let token: String
    @Binding var replyTo: MessageView?
    @Binding var panelOpen: Bool
    @Binding var scrollTick: Int
    @Binding var filePicker: Bool
    @Binding var placeSheet: Bool
    @Binding var contact: PickedContact?
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
            .sheet(item: $contact) { c in
                // docs/attachments.md "Senden": with the quote, the reply bar closes.
                ContactSendSheet(
                    contact: c,
                    onSend: { card in
                        store.sendContact(token, card, replyTo: replyTo)
                        replyTo = nil
                        contact = nil
                        panelOpen = false
                        scrollTick += 1
                    },
                    onClose: { contact = nil }
                )
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
            }
            .quickLookPreview(Binding(get: { FileOpener.shared.preview }, set: { FileOpener.shared.preview = $0 }))
    }
}
