import XCTest

/// Measurement (not a regular test): time from tapping a chat notification until the new
/// message shows in the chat (docs/notifications.md "Die Nachricht ist dann schon da"). A host
/// script drives it through files in `TALK_TAP_DIR`: the test opens the starting state, leaves
/// the app (`A`/`B`: home, `C`: terminated), waits `TALK_TAP_WAIT` s and writes `ready`; the
/// host then sends the message, may hand the push to the app's debug inbox
/// (`TALK_DEV_PUSH_INBOX`, as `simctl push` never wakes the app) and `simctl push`es the
/// notification; the test taps its banner and polls for `TALK_TAP_TEXT`, writing `result`.
/// `TALK_TAP_ROOM`: the chat the message goes to; `TALK_TAP_OTHER`: the chat open before
/// (case B; empty: the chat list); `TALK_TAP_SCROLL=1`: the open chat is not at its end.
final class NotificationTapTiming: XCTestCase {
    private let env = ProcessInfo.processInfo.environment
    private let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")

    private func write(_ name: String, _ text: String) {
        let dir = URL(fileURLWithPath: env["TALK_TAP_DIR"] ?? "/tmp/wf-tap")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? Data(text.utf8).write(to: dir.appendingPathComponent(name))
    }

    private func now() -> Double { Date().timeIntervalSince1970 }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    func testTap() throws {
        let kind = env["TALK_TAP_CASE"] ?? ""
        let wait = UInt32(env["TALK_TAP_WAIT"] ?? "60") ?? 60
        let text = env["TALK_TAP_TEXT"] ?? ""
        let room = env["TALK_TAP_ROOM"] ?? ""
        let other = env["TALK_TAP_OTHER"] ?? ""
        try XCTSkipIf(kind.isEmpty || text.isEmpty || room.isEmpty, "measurement only (host script)")

        let app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS", "TALK_DEV_SAVE", "TALK_DEV_PUSH_KEYS", "TALK_DEV_PUSH_INBOX"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        app.launchArguments += ["-AppleLanguages", "(de)", "-AppleLocale", "de_DE"]
        app.launch()
        // First start after installing: the system asks for notifications.
        let alert = springboard.alerts.firstMatch
        if alert.waitForExistence(timeout: 8) {
            for l in ["Erlauben", "Allow"] where alert.buttons[l].exists { alert.buttons[l].tap(); break }
        }
        let start = kind == "A" ? room : other
        if !start.isEmpty {
            let row = app.buttons["room:\(start)"].firstMatch
            XCTAssertTrue(row.waitForExistence(timeout: 30), "\(start) not listed")
            row.tap()
            XCTAssertTrue(app.descendants(matching: .any)["composer"].firstMatch.waitForExistence(timeout: 10))
        } else {
            XCTAssertTrue(app.buttons["settings"].waitForExistence(timeout: 30), "no chat list")
        }
        sleep(3)
        // `TALK_TAP_SCROLL=1`: the open chat is not at its end.
        let scrolled = env["TALK_TAP_SCROLL"] == "1"
        if scrolled {
            let list = app.collectionViews["messages"].firstMatch
            for _ in 0..<3 { list.swipeDown() }
            sleep(1)
        }
        shot("vorher")
        if kind == "C" {
            app.terminate()
        } else {
            XCUIDevice.shared.press(.home)
        }
        sleep(wait)
        write("ready", "\(now())")

        // The banner (title, body); gone after a few seconds, so tapped at once.
        let banner = springboard.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
        XCTAssertTrue(banner.waitForExistence(timeout: 90), "no banner")
        let seen = now()
        // 1 not running, 2 suspended, 3 running in the background.
        let before = app.state.rawValue
        banner.tap()
        let tapped = now()

        let message = app.staticTexts[text].firstMatch
        var visible: Double?
        var shots = 0
        while now() - tapped < 60 {
            // On screen, not just in the list (a chat that is not at its end).
            // (Each query costs a snapshot: the frame only where the chat was not at its end.)
            if message.exists, !scrolled || app.windows.firstMatch.frame.intersects(message.frame) {
                visible = now(); break
            }
            if shots < 3, now() - tapped > Double(shots + 1) {
                shots += 1
                shot("nach-\(shots)s")
            }
            usleep(100_000)
        }
        shot("sichtbar")
        let inRoom = app.descendants(matching: .any)["composer"].firstMatch.exists
        let line = String(format: "state=%d seen=%.3f tapped=%.3f visible=%@ composer=%@",
                          Int(before), seen, tapped, visible.map { String(format: "%.3f", $0) } ?? "-", inRoom ? "ja" : "nein")
        write("result", line)
        XCTAssertNotNil(visible, "message not shown within 60 s")
    }
}
