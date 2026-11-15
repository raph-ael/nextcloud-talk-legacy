import XCTest

/// Declining an incoming 1:1 call with the app's own button: the caller must see
/// "Abgelehnt". A host script lets the other bot ring in the bot 1:1 (`TALK_DECLINE_TEST=1`,
/// under bot-lock.sh) and reads the caller's end reason. Environment as in `ChatFlowTests`.
final class CallDeclineTests: XCTestCase {
    func testDeclineIncoming() throws {
        let env = ProcessInfo.processInfo.environment
        try XCTSkipUnless(env["TALK_DECLINE_TEST"] == "1", "needs a ringing peer (TALK_DECLINE_TEST=1)")
        let app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        app.launch()
        let decline = app.buttons["call-decline"]
        XCTAssertTrue(decline.waitForExistence(timeout: 60), "no incoming call")
        decline.tap()
        let gone = NSPredicate(format: "exists == false")
        XCTAssertEqual(XCTWaiter().wait(for: [expectation(for: gone, evaluatedWith: decline)], timeout: 10), .completed)
        // The decline for the caller runs after the screen closed.
        sleep(12)
    }
}
