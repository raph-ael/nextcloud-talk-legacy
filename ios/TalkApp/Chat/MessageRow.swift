import SwiftUI

private let groupWindow: Int64 = 3 * 60

/// Same author within three minutes on the same day: bubbles join (tighter corners).
func continues(_ a: UiMessage?, _ b: UiMessage?) -> Bool {
    guard let a, let b else { return false }
    if a.msg.kind == .system || b.msg.kind == .system { return false }
    return a.msg.actorId == b.msg.actorId && a.msg.isOwn == b.msg.isOwn
        && abs(b.msg.timestamp - a.msg.timestamp) <= groupWindow && Format.sameDay(a.msg.timestamp, b.msg.timestamp)
}

/// Width of photos in chat bubbles.
let chatImageWidth: CGFloat = 260

/// One message with its day divider and, for the first unread one, the unread line (port of `MessageRow` in Android `ChatScreen.kt`).
struct MessageRow: View {
    let m: UiMessage
    let prev: UiMessage?
    let next: UiMessage?
    let isGroup: Bool
    let read: Bool
    let highlighted: Bool
    /// "N ungelesene Nachrichten" goes above this message.
    var unread: Int? = nil
    let onRetry: (MessageView) -> Void
    let onOpenImage: (String, UIImage?) -> Void
    var onShowReactions: (MessageView) -> Void = { _ in }
    /// Search words to lay on `searchMark`, and whether the bubble lights up (docs/search.md).
    var marks: [[Unicode.Scalar]] = []
    var flash = false
    var onQuote: (Int64) -> Void = { _ in }
    /// A mention of a person was tapped (docs/mentions.md "Antippen").
    var onMention: (MentionView) -> Void = { _ in }

    var body: some View {
        let msg = m.msg
        VStack(spacing: 0) {
            if prev == nil || !Format.sameDay(prev!.msg.timestamp, msg.timestamp) {
                // Like Signal: plain muted text; a heading, so VoiceOver can jump day by day.
                Text(Format.dayLabel(msg.timestamp))
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Talk.muted)
                    .padding(.top, 16)
                    .padding(.bottom, 8)
                    .accessibilityAddTraits(.isHeader)
            }
            if let unread { UnreadDivider(count: unread) }
            if msg.kind == .system {
                Text(msg.text)
                    .font(.system(size: 13))
                    .foregroundStyle(Talk.muted)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: .infinity)
                    .padding(.horizontal, 32)
                    .padding(.vertical, 6)
            } else {
                Bubble(
                    m: m, first: !continues(prev, m), last: !continues(m, next), isGroup: isGroup,
                    read: read, highlighted: highlighted, onRetry: onRetry, onOpenImage: onOpenImage,
                    onShowReactions: onShowReactions, marks: marks, flash: flash, onQuote: onQuote,
                    onMention: onMention
                )
            }
        }
        .frame(maxWidth: .infinity)
    }
}

/// The unread line (docs/chat.md): 1 pt `muted` across the whole list, the count in bold
/// below it.
struct UnreadDivider: View {
    let count: Int

    var body: some View {
        let text = count == 1 ? "1 ungelesene Nachricht" : "\(count) ungelesene Nachrichten"
        VStack(spacing: 3) {
            Rectangle()
                .fill(Talk.muted)
                .frame(height: 1)
                // The list rows are inset by 12; the line spans the full width.
                .padding(.horizontal, -12)
            Text(text)
                .font(.system(size: 13, weight: .bold))
                .foregroundStyle(Talk.onSurface)
        }
        .padding(.top, 24)
        .padding(.bottom, 16)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(text)
        .accessibilityIdentifier("unread-line")
    }
}

/// One bubble with its content and meta line, the reaction pills under it (also drawn by the
/// long-press menu).
struct Bubble: View {
    let m: UiMessage
    let first: Bool
    let last: Bool
    let isGroup: Bool
    let read: Bool
    let highlighted: Bool
    let onRetry: (MessageView) -> Void
    let onOpenImage: (String, UIImage?) -> Void
    var onShowReactions: (MessageView) -> Void = { _ in }
    var marks: [[Unicode.Scalar]] = []
    var flash = false
    var onQuote: (Int64) -> Void = { _ in }
    var onMention: (MentionView) -> Void = { _ in }

