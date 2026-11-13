import XCTest

/// Categories in the chat list (docs/categories.md) against the real server as Bot A. The
/// test script creates "Test iOS Kat" holding only the 1:1 with Bot B (`TALK_BOT_ROOM` is
/// its name), has Bot B write once the section is collapsed on the server, and deletes the
/// category afterwards. Bot A's list holds only test and system chats.
final class CategoryTests: XCTestCase {
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

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func wait(_ e: XCUIElement, _ format: String, _ arg: String, timeout: TimeInterval = 10) {
        let x = expectation(for: NSPredicate(format: format, arg), evaluatedWith: e)
        x.expectationDescription = "\(e) \(format) \(arg)"
        wait(for: [x], timeout: timeout)
    }

    /// Collapse hides the read 1:1, an unread message brings it back while still collapsed,
    /// opening again shows it as before. The search ignores collapsing.
    func testCollapseCategory() {
        let head = app.buttons["section:Test iOS Kat"]
        XCTAssertTrue(head.waitForExistence(timeout: 30), "category header missing")
        XCTAssertEqual(head.label, "Test iOS Kat, Kategorie")
        XCTAssertEqual(head.value as? String, "ausgeklappt")
        XCTAssertTrue(app.buttons["section:Favoriten"].exists)
        XCTAssertTrue(app.buttons["section:Chats"].exists)
        let room = app.buttons["room:\(bot)"]
        XCTAssertTrue(room.waitForExistence(timeout: 10), "1:1 not in the category")

        head.tap()
        wait(head, "value == %@", "eingeklappt")
        let gone = expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: room)
        wait(for: [gone], timeout: 10)
        XCTAssertEqual(head.label, "Test iOS Kat, Kategorie, 1 Chat ausgeblendet")
        shot("collapsed")

        // Collapsed on the server: the script lets Bot B write, the unread chat shows up.
        XCTAssertTrue(room.waitForExistence(timeout: 90), "unread chat not shown while collapsed")
        XCTAssertEqual(head.value as? String, "eingeklappt")
        XCTAssertEqual(head.label, "Test iOS Kat, Kategorie")
        shot("collapsed-unread")

        // The search shows everything that matches, collapsed or not.
        let field = app.searchFields.firstMatch
        field.tap()
        field.typeText(bot)
        XCTAssertTrue(app.buttons["room:\(bot)"].waitForExistence(timeout: 5))
        shot("search")
        let cancel = app.buttons["Schließen"].exists ? app.buttons["Schließen"] : app.buttons["Abbrechen"]
        cancel.firstMatch.tap()

        XCTAssertTrue(head.waitForExistence(timeout: 5))
        head.tap()
        wait(head, "value == %@", "ausgeklappt")
        XCTAssertTrue(room.exists)
    }
}
