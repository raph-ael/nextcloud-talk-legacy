import XCTest

/// End-to-end flows against a real server, only in the test conversation.
/// Environment (passed by xcodebuild as TEST_RUNNER_<name>):
///   TALK_DEV_SERVER / TALK_DEV_USER / TALK_DEV_PASS – sign in without the browser (simulator)
///   TALK_TEST_ROOM – name of the test conversation
///   TALK_FAKE_MIC – audio file used instead of the microphone
private enum Format2 {
    static func clock(_ seconds: Int) -> String { String(format: "%d:%02d", seconds / 60, seconds % 60) }
}

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

    /// Confirms a system permission alert (microphone, camera, notifications) if one shows.
    @discardableResult
    private func allowSystemAlert(timeout: TimeInterval = 3) -> Bool {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let allow = springboard.buttons.matching(NSPredicate(
            format: "label IN {'Erlauben', 'OK', 'Allow', 'Beim Verwenden der App erlauben', 'Vollen Zugriff erlauben'}"
        )).firstMatch
        guard allow.waitForExistence(timeout: timeout) else { return false }
        allow.tap()
        return true
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
        // First touch may only ask for the microphone.
        mic.press(forDuration: 0.3)
        allowSystemAlert()
        sleep(1)
        let toast = app.descendants(matching: .any)["toast"]
        _ = toast.waitForNonExistence(timeout: 5)
        mic.press(forDuration: 4)
        // A "too short" toast means nothing was recorded; delivery is checked on the server.
        XCTAssertFalse(toast.waitForExistence(timeout: 2), "recording too short / not started")
        sleep(8)
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
        if env["TALK_RECORD_DISCARD"] != nil {
            // Real room audio: check the length as draft, then delete it (nothing is uploaded).
            XCUIDevice.shared.press(.home)
            sleep(3)
            app.activate()
            let draft = app.descendants(matching: .any)["voice-draft"].firstMatch
            XCTAssertTrue(draft.waitForExistence(timeout: 15))
            let expected = Format2.clock(seconds)
            XCTAssertTrue(draft.label >= expected, "draft \(draft.label), expected ≥ \(expected)")
            print("LONG DRAFT LENGTH \(draft.label)")
            app.buttons["Entwurf löschen"].firstMatch.tap()
            return
        }
        app.buttons["voice-send"].firstMatch.tap()
        sleep(5)
        shot("long-sent")
    }

    /// Attachment panel → newest photo → caption → send; then the viewer opens it.
    func testSendPhoto() throws {
        #if !targetEnvironment(simulator)
        throw XCTSkip("would send the newest photo of a real library")
        #endif
        openTestRoom()
        app.buttons["attach"].firstMatch.tap()
        let allow = app.buttons["Zugriff auf Fotos erlauben"]
        if allow.waitForExistence(timeout: 3) {
            allow.tap()
            let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
            let full = springboard.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'Vollen Zugriff' OR label CONTAINS[c] 'Full Access'")).firstMatch
            if full.waitForExistence(timeout: 5) { full.tap() }
        }
        let photo = app.descendants(matching: .any).matching(identifier: "recent-photo").firstMatch
        let found = photo.waitForExistence(timeout: 10)
        shot("attach-panel")
        XCTAssertTrue(found, "no recent photos")
        photo.tap()
        let caption = app.descendants(matching: .any)["caption"].firstMatch
        XCTAssertTrue(caption.waitForExistence(timeout: 15))
        caption.tap()
        let text = "iOS-Foto \(Int(Date().timeIntervalSince1970))"
        caption.typeText(text)
        shot("media-send")
        app.buttons["send-media"].tap()
        XCTAssertTrue(app.staticTexts[text].waitForExistence(timeout: 30))
        sleep(4)
        shot("photo-sent")
        let image = app.descendants(matching: .any).matching(identifier: "chat-photo").firstMatch
        XCTAssertTrue(image.waitForExistence(timeout: 10))
        image.tap()
        sleep(3)
        shot("viewer")
    }

    /// Swipe right on a text bubble shows the reply bar; sending quotes it.
    func testSwipeToReply() {
        openTestRoom()
        let text = "iOS-Antwortziel \(Int(Date().timeIntervalSince1970))"
        app.descendants(matching: .any)["composer"].firstMatch.tap()
        app.descendants(matching: .any)["composer"].firstMatch.typeText(text)
        app.buttons["send"].tap()
        let bubble = app.staticTexts[text].firstMatch
        XCTAssertTrue(bubble.waitForExistence(timeout: 20))
        sleep(2)
        let start = bubble.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5))
        start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 140, dy: 0)))
        XCTAssertTrue(app.buttons["Antwort abbrechen"].waitForExistence(timeout: 5), "reply bar missing")
        shot("reply-bar")
        app.descendants(matching: .any)["composer"].firstMatch.tap()
        app.descendants(matching: .any)["composer"].firstMatch.typeText("Antwort aus iOS")
        app.buttons["send"].tap()
        sleep(4)
        shot("reply-sent")
    }

    /// Device only: the camera opens, takes a photo and shows the send screen – closed
    /// again without sending (nothing private leaves the phone).
    func testCameraCaptureWithoutSending() throws {
        #if targetEnvironment(simulator)
        throw XCTSkip("no camera in the simulator")
        #endif
        openTestRoom()
        app.buttons["camera"].firstMatch.tap()
        allowSystemAlert(timeout: 5)
        let shutter = app.buttons["Foto aufnehmen"]
        XCTAssertTrue(shutter.waitForExistence(timeout: 10))
        sleep(2)
        shutter.tap()
        let caption = app.descendants(matching: .any)["caption"].firstMatch
        XCTAssertTrue(caption.waitForExistence(timeout: 15), "send screen missing")
        app.buttons["Schließen"].firstMatch.tap()
        // Closing the send screen of a camera photo returns to the camera (like Android).
        XCTAssertTrue(shutter.waitForExistence(timeout: 5))
        app.buttons["Schließen"].firstMatch.tap()
        XCTAssertTrue(app.descendants(matching: .any)["composer"].firstMatch.waitForExistence(timeout: 5))
    }

    /// The lowest voice message on screen (the newest one).
    private func newestVoiceBubble() -> XCUIElement {
        let bubbles = app.descendants(matching: .any).matching(identifier: "voice-bubble")
        XCTAssertTrue(bubbles.firstMatch.waitForExistence(timeout: 15), "no voice message on screen")
        let screen = app.windows.firstMatch.frame
        let visible = bubbles.allElementsBoundByIndex.filter { $0.exists && screen.intersects($0.frame) && $0.frame.height > 0 }
        return visible.max(by: { $0.frame.maxY < $1.frame.maxY }) ?? bubbles.firstMatch
    }

    /// A scroll that starts on a voice message must neither seek nor start it.
    func testScrollOverVoiceDoesNotPlay() {
        openTestRoom()
        let bubble = newestVoiceBubble()
        // Elements are bound by index and may point to another row after scrolling: compare
        // where all voice messages are instead.
        let all = app.descendants(matching: .any).matching(identifier: "voice-bubble")
        let positions = { all.allElementsBoundByIndex.filter(\.exists).map(\.frame.minY).sorted() }
        let before = positions()
        // On the waveform: finger down moves the list down (older messages come in).
        let start = bubble.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.35))
        func assertNotPlaying(_ step: String) {
            XCTAssertFalse(app.buttons["Pause"].firstMatch.exists, "\(step): playback started")
            XCTAssertFalse(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Tempo'")).firstMatch.exists,
                           "\(step): playback started")
            XCTAssertFalse(app.staticTexts["Wiedergabe nicht möglich"].firstMatch.exists, "\(step): playback started")
        }
        start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 8, dy: 250)))
        sleep(1)
        assertNotPlaying("drag down")
        shot("after-scroll")
        XCTAssertNotEqual(positions(), before, "list did not scroll")
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.5))
            .press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.1)))
        sleep(1)
        assertNotPlaying("drag up")
    }

    /// The newest voice message plays (TALK_EXPECT_PLAY_SECONDS: it must still play after that).
    func testPlayNewestVoice() {
        openTestRoom()
        let bubble = newestVoiceBubble()
        // The play button sits at the bubble's top left (its own accessibility frame is
        // unreliable inside the flipped list).
        bubble.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 20, dy: 20)).tap()
        let pause = app.buttons["Pause"].firstMatch
        XCTAssertTrue(pause.waitForExistence(timeout: 20), "did not start")
        let seconds = UInt32(env["TALK_EXPECT_PLAY_SECONDS"] ?? "2") ?? 2
        sleep(seconds)
        shot("playing")
        XCTAssertFalse(app.staticTexts["Wiedergabe nicht möglich"].firstMatch.exists)
        XCTAssertTrue(pause.exists, "stopped before \(seconds) s")
        pause.tap()
    }
}
