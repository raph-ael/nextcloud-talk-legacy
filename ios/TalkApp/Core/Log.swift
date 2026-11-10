import os

/// Unified logging (`log stream --predicate 'subsystem == "org.sirplus.talk"'`).
enum Log {
    static let voice = Logger(subsystem: "org.sirplus.talk", category: "voice")
    static let media = Logger(subsystem: "org.sirplus.talk", category: "media")
    static let session = Logger(subsystem: "org.sirplus.talk", category: "session")
    static let call = Logger(subsystem: "org.sirplus.talk", category: "call")
    static let push = Logger(subsystem: "org.sirplus.talk", category: "push")
}

/// The Rust core's logs (tracing) go to the unified log under category "core".
final class CoreLog: LogSink, @unchecked Sendable {
    private static let logger = Logger(subsystem: "org.sirplus.talk", category: "core")

    /// Once, before the first session. Debug builds log the call machinery in detail.
    static func install() {
        #if DEBUG
        initLogging(sink: CoreLog(), filter: "talk_core=info,talk_core::call=debug")
        #else
        initLogging(sink: CoreLog(), filter: nil)
        #endif
    }

    func log(level: LogLevel, target: String, message: String) {
        let type: OSLogType = switch level {
        case .error: .error
        case .warn: .default
        case .info: .info
        // Debug entries are not kept in the log store; LogDump reads info and up.
        case .debug, .trace: .info
        }
        #if DEBUG
        Self.logger.log(level: type, "\(target, privacy: .public): \(message, privacy: .public)")
        #else
        Self.logger.log(level: type, "\(target, privacy: .public): \(message, privacy: .private)")
        #endif
    }
}

#if DEBUG
import Foundation
import OSLog

/// Debug builds with `TALK_LOG_DUMP=1`: the app's own log entries (subsystem
/// org.sirplus.talk) are written to Documents/talk.log after each call, so device tests can
/// fetch them (`devicectl device copy from --domain-type appDataContainer`).
enum LogDump {
    static var enabled: Bool { DebugEnv.sticky("TALK_LOG_DUMP") == "1" }

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

#if DEBUG
/// Debug switches that survive a launch without environment (a cold start from a VoIP push):
/// a value given at launch is kept until a launch gives it again (empty value: off).
enum DebugEnv {
    /// At launch: keep the switches given now, before anything reads them.
    static func persist() {
        for key in ["TALK_AUTOANSWER", "TALK_LOG_DUMP"] { _ = sticky(key) }
    }

    static func sticky(_ key: String) -> String? {
        let d = UserDefaults.standard
        if let v = ProcessInfo.processInfo.environment[key] {
            if v.isEmpty { d.removeObject(forKey: "debug.\(key)") } else { d.set(v, forKey: "debug.\(key)") }
            return v.isEmpty ? nil : v
        }
        return d.string(forKey: "debug.\(key)")
    }
}
#endif
