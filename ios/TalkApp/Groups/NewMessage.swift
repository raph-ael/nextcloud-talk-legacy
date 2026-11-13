import PhotosUI
import SwiftUI

/// "Neue Nachricht" like Signal (docs/groups.md): contacts open a 1:1 chat, "Neue Gruppe"
/// leads through "Mitglieder hinzufügen" to "Gruppe benennen". A sheet with its own stack;
/// the picked members survive going back.
struct NewMessageSheet: View {
    @Bindable var store: SessionStore
    @Environment(\.dismiss) private var dismiss
    @State private var path: [Step] = []
    @State private var picked: [UserView] = []

    enum Step: Hashable {
        case members, name
    }

    var body: some View {
        NavigationStack(path: $path) {
            ContactsView(store: store, onNewGroup: { path.append(.members) }, onOpened: { dismiss() })
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button { dismiss() } label: { Image(systemName: "xmark") }
                            .accessibilityLabel("Schließen")
                            .accessibilityIdentifier("close")
                    }
                }
                .navigationDestination(for: Step.self) { step in
                    switch step {
                    case .members:
                        MemberPicker(
                            title: "Mitglieder hinzufügen",
                            exclude: nil,
                            picked: $picked,
                            confirm: { $0 == 0 ? "Überspringen" : "Weiter" },
                            busy: false,
                            onConfirm: { path.append(.name) }
                        )
                    case .name:
                        NameGroupView(store: store, members: picked, onCreated: { dismiss() })
                    }
                }
        }
        .tint(Talk.accent)
    }
}

/// Step 0: search, "Neue Gruppe" and the contacts.
private struct ContactsView: View {
    @Bindable var store: SessionStore
    let onNewGroup: () -> Void
    let onOpened: () -> Void
    @State private var query = ""
    @State private var load = UserLoad.loading
    @State private var retry = 0
    @State private var opening: String?

    var body: some View {
        List {
            if query.isEmpty && store.canCreateGroups {
                Button(action: onNewGroup) {
                    HStack(spacing: 14) {
                        AccentCircle(icon: "person.2.fill")
                        Text("Neue Gruppe").font(.system(size: 16)).foregroundStyle(Talk.onSurface)
                        Spacer()
                    }
                    .frame(minHeight: 52)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("new-group")
                .rowStyle()
            }
            Section {
                switch load {
                case .loading:
                    PlaceholderRows().rowStyle()
                case .failed(let text):
                    LoadFailed(text: text) { retry += 1 }.rowStyle()
                case .done(let users) where users.isEmpty:
                    Text("Keine Treffer")
                        .foregroundStyle(Talk.muted)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 20)
                        .rowStyle()
                case .done(let users):
                    ForEach(users, id: \.id) { u in
                        Button { open(u) } label: {
                            PersonRow(name: u.name, subline: u.subline, userId: u.id) {
                                if opening == u.id { ProgressView() }
                            }
                        }
                        .buttonStyle(.plain)
                        .disabled(opening != nil)
                        .accessibilityIdentifier("contact:\(u.name)")
                        .rowStyle()
                    }
                }
            } header: {
                Text("Kontakte")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(Talk.muted)
                    .textCase(nil)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Talk.background)
        .scrollDismissesKeyboard(.immediately)
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Name suchen")
        .userSearch(query, retry: retry, into: $load)
        .navigationTitle("Neue Nachricht")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func open(_ u: UserView) {
        opening = u.id
        Task {
            if await store.openDirect(u.id) { onOpened() }
            opening = nil
        }
    }
}

/// Step 1 ("Mitglieder hinzufügen"), also used by the group settings with `exclude` and
/// "Hinzufügen (N)": chips of the picked people above the list, round checks per row.
struct MemberPicker: View {
    let title: String
    let exclude: String?
    @Binding var picked: [UserView]
    let confirm: (Int) -> String
    let busy: Bool
    var requireOne = false
    let onConfirm: () -> Void
    @State private var query = ""
    @State private var load = UserLoad.loading
    @State private var retry = 0
    @State private var searching = false

    var body: some View {
        List {
            if !picked.isEmpty {
                UserChips(users: picked) { u in picked.removeAll { $0.id == u.id } }
                    .padding(.vertical, 4)
                    .rowStyle()
            }
            switch load {
            case .loading:
                PlaceholderRows().rowStyle()
            case .failed(let text):
                LoadFailed(text: text) { retry += 1 }.rowStyle()
            case .done(let users) where users.isEmpty:
                Text("Keine Treffer")
                    .foregroundStyle(Talk.muted)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 20)
                    .rowStyle()
            case .done(let users):
                ForEach(users, id: \.id) { u in
                    let on = picked.contains { $0.id == u.id }
                    Button { toggle(u) } label: {
                        PersonRow(name: u.name, subline: u.subline, userId: u.id) { RoundCheck(selected: on) }
                    }
                    .buttonStyle(.plain)
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("\(u.name), \(on ? "ausgewählt" : "nicht ausgewählt")")
                    .accessibilityAddTraits(.isButton)
                    .accessibilityIdentifier("pick:\(u.name)")
                    .rowStyle()
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Talk.background)
        .scrollDismissesKeyboard(.immediately)
        .searchable(text: $query, isPresented: $searching, placement: .navigationBarDrawer(displayMode: .always), prompt: "Name suchen")
        .userSearch(query, exclude: exclude, retry: retry, into: $load)
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                if busy {
                    ProgressView()
                } else {
                    Button(confirm(picked.count), action: onConfirm)
                        .fontWeight(.semibold)
                        .disabled(requireOne && picked.isEmpty)
                        .accessibilityIdentifier("members-next")
                }
            }
        }
    }

    private func toggle(_ u: UserView) {
        Haptics.light()
        if picked.contains(where: { $0.id == u.id }) {
            picked.removeAll { $0.id == u.id }
        } else {
            picked.append(u)
            // After a pick the search starts over (docs/groups.md); closing it brings back
            // "Weiter", which the system hides while searching.
            query = ""
            searching = false
        }
    }
}

/// Step 2 ("Gruppe benennen"): photo, name (required), the members for review, "Erstellen".
private struct NameGroupView: View {
    @Bindable var store: SessionStore
    let members: [UserView]
    let onCreated: () -> Void
    @State private var name = ""
    @State private var item: PhotosPickerItem?
    @State private var photo: GroupPhoto?
    @State private var creating = false
    @FocusState private var focused: Bool

