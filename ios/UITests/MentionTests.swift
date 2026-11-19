import XCTest

/// Mentions (docs/mentions.md "Test") against talk-demo (`TALK_DEV_SERVER`, user mira =
/// Mira Lange). "Sommerfest" has the group "Helferkreis", the team "Orga-Team" and three
/// messages with mentions; in "Familie" three people are called "Lange"; "Team Website"
/// mentions Mira.
final class MentionTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        app.launchArguments += ["-notifications.asked.start", "YES"]
        app.launch()
    }

    private var shots = 0

    private func shot(_ name: String) {
        shots += 1
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = String(format: "%02d-", shots) + name
        a.lifetime = .keepAlways
        add(a)
    }

    private var any: XCUIElementQuery { app.descendants(matching: .any) }
    private var list: XCUIElement { app.collectionViews["messages"] }
    private var composer: XCUIElement { any["composer"].firstMatch }
    private var suggestions: XCUIElement { any["mention-list"].firstMatch }
    private var mentionButton: XCUIElement { app.buttons["jump-to-mention"] }

    private func row(_ label: String) -> XCUIElement { app.buttons["mention:\(label)"] }

    private var rows: XCUIElementQuery {
        app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'mention:'"))
    }

    private func bubble(_ text: String) -> XCUIElement {
        list.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    private func gone(_ e: XCUIElement, _ message: String, timeout: TimeInterval = 5) {
        let ok = XCTWaiter().wait(for: [expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: e)], timeout: timeout)
        XCTAssertEqual(ok, .completed, message)
    }

    private func waitFor(_ e: XCUIElement, _ format: String, timeout: TimeInterval = 10, _ message: String) {
        let ok = XCTWaiter().wait(for: [expectation(for: NSPredicate(format: format), evaluatedWith: e)], timeout: timeout)
        XCTAssertEqual(ok, .completed, message)
    }

    /// Opens a chat by its name in the chat list.
    private func open(_ name: String) {
        let first = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room:'")).firstMatch
        XCTAssertTrue(first.waitForExistence(timeout: 30), "no chat list")
        let room = app.buttons["room:\(name)"].firstMatch
        // The list is lazy: rows further down exist only once scrolled to.
        for _ in 0..<3 where !(room.exists && room.isHittable) { app.swipeDown(velocity: .fast) }
        for _ in 0..<8 where !(room.exists && room.isHittable) { app.swipeUp(velocity: .slow) }
        XCTAssertTrue(room.exists, "no chat \(name)")
        room.tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 10), "chat \(name) did not open")
    }

    private func back() {
        app.navigationBars.buttons.element(boundBy: 0).tap()
    }

    private var value: String { composer.value as? String ?? "" }

    /// Empties the field.
    private func clear() {
        composer.tap()
        let n = value.count
        if n > 0 { composer.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: n)) }
    }

    /// Types `text` into the field and picks `label` from the suggestions.
    private func pick(_ text: String, _ label: String) {
        composer.typeText(text)
        XCTAssertTrue(row(label).waitForExistence(timeout: 5), "\(label) not suggested for \(text)")
        row(label).tap()
    }

    @discardableResult
    private func incoming(_ token: String, _ text: String, from: String) -> Bool {
        guard let server = env["TALK_DEV_SERVER"] else { XCTFail("no TALK_DEV_SERVER"); return false }
        var req = URLRequest(url: URL(string: "\(server)/demo/incoming")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        let form = ["token": token, "text": text, "from": from]
        req.httpBody = Data(form.map { k, v in
            "\(k)=\(v.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? v)"
        }.joined(separator: "&").utf8)
        var ok = false
        let done = expectation(description: "incoming")
        URLSession.shared.dataTask(with: req) { _, response, _ in
            ok = (200..<300).contains((response as? HTTPURLResponse)?.statusCode ?? 0)
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        XCTAssertTrue(ok, "demo/incoming failed")
        return ok
    }

    /// Tests 1 to 5: the list after "@", the search text, other chats, taking a row,
    /// backspace removing the mention as a whole.
    func testSuggestions() {
        open("Sommerfest")
        composer.tap()
        composer.typeText("@")
        XCTAssertTrue(suggestions.waitForExistence(timeout: 5), "no suggestions after @")
        // Test 1: everybody but Mira, "Alle" last (4.5 rows show, the rest scrolls).
        for label in ["Lena Hoffmann", "Jonas Richter", "Sophie Neumann", "Paul Lange", "Tobias Krüger"] {
            XCTAssertTrue(row(label).exists, "\(label) missing")
        }
        XCTAssertFalse(row("Mira Lange").exists, "the user is suggested")
        shot("suggestions")
        suggestions.swipeUp()
        for label in ["Helferkreis", "Orga-Team", "Alle"] {
            XCTAssertTrue(row(label).exists, "\(label) missing")
        }
        XCTAssertEqual(rows.allElementsBoundByIndex.last?.identifier, "mention:Alle", "Alle is not last")

        // Test 2: the search text narrows it; an unknown name closes the list.
        composer.typeText("kru")
        XCTAssertTrue(row("Tobias Krüger").waitForExistence(timeout: 3))
        XCTAssertEqual(rows.count, 1, "kru")
        clear()
        composer.typeText("@Lena H")
        XCTAssertTrue(row("Lena Hoffmann").waitForExistence(timeout: 3))
        composer.typeText(XCUIKeyboardKey.delete.rawValue + "x")
        gone(suggestions, "list stays for @Lena x")

        // Back closes the list first, the chat stays.
        clear()
        composer.typeText("@")
        XCTAssertTrue(suggestions.waitForExistence(timeout: 5))
        app.buttons["mention-back"].tap()
        gone(suggestions, "back did not close the list")
        XCTAssertTrue(composer.exists, "back left the chat")

        // Test 5: taking a row, highlighted; backspace: the space, then the whole mention.
        clear()
        pick("@Lena H", "Lena Hoffmann")
        XCTAssertEqual(value, "@Lena Hoffmann ")
        gone(suggestions, "list stays after picking")
        shot("picked")
        composer.typeText(XCUIKeyboardKey.delete.rawValue)
        XCTAssertEqual(value, "@Lena Hoffmann")
        composer.typeText(XCUIKeyboardKey.delete.rawValue)
        XCTAssertEqual(value, "", "mention not removed as a whole")
        back()

        // Test 3: "Familie", three called Lange.
        open("Familie")
        composer.tap()
        composer.typeText("@lan")
        XCTAssertTrue(suggestions.waitForExistence(timeout: 5))
        for label in ["Hilde Lange", "Paul Lange", "Emil Lange"] {
            XCTAssertTrue(row(label).exists, "\(label) missing")
        }
        clear()
        back()

        // Test 4: a 1:1 chat has no list.
        open("Lena Hoffmann")
        composer.tap()
        composer.typeText("@")
        sleep(1)
        XCTAssertFalse(suggestions.exists, "list in a 1:1 chat")
        clear()
    }

    /// Tests 6, 7, 9, 10, 14: plain text after typing into a name, a message with all four
    /// kinds, editing, the draft, the search on a mention.
    func testSendEditDraft() {
        let n = String(Int(Date().timeIntervalSince1970) % 100000)
        open("Sommerfest")
        composer.tap()

        // Test 6: typing into the name makes it plain text.
        pick("@Sophie", "Sophie Neumann")
        XCTAssertEqual(value, "@Sophie Neumann ")
        // A letter right behind the name: no mention any more (the rule needs none to follow).
        composer.typeText(XCUIKeyboardKey.delete.rawValue + "x eins \(n)")
        app.buttons["send"].tap()
        XCTAssertTrue(bubble("@Sophie Neumannx eins \(n)").waitForExistence(timeout: 10), "plain text not sent")
        XCTAssertFalse(list.links["@Sophie Neumann"].exists, "still a mention")

        // Test 7: person, group, team and everybody in one message.
        composer.tap()
        pick("@Lena", "Lena Hoffmann")
        pick("@Helfer", "Helferkreis")
        pick("@Orga", "Orga-Team")
        pick("@All", "Alle")
        composer.typeText("zwei \(n)")
        app.buttons["send"].tap()
        let sent = bubble("@Lena Hoffmann @Helferkreis @Orga-Team @Alle zwei \(n)")
        XCTAssertTrue(sent.waitForExistence(timeout: 10), "mentions not sent by name")
        sleep(2)
        shot("sent")

        // Test 9: editing shows the name; a second mention added, both stay.
        composer.tap()
        pick("@Jonas", "Jonas Richter")
        composer.typeText("drei \(n)")
        app.buttons["send"].tap()
        let three = bubble("@Jonas Richter drei \(n)")
        XCTAssertTrue(three.waitForExistence(timeout: 10))
        three.press(forDuration: 0.8)
        XCTAssertTrue(app.buttons["menu:edit"].waitForExistence(timeout: 5))
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(any["edit-bar"].waitForExistence(timeout: 5))
        waitFor(composer, "value == '@Jonas Richter drei \(n)'", "edit field does not show the name")
        shot("edit")
        composer.typeText(" ")
        pick("@Pa", "Paul Lange")
        app.buttons["save-edit"].tap()
        gone(any["edit-bar"].firstMatch, "edit mode stays")
        XCTAssertTrue(bubble("@Jonas Richter drei \(n) @Paul Lange").waitForExistence(timeout: 10), "edit not saved")
        let edited = bubble("@Jonas Richter drei \(n) @Paul Lange")
        edited.press(forDuration: 0.8)
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(any["edit-bar"].waitForExistence(timeout: 5))
        XCTAssertFalse(value.contains("@\""), "server notation in the field: \(value)")
        // Both are still mentions: backspace at the end removes "@Paul Lange" as a whole.
        composer.typeText(XCUIKeyboardKey.delete.rawValue)
        XCTAssertEqual(value, "@Jonas Richter drei \(n) ", "second mention lost after saving")
        app.buttons["edit-cancel"].tap()

        // Test 10: the draft with a mention comes back and is sent as a mention.
        composer.tap()
        pick("@Tob", "Tobias Krüger")
        composer.typeText("vier \(n)")
        back()
        open("Sommerfest")
        XCTAssertEqual(value, "@Tobias Krüger vier \(n)", "draft lost")
        app.buttons["send"].tap()
        let four = bubble("@Tobias Krüger vier \(n)")
        XCTAssertTrue(four.waitForExistence(timeout: 10), "draft not sent")
        XCTAssertTrue(list.links["@Tobias Krüger"].firstMatch.exists, "draft not sent as a mention")

        // Test 14: the search on a mention lays yellow over it.
        app.buttons["chat-search"].tap()
        let field = app.textFields["chat-search-field"]
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText("Helferkreis")
        XCTAssertTrue(app.staticTexts["chat-search-count"].waitForExistence(timeout: 5))
        XCTAssertFalse(mentionButton.exists, "@ button while searching")
        sleep(1)
        shot("search")
        app.buttons["search-close"].tap()
    }

    /// Tests 8, 11 and 15: the marks of the demo chats, tapping a mention, system messages.
    func testMarksAndTap() {
        // Test 8: "@Mira Lange" in Team Website on mentionSelf.
        open("Team Website")
        XCTAssertTrue(bubble("@Mira Lange kannst du").waitForExistence(timeout: 10), "mention of Mira not by name")
        sleep(1)
        shot("website")
        back()

        open("Sommerfest")
        XCTAssertTrue(bubble("@Alle Der Getränkewagen").waitForExistence(timeout: 10), "@all not shown as @Alle")
        XCTAssertTrue(bubble("@Helferkreis wer bringt").exists)
        XCTAssertTrue(bubble("@Paul Lange und @Orga-Team:").exists)
        // Test 15: no "@" before the acting person of a system message.
        let system = list.staticTexts.matching(NSPredicate(format: "label CONTAINS 'hat die Unterhaltung'")).firstMatch
        if system.exists { XCTAssertFalse(system.label.hasPrefix("@"), system.label) }
        sleep(1)
        shot("sommerfest")

        // Test 11: Paul has a 1:1 chat: it opens at once, back leads into the group.
        let paul = list.links["@Paul Lange"].firstMatch
        XCTAssertTrue(paul.waitForExistence(timeout: 5), "no link on @Paul Lange")
        XCTAssertFalse(list.links["@Helferkreis"].exists, "a group mention is tappable")
        XCTAssertFalse(list.links["@Alle"].exists, "@Alle is tappable")
        paul.tap()
        let title = any["chat-title"].firstMatch
        waitFor(title, "label CONTAINS 'Paul Lange'", "1:1 chat with Paul did not open")
        back()
        waitFor(title, "label CONTAINS 'Sommerfest' OR value CONTAINS 'Sommerfest'", "back did not lead into the group")

        // Jonas has none: the question first, "Abbrechen" creates nothing.
        composer.tap()
        pick("@Jon", "Jonas Richter")
        composer.typeText("fünf")
        app.buttons["send"].tap()
        let jonas = list.links["@Jonas Richter"].firstMatch
        XCTAssertTrue(jonas.waitForExistence(timeout: 10), "no link on @Jonas Richter")
        jonas.tap()
        let alert = app.alerts["Chat mit Jonas Richter beginnen?"]
        XCTAssertTrue(alert.waitForExistence(timeout: 5), "no question for a chat that does not exist")
        shot("ask")
        alert.buttons["Abbrechen"].tap()
        gone(alert, "question stays")
        waitFor(title, "label CONTAINS 'Sommerfest' OR value CONTAINS 'Sommerfest'", "left the group")
    }

    /// Tests 12 and 13: the "@" button.
    func testMentionButton() {
        let first = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room:'")).firstMatch
        XCTAssertTrue(first.waitForExistence(timeout: 30))
        // Test 12: 30 messages, a mention, 30 more into a chat that is not open.
        for i in 1...30 { incoming("lesekreis", "Vorher \(i)", from: "clara") }
        incoming("lesekreis", "@mira hast du das Buch schon?", from: "lena")
        for i in 1...30 { incoming("lesekreis", "Nachher \(i)", from: "clara") }
        open("Lesekreis")
        XCTAssertTrue(mentionButton.waitForExistence(timeout: 10), "no @ button")
        XCTAssertEqual(mentionButton.label, "Zur Erwähnung springen")
        XCTAssertTrue(app.buttons["jump-to-bottom"].exists, "no jump button below")
        XCTAssertLessThan(mentionButton.frame.maxY, app.buttons["jump-to-bottom"].frame.minY, "not above the jump button")
        shot("button")
        mentionButton.tap()
        XCTAssertTrue(bubble("@Mira Lange hast du das Buch").waitForExistence(timeout: 10))
        gone(mentionButton, "button stays after the jump")
        shot("jumped")

        // Test 13: open and scrolled up, two mentions arrive.
        for _ in 0..<4 { list.swipeDown(velocity: .fast) }
        // Far enough apart that the jump to the first does not show the second.
        incoming("lesekreis", "@mira kommst du am Donnerstag?", from: "sophie")
        for i in 1...15 { incoming("lesekreis", "Dazwischen \(i)", from: "clara") }
        incoming("lesekreis", "@all Treffen um acht", from: "clara")
        XCTAssertTrue(mentionButton.waitForExistence(timeout: 10), "no @ button for new mentions")
        waitFor(mentionButton, "label == 'Zur Erwähnung springen, 2 Erwähnungen'", "count is not 2")
        shot("two")
        mentionButton.tap()
        waitFor(mentionButton, "label == 'Zur Erwähnung springen'", "count did not go down")
        // Scrolling to the other one counts as seen too.
        for _ in 0..<4 where mentionButton.exists { list.swipeUp(velocity: .slow) }
        gone(mentionButton, "scrolling to the mention did not count", timeout: 10)
    }
}