    var body: some View {
        let msg = m.msg
        let own = msg.isOwn
        let attachment = msg.kind == .deleted ? nil : msg.attachment
        let image = attachment?.isImage == true
        let place = msg.kind == .deleted ? nil : msg.location
        // Photos and places: the picture fills the bubble up to a thin edge.
        let media = image || place != nil
        // A status stays small, also when it is one emoji.
        let jumbo = msg.kind == .text && !msg.quiet && attachment == nil && place == nil && msg.replyTo == nil
            && Format.isJumboEmoji(msg.text)
        let big: CGFloat = 18, small: CGFloat = 4
        let shape = own
            ? UnevenRoundedRectangle(topLeadingRadius: big, bottomLeadingRadius: big,
                                     bottomTrailingRadius: last ? big : small, topTrailingRadius: first ? big : small)
            : UnevenRoundedRectangle(topLeadingRadius: first ? big : small, bottomLeadingRadius: last ? big : small,
                                     bottomTrailingRadius: big, topTrailingRadius: big)
        // Sent silently by somebody else: a status, subdued (docs/chat.md "Stille Nachrichten").
        let textColor = msg.quiet ? Talk.muted : own && !jumbo ? Talk.bubbleOwnText : Talk.bubbleOtherText
        let inset: CGFloat = media ? 9 : 0
        let metaOnImage = image && msg.text.isEmpty
        let showMeta = last || m.status != nil
        let audio = !image && attachment?.isAudio == true
        // Signal: an audio bubble carries its time in the player's footer row …
        let metaInVoice = audio && msg.text.isEmpty
        // … and has more room above the player when nothing (name, quote) precedes it.
        let audioOnTop = audio && !(isGroup && !own && first) && msg.replyTo == nil
        let metaColor = jumbo ? Talk.muted : textColor.opacity(0.75)
        let pills = !msg.reactions.isEmpty && msg.kind != .deleted

        HStack(alignment: .bottom, spacing: 0) {
            if own { Spacer(minLength: 48) }
            if isGroup && !own {
                ZStack {
                    if last { PersonAvatar(name: msg.author, userId: msg.actorType == "users" ? msg.actorId : "", size: 28) }
                }
                .frame(width: 36, alignment: .leading)
            }
            // The pills hang below the bubble on its side (others left, own right), reach into
            // its bottom edge and are drawn above it; a short bubble may be narrower than them.
            VStack(alignment: own ? .trailing : .leading, spacing: -ReactionPills.overlap) {
                BubbleStack(maxWidth: image ? chatImageWidth : place != nil ? placeMapSize.width : 276) {
                    if isGroup && !own && first {
                        Text(msg.author)
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(Format.color(for: msg.author))
                            .padding(.horizontal, inset)
                            .padding(.vertical, media ? 4 : 0)
                    }
                    if let r = msg.replyTo {
                        let quoted = Format.withAttachment(r.text, r.attachment, r.location)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(r.isOwn ? "Du" : r.author).font(.system(size: 13, weight: .semibold))
                            Text(quoted.isEmpty ? "Nachricht gelöscht" : quoted)
                                .font(.system(size: 13))
                                .opacity(0.85)
                                .lineLimit(2)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                        // docs/search.md "Zitat antippen": to the quoted message; a deleted one does nothing.
                        .onTapGesture { if !quoted.isEmpty && msg.kind != .deleted { onQuote(r.id) } }
                        .accessibilityElement(children: .contain)
                        .accessibilityIdentifier("quote")
                        .foregroundStyle(textColor)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(textColor.opacity(0.1), in: RoundedRectangle(cornerRadius: 6))
                        .padding(.horizontal, inset)
                        .padding(.top, 2)
                        .padding(.bottom, 6)
                    }
                    if image, let a = attachment {
                        let above = (isGroup && !own && first) || msg.replyTo != nil
                        let below = !metaOnImage
                        let r: CGFloat = 15
                        let inner: CGFloat = 4
                        ChatImage(m: m, attachment: a, onOpen: { onOpenImage(a.id, $0) })
                            .clipShape(UnevenRoundedRectangle(
                                topLeadingRadius: above ? inner : (own || first ? r : inner),
                                bottomLeadingRadius: below ? inner : (own || last ? r : inner),
                                bottomTrailingRadius: below ? inner : (!own || last ? r : inner),
                                topTrailingRadius: above ? inner : (!own || first ? r : inner)
                            ))
                            .overlay(alignment: .bottomTrailing) {
                                if metaOnImage && showMeta {
                                    MessageMeta(m: m, own: own, read: read, color: .white)
                                        .padding(.horizontal, 6)
                                        .padding(.vertical, 2)
                                        .background(.black.opacity(0.4), in: RoundedRectangle(cornerRadius: 10))
                                        .padding(6)
                                }
                            }
                    } else if audio {
                        VoiceBubble(
                            m: m, textColor: textColor, metaColor: metaColor, own: own,
                            meta: metaInVoice && showMeta ? MessageMeta(m: m, own: own, read: read, color: metaColor) : nil
                        )
                        .padding(.bottom, msg.text.isEmpty ? 0 : 4)
                    } else if let a = attachment, !a.contactName.isEmpty {
                        ContactRow(m: m, attachment: a, textColor: textColor, onRetry: onRetry)
                            .padding(.bottom, msg.text.isEmpty ? 0 : 4)
                    } else if let a = attachment {
                        FileRow(m: m, attachment: a, textColor: textColor, onRetry: onRetry)
                            .padding(.bottom, msg.text.isEmpty ? 0 : 4)
                    } else if let place {
                        PlaceContent(
                            m: m, place: place, textColor: textColor, inset: inset,
                            above: (isGroup && !own && first) || msg.replyTo != nil,
                            corners: (own || first, own || last, !own || last, !own || first),
                            onRetry: onRetry
                        )
                    }
                    if msg.kind == .deleted {
                        Text("Diese Nachricht wurde gelöscht.").italic().foregroundStyle(textColor)
                    } else if jumbo {
                        Text(msg.text).font(.system(size: 44))
                    } else if !msg.text.isEmpty && place == nil {
                        // Mentions first, so a search hit on one wins (docs/mentions.md "Suche").
                        let rendered = MentionMarks.mark(
                            Markdown.render(msg.text, markdown: msg.markdown), msg.mentions, own: own, textColor: textColor
                        )
                        Text(SearchText.mark(rendered, marks))
                            .font(.system(size: msg.quiet ? 13 : 15))
                            .lineSpacing(msg.quiet ? 2 : 3)
                            .foregroundStyle(textColor)
                            .tint(textColor)
                            .padding(.horizontal, inset)
                            .padding(.top, image ? 6 : 0)
                            .fixedSize(horizontal: false, vertical: true)
                            .environment(\.openURL, OpenURLAction { url in
                                guard url.scheme == MentionMarks.scheme else { return .systemAction }
                                if let mention = MentionMarks.mention(url, in: msg.mentions) { onMention(mention) }
                                return .handled
                            })
                            .modifier(MentionActions(mentions: msg.mentions, onMention: onMention))
                    }
                    if showMeta && !metaOnImage && !metaInVoice {
                        MessageMeta(m: m, own: own, read: read, color: metaColor)
                            .padding(.horizontal, inset)
                            .padding(.top, 2)
                            .padding(.bottom, media ? 4 : 0)
                            .layoutValue(key: TrailingInBubble.self, value: true)
                    }
                }
                .padding(.horizontal, jumbo ? 0 : media ? 3 : 12)
                .padding(.top, media ? 3 : audioOnTop ? 12 : 7)
                .padding(.bottom, media ? 3 : 7)
                .background(jumbo ? Color.clear : own ? Talk.bubbleOwn : Talk.bubbleOther, in: shape)
                // The target of a jump lights up and fades (docs/search.md "Aufleuchten").
                .overlay {
                    shape.fill(Talk.searchMark)
                        .opacity(flash ? 1 : 0)
                        .animation(.easeOut(duration: flash ? 0.15 : 0.6), value: flash)
                        .allowsHitTesting(false)
                }
                .opacity(m.status == .failed ? 0.6 : 1)
                .scaleEffect(highlighted ? 0.97 : 1)
                .animation(.easeOut(duration: 0.15), value: highlighted)
                .onTapGesture {
                    if m.status == .failed || m.status == .waiting { onRetry(msg) }
                }
                if pills {
                    ReactionPills(msg: msg) { onShowReactions(msg) }
                        .padding(.horizontal, 8)
                }
            }
            // The avatar stays at the bubble's foot, not the pills'.
            .alignmentGuide(.bottom) { d in
                d[.bottom] - (pills ? ReactionPills.height - ReactionPills.overlap : 0)
            }
            if !own { Spacer(minLength: 48) }
        }
        .padding(.top, first ? 8 : 2)
        .accessibilityElement(children: .contain)
    }
}

