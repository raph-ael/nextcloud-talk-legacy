import SwiftUI
import UIKit

/// docs/mentions.md "Im Eingabefeld" and "Vorschlagsliste": where a text holds mentions, and
/// the search text after an `@`. Positions are UTF-16 offsets, as `UITextView` counts them.
/// Nothing is remembered: the text and the picks say it all.
enum MentionText {
    /// Where an `@` may start a mention of a pick: the start, white space or `-.'@` before it.
    private static let before = CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "-.'@"))

    /// The mentions of `text` (rule of the core): `@{label}` of a pick after the start, white
    /// space or one of `-.'@`, with no letter or digit after it. The longest name wins.
    static func ranges(_ text: String, _ picks: [MentionPick]) -> [NSRange] {
        let labels = Set(picks.map(\.label).filter { !$0.isEmpty })
            .sorted { $0.utf16.count > $1.utf16.count }
        guard !labels.isEmpty, text.contains("@") else { return [] }
        let ns = text as NSString
        let at = UInt16(UInt8(ascii: "@"))
        var out: [NSRange] = []
        var i = 0
        while i < ns.length {
            guard ns.character(at: i) == at, i == 0 || isBefore(ns.character(at: i - 1)) else {
                i += 1
                continue
            }
            if let label = labels.first(where: { endsName(ns, i + 1, $0) }) {
                let r = NSRange(location: i, length: 1 + label.utf16.count)
                out.append(r)
                i = NSMaxRange(r)
            } else {
                i += 1
            }
        }
        return out
    }

    /// `label` stands at `from`, and no letter or digit follows it.
    private static func endsName(_ ns: NSString, _ from: Int, _ label: String) -> Bool {
        let n = label.utf16.count
        guard from + n <= ns.length, ns.substring(with: NSRange(location: from, length: n)) == label else { return false }
        guard from + n < ns.length else { return true }
        let next = ns.substring(from: from + n).first
        return !(next?.isLetter == true || next?.isNumber == true)
    }

    private static func isBefore(_ unit: UInt16) -> Bool {
        Unicode.Scalar(unit).map { before.contains($0) } ?? false
    }

    /// An `@` being typed: where it stands and the text from it to the caret.
    struct Trigger: Equatable {
        /// Offset of the `@`.
        let start: Int
        /// Without the `@`.
        let query: String
    }

    /// The nearest `@` before the caret at the start or after white space, with at most 40
    /// characters and no line break up to the caret. None while the caret stands in a finished
    /// mention or right behind it; further on the text from the `@` counts as usual ("@Lena Br"
    /// with "Lena" picked).
    static func trigger(_ text: String, _ selection: NSRange, _ picks: [MentionPick]) -> Trigger? {
        guard selection.length == 0 else { return nil }
        let ns = text as NSString
        let caret = min(selection.location, ns.length)
        if ranges(text, picks).contains(where: { caret > $0.location && caret <= NSMaxRange($0) }) { return nil }
        let at = UInt16(UInt8(ascii: "@")), newline = UInt16(UInt8(ascii: "\n"))
        var i = caret - 1
        while i >= 0 {
            let c = ns.character(at: i)
            if c == newline { return nil }
            if c == at, i == 0 || Unicode.Scalar(ns.character(at: i - 1)).map({ CharacterSet.whitespacesAndNewlines.contains($0) }) == true {
                let query = ns.substring(with: NSRange(location: i + 1, length: caret - i - 1))
                guard query.count <= 40 else { return nil }
                return Trigger(start: i, query: query)
            }
            // Past 40 characters no `@` before can count any more.
            if caret - i > 80 { return nil }
            i -= 1
        }
        return nil
    }

    /// "Übernehmen": from the `@` to the caret becomes `@{label}` and a space (none when one
    /// follows); the caret goes behind the space. Returns the new text and caret.
    static func accept(_ text: String, _ trigger: Trigger, caret: Int, label: String) -> (String, Int) {
        let ns = text as NSString
        let end = min(caret, ns.length)
        let spaceFollows = end < ns.length && ns.character(at: end) == UInt16(UInt8(ascii: " "))
        let insert = "@" + label + (spaceFollows ? "" : " ")
        let out = ns.replacingCharacters(in: NSRange(location: trigger.start, length: end - trigger.start), with: insert)
        return (out, trigger.start + insert.utf16.count + (spaceFollows ? 1 : 0))
    }

    /// A pick joins the field's list; an older one with the same name goes.
    static func adding(_ pick: MentionPick, to picks: [MentionPick]) -> [MentionPick] {
        picks.filter { $0.label != pick.label } + [pick]
    }
}

