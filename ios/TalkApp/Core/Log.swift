import os

/// Unified logging (`log stream --predicate 'subsystem == "org.sirplus.talk"'`).
enum Log {
    static let voice = Logger(subsystem: "org.sirplus.talk", category: "voice")
    static let media = Logger(subsystem: "org.sirplus.talk", category: "media")
    static let session = Logger(subsystem: "org.sirplus.talk", category: "session")
    static let call = Logger(subsystem: "org.sirplus.talk", category: "call")
}
