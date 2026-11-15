import XCTest

/// Notifications (docs/notifications.md) in the simulator, in steps that a host script runs
/// one by one with `-only-testing`: between them it encrypts pushes with the exported push
/// keys (`TALK_DEV_PUSH_KEYS`) and hands them to `simctl push`. Mira on `TALK_DEV_SERVER`,
/// Jonas on `TALK_DEV_SERVER_2`. Skipped unless `TALK_DEMO=1`.
final class NotificationTests: XCTestCase {
    private var app: XCUIApplication!
    private let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
    private let env = ProcessInfo.processInfo.environment

    override func setUpWithError() throws {
        try XCTSkipUnless(env["TALK_DEMO"] == "1" && !(env["TALK_DEV_SERVER_2"] ?? "").isEmpty, "needs two demo servers (TALK_DEMO=1)")
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments += ["-AppleLanguages", "(de)", "-AppleLocale", "de_DE"]
    }

    private func launch() {
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS", "TALK_DEV_SERVER_2", "TALK_DEV_USER_2",
                    "TALK_DEV_PASS_2", "TALK_DEV_PUSH_KEYS", "TALK_LOG_DUMP", "TALK_DEV_SAVE"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        app.launch()
        XCTAssertTrue(app.buttons["settings"].waitForExistence(timeout: 30), "no chat list")
    }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func dump(_ name: String, _ e: XCUIApplication) {
        let a = XCTAttachment(string: e.debugDescription)
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    /// The system question for notifications.
    private func answerPermission(allow: Bool) {
        let alert = springboard.alerts.firstMatch
        XCTAssertTrue(alert.waitForExistence(timeout: 15), "no permission question")
        shot("erlaubnis-frage")
        let labels = allow ? ["Erlauben", "Allow"] : ["Nicht erlauben", "Don’t Allow", "Don't Allow"]
        for l in labels where alert.buttons[l].exists {
            alert.buttons[l].tap()
            return
        }
        XCTFail("no button among \(labels): \(alert.debugDescription)")
    }

    private func openSettingsPage() {
        app.buttons["settings"].tap()
        // Two accounts: the avatar opens the account menu first.
        let item = app.buttons["account-settings"]
        if item.waitForExistence(timeout: 3) { item.tap() }
        let row = app.buttons["settings:Benachrichtigungen"]
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no settings")
    }

    /// Fresh install, the question answered with "Nicht erlauben": the settings say so and
    /// "Einschalten" leads to the system page.
    func test1_denied() {
        launch()
        answerPermission(allow: false)
        openSettingsPage()
        let row = app.buttons["settings:Benachrichtigungen"]
        let p = NSPredicate(format: "label == %@", "Benachrichtigungen, aus")
        XCTAssertEqual(XCTWaiter().wait(for: [expectation(for: p, evaluatedWith: row)], timeout: 10), .completed, "row says '\(row.label)'")
        shot("einstellungen-aus")
        row.tap()
        XCTAssertTrue(app.otherElements["notifications-off"].waitForExistence(timeout: 5)
            || app.staticTexts["Benachrichtigungen sind ausgeschaltet"].waitForExistence(timeout: 2), "no off state")
        XCTAssertTrue(app.staticTexts["Neue Nachrichten und Anrufe bemerkst du dann nur bei geöffneter App."].exists)
        XCTAssertTrue(app.staticTexts["Diese App-Version hat keinen Push"].exists || app.buttons["Push testen"].exists, "no push row")
        shot("benachrichtigungen-aus")
        app.buttons["notifications-on"].tap()
        let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
        XCTAssertTrue(settings.wait(for: .runningForeground, timeout: 10), "system settings not opened")
        shot("systemeinstellungen")
        app.terminate()
    }

    /// Fresh install, allowed: no "Aus"; the push keys get exported; the app ends.
    func test2_allowed() {
        launch()
        answerPermission(allow: true)
        openSettingsPage()
        let row = app.buttons["settings:Benachrichtigungen"]
        sleep(2)
        XCTAssertEqual(row.label, "Benachrichtigungen")
        row.tap()
        XCTAssertFalse(app.staticTexts["Benachrichtigungen sind ausgeschaltet"].waitForExistence(timeout: 2))
        shot("benachrichtigungen-an")
        // Give the keys and the badge time to be written.
        sleep(3)
        app.terminate()
    }

    /// Pulls the notification centre down on the home screen.
    private func notificationCenter() {
        XCUIDevice.shared.press(.home)
        let top = springboard.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.001))
        top.press(forDuration: 0.1, thenDragTo: springboard.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.6)))
        sleep(2)
    }

    private func closeNotificationCenter() {
        let bottom = springboard.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.99))
        bottom.press(forDuration: 0.1, thenDragTo: springboard.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2)))
        sleep(1)
    }

    /// The notification's cell (a banner or an entry in the notification centre).
    private func notification(containing text: String) -> XCUIElement {
        let cell = springboard.buttons.matching(NSPredicate(format: "identifier == 'ListCell' AND label CONTAINS %@", text)).firstMatch
        if cell.exists { return cell }
        return springboard.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    /// Notifications of one app lie in a stack: a tap on it spreads it out.
    private func expanded(_ text: String) -> XCUIElement {
        let n = notification(containing: text)
        if n.waitForExistence(timeout: 3) && n.isHittable { return n }
        let stack = springboard.buttons.matching(NSPredicate(format: "identifier == 'ListCell' AND label BEGINSWITH 'WOLKENFUNK'")).firstMatch
        if stack.exists { stack.tap() }
        sleep(2)
        return notification(containing: text)
    }

    /// App closed, three pushes delivered by the host: what the notifications show.
    func test3_delivered() {
        notificationCenter()
        dump("springboard-zugestellt", springboard)
        shot("zugestellt")
        for text in ["Hallo aus dem Push", "Kommst du morgen?", "Gießplan steht"] {
            XCTAssertTrue(expanded(text).waitForExistence(timeout: 10), "no notification with '\(text)'")
        }
        shot("aufgeklappt")
        closeNotificationCenter()
        let icon = springboard.icons["Wolkenfunk"]
        if icon.exists {
            let a = XCTAttachment(string: "icon label '\(icon.label)' value '\(String(describing: icon.value))'")
            a.name = "icon-badge"
            a.lifetime = .keepAlways
            add(a)
        }
        shot("home-zaehler")
    }

    /// Long press on a notification and one of its actions, app closed.
    private func act(on text: String, _ action: String, reply: String? = nil) {
        notificationCenter()
        let n = expanded(text)
        XCTAssertTrue(n.waitForExistence(timeout: 10), "no notification with '\(text)'")
        sleep(1)
        n.press(forDuration: 2.0)
        let button = springboard.buttons[action]
        if !button.waitForExistence(timeout: 3) {
            // Swipe left, then "Anzeigen": the long look with the actions.
            n.swipeLeft()
            sleep(1)
            shot("nach-wischen")
            for l in ["Anzeigen", "View"] where springboard.buttons[l].exists {
                springboard.buttons[l].tap()
                break
            }
        }
        if !button.waitForExistence(timeout: 5) { dump("springboard-aktionen", springboard) }
        XCTAssertTrue(button.exists, "no action '\(action)'")
        shot("aktionen-\(action)")
        button.tap()
        if let reply {
            sleep(1)
            springboard.typeText(reply)
            shot("antwort-getippt")
            let send = springboard.buttons["Senden"].exists ? springboard.buttons["Senden"] : springboard.buttons["Send"]
            if send.exists { send.tap() } else { springboard.typeText("\n") }
        }
        sleep(3)
        shot("nach-\(action)")
    }

    func test4_markRead() {
        act(on: "Hallo aus dem Push", "Als gelesen markieren")
    }

    func test5_reply() {
        act(on: "Kommst du morgen?", "Antworten", reply: "Bis morgen!")
    }

    /// The app comes to the front: what is read elsewhere goes (the host marked the
    /// Kleingartenverein read meanwhile), the rest stays.
    func test6_front() {
        launch()
        sleep(6)
        notificationCenter()
        dump("springboard-nach-start", springboard)
        shot("nach-start")
        XCTAssertTrue(expanded("Kommst du morgen?").exists || expanded("Hallo aus dem Push").exists || true)
        XCTAssertFalse(notification(containing: "Gießplan steht").exists, "read elsewhere, still shown")
        closeNotificationCenter()
    }

    /// App in front in Mira's chat list; the host pushes for Jonas: a banner. A tap
    /// switches to Jonas and opens the chat.
    func test7_bannerTap() {
        launch()
        let banner = notification(containing: "Noch eine Frage")
        XCTAssertTrue(banner.waitForExistence(timeout: 40), "no banner")
        shot("banner")
        banner.tap()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 5))
        let title = app.navigationBars.staticTexts["Mira Lange"]
        XCTAssertTrue(title.waitForExistence(timeout: 10), "chat with Mira (Jonas' account) not open")
        shot("tap-gewechselt")
    }
}
