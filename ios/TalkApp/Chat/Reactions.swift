import SwiftUI
import UIKit

/// Reactions like Signal (docs/chat.md "Reaktionen"): one per person, the core applies the
/// rule – the platform only passes on the tapped emoji.
enum Reactions {
    /// Signal's quick reactions.
    static let quick = ["❤️", "👍", "👎", "😂", "😮", "😢"]

    /// The six in the bar; an own reaction outside them takes the last place instead of 😢.
    static func bar(mine: [String]) -> [String] {
        guard let other = mine.first(where: { !quick.contains($0) }) else { return quick }
        return quick.dropLast() + [other]
    }

    /// Pills in display order: most used first, then by emoji.
    static func sorted(_ r: [String: Int64]) -> [(emoji: String, count: Int64)] {
        r.sorted { $0.value == $1.value ? $0.key < $1.key : $0.value > $1.value }.map { ($0.key, $0.value) }
    }

    /// A delivered message (not in the outbox), no system or deleted message.
    static func reactable(_ m: UiMessage) -> Bool {
        m.msg.kind == .text && m.status == nil
    }
}

/// Pills under the bubble (docs/chat.md "Pillen unter der Blase", Android and iOS): emoji and
/// from 2 the count, the own one darker, each with a ring in the chat background so it stands
/// off the bubble. At most three: with more emojis the two most used and "+N" for the remaining
/// reactions (highlighted when the own one is among them). A tap opens "Wer hat reagiert" (the
/// pills do not toggle). A new pill pops in (0.6 → 1).
struct ReactionPills: View {
    let msg: MessageView
    let onTap: () -> Void

    /// Height of a pill; it reaches `overlap` into the bubble's bottom edge.
    static let height: CGFloat = 26
    static let overlap: CGFloat = 4

    var body: some View {
        let all = Reactions.sorted(msg.reactions)
        let shown = all.count > 3 ? Array(all.prefix(2)) : all
        let rest = all.count > 3 ? Array(all.dropFirst(2)) : []
        let restCount = rest.reduce(Int64(0)) { $0 + $1.count }
        HStack(spacing: 2) {
            ForEach(shown, id: \.emoji) { r in
                pill(mine: msg.reactionsSelf.contains(r.emoji)) {
                    Text(r.emoji).font(.system(size: 15))
                    if r.count > 1 { Text("\(r.count)").font(.system(size: 12, weight: .semibold)) }
                }
                .accessibilityLabel("\(r.emoji) \(r.count), Reaktionen anzeigen")
            }
            if !rest.isEmpty {
                pill(mine: rest.contains { msg.reactionsSelf.contains($0.emoji) }) {
                    Text("+\(restCount)").font(.system(size: 12, weight: .semibold))
                }
                .accessibilityLabel("\(restCount) weitere, Reaktionen anzeigen")
            }
        }
        .animation(.easeOut(duration: 0.15), value: shown.map(\.emoji) + (rest.isEmpty ? [] : ["+"]))
    }

    private func pill(mine: Bool, @ViewBuilder _ label: () -> some View) -> some View {
        Button(action: onTap) {
            HStack(spacing: 3) { label() }
                .foregroundStyle(Talk.onSurface)
                .padding(.horizontal, 8)
                .frame(minWidth: Self.height, minHeight: Self.height, maxHeight: Self.height)
                .background(mine ? Talk.reactionMine : Talk.reaction, in: Capsule())
                .overlay(Capsule().strokeBorder(Talk.background, lineWidth: 1.5))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .transition(.scale(scale: 0.6).combined(with: .opacity))
        .accessibilityIdentifier("reaction-pill")
    }
}

/// Quick reactions above the bubble: six emojis and ＋. The own one sits on a light accent
/// circle; tapping it again takes it back.
struct ReactionBar: View {
    let mine: [String]
    let onReact: (String) -> Void
    let onMore: () -> Void

