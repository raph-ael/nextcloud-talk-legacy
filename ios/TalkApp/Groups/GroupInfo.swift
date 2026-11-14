import PhotosUI
import SwiftUI

/// Group settings like Signal (docs/groups.md), pushed over the chat: header with photo,
/// name, description and "Bearbeiten", notifications, members, group link, leave/delete.
/// Everything but the member list comes straight from `RoomView`.
struct GroupInfoView: View {
    @Bindable var store: SessionStore
    let token: String

    /// Settings exist for invite-only groups and groups with a link (types 2 and 3).
    static func applies(_ r: RoomView) -> Bool { r.roomType == 2 || r.roomType == 3 }

    @State private var members: [MemberView]?
    @State private var failed = false
    @State private var retry = 0
    @State private var showAll = false
    @State private var sheet: InfoSheet?
    @State private var confirmLeave = false
    @State private var confirmDelete = false
    @State private var linkOverride: Bool?
    @State private var busy = false

    enum InfoSheet: Identifiable {
        case edit, add, newModerator
        case member(MemberView)

        var id: String {
            switch self {
            case .edit: "edit"
            case .add: "add"
            case .newModerator: "new-moderator"
            case .member(let m): "member:\(m.attendeeId)"
            }
        }
    }

    var body: some View {
        Group {
            if let room = store.room(token) {
                content(room)
            } else {
                Talk.groupedBackground.ignoresSafeArea()
            }
        }
        .navigationTitle("")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Talk.groupedBackground, for: .navigationBar)
        .task(id: "\(store.participantsVersion[token] ?? 0)|\(retry)") { await loadMembers() }
    }

    private func loadMembers() async {
        do {
            let list = try await store.members(token)
            members = list
            failed = false
        } catch {
            if members == nil { failed = true }
        }
    }

    private func content(_ room: RoomView) -> some View {
        List {
            Section { GroupHeader(room: room, count: members?.count) { sheet = .edit } }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))

            Section { notifications(room) }

            Section {
                membersRows(room)
            } header: {
                SettingsHeader(members.map { $0.count == 1 ? "1 Mitglied" : "\($0.count) Mitglieder" } ?? "Mitglieder")
            }

            if room.isModerator || room.isPublic { linkSection(room) }

            Section {
                Button(role: .destructive) { askLeave() } label: {
                    SettingsLabel(icon: "rectangle.portrait.and.arrow.right", text: "Gruppe verlassen", danger: true)
                }
                .accessibilityIdentifier("group-leave")
                if room.canDelete {
                    Button(role: .destructive) { confirmDelete = true } label: {
                        SettingsLabel(icon: "trash", text: "Gruppe löschen", danger: true)
                    }
                    .accessibilityIdentifier("group-delete")
                }
            }
        }
        .listStyle(.insetGrouped)
        .listSectionSpacing(20)
        .scrollContentBackground(.hidden)
        .background(Talk.groupedBackground)
        .disabled(busy)
        .confirmationDialog("Gruppe verlassen?", isPresented: $confirmLeave, titleVisibility: .visible) {
            Button("Verlassen", role: .destructive) { leave(delete: false) }
            Button("Abbrechen", role: .cancel) {}
        } message: {
            Text("Du erhältst keine Nachrichten mehr aus dieser Gruppe.")
        }
        .confirmationDialog("Gruppe für alle löschen?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Löschen", role: .destructive) { leave(delete: true) }
            Button("Abbrechen", role: .cancel) {}
        } message: {
            Text("Alle Nachrichten gehen verloren.")
        }
        .sheet(item: $sheet) { s in
            switch s {
            case .edit:
                EditGroupView(store: store, room: room)
            case .add:
                AddMembersSheet(store: store, token: token)
            case .newModerator:
                NewModeratorSheet(store: store, token: token, members: (members ?? []).filter { !$0.isSelf })
            case .member(let m):
                MemberSheet(store: store, token: token, member: m, canManage: room.isModerator && !m.isOwner)
            }
        }
    }

    // MARK: Notifications

    private static let levels: [(level: UInt8, label: String)] = [
        (1, "Alle Nachrichten"), (2, "Nur Erwähnungen"), (3, "Aus"),
    ]

    private func notifications(_ room: RoomView) -> some View {
        let current = room.notificationLevel
        let label = Self.levels.first { $0.level == current }?.label ?? "Alle Nachrichten"
        return Menu {
            Picker("Benachrichtigungen", selection: Binding(
                get: { current },
                set: { level in Task { await store.groupChange { await $0.setNotificationLevel(token: token, level: level) } } }
            )) {
                ForEach(Self.levels, id: \.level) { Text($0.label).tag($0.level) }
            }
        } label: {
            HStack(spacing: 14) {
                Image(systemName: "bell")
                    .font(.system(size: 17))
                    .foregroundStyle(Talk.muted)
                    .frame(width: 28)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Benachrichtigungen").foregroundStyle(Talk.onSurface).lineLimit(1)
                    Text(label).font(.system(size: 13)).foregroundStyle(Talk.muted).lineLimit(1)
                }
                Spacer(minLength: 8)
                Image(systemName: "chevron.up.chevron.down").font(.system(size: 12)).foregroundStyle(Talk.muted)
            }
            .frame(minHeight: 48)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Benachrichtigungen")
        .accessibilityValue(label)
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier("notifications")
        .listRowBackground(Talk.groupedRow)
    }

    // MARK: Members

    @ViewBuilder
    private func membersRows(_ room: RoomView) -> some View {
        if room.isModerator {
            Button { sheet = .add } label: {
                HStack(spacing: 14) {
                    Image(systemName: "plus")
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(.white)
                        .frame(width: 40, height: 40)
                        .background(Talk.accent, in: Circle())
                    Text("Mitglieder hinzufügen").foregroundStyle(Talk.onSurface)
                    Spacer()
                }
                .frame(minHeight: 52)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("members-add")
            .listRowBackground(Talk.groupedRow)
        }
        if let members {
            let shown = showAll || members.count <= 10 ? members : Array(members.prefix(10))
            ForEach(shown, id: \.attendeeId) { m in
                Button { if !m.isSelf { sheet = .member(m) } } label: {
                    PersonRow(
                        name: m.isSelf ? "Du" : m.name, subline: Self.subline(m),
                        userId: m.actorType == "users" ? m.actorId : ""
                    ) {
                        if m.isModerator || m.isOwner {
                            Text("Moderator").font(.system(size: 14)).foregroundStyle(Talk.muted)
                        }
                    }
                }
                .buttonStyle(.plain)
                .allowsHitTesting(!m.isSelf)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(m.isModerator || m.isOwner ? "\(m.isSelf ? "Du" : m.name), Moderator" : (m.isSelf ? "Du" : m.name))
                .accessibilityAddTraits(m.isSelf ? [] : .isButton)
                .accessibilityIdentifier("member:\(m.isSelf ? "self" : m.name)")
                .listRowBackground(Talk.groupedRow)
            }
            if shown.count < members.count {
                Button("Alle anzeigen") { withAnimation { showAll = true } }
                    .frame(minHeight: 44)
                    .accessibilityIdentifier("members-all")
                    .listRowBackground(Talk.groupedRow)
            }
        } else if failed {
            LoadFailed(text: store.online ? "Konnte nicht geladen werden" : "Keine Verbindung") {
                failed = false
                retry += 1
            }
            .listRowBackground(Talk.groupedRow)
        } else {
            PlaceholderRows().listRowBackground(Talk.groupedRow)
        }
    }

    static func subline(_ m: MemberView) -> String {
        switch m.actorType {
        case "guests": "Gast"
        case "groups": "Nextcloud-Gruppe"
        case "emails": "E-Mail"
        default: ""
        }
    }

    // MARK: Group link

    @ViewBuilder
    private func linkSection(_ room: RoomView) -> some View {
        let on = linkOverride ?? room.isPublic
        Section {
            if room.isModerator {
                Toggle(isOn: Binding(get: { on }, set: setLink)) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Gruppenlink").foregroundStyle(Talk.onSurface)
                        Text("Jeder mit dem Link kann beitreten").font(.system(size: 13)).foregroundStyle(Talk.muted)
                    }
                }
                .tint(Talk.accent)
                .frame(minHeight: 48)
                .accessibilityIdentifier("group-link")
                .listRowBackground(Talk.groupedRow)
            }
            if on {
                let link = store.groupLink(token)
                Text(link)
                    .font(.system(size: 15))
                    .foregroundStyle(Talk.muted)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                    .accessibilityIdentifier("group-link-url")
                    .listRowBackground(Talk.groupedRow)
                Button {
                    UIPasteboard.general.string = link
                    Toast.show("Link kopiert")
                } label: {
                    SettingsLabel(icon: "doc.on.doc", text: "Link kopieren")
                }
                .accessibilityIdentifier("group-link-copy")
                .listRowBackground(Talk.groupedRow)
                if let url = URL(string: link) {
                    ShareLink(item: url) {
                        SettingsLabel(icon: "square.and.arrow.up", text: "Teilen")
                    }
                    .accessibilityIdentifier("group-link-share")
                    .listRowBackground(Talk.groupedRow)
                }
            }
        }
    }

    private func setLink(_ on: Bool) {
        linkOverride = on
        Task {
            await store.groupChange { await $0.setGroupLink(token: token, on: on) }
            linkOverride = nil
        }
    }

    /// The last moderator names a successor first (`leaveNeedsModerator`).
    private func askLeave() {
        busy = true
        Task {
            do {
                if try await store.leaveNeedsModerator(token) { sheet = .newModerator } else { confirmLeave = true }
            } catch {
                Toast.show(store.online ? "Hat nicht geklappt" : "Keine Verbindung")
            }
            busy = false
        }
    }

    private func leave(delete: Bool) {
        busy = true
        Task {
            _ = await store.leaveGroup(token, delete: delete)
            busy = false
        }
    }
}

