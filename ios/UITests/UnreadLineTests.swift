import XCTest

/// Date dividers and the unread line (docs/chat.md, "Datum und Ungelesen-Linie") against the
/// real server as Bot A, in the bots' 1:1 (`TALK_BOT_ROOM` is its name). The test script marks
/// the chat read and lets Bot B write messages tagged with `TALK_RUN`, each once we read the
/// ones before: "Vorlauf · run" before the start, "Sofort · run" (we open right after it,
/// while the chat list may not know it yet), "Test 1 · run" … "Test 3 · run", "Test 4 · run",
/// and "Serie 1 · run" … "Serie 60 · run" once our own message is there.
final class UnreadLineTests: XCTestCase {
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

    private var bot: String { env["TALK_BOT_ROOM"] ?? "Talk Bot B" }
    private var run: String { env["TALK_RUN"] ?? "" }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func wait(_ e: XCUIElement, _ format: String, timeout: TimeInterval = 10, _ message: String) {
        let x = expectation(for: NSPredicate(format: format), evaluatedWith: e)
        x.expectationDescription = message
        wait(for: [x], timeout: timeout)
    }

    private var line: XCUIElement { app.descendants(matching: .any)["unread-line"].firstMatch }
    private var list: XCUIElement { app.collectionViews["messages"].firstMatch }
    private var jump: XCUIElement { app.buttons["jump-to-bottom"].firstMatch }

    private func openChat() {
        let row = app.buttons["room:\(bot)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "bot room not listed")
        row.tap()
        XCTAssertTrue(list.waitForExistence(timeout: 10))
    }

    private func back() {
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(app.buttons["new-message"].waitForExistence(timeout: 10))
    }

    /// The line sits right above the message (both on screen, line just above it).
    private func assertAbove(_ text: String) {
        let msg = app.staticTexts[text].firstMatch
        XCTAssertTrue(msg.waitForExistence(timeout: 10), "\(text) not shown")
        XCTAssertTrue(line.isHittable && msg.isHittable, "line or \(text) off screen")
        let gap = msg.frame.minY - line.frame.maxY
        XCTAssertTrue(gap >= 0 && gap < 60, "line not right above \(text) (gap \(gap))")
    }

    /// Bot B writes and we open at once (the chat list a few seconds behind): one unread.
    func testOpenRightAway() {
        openChat()
        XCTAssertTrue(line.waitForExistence(timeout: 20), "no line for 'Vorlauf'")
        XCTAssertEqual(line.label, "1 ungelesene Nachricht")
        back()
        // The script writes "Sofort" about 6–8 s after we read "Vorlauf".
        sleep(12)
        let row = app.buttons["room:\(bot)"].firstMatch
        let listKnew = row.label.contains("Sofort · \(run)")
        openChat()
        XCTAssertTrue(line.waitForExistence(timeout: 20), "no line right after Bot B wrote")
        XCTAssertEqual(line.label, "1 ungelesene Nachricht")
        assertAbove("Sofort · \(run)")
        shot(listKnew ? "right-away-list-current" : "right-away-list-stale")
        back()
    }

    func testUnreadLine() {
        // 1–2. Three unread from Bot B: the line stands above the first one.
        let row0 = app.buttons["room:\(bot)"].firstMatch
        XCTAssertTrue(row0.waitForExistence(timeout: 30), "bot room not listed")
        wait(row0, "label CONTAINS 'Test 3 · \(run)'", timeout: 90, "Bot B's three did not arrive")
        openChat()
        XCTAssertTrue(line.waitForExistence(timeout: 20), "no unread line")
        XCTAssertEqual(line.label, "3 ungelesene Nachrichten")
        assertAbove("Test 1 · \(run)")
        shot("line-3")

        // 3. Bot B writes again (it waits until we read): 4. Our own message removes it.
        wait(line, "label == '4 ungelesene Nachrichten'", timeout: 90, "count did not rise to 4")
        shot("line-4")
        let composer = app.descendants(matching: .any)["composer"].firstMatch
        composer.tap()
        composer.typeText("iOS hier · \(run)")
        app.buttons["send"].tap()
        wait(line, "exists == false", "line stays after sending")

        // 4. Leave and open again: everything read, no line.
        back()
        openChat()
        sleep(3)
        XCTAssertFalse(line.exists, "line after reopening")
        back()

        // 5. Sixty from Bot B: the chat opens at the line (one older page loaded); the read
        // marker stays until the bottom ("Zum Ende springen" keeps counting).
        let row = app.buttons["room:\(bot)"].firstMatch
        wait(row, "label CONTAINS 'Serie 60 · \(run)'", timeout: 180, "Bot B's 60 did not arrive")
        sleep(2)
        openChat()
        XCTAssertTrue(line.waitForExistence(timeout: 30), "no line with 60 unread")
        XCTAssertEqual(line.label, "60 ungelesene Nachrichten")
        assertAbove("Serie 1 · \(run)")
        let lineY = line.frame.minY - list.frame.minY
        XCTAssertTrue(lineY < list.frame.height * 0.45, "line not in the upper part (\(lineY) of \(list.frame.height))")
        XCTAssertTrue(jump.waitForExistence(timeout: 5), "no jump button")
        XCTAssertEqual(jump.label, "Zum Ende springen, 60 neue Nachrichten")
        shot("line-60")
        sleep(8) // the script checks Bot A's read marker meanwhile
        jump.tap()
        wait(jump, "exists == false", timeout: 15, "jump button stays at the bottom")
        XCTAssertTrue(app.staticTexts["Serie 60 · \(run)"].waitForExistence(timeout: 5))
        sleep(1) // the jump animation ends at the very bottom

        // 6. At the bottom a little drag shows no date pill (screenshot); far up it does,
        // and the dividers of other days are plain text.
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            .press(forDuration: 0.05, thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.53)),
                   withVelocity: .slow, thenHoldForDuration: 0.3)
        shot("bottom-no-pill")
        let older = app.staticTexts.matching(NSPredicate(
            format: "label == 'Gestern' OR label MATCHES '^(Mo|Di|Mi|Do|Fr|Sa|So)\\\\., [0-9]{1,2}\\\\. .+' OR label MATCHES '^[0-9]{1,2}\\\\. .+ [0-9]{4}$'"
        )).firstMatch
        // Earlier test runs may have left a few hundred messages of today.
        for _ in 0..<80 where !older.exists { list.swipeDown(velocity: .fast) }
        XCTAssertTrue(older.exists, "no divider of an earlier day")
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.4))
            .press(forDuration: 0.05, thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.6)),
                   withVelocity: .slow, thenHoldForDuration: 0.5)
        shot("pill-\(older.label)")
        sleep(1)
        shot("pill-gone")
        jump.tap()
    }
}
