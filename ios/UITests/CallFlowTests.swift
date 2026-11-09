import XCTest

/// Call flow against the real server, in the shared test conversation and always silent
/// (`TALK_CALL_SILENT=1`: nobody is rung). Alone in the group call the screen says
/// "Warte auf andere …". Environment as in `ChatFlowTests`.
final class CallFlowTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS", "TALK_FAKE_MIC"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        // Never ring the members of the shared test room.
        app.launchEnvironment["TALK_CALL_SILENT"] = "1"
        app.launch()
    }

    override func tearDown() {
        // Leave a call that a failed assertion left running.
        let hangup = app.buttons["call-hangup"].firstMatch
        if hangup.exists && hangup.isHittable { hangup.tap() }
    }

    private var room: String { env["TALK_TEST_ROOM"] ?? "Talk-Client-Tests" }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func allowMicrophone() {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let allow = springboard.buttons.matching(NSPredicate(
            format: "label IN {'Erlauben', 'OK', 'Allow', 'Beim Verwenden der App erlauben'}"
        )).firstMatch
        if allow.waitForExistence(timeout: 3) { allow.tap() }
    }

    func testSilentGroupCall() {
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "test room not listed")
        row.tap()

        let call = app.buttons["call-button"].firstMatch
        XCTAssertTrue(call.waitForExistence(timeout: 10))
        call.tap()
        allowMicrophone()

        let status = app.staticTexts["call-status"].firstMatch
        XCTAssertTrue(status.waitForExistence(timeout: 10), "call screen did not open")
        XCTAssertTrue(app.staticTexts["call-title"].firstMatch.label.contains(room))
        // Dialing → alone in the group call.
        let waiting = NSPredicate(format: "label == 'Warte auf andere …'")
        expectation(for: waiting, evaluatedWith: status)
        waitForExpectations(timeout: 20)
        shot("call-waiting")

        // Mute is reflected by the core's state.
        let mute = app.buttons["call-mute"].firstMatch
        mute.tap()
        XCTAssertTrue(app.buttons["Stummschaltung aufheben"].firstMatch.waitForExistence(timeout: 5))
        mute.tap()
        XCTAssertTrue(app.buttons["Stummschalten"].firstMatch.waitForExistence(timeout: 5))

        // Minimise: the bar leads back.
        app.buttons["call-minimize"].firstMatch.tap()
        let bar = app.buttons["call-bar"].firstMatch
        XCTAssertTrue(bar.waitForExistence(timeout: 5))
        XCTAssertTrue(bar.label.hasPrefix("Zurück zum Anruf"))
        shot("call-bar")
        bar.tap()
        XCTAssertTrue(app.buttons["call-hangup"].firstMatch.waitForExistence(timeout: 5))

        // Hang up: end state, then the screen closes and the bar is gone.
        app.buttons["call-hangup"].firstMatch.tap()
        let gone = NSPredicate(format: "exists == false")
        expectation(for: gone, evaluatedWith: app.buttons["call-hangup"].firstMatch)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.buttons["call-bar"].firstMatch.exists)
        XCTAssertTrue(app.buttons["call-button"].firstMatch.exists)
    }
}