/// Avatar 80, name, "Gruppe · N Mitglieder", description (3 lines with "Mehr"), "Bearbeiten".
private struct GroupHeader: View {
    let room: RoomView
    let count: Int?
    let onEdit: () -> Void
    @State private var expanded = false
    @State private var fullHeight: CGFloat = 0
    @State private var shownHeight: CGFloat = 0

    var body: some View {
        VStack(spacing: 6) {
            Avatar(name: room.name, size: 80, token: room.token, version: room.avatarVersion)
                .padding(.bottom, 6)
            Text(room.name)
                .font(.system(size: 24, weight: .semibold))
                .foregroundStyle(Talk.onSurface)
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("group-name-title")
            Text(subtitle)
                .font(.system(size: 15))
                .foregroundStyle(Talk.muted)
                .accessibilityIdentifier("group-subtitle")
            description
            if room.isModerator {
                Button(action: onEdit) {
                    Text("Bearbeiten")
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(Talk.onSurface)
                        .padding(.horizontal, 18)
                        .frame(height: 34)
                        .background(Talk.groupedRow, in: Capsule())
                }
                .buttonStyle(.plain)
                .padding(.top, 8)
                .accessibilityIdentifier("group-edit")
            }
        }
        .frame(maxWidth: .infinity)
    }