/// VoiceOver: every mention of a person as an action "Chat mit {Name} öffnen".
private struct MentionActions: ViewModifier {
    let mentions: [MentionView]
    let onMention: (MentionView) -> Void

    func body(content: Content) -> some View {
        var seen = Set<String>()
        let people = mentions.filter { MentionMarks.tappable($0) && seen.insert($0.id).inserted }
        if people.isEmpty {
            content
        } else {
            content.accessibilityActions {
                ForEach(people, id: \.id) { m in
                    Button("Chat mit \(MentionMarks.name(m)) öffnen") { onMention(m) }
                }
            }
        }
    }
}

private struct TrailingInBubble: LayoutValueKey {
    static let defaultValue = false
}

/// Bubble content: as wide as its widest line (at most `maxWidth`), the meta line
/// right-aligned, everything else left-aligned – like a chat bubble should hug its text.
private struct BubbleStack: Layout {
    let maxWidth: CGFloat

    private func width(_ proposal: ProposedViewSize) -> CGFloat {
        min(proposal.width ?? maxWidth, maxWidth)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let w = width(proposal)
        var size = CGSize.zero
        for v in subviews {
            let s = v.sizeThatFits(ProposedViewSize(width: w, height: nil))
            size.width = max(size.width, min(s.width, w))
            size.height += s.height
        }
        return size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let w = min(width(proposal), bounds.width)
        var y = bounds.minY
        for v in subviews {
            let s = v.sizeThatFits(ProposedViewSize(width: w, height: nil))
            let cw = min(s.width, bounds.width)
            let x = v[TrailingInBubble.self] ? bounds.maxX - cw : bounds.minX
            v.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(width: cw, height: s.height))
            y += s.height
        }
    }
}

