import os

/// Unified logging (`log stream --predicate 'subsystem == "org.sirplus.talk"'`).
enum Log {
    static let voice = Logger(subsystem: "org.sirplus.talk", category: "voice")
    static let media = Logger(subsystem: "org.sirplus.talk", category: "media")
    static let session = Logger(subsystem: "org.sirplus.talk", category: "session")
    static let call = Logger(subsystem: "org.sirplus.talk", category: "call")
}

#if DEBUG
import Foundation
import OSLog

/// Debug builds with `TALK_LOG_DUMP=1`: the app's own log entries (subsystem
/// org.sirplus.talk) are written to Documents/talk.log after each call, so device tests can
/// fetch them (`devicectl device copy from --domain-type appDataContainer`).
enum LogDump {
    static var enabled: Bool { ProcessInfo.processInfo.environment["TALK_LOG_DUMP"] == "1" }

    static func write() {
        guard enabled else { return }
        DispatchQueue.global(qos: .utility).async {
            guard let store = try? OSLogStore(scope: .currentProcessIdentifier) else { return }
            let since = store.position(timeIntervalSinceLatestBoot: 0)
            let entries = (try? store.getEntries(at: since, matching: NSPredicate(format: "subsystem == 'org.sirplus.talk'"))) ?? AnySequence([])
            let f = DateFormatter()
            f.dateFormat = "HH:mm:ss.SSS"
            let text = entries.compactMap { $0 as? OSLogEntryLog }
                .map { "\(f.string(from: $0.date)) [\($0.category)] \($0.composedMessage)" }
                .joined(separator: "\n")
            let url = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("talk.log")
            try? text.write(to: url, atomically: true, encoding: .utf8)
        }
    }
}
#endif