    private var subtitle: String {
        let kind = room.isPublic ? "Gruppe mit Link" : "Gruppe"
        guard let count else { return kind }
        return "\(kind) · \(count == 1 ? "1 Mitglied" : "\(count) Mitglieder")"
    }

    @ViewBuilder
    private var description: some View {
        if !room.description.isEmpty {
            VStack(spacing: 2) {
                Text(room.description)
                    .font(.system(size: 15))
                    .foregroundStyle(Talk.onSurface)
                    .multilineTextAlignment(.center)
                    .lineLimit(expanded ? nil : 3)
                    .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { shownHeight = $0 }
                    .background(alignment: .top) {
                        Text(room.description)
                            .font(.system(size: 15))
                            .multilineTextAlignment(.center)
                            .fixedSize(horizontal: false, vertical: true)
                            .hidden()
                            .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { fullHeight = $0 }
                    }
                    .accessibilityIdentifier("group-description")
                if !expanded && fullHeight > shownHeight + 1 {
                    Button("Mehr") { withAnimation(.easeOut(duration: 0.2)) { expanded = true } }
                        .font(.system(size: 15, weight: .medium))
                        .accessibilityIdentifier("group-description-more")
                }
            }
            .padding(.top, 4)
            .padding(.horizontal, 12)
        } else if room.isModerator {
            Button("Beschreibung hinzufügen", action: onEdit)
                .font(.system(size: 15))
                .padding(.top, 2)
                .accessibilityIdentifier("group-description-add")
        }
    }
}