private typealias SwiftUIFont = AttributeScopes.SwiftUIAttributes.FontAttribute

/// docs/mentions.md "In der Nachricht": the mentions in a bubble's finished text.
enum MentionMarks {
    /// URL of a tappable mention: its index in `MessageView.mentions`.
    static let scheme = "talk-mention"

    /// Whether tapping a mention opens a chat: people other than the user.
    static func tappable(_ m: MentionView) -> Bool { m.kind == "user" && !m.isSelf }

    /// Every occurrence of a label (longest first) with no letter or digit after it, not in
    /// code: semibold on `mention` (the text colour at 12 %), or `mentionSelf` where somebody
    /// else means the user. Tappable ones become a link `talk-mention:<index>`.
    @MainActor
    static func mark(_ text: AttributedString, _ mentions: [MentionView], own: Bool, textColor: Color) -> AttributedString {
        guard !mentions.isEmpty else { return text }
        let plain = String(text.characters)
        let chars = Array(plain)
        var taken = [Bool](repeating: false, count: chars.count)
        var found: [(from: Int, to: Int, index: Int)] = []
        let order = mentions.indices.sorted { mentions[$0].label.count > mentions[$1].label.count }
        for mi in order {
            let label = Array(mentions[mi].label)
            guard !label.isEmpty, label.count <= chars.count else { continue }
            var i = 0
            while i + label.count <= chars.count {
                let end = i + label.count
                if !taken[i], Array(chars[i..<end]) == label,
                   end == chars.count || !(chars[end].isLetter || chars[end].isNumber),
                   !taken[i..<end].contains(true) {
                    for k in i..<end { taken[k] = true }
                    found.append((i, end, mi))
                    i = end
                } else {
                    i += 1
                }
            }
        }
        guard !found.isEmpty else { return text }
        var out = text
        let mention = textColor.opacity(0.12)
        for f in found {
            let lo = out.characters.index(out.characters.startIndex, offsetBy: f.from)
            let hi = out.characters.index(out.characters.startIndex, offsetBy: f.to)
            let range = lo..<hi
            // Not in code (`…` and code blocks).
            if out[range].runs.contains(where: {
                $0.inlinePresentationIntent?.contains(.code) == true || $0.attributes[SwiftUIFont.self] == Markdown.codeFont
            }) {
                continue
            }
            let m = mentions[f.index]
            out[range].backgroundColor = m.isSelf && !own ? Talk.mentionSelf : mention
            for run in out[range].runs where run.attributes[SwiftUIFont.self] == nil {
                out[run.range][SwiftUIFont.self] = .system(size: 15, weight: .semibold)
            }
            if tappable(m) { out[range].link = URL(string: "\(scheme):\(f.index)") }
        }
        return out
    }

    /// The mention a tapped link stands for.
    static func mention(_ url: URL, in mentions: [MentionView]) -> MentionView? {
        guard url.scheme == scheme, let i = Int(url.absoluteString.dropFirst(scheme.count + 1)),
              mentions.indices.contains(i) else { return nil }
        return mentions[i]
    }

    /// "Lena Brandt" for "@Lena Brandt".
    static func name(_ m: MentionView) -> String {
        m.label.hasPrefix("@") ? String(m.label.dropFirst()) : m.label
    }
}

/// The input field (docs/mentions.md "Im Eingabefeld"): a `UITextView`, since a SwiftUI
/// `TextField` knows neither the caret nor backgrounds. Grows up to five lines, then scrolls;
/// the mentions are laid on `mention`; backspace right behind one removes it as a whole.
struct ComposerField: UIViewRepresentable {
    @Binding var text: String
    @Binding var selection: NSRange
    @Binding var focused: Bool
    let picks: [MentionPick]
    /// Bumps when the caret is to move to `selection` without the text changing.
    var caretSerial = 0

