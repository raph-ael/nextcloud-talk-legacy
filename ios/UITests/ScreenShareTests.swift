import XCTest

/// Receiving shared screens (docs/calls.md "Bildschirmfreigabe").
/// - `testMockScreens`: `TALK_CALL_MOCK` with two shared screens: row, switching, rotation.
/// - `testScreenFromBot`: against the real server in the bots' 1:1 (Bot A here). Bot B
///   answers and shares its fake screen (`talk-cli -a b call <token> --answer --screen
///   --screen-after 3 --screen-off-after 45 --screen-again-after 55`, started by the test
///   script). Environment as in `ChatFlowTests`; `TALK_BOT_ROOM` names the 1:1.
final class ScreenShareTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = false
        XCUIDevice.shared.orientation = .portrait
        app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS", "TALK_FAKE_MIC"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
    }

    override func tearDown() {
        XCUIDevice.shared.orientation = .portrait
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

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }

    /// The app's window is wider than tall.
    private var appIsLandscape: Bool {
        let f = app.windows.firstMatch.frame
        return f.width > f.height
    }

    func testMockScreens() {
        app.launchEnvironment["TALK_CALL_MOCK"] = "group"
        app.launchEnvironment["TALK_CALL_MOCK_SCREEN"] = "2"
        app.launchEnvironment["TALK_CALL_MOCK_VIDEO"] = "1"
        app.launch()

        // The newest screen (Anna) is on the stage, Bernd's in the row.
        let stage = element("call-screen-stage")
        XCTAssertTrue(stage.waitForExistence(timeout: 20), "no screen stage")
        XCTAssertEqual(stage.label, "Bildschirm von Anna Berg")
        XCTAssertTrue(element("call-screen-row").exists)
        shot("mock-portrait")

        // Tap the other screen: it takes the stage.
        app.buttons["call-screen-thumb"].firstMatch.tap()
        wait(stage, "label == 'Bildschirm von Bernd Krause'", timeout: 5, "picked screen on stage")

        // Landscape: everything is the screen; the controls fade out after 3 s.
        XCUIDevice.shared.orientation = .landscapeLeft
        sleep(2)
        XCTAssertTrue(appIsLandscape, "app did not turn")
        XCTAssertTrue(app.buttons["call-hangup"].firstMatch.isHittable)
        XCTAssertTrue(app.buttons["call-screen-picker"].firstMatch.exists)
        shot("mock-landscape")
        wait(app.buttons["call-hangup"].firstMatch, "exists == false", timeout: 6, "controls faded")
        shot("mock-landscape-clean")
        stage.tap()
        XCTAssertTrue(app.buttons["call-hangup"].firstMatch.waitForExistence(timeout: 3), "tap brings the controls back")

        // Back to the chat: portrait again although the device is still landscape.
        app.buttons["call-minimize"].firstMatch.tap()
        XCTAssertTrue(app.buttons["call-bar"].firstMatch.waitForExistence(timeout: 5))
        sleep(2)
        XCTAssertFalse(appIsLandscape, "app stayed in landscape without a screen")
        shot("mock-minimized")
    }

    /// Calls the bot and waits until the call is connected.
    private func callBot() {
        app.launch()
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

    func testScreenFromBot() {
        callBot()

        // 1. Bot B shares: its screen fills the stage (portrait), Bot B in the row.
        let stage = element("call-screen-stage")
        XCTAssertTrue(stage.waitForExistence(timeout: 30), "no shared screen")
        XCTAssertTrue(element("call-screen-label").exists)
        sleep(2)
        shot("bot-portrait")

        // 2. Landscape, controls fade out, tap brings them back.
        XCUIDevice.shared.orientation = .landscapeLeft
        sleep(2)
        XCTAssertTrue(appIsLandscape, "app did not turn")
        shot("bot-landscape")
        wait(app.buttons["call-hangup"].firstMatch, "exists == false", timeout: 6, "controls faded")
        stage.tap()
        XCTAssertTrue(app.buttons["call-hangup"].firstMatch.waitForExistence(timeout: 3))

        // 3. Zoom with two fingers, then double tap back.
        stage.pinch(withScale: 3, velocity: 2)
        sleep(1)
        shot("bot-zoomed")
        stage.doubleTap()
        sleep(1)
        XCUIDevice.shared.orientation = .portrait
        sleep(2)

        // 4. Bot B stops sharing: the normal call view, portrait only.
        wait(stage, "exists == false", timeout: 40, "screen gone")
        XCTAssertTrue(app.staticTexts["call-title"].firstMatch.exists)
        XCTAssertFalse(element("call-screen-row").exists)
        XCUIDevice.shared.orientation = .landscapeLeft
        sleep(2)
        XCTAssertFalse(appIsLandscape, "turned without a screen")
        shot("bot-ended")
        XCUIDevice.shared.orientation = .portrait

        // 5. It shares again: back on the stage.
        XCTAssertTrue(stage.waitForExistence(timeout: 30), "second share not shown")
        sleep(2)
        shot("bot-again")

        app.buttons["call-hangup"].firstMatch.tap()
        wait(app.buttons["call-hangup"].firstMatch, "exists == false", timeout: 10, "call closed")
    }
}
