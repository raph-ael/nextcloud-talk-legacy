import XCTest

/// Call recording against the real server, in the bots' 1:1 (Bot A here, Bot B answers with
/// `talk-cli -a b call <token> --answer …`, started by the test script alongside). Needs the
/// server's recording backend. Environment as in `ChatFlowTests`; `TALK_BOT_ROOM` names the
/// 1:1 in Bot A's list.
final class RecordingFlowTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS", "TALK_FAKE_MIC"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        app.launch()
    }

    override func tearDown() {
        let hangup = app.buttons["call-hangup"].firstMatch
        if hangup.exists && hangup.isHittable { hangup.tap() }
    }

    private var room: String { env["TALK_BOT_ROOM"] ?? "Talk Bot B" }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func wait(_ element: XCUIElement, _ format: String, timeout: TimeInterval, _ message: String) {
        let e = expectation(for: NSPredicate(format: format), evaluatedWith: element)
        e.expectationDescription = message
        wait(for: [e], timeout: timeout)
    }

    private var indicator: XCUIElement { app.descendants(matching: .any)["call-recording"].firstMatch }

    /// Calls the bot and waits until the call is connected (timer instead of a status text).
    private func callBot() {
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "bot room not listed")
        row.tap()
        let call = app.buttons["call-button"].firstMatch
        XCTAssertTrue(call.waitForExistence(timeout: 10))
        call.tap()
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let allow = springboard.buttons.matching(NSPredicate(
            format: "label IN {'Erlauben', 'OK', 'Allow', 'Beim Verwenden der App erlauben'}"
        )).firstMatch
        if allow.waitForExistence(timeout: 3) { allow.tap() }
        let status = app.staticTexts["call-status"].firstMatch
        XCTAssertTrue(status.waitForExistence(timeout: 10), "call screen did not open")
        wait(status, "label MATCHES '[0-9]+:[0-9]{2}'", timeout: 40, "call connected")
    }

    /// Bot B starts the recording after 5 s and stops it after 20 s: indicator and
    /// announcement here, gone again after the stop.
    func testBotRecordingIsShown() {
        callBot()
        wait(indicator, "label == 'Aufnahme läuft'", timeout: 45, "recording indicator")
        XCTAssertTrue(app.staticTexts["Dieser Anruf wird aufgezeichnet"].firstMatch.waitForExistence(timeout: 3))
        shot("bot-recording")
        // The bar leads with "● REC" while the call screen is minimised.
        app.buttons["call-minimize"].firstMatch.tap()
        let bar = app.buttons["call-bar"].firstMatch
        XCTAssertTrue(bar.waitForExistence(timeout: 5))
        XCTAssertTrue(bar.label.hasPrefix("Aufnahme läuft"), bar.label)
        shot("bot-recording-bar")
        bar.tap()
        wait(indicator, "exists == false", timeout: 40, "indicator gone after stop")
        app.buttons["call-hangup"].firstMatch.tap()
        wait(app.buttons["call-hangup"].firstMatch, "exists == false", timeout: 10, "call closed")
    }

    /// We start and stop an audio recording; afterwards the chat shows the notices: the
    /// recording is shared into the chat, transcript/summary (or failures) are dismissed.
    /// Only notices of this recording are touched (file names carry its start time; the
    /// bot account may hold notices of other clients' tests).
    func testOwnRecordingAndNotices() {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH-mm"
        let ours = "value >= 'Recording \(f.string(from: Date()))'"
        callBot()
        let more = app.buttons["call-more"].firstMatch
        XCTAssertTrue(more.waitForExistence(timeout: 10), "no recording controls (canRecord)")
        more.tap()
        app.buttons["Audio aufnehmen"].firstMatch.tap()
        wait(indicator, "label IN {'Aufnahme startet …', 'Aufnahme läuft'}", timeout: 15, "recording starting")
        wait(indicator, "label == 'Aufnahme läuft'", timeout: 60, "recording running")
        shot("own-recording")
        sleep(10)
        more.tap()
        app.buttons["Aufnahme beenden"].firstMatch.tap()
        let confirm = app.buttons["Aufnahme beenden"].firstMatch
        XCTAssertTrue(confirm.waitForExistence(timeout: 5), "stop confirmation")
        XCTAssertTrue(app.staticTexts["Aufnahme beenden?"].firstMatch.exists)
        confirm.tap()
        wait(indicator, "exists == false", timeout: 40, "recording stopped")
        app.buttons["call-hangup"].firstMatch.tap()
        wait(app.buttons["call-hangup"].firstMatch, "exists == false", timeout: 10, "call closed")

        // The recording arrives as a notice (the core polls every 30 s after our recording).
        let share = app.buttons.matching(NSPredicate(format: "identifier == 'recording-share' AND value ENDSWITH '.ogg' AND \(ours)")).firstMatch
        XCTAssertTrue(share.waitForExistence(timeout: 240), "no recording notice")
        shot("notice-recording")
        share.tap()
        wait(share, "exists == false", timeout: 30, "notice gone after sharing")
        // Transcript and summary (or their failures) follow after about a minute.
        for _ in 0..<2 {
            let next = app.buttons.matching(NSPredicate(format: "identifier IN {'recording-dismiss', 'recording-ok'} AND \(ours)")).firstMatch
            guard next.waitForExistence(timeout: 150) else { break }
            shot("notice-followup")
            next.tap()
            sleep(3)
        }
        shot("notices-done")
    }
}