    static let font = UIFont.systemFont(ofSize: 16)
    private static let inset = UIEdgeInsets(top: 9, left: 14, bottom: 9, right: 14)
    private static let maxLines: CGFloat = 5

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> UITextView {
        let v = UITextView()
        v.delegate = context.coordinator
        v.font = Self.font
        v.textColor = UIColor(Talk.onSurface)
        v.backgroundColor = .clear
        v.textContainerInset = Self.inset
        v.textContainer.lineFragmentPadding = 0
        v.isScrollEnabled = false
        v.autocapitalizationType = .sentences
        v.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        v.setContentHuggingPriority(.defaultLow, for: .horizontal)
        v.accessibilityIdentifier = "composer"
        v.accessibilityLabel = "Nachricht"
        let placeholder = UILabel()
        placeholder.text = "Nachricht"
        placeholder.font = Self.font
        placeholder.textColor = .placeholderText
        placeholder.isAccessibilityElement = false
        placeholder.translatesAutoresizingMaskIntoConstraints = false
        v.addSubview(placeholder)
        NSLayoutConstraint.activate([
            placeholder.leadingAnchor.constraint(equalTo: v.leadingAnchor, constant: Self.inset.left),
            placeholder.topAnchor.constraint(equalTo: v.topAnchor, constant: Self.inset.top),
        ])
        context.coordinator.placeholder = placeholder
        return v
    }

    func updateUIView(_ v: UITextView, context: Context) {
        let c = context.coordinator
        c.parent = self
        if v.text != text {
            v.text = text
            c.style(v)
            v.selectedRange = clamped(selection, v)
        } else if c.picks != picks {
            c.style(v)
        }
        if caretSerial != c.caretSerial {
            c.caretSerial = caretSerial
            v.selectedRange = clamped(selection, v)
        }
        c.picks = picks
        c.placeholder?.isHidden = !text.isEmpty
        if focused != v.isFirstResponder {
            // Not inside the view update: the keyboard's notifications change SwiftUI state.
            DispatchQueue.main.async {
                guard focused != v.isFirstResponder, v.window != nil else { return }
                if focused { v.becomeFirstResponder() } else { v.resignFirstResponder() }
            }
        }
    }

    private func clamped(_ r: NSRange, _ v: UITextView) -> NSRange {
        let n = (v.text as NSString).length
        let loc = min(max(0, r.location), n)
        return NSRange(location: loc, length: min(r.length, n - loc))
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView v: UITextView, context: Context) -> CGSize? {
        let width = proposal.width ?? 240
        let fit = v.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude)).height
        let max = (Self.font.lineHeight * Self.maxLines + Self.inset.top + Self.inset.bottom).rounded(.up)
        let scroll = fit > max
        if v.isScrollEnabled != scroll {
            DispatchQueue.main.async { v.isScrollEnabled = scroll }
        }
        return CGSize(width: width, height: min(fit, max))
    }

    @MainActor
    final class Coordinator: NSObject, UITextViewDelegate {
        var parent: ComposerField?
        var picks: [MentionPick] = []
        var caretSerial = 0
        weak var placeholder: UILabel?

        /// The base look for everything, `mention` behind every mention.
        func style(_ v: UITextView) {
            guard v.markedTextRange == nil else { return }
            let base: [NSAttributedString.Key: Any] = [.font: ComposerField.font, .foregroundColor: UIColor(Talk.onSurface)]
            let storage = v.textStorage
            let full = NSRange(location: 0, length: storage.length)
            storage.beginEditing()
            storage.setAttributes(base, range: full)
            for r in MentionText.ranges(v.text, parent?.picks ?? picks) where NSMaxRange(r) <= storage.length {
                storage.addAttribute(.backgroundColor, value: ComposerField.mention, range: r)
            }
            storage.endEditing()
            v.typingAttributes = base
        }

        func textView(_ v: UITextView, shouldChangeTextIn range: NSRange, replacementText string: String) -> Bool {
            // Backspace right behind a mention, nothing selected: the mention goes as a whole.
            guard string.isEmpty, range.length == 1, v.selectedRange.length == 0,
                  let m = MentionText.ranges(v.text, parent?.picks ?? picks).first(where: { NSMaxRange($0) == NSMaxRange(range) })
            else { return true }
            v.textStorage.replaceCharacters(in: m, with: "")
            v.selectedRange = NSRange(location: m.location, length: 0)
            textViewDidChange(v)
            return false
        }

        func textViewDidChange(_ v: UITextView) {
            style(v)
            placeholder?.isHidden = !v.text.isEmpty
            parent?.text = v.text
            parent?.selection = v.selectedRange
        }

        func textViewDidChangeSelection(_ v: UITextView) {
            guard parent?.selection != v.selectedRange else { return }
            parent?.selection = v.selectedRange
        }

        func textViewDidBeginEditing(_ v: UITextView) {
            if parent?.focused != true { parent?.focused = true }
        }

        func textViewDidEndEditing(_ v: UITextView) {
            if parent?.focused != false { parent?.focused = false }
        }
    }

    /// `mention` in the field: its text colour at 12 %.
    static let mention = UIColor { tc in
        UIColor(Talk.onSurface).resolvedColor(with: tc).withAlphaComponent(0.12)
    }
}