    var body: some View {
        HStack(spacing: 2) {
            ForEach(Reactions.bar(mine: mine), id: \.self) { e in
                Button {
                    onReact(e)
                } label: {
                    Text(e)
                        .font(.system(size: 28))
                        .frame(width: 44, height: 44)
                        .background(Talk.accent.opacity(mine.contains(e) ? 0.22 : 0), in: Circle())
                }
                .buttonStyle(ReactionPress())
                .accessibilityLabel("Mit \(e) reagieren")
                .accessibilityAddTraits(mine.contains(e) ? .isSelected : [])
                .accessibilityIdentifier("react:\(e)")
            }
            Button(action: onMore) {
                Image(systemName: "plus")
                    .font(.system(size: 18, weight: .semibold))
                    .foregroundStyle(Talk.muted)
                    .frame(width: 36, height: 36)
                    .background(Talk.chip, in: Circle())
                    .frame(width: 44, height: 44)
            }
            .buttonStyle(ReactionPress())
            .accessibilityLabel("Weitere Reaktionen")
            .accessibilityIdentifier("react-more")
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 4)
        .background(Talk.floating, in: Capsule())
        .shadow(color: .black.opacity(0.18), radius: 10, y: 3)
    }
}

/// The emoji grows a little while pressed.
private struct ReactionPress: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 1.25 : 1)
            .animation(.easeOut(duration: 0.1), value: configuration.isPressed)
    }
}

/// ＋: an invisible text field that brings up the emoji keyboard; the first emoji typed is
/// taken, then the keyboard closes. Closing it without one calls `onCancel`.
struct EmojiCatcher: UIViewRepresentable {
    let active: Bool
    let onPick: (String) -> Void
    let onCancel: () -> Void

    final class Field: UITextField {
        override var textInputMode: UITextInputMode? {
            UITextInputMode.activeInputModes.first { $0.primaryLanguage == "emoji" } ?? super.textInputMode
        }
    }

    final class Coordinator: NSObject, UITextFieldDelegate {
        var parent: EmojiCatcher
        var picked = false
        init(_ parent: EmojiCatcher) { self.parent = parent }

        @objc func changed(_ f: UITextField) {
            let text = f.text ?? ""
            f.text = ""
            guard !picked, let e = text.first(where: \.isEmoji) else { return }
            picked = true
            f.resignFirstResponder()
            parent.onPick(String(e))
        }

        func textFieldDidEndEditing(_ textField: UITextField) {
            if !picked { parent.onCancel() }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> Field {
        let f = Field()
        f.delegate = context.coordinator
        f.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .editingChanged)
        f.tintColor = .clear
        f.textColor = .clear
        f.autocorrectionType = .no
        f.spellCheckingType = .no
        f.accessibilityIdentifier = "emoji-catcher"
        f.accessibilityLabel = "Emoji wählen"
        return f
    }

    func updateUIView(_ f: Field, context: Context) {
        context.coordinator.parent = self
        if active && !f.isFirstResponder {
            DispatchQueue.main.async { f.becomeFirstResponder() }
        }
    }
}

extension Character {
    /// Shown as an emoji (not digits or # that merely may be one).
    var isEmoji: Bool {
        guard let first = unicodeScalars.first else { return false }
        return first.properties.isEmojiPresentation || (first.properties.isEmoji && unicodeScalars.count > 1)
    }
}

/// The message whose reactions are listed.
struct ReactionsTarget: Identifiable {
    let token: String
    let messageId: Int64
    var id: Int64 { messageId }
}

/// "Wer hat reagiert": tabs "Alle · N" and one per emoji (by count), rows with avatar, name
/// and emoji. The own row ("Du · Tippen zum Entfernen") removes the reaction. Loaded fresh
/// on every open; after removing, the own rows are hidden locally (a reload could overtake
/// the DELETE) and the sheet closes when nothing is left.
struct WhoReactedSheet: View {
    let target: ReactionsTarget
    @Environment(\.dismiss) private var dismiss
    @State private var list: [ReactionView]?
    @State private var failed = false
    @State private var tab = "*"