    private var trimmed: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        List {
            VStack(spacing: 8) {
                GroupPhotoPicker(item: $item, photo: photo?.image) {
                    ZStack {
                        Talk.chip
                        Image(systemName: "person.2.fill").font(.system(size: 30)).foregroundStyle(Talk.muted)
                    }
                }
                if photo != nil {
                    Button("Entfernen") {
                        photo = nil
                        item = nil
                    }
                    .font(.system(size: 14))
                    .accessibilityIdentifier("photo-remove")
                }
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 8)
            .rowStyle()
            TextField("Gruppenname (erforderlich)", text: $name)
                .font(.system(size: 17))
                .focused($focused)
                .submitLabel(.done)
                .padding(.horizontal, 14)
                .frame(height: 48)
                .background(Talk.chip, in: RoundedRectangle(cornerRadius: 12))
                .onChange(of: name) { _, v in if v.count > 255 { name = String(v.prefix(255)) } }
                .accessibilityIdentifier("group-name")
                .rowStyle()
            if !members.isEmpty {
                Section {
                    ForEach(members, id: \.id) { u in
                        PersonRow(name: u.name, subline: u.subline, userId: u.id).rowStyle()
                    }
                } header: {
                    Text("Mitglieder · \(members.count)")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(Talk.muted)
                        .textCase(nil)
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Talk.background)
        .disabled(creating)
        .navigationTitle("Gruppe benennen")
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden(creating)
        .interactiveDismissDisabled(creating)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                if creating {
                    ProgressView()
                } else {
                    Button("Erstellen", action: create)
                        .fontWeight(.semibold)
                        .disabled(trimmed.isEmpty)
                        .accessibilityIdentifier("group-create")
                }
            }
        }
        .onAppear { focused = true }
        .onChange(of: item) { _, item in
            guard let item else { return }
            Task {
                if let p = await GroupPhoto.load(item) { photo = p } else { Toast.show("Bild konnte nicht gelesen werden") }
            }
        }
    }

    private func create() {
        guard !trimmed.isEmpty, !creating else { return }
        focused = false
        creating = true
        Task {
            if await store.createGroup(trimmed, members.map(\.id), photo: photo?.jpeg) {
                onCreated()
            } else {
                creating = false
            }
        }
    }
}

extension View {
    /// Plain list row on the page background, no separators, 16 pt sides.
    func rowStyle() -> some View {
        listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 2, trailing: 16))
            .listRowSeparator(.hidden)
            .listRowBackground(Color.clear)
    }
}
