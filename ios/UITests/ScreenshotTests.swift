import XCTest

/// Website and store screenshots against the demo server (scripts/screenshots/README.md),
/// driven by `scripts/screenshots/ios.sh`. Skipped unless `TALK_SCREENSHOTS=1`. Each scene is
/// an attachment named after it (chats, ungelesen, sprachnachricht).
final class ScreenshotTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUpWithError() throws {
        try XCTSkipUnless(env["TALK_SCREENSHOTS"] == "1", "screenshots only via scripts/screenshots/ios.sh")
        continueAfterFailure = false
        app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        // German UI whatever the simulator is set to.
        app.launchArguments += ["-AppleLanguages", "(de)", "-AppleLocale", "de_DE"]
        app.launch()
    }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    /// Opens a conversation, scrolling the list until its row is on screen (`up`: it lies
    /// further down).
    private func open(_ room: String, up: Bool) {
        let row = app.buttons["room:\(room)"].firstMatch
        for _ in 0..<8 where !(row.exists && row.isHittable) {
            if up { app.swipeUp() } else { app.swipeDown() }
            sleep(1)
        }
        XCTAssertTrue(row.waitForExistence(timeout: 5), "\(room) not listed")
        row.tap()
        XCTAssertTrue(app.collectionViews["messages"].firstMatch.waitForExistence(timeout: 10))
    }

    private func back() {
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(app.buttons["new-message"].waitForExistence(timeout: 10))
    }

    func testScenes() {
        // chats: the list with both categories, avatars loaded.
        XCTAssertTrue(app.buttons["section:Familie & Freunde"].waitForExistence(timeout: 30), "no categories")
        XCTAssertTrue(app.buttons["section:Arbeit"].exists)
        sleep(4)
        shot("chats")

        // ungelesen: "Gartenprojekt" opens at "5 ungelesene Nachrichten", with the photo.
        open("Gartenprojekt", up: true)
        let line = app.descendants(matching: .any)["unread-line"].firstMatch
        XCTAssertTrue(line.waitForExistence(timeout: 20), "no unread line")
        XCTAssertEqual(line.label, "5 ungelesene Nachrichten")
        sleep(4)
        shot("ungelesen")
        back()

        // sprachnachricht: "Familie" with the voice message on screen.
        open("Familie", up: false)
        XCTAssertTrue(app.descendants(matching: .any)["voice-bubble"].firstMatch.waitForExistence(timeout: 20), "no voice message")
        sleep(4)
        shot("sprachnachricht")
    }
}