    var body: some View {
        VStack(spacing: 0) {
            Text("Reaktionen")
                .font(.system(size: 17, weight: .semibold))
                .padding(.top, 18)
                .padding(.bottom, 10)
            if failed {
                Text("Reaktionen konnten nicht geladen werden.")
                    .foregroundStyle(Talk.muted)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let all = list {
                content(all)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .presentationBackground(Talk.surface)
        .task(id: target.messageId) {
            do {
                list = try await SessionStore.shared.reactions(target.token, target.messageId)
            } catch {
                failed = true
            }
        }
    }

    private func content(_ all: [ReactionView]) -> some View {
        let groups = Dictionary(grouping: all, by: \.emoji)
            .sorted { $0.value.count == $1.value.count ? $0.key < $1.key : $0.value.count > $1.value.count }
        let tabs: [(id: String, label: String, rows: [ReactionView])] =
            [("*", "Alle · \(all.count)", all)] + groups.map { ($0.key, "\($0.key) \($0.value.count)", $0.value) }
        let current = tabs.first { $0.id == tab } ?? tabs[0]
        return VStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(tabs, id: \.id) { t in
                        Button {
                            tab = t.id
                        } label: {
                            Text(t.label)
                                .font(.system(size: 15, weight: t.id == current.id ? .semibold : .regular))
                                .foregroundStyle(Talk.onSurface)
                                .padding(.horizontal, 14)
                                .frame(height: 32)
                                .background(t.id == current.id ? Talk.chip : Color.clear, in: Capsule())
                                .overlay(Capsule().stroke(Talk.muted.opacity(t.id == current.id ? 0.35 : 0.15), lineWidth: 1))
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(t.id == current.id ? .isSelected : [])
                        .accessibilityIdentifier("reactions-tab")
                    }
                }
                .padding(.horizontal, 16)
            }
            .padding(.bottom, 6)
            List(current.rows, id: \.self) { r in
                ReactorRow(r: r) { remove(all) }
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
                    .listRowInsets(EdgeInsets(top: 6, leading: 20, bottom: 6, trailing: 20))
            }
            .listStyle(.plain)
            .accessibilityIdentifier("reactions-list")
        }
    }

    private func remove(_ all: [ReactionView]) {
        Haptics.light()
        SessionStore.shared.setReaction(target.token, target.messageId, nil)
        let rest = all.filter { !$0.isOwn }
        if rest.isEmpty {
            dismiss()
        } else {
            list = rest
        }
    }
}

private struct ReactorRow: View {
    let r: ReactionView
    let onRemove: () -> Void

    var body: some View {
        let row = HStack(spacing: 14) {
            PersonAvatar(name: r.name, userId: r.actorType == "users" ? r.actorId : "", size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(r.isOwn ? "Du" : r.name)
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(Talk.onSurface)
                    .lineLimit(1)
                if r.isOwn {
                    Text("Tippen zum Entfernen").font(.system(size: 13)).foregroundStyle(Talk.muted)
                }
            }
            Spacer(minLength: 8)
            Text(r.emoji).font(.system(size: 24))
        }
        .contentShape(Rectangle())
        if r.isOwn {
            Button(action: onRemove) { row }
                .buttonStyle(.plain)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("Du, \(r.emoji)")
                .accessibilityHint("Tippen zum Entfernen")
                .accessibilityIdentifier("reaction-own")
        } else {
            row
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("reaction-row")
        }
    }
}

/// A Talk user's avatar from the server; initials while loading and for guests.
struct PersonAvatar: View {
    let name: String
    let userId: String
    let size: CGFloat
    /// Another account than the active one (account menu, call of an inactive account).
    var account: String? = nil
    @State private var image: UIImage?
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let owner = account ?? SessionStore.shared.activeId ?? ""
        ZStack {
            if let image = image ?? (userId.isEmpty ? nil : CallAvatars.shared.cached(userId, dark: scheme == .dark, account: owner)) {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                Format.color(for: name)
                Text(Format.initials(name))
                    .font(.system(size: size * 0.4, weight: .semibold))
                    .foregroundStyle(.white)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .task(id: "\(owner)|\(userId)|\(scheme == .dark)") {
            image = nil
            guard !userId.isEmpty else { return }
            image = await CallAvatars.shared.image(userId, dark: scheme == .dark, account: owner)
        }
    }
}