/// Section title like the system settings, without capitals.
struct SettingsHeader: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text)
            .font(.system(size: 14, weight: .medium))
            .foregroundStyle(Talk.muted)
            .textCase(nil)
    }
}

/// Icon (muted, or danger) and text of a settings row.
struct SettingsLabel: View {
    let icon: String
    let text: String
    var danger = false

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: icon)
                .font(.system(size: 17))
                .foregroundStyle(danger ? Talk.danger : Talk.muted)
                .frame(width: 28)
            Text(text).foregroundStyle(danger ? Talk.danger : Talk.onSurface).lineLimit(1)
        }
        .frame(minHeight: 48, alignment: .leading)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }
}

/// Tapping a member: big avatar and name, "Nachricht senden", moderator rights and removing
/// (moderators only, never the owner). Closes after an action; the list reloads through
/// `participantsChanged`.
private struct MemberSheet: View {
    @Bindable var store: SessionStore
    let token: String
    let member: MemberView
    let canManage: Bool
    @Environment(\.dismiss) private var dismiss
    @State private var busy = false
    @State private var confirmRemove = false

    var body: some View {
        VStack(spacing: 0) {
            PersonAvatar(name: member.name, userId: member.actorType == "users" ? member.actorId : "", size: 80)
                .padding(.top, 28)
            Text(member.name)
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(Talk.onSurface)
                .multilineTextAlignment(.center)
                .padding(.top, 10)
            let sub = GroupInfoView.subline(member)
            if !sub.isEmpty || member.isModerator || member.isOwner {
                Text(sub.isEmpty ? "Moderator" : sub)
                    .font(.system(size: 15))
                    .foregroundStyle(Talk.muted)
                    .padding(.top, 2)
            }
            VStack(spacing: 0) {
                if member.actorType == "users" {
                    row("message", "Nachricht senden", id: "member-message") {
                        if await store.openDirect(member.actorId) { dismiss() }
                    }
                }
                if canManage {
                    let mod = member.isModerator
                    row(mod ? "person.badge.minus" : "person.badge.key", mod ? "Moderatorrechte entziehen" : "Zum Moderator machen",
                        id: "member-moderator") {
                        if await store.groupChange({ await $0.setModerator(token: token, attendeeId: member.attendeeId, moderator: !mod) }) {
                            dismiss()
                        }
                    }
                    Button { confirmRemove = true } label: {
                        SettingsLabel(icon: "person.fill.xmark", text: "Aus der Gruppe entfernen", danger: true)
                            .padding(.horizontal, 20)
                    }
                    .buttonStyle(SheetRowStyle())
                    .accessibilityIdentifier("member-remove")
                }
            }
            .padding(.top, 20)
            .disabled(busy)
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity)
        .overlay { if busy { ProgressView() } }
        .presentationDetents([.medium])
        .presentationDragIndicator(.visible)
        .presentationBackground(Talk.surface)
        .confirmationDialog("\(member.name) aus der Gruppe entfernen?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Entfernen", role: .destructive) {
                run {
                    if await store.groupChange({ await $0.removeMember(token: token, attendeeId: member.attendeeId) }) { dismiss() }
                }
            }
            Button("Abbrechen", role: .cancel) {}
        }
    }

    private func row(_ icon: String, _ text: String, id: String, action: @escaping () async -> Void) -> some View {
        Button { run(action) } label: {
            SettingsLabel(icon: icon, text: text).padding(.horizontal, 20)
        }
        .buttonStyle(SheetRowStyle())
        .accessibilityIdentifier(id)
    }

    private func run(_ action: @escaping () async -> Void) {
        busy = true
        Task {
            await action()
            busy = false
        }
    }
}