private func failureText(_ e: SendError?) -> String {
    switch e {
    case .forbidden: return "keine Berechtigung"
    case .notFound: return "Unterhaltung nicht gefunden"
    case .tooLarge: return "Datei zu groß"
    case .quotaExceeded: return "Speicher voll"
    case .fileMissing: return "Datei fehlt"
    default: return "Tippen zum Wiederholen"
    }
}

/// Time, "edited", send status and read ticks below (or on) a bubble.
struct MessageMeta: View {
    let m: UiMessage
    let own: Bool
    let read: Bool
    let color: Color

    var body: some View {
        HStack(spacing: 3) {
            if m.msg.edited { Text("bearbeitet ·") }
            switch m.status {
            case .sending: Text("Wird gesendet …")
            case .waiting: Text("Wartet auf Netz …")
            case .failed: Text("Nicht gesendet · " + failureText(m.error)).foregroundStyle(Talk.danger)
            case nil: Text(Format.messageTime(m.msg.timestamp))
            }
            if own && m.status == nil { Text(read ? "✓✓" : "✓") }
        }
        .font(.system(size: 12))
        .foregroundStyle(color)
        .lineLimit(1)
    }
}

/// A photo inside a bubble. Starts from the synchronous caches (local send or earlier load)
/// so re-shown bubbles never flash; the height comes from known dimensions to avoid jumps.
struct ChatImage: View {
    let m: UiMessage
    let attachment: FileAttachment
    let onOpen: (UIImage?) -> Void

    @State private var image: UIImage?
    private var media: MediaCache { SessionStore.shared.media }

