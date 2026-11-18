import SwiftUI

/// The search rule of the core (docs/search.md "Suchregel") for marking words in bubbles:
/// lowercase, accents dropped ("Hüpfburg" → "hupfburg"), "ß" stays, hits also inside words.
enum SearchText {
    /// The words of a search text, folded; empty while there is nothing to search for
    /// (less than two characters without spaces).
    static func words(_ query: String) -> [[Unicode.Scalar]] {
        let words = query.split(whereSeparator: { $0.isWhitespace }).map { fold(String($0)) }.filter { !$0.isEmpty }
        return words.reduce(0) { $0 + $1.count } >= 2 ? words : []
    }

    /// Whether a search text is long enough for message hits.
    static func searchable(_ query: String) -> Bool { !words(query).isEmpty }

    static func fold(_ text: String) -> [Unicode.Scalar] {
        text.unicodeScalars.flatMap(fold)
    }

    /// Like the core's `fold_char`: lowercase, canonically decomposed, without marks.
    private static func fold(_ c: Unicode.Scalar) -> [Unicode.Scalar] {
        if c.isASCII { return String(c).lowercased().unicodeScalars.map { $0 } }
        return String(c).lowercased().decomposedStringWithCanonicalMapping.unicodeScalars.filter {
            switch $0.properties.generalCategory {
            case .nonspacingMark, .spacingMark, .enclosingMark: false
            default: true
            }
        }
    }

    /// `text` with every occurrence of the words laid on `searchMark` (docs/search.md
    /// "Hervorhebung"); the text colour stays.
    static func mark(_ text: AttributedString, _ words: [[Unicode.Scalar]]) -> AttributedString {
        guard !words.isEmpty else { return text }
        // Folded scalars, each with the character of `text` it came from.
        let chars = Array(text.characters.indices)
        var folded: [Unicode.Scalar] = []
        var origin: [Int] = []
        for (i, at) in chars.enumerated() {
            for s in String(text.characters[at]).unicodeScalars {
                let f = fold(s)
                folded += f
                origin += Array(repeating: i, count: f.count)
            }
        }
        var marked = [Bool](repeating: false, count: chars.count)
        var any = false
        for w in words where !w.isEmpty && w.count <= folded.count {
            var from = 0
            while from + w.count <= folded.count {
                if Array(folded[from..<from + w.count]) == w {
                    for i in origin[from]...origin[from + w.count - 1] { marked[i] = true }
                    any = true
                    from += w.count
                } else {
                    from += 1
                }
            }
        }
        guard any else { return text }
        var out = text
        var i = 0
        while i < chars.count {
            guard marked[i] else { i += 1; continue }
            var j = i
            while j + 1 < chars.count && marked[j + 1] { j += 1 }
            let range = chars[i]..<out.characters.index(after: chars[j])
            out[range].backgroundColor = Talk.searchMark
            i = j + 1
        }
        return out
    }
}

/// Message hits for a search text (docs/search.md): `searchMessages` off the main thread on
/// every change, results for an outdated text dropped; while the history is not all on the
/// device, `searchMessagesEverywhere` 300 ms after the last change replaces them.
@MainActor
@Observable
final class MessageSearch {
    /// Hits for `query`; nil while there is nothing to search for.
    private(set) var results: SearchResults?
    /// Bumps with every new result, also for the same text (the chat jumps to the newest).
    private(set) var version = 0
    private(set) var query = ""
    @ObservationIgnored private var limit: UInt32
    @ObservationIgnored private let step: UInt32
    @ObservationIgnored private let token: String?
    @ObservationIgnored private var local: Task<Void, Never>?
    @ObservationIgnored private var remote: Task<Void, Never>?
    /// The shown hits came from the server too: a refresh keeps them until the server answers.
    @ObservationIgnored private var fromServer = false

    init(token: String? = nil, limit: UInt32) {
        self.token = token
        self.limit = limit
        self.step = limit
    }

    /// A new search text: from the first page again.
    func set(_ text: String, session: TalkSession?) {
        guard text != query else { return }
        query = text
        limit = step
        fromServer = false
        run(session, refresh: false)
    }

    /// The end of the list was reached and there are more: one page more.
    func more(session: TalkSession?) {
        guard results?.more == true else { return }
        limit += step
        run(session, refresh: true)
    }

