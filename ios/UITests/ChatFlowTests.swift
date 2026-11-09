import XCTest

/// End-to-end flows against a real server, only in the test conversation.
/// Environment (passed by xcodebuild as TEST_RUNNER_<name>):
///   TALK_DEV_SERVER / TALK_DEV_USER / TALK_DEV_PASS – sign in without the browser (simulator)
///   TALK_TEST_ROOM – name of the test conversation
///   TALK_FAKE_MIC – audio file used instead of the microphone
final class ChatFlowTests: XCTestCase {
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

    private var room: String { env["TALK_TEST_ROOM"] ?? "Talk-Client-Tests" }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func openTestRoom() {
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "test room not listed")
        shot("rooms")
        row.tap()
        let ready = app.descendants(matching: .any)["composer"].firstMatch.waitForExistence(timeout: 10)
            || app.buttons["Entwurf löschen"].firstMatch.waitForExistence(timeout: 2)
        XCTAssertTrue(ready, "chat did not open")
        // A draft left by an earlier run would hide the mic.
        let discard = app.buttons["Entwurf löschen"].firstMatch
        if discard.exists { discard.tap() }
    }

    private var micButton: XCUIElement {
        app.descendants(matching: .any)["mic"].firstMatch
    }

    /// Hold the mic, slide up: hands-free recording.
    private func startLockedRecording() {
        let mic = micButton
        XCTAssertTrue(mic.waitForExistence(timeout: 5))
        let start = mic.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        start.press(forDuration: 1.0, thenDragTo: start.withOffset(CGVector(dx: 0, dy: -160)))
        XCTAssertTrue(app.descendants(matching: .any)["recording-locked"].firstMatch.waitForExistence(timeout: 5))
    }

    private var composer: XCUIElement {
        let tv = app.textViews["composer"].firstMatch
        return tv.exists ? tv : app.textFields["composer"].firstMatch
    }

    func testSendText() {
        openTestRoom()
        let text = "iOS-Test \(Int(Date().timeIntervalSince1970))"
        composer.tap()
        composer.typeText(text)
        app.buttons["send"].tap()
        XCTAssertTrue(app.staticTexts[text].waitForExistence(timeout: 20))
        shot("sent-text")
    }

    func testVoiceHoldAndSend() throws {
        try XCTSkipIf(env["TALK_FAKE_MIC"] == nil && env["TALK_REAL_MIC"] == nil, "no microphone source")
        openTestRoom()
        let mic = micButton
        XCTAssertTrue(mic.waitForExistence(timeout: 5))
        mic.press(forDuration: 4)
        XCTAssertTrue(app.descendants(matching: .any)["voice-bubble"].firstMatch.waitForExistence(timeout: 20))
        shot("voice-sent")
    }

    func testVoiceLockAndDraft() throws {
        try XCTSkipIf(env["TALK_FAKE_MIC"] == nil && env["TALK_REAL_MIC"] == nil, "no microphone source")
        openTestRoom()
        startLockedRecording()
        sleep(3)
        shot("recording-locked")
        // Leaving the app turns the recording into a draft (like Signal).
        XCUIDevice.shared.press(.home)
        sleep(2)
        app.activate()
        XCTAssertTrue(app.descendants(matching: .any)["voice-draft"].firstMatch.waitForExistence(timeout: 10))
        shot("voice-draft")
        app.buttons["voice-send"].firstMatch.tap()
        XCTAssertTrue(app.descendants(matching: .any)["voice-bubble"].firstMatch.waitForExistence(timeout: 20))
    }

    /// The process dies while recording: what was recorded comes back as draft.
    func testKillWhileRecordingKeepsDraft() throws {
        try XCTSkipIf(env["TALK_FAKE_MIC"] == nil && env["TALK_REAL_MIC"] == nil, "no microphone source")
        openTestRoom()
        startLockedRecording()
        sleep(6)
        app.terminate()
        app.launch()
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30))
        row.tap()
        let draft = app.descendants(matching: .any)["voice-draft"].firstMatch
        XCTAssertTrue(draft.waitForExistence(timeout: 10))
        shot("recovered-draft")
        // Recorded ≈ 7–8 s (1 s hold + 6 s + kill latency), minus what was in flight.
        XCTAssertTrue(["0:06", "0:07", "0:08", "0:09"].contains(draft.label), "draft length \(draft.label)")
        app.buttons["Entwurf löschen"].firstMatch.tap()
    }

    /// Keeps a hands-free recording running for TALK_RECORD_SECONDS, then sends it.
    func testLongRecording() throws {
        try XCTSkipIf(env["TALK_RECORD_SECONDS"] == nil, "TALK_RECORD_SECONDS not set")
        let seconds = Int(env["TALK_RECORD_SECONDS"]!) ?? 60
        openTestRoom()
        startLockedRecording()
        var waited = 0
        while waited < seconds {
            sleep(30)
            waited += 30
            XCTAssertTrue(app.descendants(matching: .any)["recording-locked"].firstMatch.exists, "recording stopped after \(waited) s")
        }
        shot("long-recording")
        app.buttons["voice-send"].firstMatch.tap()
        sleep(5)
        shot("long-sent")
    }
}
