import XCTest

/// Groups (docs/groups.md) against the real server as Bot A; the only other member is ever
/// Bot B (`TALK_BOT_ROOM` is Bot B's name, as in its 1:1). Test groups are called "Test iOS …";
/// the test script deletes any that are left. No screenshots of unfiltered user lists.
final class GroupTests: XCTestCase {
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

    private func el(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }

    /// Scrolls the settings down until the (lazily built) row exists.
    private func scrollTo(_ id: String) -> XCUIElement {
        let e = el(id)
        for _ in 0..<5 where !e.exists || !e.isHittable { app.swipeUp() }
        XCTAssertTrue(e.exists, "\(id) not found")
        return e
    }

    private func waitGone(_ e: XCUIElement, timeout: TimeInterval = 10, _ message: String) {
        let x = expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: e)
        x.expectationDescription = message
        wait(for: [x], timeout: timeout)
    }

    private func waitLabel(_ e: XCUIElement, _ label: String, timeout: TimeInterval = 10) {
        let x = expectation(for: NSPredicate(format: "label == %@", label), evaluatedWith: e)
        x.expectationDescription = "\(e) label \(label)"
        wait(for: [x], timeout: timeout)
    }

    private func roomList() {
        XCTAssertTrue(app.buttons["new-message"].waitForExistence(timeout: 30), "room list missing")
    }

    /// The search field of the screen on top, filled with the bot's name.
    private func searchBot() {
        let field = app.searchFields["Name suchen"].firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText(bot)
    }

    private func createGroup(_ name: String, withBot: Bool, photo: Bool = false) {
        roomList()
        app.buttons["new-message"].tap()
        XCTAssertTrue(el("new-group").waitForExistence(timeout: 10), "no 'Neue Gruppe'")
        el("new-group").tap()
        let next = app.buttons["members-next"]
        XCTAssertTrue(next.waitForExistence(timeout: 5))
        XCTAssertEqual(next.label, "Überspringen")
        if withBot {
            searchBot()
            let row = el("pick:\(bot)")
            XCTAssertTrue(row.waitForExistence(timeout: 10), "bot not found")
            XCTAssertEqual(row.label, "\(bot), nicht ausgewählt")
            row.tap()
            XCTAssertTrue(app.buttons["\(bot) entfernen"].waitForExistence(timeout: 3), "no chip")
            XCTAssertEqual(next.label, "Weiter")
        }
        next.tap()
        let field = app.textFields["group-name"]
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["group-create"].isEnabled, "create without a name")
        if photo { pickPhoto() }
        field.tap()
        field.typeText(name)
        shot("name-group")
        app.buttons["group-create"].tap()
        let title = app.buttons["chat-title"]
        XCTAssertTrue(title.waitForExistence(timeout: 20), "group chat not opened")
        XCTAssertEqual(title.value as? String, name)
    }

    /// The first picture of the simulator's library, if the picker shows one.
    private func pickPhoto() {
        app.buttons["group-photo"].tap()
        let image = app.images.matching(NSPredicate(format: "label CONTAINS[c] 'Photo' OR label CONTAINS[c] 'Foto'")).firstMatch
        if image.waitForExistence(timeout: 8) {
            sleep(1)
            // Out-of-process picker: its cells are not "hittable" for XCTest, coordinates are.
            image.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
            XCTAssertTrue(app.buttons["photo-remove"].waitForExistence(timeout: 10), "photo not taken")
        } else {
            shot("picker")
            app.buttons["Abbrechen"].firstMatch.tap()
        }
    }

    private func openSettings() {
        app.buttons["chat-title"].tap()
        XCTAssertTrue(el("group-subtitle").waitForExistence(timeout: 5), "settings not opened")
    }

    /// Create with Bot B, edit, notifications, link, moderator, remove/add, leave as the last
    /// moderator (Bot B takes over; the script deletes the group as Bot B).
    func testCreateManageLeave() {
        let name = "Test iOS \(Int(Date().timeIntervalSince1970))"
        createGroup(name, withBot: true, photo: true)
        openSettings()
        let subtitle = el("group-subtitle")
        waitLabel(subtitle, "Gruppe · 2 Mitglieder")
        XCTAssertTrue(el("member:self").exists)
        XCTAssertEqual(el("member:self").label, "Du, Moderator")
        XCTAssertEqual(el("member:\(bot)").label, bot)
        shot("settings")

        // Description through "Gruppe bearbeiten".
        el("group-description-add").tap()
        let desc = app.textViews["edit-description"].exists ? app.textViews["edit-description"] : app.textFields["edit-description"]
        XCTAssertTrue(desc.waitForExistence(timeout: 5))
        desc.tap()
        desc.typeText("Beschreibung aus dem iOS-Test")
        shot("edit")
        app.buttons["edit-save"].tap()
        XCTAssertTrue(el("group-description").waitForExistence(timeout: 10), "description not shown")
        XCTAssertEqual(el("group-description").label, "Beschreibung aus dem iOS-Test")

        // Notifications: mentions only.
        let bell = el("notifications")
        XCTAssertEqual(bell.value as? String, "Alle Nachrichten")
        bell.tap()
        app.buttons["Nur Erwähnungen"].firstMatch.tap()
        let mentions = expectation(for: NSPredicate(format: "value == 'Nur Erwähnungen'"), evaluatedWith: bell)
        wait(for: [mentions], timeout: 10)

        // Group link on and off.
        let link = app.switches["group-link"].firstMatch
        XCTAssertTrue(link.exists)
        link.switches.firstMatch.tap()
        waitLabel(subtitle, "Gruppe mit Link · 2 Mitglieder")
        app.swipeUp()
        XCTAssertTrue(el("group-link-url").waitForExistence(timeout: 10), "link not shown")
        XCTAssertTrue(el("group-link-copy").exists)
        XCTAssertTrue(el("group-link-share").exists)
        shot("link")
        link.switches.firstMatch.tap()
        waitGone(el("group-link-url"), "link still shown")
        waitLabel(subtitle, "Gruppe · 2 Mitglieder")
        app.swipeDown()

        // Moderator rights for Bot B and back.
        let member = el("member:\(bot)")
        member.tap()
        XCTAssertTrue(app.buttons["member-moderator"].waitForExistence(timeout: 5))
        XCTAssertEqual(app.buttons["member-moderator"].label, "Zum Moderator machen")
        shot("member")
        app.buttons["member-moderator"].tap()
        waitLabel(member, "\(bot), Moderator")
        member.tap()
        XCTAssertTrue(app.buttons["member-moderator"].waitForExistence(timeout: 5))
        XCTAssertEqual(app.buttons["member-moderator"].label, "Moderatorrechte entziehen")
        app.buttons["member-moderator"].tap()
        waitLabel(member, bot)

        // Remove Bot B, then add again.
        member.tap()
        app.buttons["member-remove"].tap()
        app.buttons["Entfernen"].firstMatch.tap()
        waitLabel(subtitle, "Gruppe · 1 Mitglied")
        el("members-add").tap()
        searchBot()
        let pick = el("pick:\(bot)")
        XCTAssertTrue(pick.waitForExistence(timeout: 10), "bot not offered")
        pick.tap()
        XCTAssertEqual(app.buttons["members-next"].label, "Hinzufügen (1)")
        app.buttons["members-next"].tap()
        waitLabel(subtitle, "Gruppe · 2 Mitglieder")

        // Leave as the only moderator: Bot B takes over first.
        scrollTo("group-leave").tap()
        let successor = el("successor:\(bot)")
        XCTAssertTrue(successor.waitForExistence(timeout: 5), "no 'Neuen Moderator wählen'")
        shot("new-moderator")
        successor.tap()
        roomList()
        waitGone(app.buttons["room:\(name)"], "left group still listed")
        XCTAssertFalse(app.staticTexts["Du bist nicht mehr in dieser Gruppe"].exists, "notice after leaving ourselves")
    }

    /// "Neue Nachricht" opens the existing 1:1; a group of one is created and deleted.
    func testDirectAndDelete() {
        roomList()
        app.buttons["new-message"].tap()
        searchBot()
        XCTAssertFalse(el("new-group").exists, "'Neue Gruppe' while searching")
        let contact = el("contact:\(bot)")
        XCTAssertTrue(contact.waitForExistence(timeout: 10))
        shot("contacts-search")
        contact.tap()
        let title = el("chat-title")
        XCTAssertTrue(title.waitForExistence(timeout: 20), "1:1 not opened")
        XCTAssertEqual(title.label, bot)
        app.navigationBars.buttons.element(boundBy: 0).tap()

        let name = "Test iOS leer \(Int(Date().timeIntervalSince1970))"
        createGroup(name, withBot: false)
        openSettings()
        waitLabel(el("group-subtitle"), "Gruppe · 1 Mitglied")
        scrollTo("group-delete").tap()
        app.buttons["Löschen"].firstMatch.tap()
        roomList()
        waitGone(app.buttons["room:\(name)"], "deleted group still listed")
    }

    /// Bot B created "Test iOS entfernt …" with us; we open its settings and say so in the
    /// chat; Bot B removes us: chat and settings close with the notice.
    func testRemovedByOther() {
        roomList()
        let row = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room:Test iOS entfernt'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 60), "Bot B's group not listed")
        row.tap()
        let composer = el("composer")
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
        composer.tap()
        composer.typeText("iOS bereit")
        app.buttons["send"].tap()
        openSettings()
        XCTAssertTrue(el("member:\(bot)").waitForExistence(timeout: 10))
        XCTAssertEqual(el("member:\(bot)").label, "\(bot), Moderator")
        XCTAssertFalse(el("group-edit").exists, "edit offered to a plain member")
        XCTAssertFalse(el("group-delete").exists)
        shot("settings-member")
        XCTAssertTrue(app.staticTexts["Du bist nicht mehr in dieser Gruppe"].waitForExistence(timeout: 90), "no notice")
        XCTAssertTrue(app.buttons["new-message"].waitForExistence(timeout: 5), "settings still open")
        waitGone(row, "group still listed")
    }
}
