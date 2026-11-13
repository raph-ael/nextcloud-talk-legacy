import XCTest

/// Reactions (docs/chat.md "Reaktionen") against the real server in the bots' 1:1 (Bot A
/// here). The test script waits for the message "iOS-Reaktion …" and lets Bot B react with
/// ❤️ (`talk-cli -a b react <token> <id> ❤️`). Environment as in `ChatFlowTests`;
/// `TALK_BOT_ROOM` names the 1:1 in Bot A's list.
final class ReactionTests: XCTestCase {
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

    /// The test message; pills are looked up only in its row (older messages have some too).
    private var text = ""

    private func pill(_ label: String) -> XCUIElement {
        app.cells.containing(NSPredicate(format: "label == %@", text)).firstMatch
            .buttons.matching(identifier: "reaction-pill").matching(NSPredicate(format: "label == %@", label)).firstMatch
    }

    private func openMenu(_ bubble: XCUIElement) {
        bubble.press(forDuration: 0.8)
        XCTAssertTrue(app.buttons["react:👍"].waitForExistence(timeout: 5), "no reaction bar")
    }

    func testReactions() {
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "bot room not listed")
        row.tap()
        let composer = app.descendants(matching: .any)["composer"].firstMatch
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
        text = "iOS-Reaktion \(Int(Date().timeIntervalSince1970))"
        composer.tap()
        composer.typeText(text)
        app.buttons["send"].tap()
        let bubble = app.staticTexts[text].firstMatch
        XCTAssertTrue(bubble.waitForExistence(timeout: 20))
        sleep(3)

        // 1. Long press: bar above, actions below; 👍 sets the reaction.
        openMenu(bubble)
        XCTAssertTrue(app.buttons["menu:reply"].exists)
        XCTAssertTrue(app.buttons["menu:copy"].exists)
        shot("menu")
        app.buttons["react:👍"].tap()
        XCTAssertTrue(pill("👍 1, Reaktionen anzeigen").waitForExistence(timeout: 10), "own pill missing")
        XCTAssertFalse(app.buttons["react:👍"].exists, "menu still open")

        // 2. Bot B reacts with ❤️ (live).
        XCTAssertTrue(pill("❤️ 1, Reaktionen anzeigen").waitForExistence(timeout: 90), "Bot B's reaction did not arrive")
        shot("pills")

        // 3. Another emoji replaces ours; the bar marks the current one.
        openMenu(bubble)
        XCTAssertTrue(app.buttons["react:👍"].isSelected, "own reaction not marked")
        shot("menu-own")
        app.buttons["react:😂"].tap()
        XCTAssertTrue(pill("😂 1, Reaktionen anzeigen").waitForExistence(timeout: 10))
        wait(pill("👍 1, Reaktionen anzeigen"), "exists == false", timeout: 10, "👍 replaced")

        // 4. "Wer hat reagiert": Bot B and us; tapping our row removes it, Bot B stays.
        pill("❤️ 1, Reaktionen anzeigen").tap()
        let own = app.descendants(matching: .any)["reaction-own"].firstMatch
        XCTAssertTrue(own.waitForExistence(timeout: 10), "own row missing")
        XCTAssertTrue(app.descendants(matching: .any)["reaction-row"].firstMatch.exists, "Bot B's row missing")
        shot("sheet")
        own.tap()
        wait(own, "exists == false", timeout: 5, "own row gone")
        XCTAssertTrue(app.descendants(matching: .any)["reaction-row"].firstMatch.exists)
        app.swipeDown(velocity: .fast)
        wait(pill("😂 1, Reaktionen anzeigen"), "exists == false", timeout: 10, "own reaction removed")

        // 5. ＋ opens the emoji keyboard; the first emoji is taken.
        openMenu(bubble)
        app.buttons["react-more"].tap()
        let field = app.textFields["emoji-catcher"].firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        sleep(1)
        shot("emoji-keyboard")
        field.typeText("🎉")
        XCTAssertTrue(pill("🎉 1, Reaktionen anzeigen").waitForExistence(timeout: 10), "picked emoji not set")
        shot("picked")

        // 6. An own reaction outside the six takes 😢's place in the bar; tapping it takes it back.
        openMenu(bubble)
        XCTAssertTrue(app.buttons["react:🎉"].isSelected, "🎉 not in the bar")
        XCTAssertFalse(app.buttons["react:😢"].exists)
        app.buttons["react:🎉"].tap()
        wait(pill("🎉 1, Reaktionen anzeigen"), "exists == false", timeout: 10, "own reaction taken back")
    }
}
