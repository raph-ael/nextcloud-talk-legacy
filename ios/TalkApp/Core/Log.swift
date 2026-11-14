import os

/// Unified logging (`log stream --predicate 'subsystem == "org.wolkenfunk.app"'`).
enum Log {
    static let voice = Logger(subsystem: "org.wolkenfunk.app", category: "voice")
    static let media = Logger(subsystem: "org.wolkenfunk.app", category: "media")
    static let session = Logger(subsystem: "org.wolkenfunk.app", category: "session")
    static let call = Logger(subsystem: "org.wolkenfunk.app", category: "call")
    static let push = Logger(subsystem: "org.wolkenfunk.app", category: "push")
}

/// The Rust core's logs (tracing) go to the unified log under category "core".
final class CoreLog: LogSink, @unchecked Sendable {
    private static let logger = Logger(subsystem: "org.wolkenfunk.app", category: "core")

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
import UserNotifications

/// Debug builds with `TALK_LOG_DUMP=1`: the app's own log entries (subsystem
/// org.wolkenfunk.app) are written to Documents/talk.log after each call, so device tests can
/// fetch them (`devicectl device copy from --domain-type appDataContainer`).
enum LogDump {
    static var enabled: Bool { DebugEnv.sticky("TALK_LOG_DUMP") == "1" }

    /// `sync`: right before the app is suspended (an async write would not finish).
    static func write(sync: Bool = false) {
        guard enabled else { return }
        let run = {
            guard let store = try? OSLogStore(scope: .currentProcessIdentifier) else { return }
            let since = store.position(timeIntervalSinceLatestBoot: 0)
            let entries = (try? store.getEntries(at: since, matching: NSPredicate(format: "subsystem == 'org.wolkenfunk.app'"))) ?? AnySequence([])
            let f = DateFormatter()
            f.dateFormat = "HH:mm:ss.SSS"
            let text = entries.compactMap { $0 as? OSLogEntryLog }
                .map { "\(f.string(from: $0.date)) [\($0.category)] \($0.composedMessage)" }
                .joined(separator: "\n")
            let url = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("talk.log")
            try? text.write(to: url, atomically: true, encoding: .utf8)
        }
        if sync { run() } else { DispatchQueue.global(qos: .utility).async(execute: run) }
    }

    /// At launch: what Notification Center shows (checks the NotifyExt texts on the device).
    @MainActor static func deliveredNotifications() async {
        guard enabled else { return }
        let center = UNUserNotificationCenter.current()
        let auth = await center.notificationSettings().authorizationStatus.rawValue
        let delivered = await center.deliveredNotifications()
        Log.push.info("notifications: auth \(auth, privacy: .public), \(delivered.count, privacy: .public) delivered")
        for n in delivered {
            let c = n.request.content
            Log.push.info("delivered: title '\(c.title, privacy: .public)' body '\(c.body, privacy: .public)' thread \(c.threadIdentifier.isEmpty ? "-" : "set", privacy: .public) nid \(String(describing: c.userInfo["nid"] ?? "-"), privacy: .public)")
        }
        write()
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

#if DEBUG
import UIKit

/// Debug (`TALK_LOG_DUMP=1`): every 2 s a background thread appends the app state and how
/// long ago the main thread last ticked to Documents/heartbeat.log – tells a suspended app
/// (no lines) from a hung main thread (lines, main age growing).
enum Heartbeat {
    nonisolated(unsafe) private static var mainTick = Date()
    nonisolated(unsafe) private static var state = "?"
    nonisolated(unsafe) private static var timers: [DispatchSourceTimer] = []

    @MainActor static func start() {
        guard LogDump.enabled, timers.isEmpty else { return }
        let url = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("heartbeat.log")
        try? "".write(to: url, atomically: true, encoding: .utf8)
        let main = DispatchSource.makeTimerSource(queue: .main)
        main.schedule(deadline: .now(), repeating: 1)
        main.setEventHandler {
            mainTick = Date()
            state = MainActor.assumeIsolated {
                let call = CallController.shared.call.map { "\($0.state)" } ?? "none"
                return "\(UIApplication.shared.applicationState.rawValue) bg=\(Int(min(UIApplication.shared.backgroundTimeRemaining, 9999))) call=\(call)"
            }
        }
        let bg = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
        bg.schedule(deadline: .now(), repeating: 2)
        let f = DateFormatter()
        f.dateFormat = "HH:mm:ss"
        bg.setEventHandler {
            let line = "\(f.string(from: Date())) state=\(state) mainAge=\(String(format: "%.1f", Date().timeIntervalSince(mainTick)))\n"
            if let h = try? FileHandle(forWritingTo: url) {
                h.seekToEndOfFile()
                h.write(Data(line.utf8))
                try? h.close()
            }
        }
        main.resume()
        bg.resume()
        timers = [main, bg]
    }
}
#endif
