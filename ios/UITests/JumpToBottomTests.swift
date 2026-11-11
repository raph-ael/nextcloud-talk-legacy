import XCTest

/// "Zum Ende springen" (docs/chat.md) against the real server, in the bots' 1:1 (Bot A here).
/// The test script lets Bot B send three messages (`talk-cli -a b send …`) about 40 s after
/// the test started, while the list is scrolled up. Environment as in `ChatFlowTests`;
/// `TALK_BOT_ROOM` names the 1:1 in Bot A's list.
final class JumpToBottomTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        app.launch()
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

    func testJumpToBottom() {
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "bot room not listed")
        row.tap()
        let list = app.collectionViews["messages"].firstMatch
        XCTAssertTrue(list.waitForExistence(timeout: 10))
        sleep(2)
        let jump = app.buttons["jump-to-bottom"].firstMatch
        XCTAssertFalse(jump.exists, "button shown at the bottom")

        // 1. A little scrolling: no button. Far up: it fades in.
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            .press(forDuration: 0.05, thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.65)),
                   withVelocity: .slow, thenHoldForDuration: 0.3)
        sleep(1)
        XCTAssertFalse(jump.exists, "button shown after a small scroll")
        shot("small-scroll")
        list.swipeDown(velocity: .fast)
        list.swipeDown(velocity: .fast)
        XCTAssertTrue(jump.waitForExistence(timeout: 5), "button did not appear")
        XCTAssertEqual(jump.label, "Zum Ende springen")
        shot("button")

        // 2. Bot B sends three messages: badge 3, read marker stays.
        wait(jump, "label == 'Zum Ende springen, 3 neue Nachrichten'", timeout: 150, "badge with 3")
        shot("badge-3")

        // 3. Tap: at the end, button and badge gone.
        jump.tap()
        wait(jump, "exists == false", timeout: 5, "button gone after the jump")
        sleep(2)
        XCTAssertFalse(jump.exists)
        shot("after-jump")

        // 4. Far up (older pages loaded): the jump is quick.
        for _ in 0..<12 { list.swipeDown(velocity: .fast) }
        sleep(2)
        for _ in 0..<6 { list.swipeDown(velocity: .fast) }
        XCTAssertTrue(jump.waitForExistence(timeout: 5))
        shot("far-up")
        let start = Date()
        jump.tap()
        wait(jump, "exists == false", timeout: 3, "far jump took too long")
        let took = Date().timeIntervalSince(start)
        XCTAssertLessThan(took, 2.5, "far jump took \(took) s")
        sleep(1)
        shot("after-far-jump")
    }
}