    var body: some View {
        let a = attachment
        let pending = a.id.isEmpty
        let shown = image ?? (pending ? media.localPreview(m.msg.referenceId) : media.cachedPreview(a.id))
        let aspect: CGFloat = {
            if a.width > 0 && a.height > 0 { return CGFloat(a.width) / CGFloat(a.height) }
            if let shown, shown.size.height > 0 { return shown.size.width / shown.size.height }
            return media.aspect(a.id) ?? 1
        }()
        let height = min(340, max(120, chatImageWidth / aspect))
        ZStack {
            Color.black.opacity(0.08)
            if let shown {
                Image(uiImage: shown).resizable().scaledToFill()
                    .frame(width: chatImageWidth, height: height)
                    .clipped()
            }
            if m.status == .sending && (0..<0.999).contains(m.progress) {
                ZStack {
                    Circle().fill(.black.opacity(0.35)).frame(width: 44, height: 44)
                    Circle().stroke(.white.opacity(0.25), lineWidth: 3).frame(width: 34, height: 34)
                    Circle().trim(from: 0, to: CGFloat(max(0.03, m.progress)))
                        .stroke(.white, style: StrokeStyle(lineWidth: 3, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                        .frame(width: 34, height: 34)
                }
            } else if m.status == .sending || (m.status == nil && shown == nil && a.previewAvailable) {
                ProgressView().tint(.white).controlSize(.large)
            }
        }
        .frame(width: chatImageWidth, height: height)
        .contentShape(Rectangle())
        .onTapGesture { onOpen(shown) }
        .task(id: "\(a.id)|\(m.msg.referenceId)") {
            guard shown == nil else { return }
            image = pending
                ? await media.loadLocalPreview(m.msg.referenceId, path: m.localFile)
                : await media.preview(a.id)
        }
        .accessibilityLabel("Foto")
        .accessibilityIdentifier("chat-photo")
        .accessibilityAddTraits(.isButton)
    }
}

/// A non-image file (docs/attachments.md "Dateikachel"): icon, name, size. Sending or
/// waiting: the upload ring in place of the icon (a spinner while the progress is unknown),
/// failed: the icon again; tapping a failed or waiting one sends it again.
/// Delivered: tapping loads it once (spinner) and opens it in the preview.
struct FileRow: View {
    let m: UiMessage
    let attachment: FileAttachment
    let textColor: Color
    let onRetry: (MessageView) -> Void

    var body: some View {
        let a = attachment
        let opener = FileOpener.shared
        let loading = opener.isLoading(a)
        HStack(spacing: 10) {
            ZStack {
                Circle().fill(textColor.opacity(0.12))
                let pending = m.status == .sending || m.status == .waiting
                if pending && (0..<0.999).contains(m.progress) {
                    Circle().stroke(textColor.opacity(0.25), lineWidth: 3).padding(5)
                    Circle().trim(from: 0, to: CGFloat(max(0.03, m.progress)))
                        .stroke(textColor, style: StrokeStyle(lineWidth: 3, lineCap: .round))
                        .rotationEffect(.degrees(-90))
                        .padding(5)
                } else if pending || loading {
                    ProgressView().tint(textColor)
                } else {
                    Image(systemName: FileIcon.name(a.mimetype)).font(.system(size: 18))
                }
            }
            .frame(width: 40, height: 40)
            VStack(alignment: .leading, spacing: 1) {
                Text(a.name).font(.system(size: 15, weight: .medium)).lineLimit(2)
                Text(Format.fileSize(a.size)).font(.system(size: 12)).opacity(0.75)
            }
        }
        .foregroundStyle(textColor)
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        // A tap gesture, not a Button: a Button would swallow the long press of the list
        // that opens the message menu.
        .onTapGesture {
            switch m.status {
            case .failed, .waiting: onRetry(m.msg)
            case .sending: break
            case nil: opener.open(a)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(a.name), \(Format.fileSize(a.size))")
        .accessibilityValue(loading ? "Wird geladen" : "")
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier("file-row")
    }
}

/// SF Symbols for file types.
enum FileIcon {
    static func name(_ mime: String) -> String {
        if mime.hasPrefix("video/") { return "film" }
        if mime.hasPrefix("audio/") { return "music.note" }
        if mime.hasPrefix("image/") { return "photo" }
        if mime == "application/pdf" { return "doc.richtext" }
        if mime.hasPrefix("text/") { return "doc.text" }
        if mime.contains("zip") || mime.contains("compressed") || mime.contains("tar") { return "doc.zipper" }
        return "doc"
    }
}

/// A shared place in a bubble (docs/attachments.md "Blase"): the map with the corners of a
/// photo, the name below in the bubble's text colour. Tapping opens Apple Maps.
private struct PlaceContent: View {
    let m: UiMessage
    let place: GeoLocation
    let textColor: Color
    let inset: CGFloat
    let above: Bool
    /// Outer corners that may be round: top leading, bottom leading, bottom trailing, top trailing.
    let corners: (Bool, Bool, Bool, Bool)
    let onRetry: (MessageView) -> Void

    var body: some View {
        let r: CGFloat = 15, inner: CGFloat = 4
        VStack(alignment: .leading, spacing: 6) {
            PlaceMap(latitude: place.latitude, longitude: place.longitude, size: placeMapSize)
                .clipShape(UnevenRoundedRectangle(
                    topLeadingRadius: above ? inner : (corners.0 ? r : inner),
                    bottomLeadingRadius: inner,
                    bottomTrailingRadius: inner,
                    topTrailingRadius: above ? inner : (corners.3 ? r : inner)
                ))
            Text(Format.placeName(place))
                .font(.system(size: 15))
                .foregroundStyle(textColor)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, inset)
        }
        .frame(width: placeMapSize.width, alignment: .leading)
        .contentShape(Rectangle())
        .onTapGesture {
            switch m.status {
            case .failed, .waiting: onRetry(m.msg)
            case .sending: break
            case nil: Places.openInMaps(place)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Standort: \(Format.placeName(place))")
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier("place-bubble")
    }
}
