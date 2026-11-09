import Foundation
import SwiftUI

/// Talk's Markdown subset → AttributedString (headings, lists, quotes, code, emphasis, links).
/// Port of Android `ui/Markdown.kt`. Plain text when `markdown` is false (links stay clickable).
enum Markdown {
    private static let inline = try! NSRegularExpression(
        pattern: "(`[^`\\n]+`)|(\\*\\*[^*\\n]+\\*\\*|__[^_\\n]+__)|(~~[^~\\n]+~~)|(\\*[^*\\s][^*\\n]*\\*|_[^_\\s][^_\\n]*_)"
            + "|(\\[[^\\]\\n]+\\]\\(https?://[^\\s)]+\\))|(https?://[^\\s<>\"']+[^\\s<>\"'.,;:!?)\\]])"
    )
    private static let heading = try! NSRegularExpression(pattern: "^\\s{0,3}#{1,6}\\s+(.*)$")
    private static let bullet = try! NSRegularExpression(pattern: "^\\s*[-*+]\\s+(.*)$")
    private static let quote = try! NSRegularExpression(pattern: "^\\s*>\\s?(.*)$")
    private static let rule = try! NSRegularExpression(pattern: "^\\s{0,3}([-*_])(\\s*\\1){2,}\\s*$")

    static func render(_ text: String, markdown: Bool) -> AttributedString {
        var out = AttributedString()
        if !markdown {
            appendInline(text, into: &out, markdown: false, intent: [])
            return out
        }
        var inCode = false
        let lines = text.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
        for (i, raw) in lines.enumerated() {
            if raw.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                inCode.toggle()
                continue
            }
            if inCode {
                var code = AttributedString(raw)
                code.font = .system(size: 13, design: .monospaced)
                out += code
            } else if let h = group(heading, raw) {
                var part = AttributedString()
                appendInline(h, into: &part, markdown: true, intent: [])
                part.font = .system(size: 16, weight: .bold)
                out += part
            } else if matches(rule, raw) {
                out += AttributedString("———")
            } else if let b = group(bullet, raw) {
                out += AttributedString("•  ")
                appendInline(b, into: &out, markdown: true, intent: [])
            } else if let q = group(quote, raw) {
                var part = AttributedString("▎ ")
                appendInline(q, into: &part, markdown: true, intent: .emphasized)
                out += part
            } else {
                appendInline(raw, into: &out, markdown: true, intent: [])
            }
            if i < lines.count - 1 { out += AttributedString("\n") }
        }
        return out
    }

    private static func group(_ re: NSRegularExpression, _ s: String) -> String? {
        let ns = s as NSString
        guard let m = re.firstMatch(in: s, range: NSRange(location: 0, length: ns.length)) else { return nil }
        return ns.substring(with: m.range(at: 1))
    }

    private static func matches(_ re: NSRegularExpression, _ s: String) -> Bool {
        let ns = s as NSString
        return re.firstMatch(in: s, range: NSRange(location: 0, length: ns.length)) != nil
    }

    private static func plain(_ s: String, _ intent: InlinePresentationIntent) -> AttributedString {
        var a = AttributedString(s)
        if !intent.isEmpty { a.inlinePresentationIntent = intent }
        return a
    }

    private static func appendInline(
        _ s: String, into out: inout AttributedString, markdown: Bool, intent: InlinePresentationIntent
    ) {
        let ns = s as NSString
        var last = 0
        for m in inline.matches(in: s, range: NSRange(location: 0, length: ns.length)) {
            let isURL = m.range(at: 6).location != NSNotFound
            if !markdown && !isURL { continue }
            out += plain(ns.substring(with: NSRange(location: last, length: m.range.location - last)), intent)
            let tok = ns.substring(with: m.range)
            if m.range(at: 1).location != NSNotFound {
                out += plain(String(tok.dropFirst().dropLast()), intent.union(.code))
            } else if m.range(at: 2).location != NSNotFound {
                appendInline(String(tok.dropFirst(2).dropLast(2)), into: &out, markdown: true, intent: intent.union(.stronglyEmphasized))
            } else if m.range(at: 3).location != NSNotFound {
                appendInline(String(tok.dropFirst(2).dropLast(2)), into: &out, markdown: true, intent: intent.union(.strikethrough))
            } else if m.range(at: 4).location != NSNotFound {
                appendInline(String(tok.dropFirst().dropLast()), into: &out, markdown: true, intent: intent.union(.emphasized))
            } else if m.range(at: 5).location != NSNotFound, let close = tok.range(of: "](") {
                let label = String(tok[tok.index(after: tok.startIndex)..<close.lowerBound])
                let url = String(tok[close.upperBound..<tok.index(before: tok.endIndex)])
                out += link(label, url, intent)
            } else {
                out += link(tok, tok, intent)
            }
            last = m.range.location + m.range.length
        }
        out += plain(ns.substring(from: last), intent)
    }

    private static func link(_ label: String, _ url: String, _ intent: InlinePresentationIntent) -> AttributedString {
        var a = plain(label, intent)
        a.link = URL(string: url)
        a.underlineStyle = .single
        return a
    }
}