private struct SheetRowStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .frame(minHeight: 52)
            .background(configuration.isPressed ? Talk.chip : Color.clear)
    }
}

/// "Mitglieder hinzufügen" from the settings: the picker without the members, "Hinzufügen (N)".
private struct AddMembersSheet: View {
    @Bindable var store: SessionStore
    let token: String
    @Environment(\.dismiss) private var dismiss
    @State private var picked: [UserView] = []
    @State private var busy = false

    var body: some View {
        NavigationStack {
            MemberPicker(
                title: "Mitglieder hinzufügen",
                exclude: token,
                picked: $picked,
                confirm: { "Hinzufügen (\($0))" },
                busy: busy,
                requireOne: true,
                onConfirm: add
            )
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Abbrechen") { dismiss() }.disabled(busy)
                }
            }
        }
        .tint(Talk.accent)
        .interactiveDismissDisabled(busy)
    }

    private func add() {
        busy = true
        let ids = picked.map(\.id)
        Task {
            if await store.groupChange({ await $0.addMembers(token: token, userIds: ids) }) { dismiss() }
            busy = false
        }
    }
}

/// Leaving as the last moderator: pick a successor, then leave.
private struct NewModeratorSheet: View {
    @Bindable var store: SessionStore
    let token: String
    let members: [MemberView]
    @Environment(\.dismiss) private var dismiss
    @State private var busy: Int64?

    var body: some View {
        NavigationStack {
            List {
                Text("Bevor du gehst, ernenne jemanden zum Moderator.")
                    .foregroundStyle(Talk.muted)
                    .rowStyle()
                if members.isEmpty {
                    Text("Keine anderen Mitglieder").foregroundStyle(Talk.muted).rowStyle()
                }
                ForEach(members, id: \.attendeeId) { m in
                    Button { pick(m) } label: {
                        PersonRow(name: m.name, subline: GroupInfoView.subline(m), userId: m.actorType == "users" ? m.actorId : "") {
                            if busy == m.attendeeId { ProgressView() }
                        }
                    }
                    .buttonStyle(.plain)
                    .disabled(busy != nil)
                    .accessibilityIdentifier("successor:\(m.name)")
                    .rowStyle()
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(Talk.surface)
            .navigationTitle("Neuen Moderator wählen")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Abbrechen") { dismiss() }.disabled(busy != nil)
                }
            }
        }
        .tint(Talk.accent)
        .presentationDetents([.medium, .large])
        .presentationBackground(Talk.surface)
        .interactiveDismissDisabled(busy != nil)
    }

    private func pick(_ m: MemberView) {
        busy = m.attendeeId
        Task {
            let ok = await store.groupChange { await $0.setModerator(token: token, attendeeId: m.attendeeId, moderator: true) }
            if ok, await store.leaveGroup(token, delete: false) {
                dismiss()
            }
            busy = nil
        }
    }
}

/// "Gruppe bearbeiten": photo (change/remove), name, description (counter from 1800).
/// Only changed fields are saved, one after the other; the first error stops with a toast.
private struct EditGroupView: View {
    @Bindable var store: SessionStore
    let room: RoomView
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var text = ""
    @State private var item: PhotosPickerItem?
    @State private var photo: PhotoChange = .keep
    @State private var saving = false

