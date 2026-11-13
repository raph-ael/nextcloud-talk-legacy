import Foundation
import SwiftUI

/// Dates, names and labels – same rules as Android `ui/Format.kt`.
enum Format {
    private static let de = Locale(identifier: "de_DE")
    private static var calendar: Calendar {
        var c = Calendar(identifier: .gregorian)
        c.locale = de
        c.timeZone = .current
        return c
    }

    private static func formatter(_ pattern: String) -> DateFormatter {
        let f = DateFormatter()
        f.locale = de
        f.dateFormat = pattern
        return f
    }

    private static let time = formatter("HH:mm")
    private static let weekday = formatter("EEE")
    private static let shortDate = formatter("dd.MM.yy")
    private static let monthDay = formatter("d. MMM")
    private static let day = formatter("EEE, d. MMM")
    private static let dayYear = formatter("d. MMM yyyy")
    private static let fileStamp: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd HH-mm-ss"
        return f
    }()

    private static func date(_ unix: Int64) -> Date { Date(timeIntervalSince1970: TimeInterval(unix)) }

    private static func daysAgo(_ unix: Int64) -> Int {
        let c = calendar
        return c.dateComponents([.day], from: c.startOfDay(for: date(unix)), to: c.startOfDay(for: Date())).day ?? 0
    }

    /// Conversation list, like Signal: Jetzt · 5 Min. · 3 Std. · Fr. · 12. Okt. · 12.10.25
    static func listTime(_ unix: Int64, now: Date = Date()) -> String {
        guard unix > 0 else { return "" }
        let secs = Int64(now.timeIntervalSince1970) - unix
        let d = date(unix)
        if secs < 60 { return "Jetzt" }
        if secs < 3600 { return "\(secs / 60) Min." }
        if secs < 24 * 3600 { return "\(secs / 3600) Std." }
        if daysAgo(unix) < 7 { return weekday.string(from: d) }
        if calendar.component(.year, from: d) == calendar.component(.year, from: now) { return monthDay.string(from: d) }
        return shortDate.string(from: d)
    }

    static func messageTime(_ unix: Int64) -> String { time.string(from: date(unix)) }

    /// Day dividers and the floating date (docs/chat.md): Heute · Gestern · Mi., 11. Nov.
    /// (up to six months back) · 11. Nov. 2025.
    static func dayLabel(_ unix: Int64, now: Date = Date()) -> String {
        let c = calendar
        let value = date(unix)
        let d = c.dateComponents([.day], from: c.startOfDay(for: value), to: c.startOfDay(for: now)).day ?? 0
        if d == 0 { return "Heute" }
        if d == 1 { return "Gestern" }
        if let limit = c.date(byAdding: .month, value: -6, to: c.startOfDay(for: now)), value >= limit {
            return day.string(from: value)
        }
        return dayYear.string(from: value)
    }

    static func sameDay(_ a: Int64, _ b: Int64) -> Bool {
        calendar.isDate(date(a), inSameDayAs: date(b))
    }

    /// "Sprachnachricht 2026-11-09 16-05-12" (the core appends .mp3).
    static func voiceFileName(_ now: Date = Date()) -> String {
        "Sprachnachricht " + fileStamp.string(from: now)
    }

    private static let palette: [UInt32] = [
        0xD0463B, 0xD9822B, 0x3D9A50, 0x2C7BD6, 0x6B5BD2, 0xC2408F, 0x2A9A9E, 0x8A7350,
    ]

    /// Stable color per name (same hash as Android and desktop).
    static func color(for name: String) -> Color {
        var h: Int64 = 0
        for scalar in name.unicodeScalars {
            h = (h &* 31 &+ Int64(scalar.value)) & 0x7fff_ffff
        }
        return Color(hex: palette[Int(h % Int64(palette.count))])
    }

    static func initials(_ name: String) -> String {
        let words = name.split(whereSeparator: { $0 == " " || $0 == "." }).prefix(2)
        let letters = words.compactMap { $0.first.map(String.init) }.joined()
        return letters.isEmpty ? "?" : letters.uppercased()
    }

    /// 1–3 emoji without other text are shown large (like Signal).
    static func isJumboEmoji(_ text: String) -> Bool {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, t.count <= 10 else { return false }
        var count = 0
        for ch in t where !ch.isWhitespace {
            let scalars = ch.unicodeScalars
            let emoji = scalars.contains { $0.properties.isEmojiPresentation }
                || (scalars.first?.properties.isEmoji == true && scalars.count > 1)
            if !emoji { return false }
            count += 1
        }
        return (1...3).contains(count)
    }

    /// Short label for a file message: "📷 Foto", "🎤 Sprachnachricht", "📎 name.pdf".
    static func attachmentLabel(_ a: FileAttachment) -> String {
        if a.voiceMessage { return "🎤 Sprachnachricht" }
        if a.mimetype.hasPrefix("image/") { return "📷 Foto" }
        if a.mimetype.hasPrefix("video/") { return "🎬 Video" }
        return "📎 \(a.name)"
    }

    /// One-line preview of a message that may carry a file: label plus caption.
    static func withAttachment(_ text: String, _ a: FileAttachment?) -> String {
        guard let a else { return text }
        return text.isEmpty ? attachmentLabel(a) : "\(attachmentLabel(a)) · \(text)"
    }

    /// 1,2 MB style file size.
    static func fileSize(_ bytes: Int64) -> String {
        if bytes < 1024 { return "\(bytes) B" }
        if bytes < 1024 * 1024 { return "\(bytes / 1024) KB" }
        return String(format: "%.1f MB", locale: de, Double(bytes) / (1024 * 1024))
    }

    /// 75 000 → "1:15", 3 723 000 → "1:02:03".
    static func clock(_ ms: Int64) -> String {
        let total = max(0, (ms + 500) / 1000)
        let h = total / 3600, m = (total % 3600) / 60, s = total % 60
        return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
    }
}

extension FileAttachment {
    var isImage: Bool { mimetype.hasPrefix("image/") && !voiceMessage }
    var isAudio: Bool { voiceMessage || mimetype.hasPrefix("audio/") }
}

extension MessageView {
    var toReply: ReplyPreview {
        ReplyPreview(id: id, author: author, text: text, isOwn: isOwn, attachment: attachment)
    }
}

extension RoomView {
    /// One-to-one (1) and note-to-self (6) conversations show no author names.
    var isGroup: Bool { roomType != 1 && roomType != 6 }
}