    /// The same search again (new messages, history loaded further).
    func refresh(session: TalkSession?) {
        run(session, refresh: true)
    }

    func stop() {
        local?.cancel()
        remote?.cancel()
    }

    private func run(_ session: TalkSession?, refresh: Bool) {
        local?.cancel()
        remote?.cancel()
        let q = query, limit = limit, token = token
        guard let session, SearchText.searchable(q) else {
            results = nil
            version += 1
            return
        }
        local = Task {
            let r = await Task.detached(priority: .userInitiated) {
                session.searchMessages(query: q, token: token, limit: limit)
            }.value
            guard !Task.isCancelled, q == self.query else { return }
            if refresh && fromServer && !r.complete, var shown = results {
                // Until the server answers again its hits stay; only the progress moves on.
                shown.chatsDone = r.chatsDone
                shown.chatsTotal = r.chatsTotal
                results = shown
            } else {
                results = r
                fromServer = false
                version += 1
            }
            guard !r.complete else { return }
            remote = Task {
                try? await Task.sleep(for: .milliseconds(300))
                guard !Task.isCancelled else { return }
                // An error (no network) changes nothing and shows nothing.
                guard let all = try? await session.searchMessagesEverywhere(query: q, token: token, limit: limit),
                      !Task.isCancelled, q == self.query else { return }
                results = all
                fromServer = true
                version += 1
            }
        }
    }
}

/// One message hit in the chat list (docs/search.md "Trefferzeile"): chat avatar and name,
/// the date on the right, below it the snippet with the search words bold.
struct MessageHitRow: View {
    let hit: SearchHit
    let avatarVersion: String
    let now: Date

    var body: some View {
        HStack(spacing: 16) {
            Avatar(name: hit.roomName, size: 48, token: hit.token, version: avatarVersion)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(hit.roomName)
                        .font(.system(size: 16))
                        .foregroundStyle(Talk.onSurface)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Text(Format.listTime(hit.timestamp, now: now))
                        .font(.system(size: 14))
                        .foregroundStyle(Talk.muted)
                        .lineLimit(1)
                }
                snippet
                    .font(.system(size: 14))
                    .lineLimit(2, reservesSpace: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .frame(minHeight: 84)
        .contentShape(Rectangle())
    }

    private var snippet: Text {
        var out = Text(hit.showActor ? (hit.isOwn ? "Du: " : "\(hit.actorName): ") : "").foregroundColor(Talk.muted)
        for part in hit.snippet {
            out = out + (part.hit
                ? Text(part.text).fontWeight(.bold).foregroundColor(Talk.onSurface)
                : Text(part.text).foregroundColor(Talk.muted))
        }
        return out
    }
}

/// Instead of the input row while searching in a chat (docs/search.md "Leiste"): the count
/// on the left, the arrows to the older and the newer hit on the right.
struct ChatSearchBar: View {
    /// nil: the field is empty (no count).
    let results: SearchResults?
    /// Index into `hits`, 0 = newest.
    let selected: Int
    let onOlder: () -> Void
    let onNewer: () -> Void

    var body: some View {
        let hits = results?.hits.count ?? 0
        HStack(spacing: 4) {
            Group {
                if let results {
                    Text(hits == 0 ? "Keine Treffer" : "\(selected + 1) von \(results.total)")
                }
            }
            .font(.system(size: 15))
            .foregroundStyle(Talk.onSurface)
            .monospacedDigit()
            .accessibilityIdentifier("chat-search-count")
            Spacer(minLength: 0)
            arrow("chevron.up", "Älterer Treffer", id: "chat-search-older", enabled: selected + 1 < hits, action: onOlder)
            arrow("chevron.down", "Neuerer Treffer", id: "chat-search-newer", enabled: hits > 0 && selected > 0, action: onNewer)
        }
        .padding(.leading, 16)
        .padding(.trailing, 6)
        .frame(minHeight: 52)
        .background(Talk.background)
    }

    private func arrow(_ icon: String, _ label: String, id: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 18, weight: .semibold))
                .frame(width: 44, height: 44)
        }
        .foregroundStyle(enabled ? Talk.accent : Talk.muted.opacity(0.5))
        .disabled(!enabled)
        .accessibilityLabel(label)
        .accessibilityIdentifier(id)
    }
}
