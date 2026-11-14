import XCTest

/// Settings, "Darstellung" and the chat menu (docs/settings.md) against the demo server
/// (`cargo run -p talk-demo`, every login works). Skipped unless `TALK_DEMO=1`; the server
/// URL comes in `TALK_DEV_SERVER`. Screenshots are attachments named after the scene.
final class SettingsMenuTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUpWithError() throws {
        try XCTSkipUnless(env["TALK_DEMO"] == "1", "needs the demo server (TALK_DEMO=1)")
        continueAfterFailure = false
        app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        app.launchArguments += ["-AppleLanguages", "(de)", "-AppleLocale", "de_DE"]
        app.launch()
        XCTAssertTrue(app.buttons["settings"].waitForExistence(timeout: 30), "no chat list")
    }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func row(_ name: String) -> XCUIElement {
        let r = app.buttons["room:\(name)"].firstMatch
        // From the top (favorites move up), then down the list.
        if !(r.exists && r.isHittable) {
            for _ in 0..<3 { app.swipeDown() }
            sleep(1)
        }
        for _ in 0..<6 where !(r.exists && r.isHittable) {
            app.swipeUp()
            sleep(1)
        }
        XCTAssertTrue(r.waitForExistence(timeout: 5), "\(name) not listed")
        return r
    }

    private func menu(_ name: String) {
        row(name).press(forDuration: 1.2)
        XCTAssertTrue(app.buttons["Benachrichtigungen"].waitForExistence(timeout: 5), "no chat menu for \(name)")
    }

    /// An entry of the open "Kategorie" submenu (the list has section heads of that name).
    private func catItem(_ title: String) -> XCUIElement {
        app.collectionViews.containing(.button, identifier: "plus").firstMatch.buttons[title]
    }

    /// Taps outside an open context menu.
    private func closeMenu() {
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.08)).tap()
        sleep(1)
    }

    private func openAppearance() {
        app.buttons["settings"].tap()
        XCTAssertTrue(app.navigationBars["Einstellungen"].waitForExistence(timeout: 5))
        app.buttons["settings:Darstellung"].tap()
        XCTAssertTrue(app.navigationBars["Darstellung"].waitForExistence(timeout: 5))
    }

    private func backToList() {
        while !app.buttons["settings"].isHittable {
            app.navigationBars.buttons.element(boundBy: 0).tap()
            sleep(1)
        }
    }

    func testSettingsAndAppearance() {
        XCUIDevice.shared.appearance = .light
        // Start page: profile, rows in order, "Abmelden" with its confirmation.
        app.buttons["settings"].tap()
        XCTAssertTrue(app.navigationBars["Einstellungen"].waitForExistence(timeout: 5))
        for title in ["Darstellung", "Benachrichtigungen", "Über Wolkenfunk"] {
            XCTAssertTrue(app.buttons["settings:\(title)"].exists, title)
        }
        XCTAssertTrue(app.staticTexts["mira"].exists)
        sleep(2)
        shot("einstellungen")
        app.buttons["settings-logout"].tap()
        XCTAssertTrue(app.buttons["Abmelden"].firstMatch.waitForExistence(timeout: 3))
        app.swipeDown()
        sleep(1)

        // Darstellung: design and accent apply at once.
        app.buttons["settings:Darstellung"].tap()
        XCTAssertTrue(app.staticTexts["Wie gefällt dir die neue Farbe?"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Blau, ausgewählt"].exists)
        XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'accent:'")).count, 10)
        shot("darstellung-hell")
        app.buttons["Dunkel"].tap()
        app.buttons["accent:gruen"].tap()
        XCTAssertTrue(app.buttons["Grün, ausgewählt"].waitForExistence(timeout: 3))
        XCTAssertFalse(app.buttons["Blau, ausgewählt"].exists)
        sleep(1)
        shot("darstellung-dunkel-gruen")

        // Kept after a restart.
        app.terminate()
        app.launch()
        XCTAssertTrue(app.buttons["settings"].waitForExistence(timeout: 30))
        sleep(2)
        shot("chats-dunkel-gruen")
        openAppearance()
        XCTAssertTrue(app.buttons["Grün, ausgewählt"].exists, "accent not kept")
        XCTAssertTrue(app.buttons["Dunkel"].isSelected, "design not kept")

        // A sheet follows too: "Hell" over a dark system, then "System" while it is open.
        XCUIDevice.shared.appearance = .dark
        app.buttons["Hell"].tap()
        app.buttons["accent:violett"].tap()
        backToList()
        app.buttons["new-message"].tap()
        sleep(2)
        shot("sheet-hell-violett")
        app.swipeDown(velocity: .fast)
        sleep(1)
        openAppearance()
        app.buttons["System"].tap()
        backToList()
        app.buttons["new-message"].tap()
        sleep(2)
        shot("sheet-system-dunkel")
        XCUIDevice.shared.appearance = .light
        sleep(2)
        shot("sheet-system-hell")
        app.swipeDown(velocity: .fast)
        sleep(1)

        // Benachrichtigungen: opens the system page.
        app.buttons["settings"].tap()
        app.buttons["settings:Benachrichtigungen"].tap()
        XCTAssertTrue(app.staticTexts["Für einzelne Chats: Chat in der Liste lange gedrückt halten."].waitForExistence(timeout: 5))
        shot("benachrichtigungen")
        app.buttons["settings-system"].tap()
        let prefs = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
        XCTAssertTrue(prefs.wait(for: .runningForeground, timeout: 10), "system settings not opened")
        app.activate()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let back = app.navigationBars.buttons["BackButton"]
        XCTAssertTrue(back.waitForExistence(timeout: 5))
        sleep(1)
        back.tap()
        XCTAssertTrue(app.buttons["settings:Über Wolkenfunk"].waitForExistence(timeout: 5))

        // Über Wolkenfunk.
        app.buttons["settings:Über Wolkenfunk"].tap()
        XCTAssertTrue(app.staticTexts["Lizenz: AGPL-3.0"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Version '")).count > 0)
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Kern '")).count > 0)
        XCTAssertTrue(app.buttons["Webseite"].exists && app.buttons["Quellcode"].exists)
        shot("ueber")

        // Back to the defaults for later runs.
        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.buttons["settings:Darstellung"].tap()
        app.buttons["accent:blau"].tap()
        XCTAssertTrue(app.buttons["Blau, ausgewählt"].waitForExistence(timeout: 3))
        backToList()
    }

    func testChatMenu() {
        XCUIDevice.shared.appearance = .light
        // Muted chat: crossed-out bell in the row label.
        XCTAssertTrue(row("Sommerfest").label.contains("stummgeschaltet"), "Sommerfest not shown muted")

        // Gartenprojekt: unread favorite group, every entry.
        menu("Gartenprojekt")
        for title in ["Als gelesen markieren", "Aus Favoriten entfernen", "Kategorie", "Benachrichtigungen", "Gruppeneinstellungen"] {
            XCTAssertTrue(app.buttons[title].exists, title)
        }
        sleep(1)
        shot("chatmenue")
        app.buttons["Als gelesen markieren"].tap()
        sleep(2)
        menu("Gartenprojekt")
        XCTAssertFalse(app.buttons["Als gelesen markieren"].exists, "still unread")
        closeMenu()

        // Favorite on and off again.
        menu("Lesekreis")
        app.buttons["Zu Favoriten hinzufügen"].tap()
        sleep(2)
        menu("Lesekreis")
        XCTAssertTrue(app.buttons["Aus Favoriten entfernen"].exists, "not a favorite")
        app.buttons["Aus Favoriten entfernen"].tap()
        sleep(2)
        menu("Lesekreis")
        XCTAssertTrue(app.buttons["Zu Favoriten hinzufügen"].exists, "still a favorite")

        // Kategorie: a tick closes the menu; it shows on the next opening.
        app.buttons["Kategorie"].tap()
        XCTAssertTrue(app.buttons["Neue Kategorie …"].waitForExistence(timeout: 3))
        XCTAssertTrue(catItem("Familie & Freunde").exists && catItem("Arbeit").exists)
        XCTAssertFalse(catItem("Arbeit").isSelected)
        shot("chatmenue-kategorie")
        catItem("Arbeit").tap()
        sleep(2)
        XCTAssertFalse(app.buttons["Neue Kategorie …"].exists, "menu still open after a tick")
        XCTAssertTrue(app.buttons["settings"].isHittable)
        menu("Lesekreis")
        app.buttons["Kategorie"].tap()
        XCTAssertTrue(catItem("Arbeit").waitForExistence(timeout: 3))
        XCTAssertTrue(catItem("Arbeit").isSelected, "not in Arbeit")
        catItem("Arbeit").tap()
        sleep(1)
        closeMenu()

        // Neue Kategorie …
        menu("Lesekreis")
        app.buttons["Kategorie"].tap()
        app.buttons["Neue Kategorie …"].tap()
        let field = app.alerts["Neue Kategorie"].textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.typeText("Bücher")
        shot("neue-kategorie")
        app.buttons["Anlegen"].tap()
        XCTAssertTrue(app.buttons["section:Bücher"].waitForExistence(timeout: 10), "category not created")

        // Benachrichtigungen: radio, without "Nur Erwähnungen" in one-to-one chats.
        menu("Sommerfest")
        app.buttons["Benachrichtigungen"].tap()
        XCTAssertTrue(app.buttons["Aus"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.buttons["Aus"].isSelected)
        XCTAssertTrue(app.buttons["Nur Erwähnungen"].exists)
        shot("chatmenue-benachrichtigungen")
        app.buttons["Alle Nachrichten"].tap()
        sleep(2)
        XCTAssertFalse(row("Sommerfest").label.contains("stummgeschaltet"), "still muted")
        menu("Sommerfest")
        app.buttons["Benachrichtigungen"].tap()
        app.buttons["Aus"].tap()
        sleep(2)
        XCTAssertTrue(row("Sommerfest").label.contains("stummgeschaltet"), "not muted again")
        menu("Tobias Krüger")
        XCTAssertFalse(app.buttons["Gruppeneinstellungen"].exists)
        app.buttons["Benachrichtigungen"].tap()
        XCTAssertTrue(app.buttons["Alle Nachrichten"].waitForExistence(timeout: 3))
        XCTAssertFalse(app.buttons["Nur Erwähnungen"].exists, "mentions offered in a one-to-one chat")
        closeMenu()

        // Gruppeneinstellungen: pushed on the list, back leads to the list.
        menu("Familie")
        app.buttons["Gruppeneinstellungen"].tap()
        XCTAssertTrue(app.buttons["notifications"].waitForExistence(timeout: 5), "no group settings")
        sleep(1)
        shot("gruppeneinstellungen")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(app.buttons["settings"].waitForExistence(timeout: 5))
    }
}