/// The suggestions above the input field (docs/mentions.md "Android und iOS"): full width on
/// `floating` with a hairline on top, rows of 48 with a picture of 32, at most four and a
/// half rows high; the best row on top.
struct MentionSuggestions: View {
    let candidates: [MentionCandidate]
    let token: String
    let onPick: (MentionCandidate) -> Void

    private static let rowHeight: CGFloat = 48

    /// At most four and a half rows.
    static func height(_ count: Int) -> CGFloat { min(CGFloat(count), 4.5) * rowHeight }

    var body: some View {
        let room = SessionStore.shared.room(token)
        ScrollView {
            LazyVStack(spacing: 0) {
                ForEach(candidates, id: \.mentionId) { c in
                    Button { onPick(c) } label: { row(c, room: room) }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("mention:\(c.label)")
                }
            }
        }
        .scrollBounceBehavior(.basedOnSize)
        .frame(height: Self.height(candidates.count))
        .background(Talk.floating)
        .overlay(alignment: .top) {
            Rectangle().fill(Color(uiColor: .separator)).frame(height: 1 / UIScreen.main.scale)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mention-list")
    }

    private func row(_ c: MentionCandidate, room: RoomView?) -> some View {
        HStack(spacing: 12) {
            picture(c, room: room)
            VStack(alignment: .leading, spacing: 1) {
                Text(c.label)
                    .font(.system(size: 16))
                    .foregroundStyle(Talk.onSurface)
                    .lineLimit(1)
                    .truncationMode(.tail)
                if !c.subline.isEmpty {
                    Text(c.subline)
                        .font(.system(size: 12))
                        .foregroundStyle(Talk.muted)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .frame(height: Self.rowHeight)
        .contentShape(Rectangle())
    }

    @ViewBuilder
    private func picture(_ c: MentionCandidate, room: RoomView?) -> some View {
        switch c.kind {
        case "user":
            PersonAvatar(name: c.label, userId: c.userId ?? "", size: 32)
        case "all":
            Avatar(name: room?.name ?? c.label, size: 32, token: token, version: room?.avatarVersion ?? "")
        case "group", "team":
            ZStack {
                Circle().fill(Talk.chip)
                Image(systemName: "person.2.fill")
                    .font(.system(size: 13))
                    .foregroundStyle(Talk.muted)
            }
            .frame(width: 32, height: 32)
            .accessibilityHidden(true)
        default:
            PersonAvatar(name: c.label, userId: "", size: 32)
        }
    }
}

/// Round button above "Zum Ende springen" (docs/mentions.md "„@“-Knopf im Chat"): same size
/// and surface, an "@", from two mentions on the count in the same badge.
struct MentionJumpButton: View {
    let count: Int
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text("@")
                .font(.system(size: 18, weight: .semibold))
                .foregroundStyle(Talk.muted)
                .frame(width: 40, height: 40)
                .background(Talk.floating, in: Circle())
                .shadow(color: .black.opacity(0.18), radius: 4, y: 1)
                .overlay(alignment: .top) {
                    if count >= 2 {
                        Text(count > 99 ? "99+" : "\(count)")
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
        .accessibilityLabel(count >= 2 ? "Zur Erwähnung springen, \(count) Erwähnungen" : "Zur Erwähnung springen")
        .accessibilityIdentifier("jump-to-mention")
    }
}