    enum PhotoChange: Equatable {
        case keep, remove
        case set(GroupPhoto)
    }

    private static let maxDescription = 2000

    var body: some View {
        NavigationStack {
            List {
                VStack(spacing: 8) {
                    GroupPhotoPicker(item: $item, photo: newImage) {
                        if photo == .remove {
                            ZStack {
                                Format.color(for: name)
                                Text(Format.initials(name)).font(.system(size: 32, weight: .semibold)).foregroundStyle(.white)
                            }
                        } else {
                            Avatar(name: room.name, size: 80, token: room.token, version: room.avatarVersion)
                        }
                    }
                    HStack(spacing: 20) {
                        PhotosPicker("Ändern", selection: $item, matching: .images)
                            .accessibilityIdentifier("photo-change")
                        if hasPhoto {
                            Button("Entfernen", role: .destructive) {
                                photo = .remove
                                item = nil
                            }
                            .accessibilityIdentifier("photo-remove")
                        }
                    }
                    .font(.system(size: 15))
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 6)
                .listRowBackground(Color.clear)

                Section {
                    TextField("Gruppenname (erforderlich)", text: $name)
                        .onChange(of: name) { _, v in if v.count > 255 { name = String(v.prefix(255)) } }
                        .accessibilityIdentifier("edit-name")
                        .listRowBackground(Talk.groupedRow)
                }
                Section {
                    TextField("Beschreibung", text: $text, axis: .vertical)
                        .lineLimit(3...10)
                        .onChange(of: text) { _, v in
                            if v.count > Self.maxDescription { text = String(v.prefix(Self.maxDescription)) }
                        }
                        .accessibilityIdentifier("edit-description")
                        .listRowBackground(Talk.groupedRow)
                } footer: {
                    if text.count >= 1800 {
                        Text("\(text.count)/\(Self.maxDescription)")
                            .font(.system(size: 13))
                            .monospacedDigit()
                            .foregroundStyle(text.count >= Self.maxDescription ? Talk.danger : Talk.muted)
                            .frame(maxWidth: .infinity, alignment: .trailing)
                    }
                }
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Talk.groupedBackground)
            .disabled(saving)
            .navigationTitle("Gruppe bearbeiten")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Abbrechen") { dismiss() }.disabled(saving)
                }
                ToolbarItem(placement: .confirmationAction) {
                    if saving {
                        ProgressView()
                    } else {
                        Button("Speichern", action: save)
                            .fontWeight(.semibold)
                            .accessibilityIdentifier("edit-save")
                    }
                }
            }
        }
        .tint(Talk.accent)
        .interactiveDismissDisabled(saving)
        .onAppear {
            name = room.name
            text = room.description
        }
        .onChange(of: item) { _, item in
            guard let item else { return }
            Task {
                if let p = await GroupPhoto.load(item) { photo = .set(p) } else { Toast.show("Bild konnte nicht gelesen werden") }
            }
        }
    }

    private var newImage: UIImage? {
        if case .set(let p) = photo { return p.image }
        return nil
    }

    private var hasPhoto: Bool {
        switch photo {
        case .keep: room.hasCustomAvatar
        case .remove: false
        case .set: true
        }
    }

    private func save() {
        let token = room.token
        let newName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let newText = text.trimmingCharacters(in: .whitespacesAndNewlines)
        var steps: [(TalkSession) async -> String?] = []
        if newName != room.name { steps.append { await $0.renameGroup(token: token, name: newName) } }
        if newText != room.description { steps.append { await $0.setDescription(token: token, description: newText) } }
        switch photo {
        case .keep: break
        case .remove: steps.append { await $0.setGroupPhoto(token: token, photo: nil) }
        case .set(let p): steps.append { await $0.setGroupPhoto(token: token, photo: p.jpeg) }
        }
        guard !steps.isEmpty else { return dismiss() }
        saving = true
        Task {
            for step in steps {
                guard await store.groupChange(step) else {
                    saving = false
                    return
                }
            }
            dismiss()
        }
    }
}
