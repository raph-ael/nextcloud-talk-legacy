import XCTest

/// Contacts (docs/attachments.md "Kontakt", Tests 12 to 18) against talk-demo
/// (`TALK_DEV_SERVER`, user mira). Only invented entries: the simulator's own sample
/// contacts and two the script adds with `simctl addmedia` — "Hanna Lindqvist" (Gärtnerei am
/// Hof, mobile +49 30 23125 117, work +49 30 23125 118, hanna@example.org, home Am Hof 3,
/// 10249 Berlin, plus a note and a birthday that must never go out) and "Theo Brinkmann"
/// (a name and a note only).
final class ContactTests: XCTestCase {
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

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func wait(_ e: XCUIElement, _ format: String, timeout: TimeInterval = 15, _ message: String) {
        let ok = XCTWaiter().wait(for: [expectation(for: NSPredicate(format: format), evaluatedWith: e)], timeout: timeout)
        XCTAssertEqual(ok, .completed, message)
    }

    private var any: XCUIElementQuery { app.descendants(matching: .any) }

    /// A chat row, scrolled into view (the demo list is longer than the screen).
    private func row(_ room: String) -> XCUIElement {
        let row = app.buttons["room:\(room)"].firstMatch
        let first = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room:'")).firstMatch
        XCTAssertTrue(first.waitForExistence(timeout: 30), "no chat list")
        for _ in 0..<6 where !(row.exists && row.isHittable && row.frame.maxY < app.frame.height - 120) {
            app.swipeUp(velocity: 600)
            sleep(1)
        }
        XCTAssertTrue(row.exists, "\(room) not listed")
        return row
    }

    private func open(_ room: String) {
        row(room).tap()
        XCTAssertTrue(any["composer"].firstMatch.waitForExistence(timeout: 10))
        sleep(1)
    }

    /// Scrolls the chat until the element is well inside it.
    private func reveal(_ e: XCUIElement) {
        let list = app.collectionViews["messages"]
        for _ in 0..<8 {
            if e.exists && e.isHittable && e.frame.minY > list.frame.minY + 20 && e.frame.maxY < list.frame.maxY - 20 { return }
            if e.exists && e.frame.minY > list.frame.midY { list.swipeUp(velocity: 300) } else { list.swipeDown(velocity: 300) }
            sleep(1)
        }
        XCTAssertTrue(e.exists && e.isHittable, "\(e) not visible")
    }

    private func tile(_ name: String) -> XCUIElement {
        any.matching(identifier: "contact-row").matching(NSPredicate(format: "label == %@", "Kontakt: \(name)")).firstMatch
    }

    private func post(_ path: String, _ form: [String: String]) {
        guard let server = env["TALK_DEV_SERVER"] else { return XCTFail("no TALK_DEV_SERVER") }
        var req = URLRequest(url: URL(string: "\(server)\(path)")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.httpBody = Data(form.map { k, v in
            "\(k)=\(v.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? v)"
        }.joined(separator: "&").utf8)
        let done = expectation(description: path)
        URLSession.shared.dataTask(with: req) { _, response, _ in
            XCTAssertEqual((response as? HTTPURLResponse)?.statusCode, 201, path)
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
    }

    /// The file as the server stores it.
    private func stored(_ path: String) -> String {
        guard let server = env["TALK_DEV_SERVER"] else { XCTFail("no TALK_DEV_SERVER"); return "" }
        let enc = path.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? path
        var req = URLRequest(url: URL(string: "\(server)/remote.php/dav/files/mira/\(enc)")!)
        req.setValue("Basic " + Data("mira:demo".utf8).base64EncodedString(), forHTTPHeaderField: "Authorization")
        var text = ""
        let done = expectation(description: "dav")
        URLSession.shared.dataTask(with: req) { data, _, _ in
            text = data.flatMap { String(data: $0, encoding: .utf8) } ?? ""
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return text
    }

    private func reply(to e: XCUIElement) {
        reveal(e)
        e.press(forDuration: 0.8)
        XCTAssertTrue(app.buttons["menu:reply"].waitForExistence(timeout: 5), "no Antworten")
        app.buttons["menu:reply"].tap()
        XCTAssertTrue(app.buttons["Antwort abbrechen"].waitForExistence(timeout: 5), "no reply bar")
    }

    /// Opens the system's contact picker and picks one entry.
    private func pick(_ name: String) {
        if !app.buttons["attach-contact"].exists { app.buttons["attach"].tap() }
        XCTAssertTrue(app.buttons["attach-contact"].waitForExistence(timeout: 5))
        app.buttons["attach-contact"].tap()
        let entry = app.cells.containing(NSPredicate(format: "label CONTAINS %@", name)).firstMatch
        let text = app.staticTexts.matching(NSPredicate(format: "label == %@", name)).firstMatch
        XCTAssertTrue(entry.waitForExistence(timeout: 10) || text.exists, "\(name) not in the picker: \(app.debugDescription)")
        sleep(1)
        shot("picker-\(name)")
        (entry.exists ? entry : text).tap()
    }

    private var lines: XCUIElementQuery { any.matching(identifier: "contact-line") }

    private func line(_ value: String) -> XCUIElement {
        lines.matching(NSPredicate(format: "label BEGINSWITH %@", value)).firstMatch
    }

    /// Test 12: "Galerie", "Datei", "Standort", "Kamera", "Kontakt" side by side, all in
    /// view, no "Umfrage".
    func testAttachMenu() {
        open("Sommerfest")
        app.buttons["attach"].tap()
        let ids = ["gallery", "attach-file", "attach-place", "attach-camera", "attach-contact"]
        XCTAssertTrue(app.buttons["attach-contact"].waitForExistence(timeout: 5))
        sleep(1)
        let buttons = ids.map { app.buttons[$0] }
        for (id, b) in zip(ids, buttons) { XCTAssertTrue(b.isHittable, "\(id) not in view") }
        let xs = buttons.map(\.frame.midX)
        XCTAssertEqual(xs, xs.sorted(), "order")
        XCTAssertEqual(buttons.map(\.label), ["Galerie", "Datei", "Standort", "Kamera", "Kontakt"])
        XCTAssertFalse(any.matching(NSPredicate(format: "label CONTAINS 'Umfrage'")).firstMatch.exists, "Umfrage still there")
        shot("01-panel")
    }

    /// Test 18: somebody else's contact as a tile, in the list and in the quote, and the
    /// preview of the card.
    func testIncomingContact() {
        _ = row("Sommerfest")
        post("/demo/incoming", ["token": "sommerfest", "text": "Ole Petersen", "contact": "1", "from": "paul"])
        let preview = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "👤 Ole Petersen")).firstMatch
        XCTAssertTrue(preview.waitForExistence(timeout: 20), "chat list shows no 👤")
        shot("02-list")

        open("Sommerfest")
        let ole = tile("Ole Petersen")
        XCTAssertTrue(ole.waitForExistence(timeout: 10), "no tile for the new contact")
        let karla = tile("Karla Brandt")
        reveal(karla)
        sleep(1)
        shot("03-chat")

        reply(to: karla)
        let quote = app.staticTexts.matching(NSPredicate(format: "label == %@", "👤 Karla Brandt")).firstMatch
        XCTAssertTrue(quote.waitForExistence(timeout: 5), "reply bar shows no 👤")
        shot("04-reply")
        app.buttons["Antwort abbrechen"].tap()
        sleep(1)

        reveal(karla)
        karla.tap()
        // docs/attachments.md "Kontaktkachel": the system's contact view with saving.
        let create = app.buttons.matching(NSPredicate(format: "label IN %@", ["Neuen Kontakt erstellen", "Create New Contact"])).firstMatch
        let add = app.buttons.matching(NSPredicate(format: "label IN %@", ["Zu Kontakt hinzufügen", "Zu vorhandenem Kontakt hinzufügen", "Add to Existing Contact"])).firstMatch
        if !create.waitForExistence(timeout: 15) { print("NOTE card tree: \(app.debugDescription)") }
        XCTAssertTrue(create.exists, "no Neuen Kontakt erstellen")
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS 'Karla Brandt'")).firstMatch.exists, "card without the name")
        print("NOTE add button: \(add.exists ? add.label : "-")")
        XCTAssertFalse(XCUIApplication(bundleIdentifier: "com.apple.springboard").alerts.firstMatch.exists, "asks for access")
        sleep(2)
        shot("05-card")
        if !add.isHittable { app.swipeUp(velocity: 400); sleep(1) }
        shot("05b-card-below")
        XCTAssertTrue(add.exists, "no Zu Kontakt hinzufügen")

        // Saving once, only when asked (it adds to the simulator's address book).
        if env["TALK_CONTACT_SAVE"] == "1" {
            if !create.isHittable { app.swipeDown(velocity: 400); sleep(1) }
            create.tap()
            let done = app.buttons.matching(NSPredicate(format: "label IN %@", ["Fertig", "Done", "Sichern", "Save"])).firstMatch
            XCTAssertTrue(done.waitForExistence(timeout: 10), "no editor: \(app.debugDescription)")
            sleep(1)
            shot("05c-editor")
            done.tap()
            sleep(2)
            XCTAssertFalse(XCUIApplication(bundleIdentifier: "com.apple.springboard").alerts.firstMatch.exists, "asks for access")
            shot("05d-saved")
        }

        let close = app.buttons["contact-card-close"]
        XCTAssertTrue(close.waitForExistence(timeout: 5), "no close button")
        close.tap()
        XCTAssertTrue(any["composer"].firstMatch.waitForExistence(timeout: 5), "card did not close")
    }

    /// Tests 13, 14 and 16: everything ticked, no tick leaves the button off, one number
    /// unticked goes out with the quote; the card holds what was ticked and nothing else.
    func testSendContact() {
        open("Sommerfest")
        let last = app.staticTexts["Danke euch allen, das war ein toller Tag!"].firstMatch
        XCTAssertTrue(last.waitForExistence(timeout: 10))
        reply(to: last)

        pick("Hanna Lindqvist")
        let send = app.buttons["contact-send"]
        XCTAssertTrue(send.waitForExistence(timeout: 10), "no sheet")
        XCTAssertEqual(app.staticTexts["contact-name"].label, "Hanna Lindqvist")
        XCTAssertEqual(app.staticTexts["contact-organization"].label, "Gärtnerei am Hof")
        XCTAssertEqual(lines.count, 4, "lines: \(lines.allElementsBoundByIndex.map(\.label))")
        for l in lines.allElementsBoundByIndex { XCTAssertTrue(l.isSelected, "\(l.label) not ticked") }
        XCTAssertTrue(line("+49 30 23125 117").label.hasSuffix("Mobil"), line("+49 30 23125 117").label)
        XCTAssertTrue(line("+49 30 23125 118").label.hasSuffix("Arbeit"))
        XCTAssertTrue(line("hanna@example.org").label.hasSuffix("Privat"))
        XCTAssertTrue(line("Am Hof 3").exists, "no address")
        XCTAssertFalse(any.matching(NSPredicate(format: "label CONTAINS 'Nie mitsenden'")).firstMatch.exists, "note shown")
        XCTAssertTrue(send.isEnabled)
        sleep(1)
        shot("06-sheet")

        for l in lines.allElementsBoundByIndex { l.tap() }
        wait(send, "isEnabled == false", timeout: 3, "button on without a tick")
        shot("07-none")
        for l in lines.allElementsBoundByIndex { l.tap() }
        line("+49 30 23125 118").tap()
        XCTAssertFalse(line("+49 30 23125 118").isSelected)
        XCTAssertTrue(send.isEnabled)
        shot("08-one-off")
        send.tap()

        let hanna = tile("Hanna Lindqvist")
        XCTAssertTrue(hanna.waitForExistence(timeout: 15), "no tile")
        XCTAssertFalse(app.buttons["Antwort abbrechen"].exists, "reply bar stayed")
        let quotes = app.staticTexts.matching(NSPredicate(format: "label == %@", "Danke euch allen, das war ein toller Tag!"))
        XCTAssertEqual(quotes.count, 2, "no quote on the contact")
        if quotes.count == 2 {
            XCTAssertLessThan(quotes.element(boundBy: 1).frame.maxY, hanna.frame.minY + 1, "quote not on the contact")
        }
        // Delivered: the meta shows no clock any more.
        sleep(4)
        shot("09-sent")

        let card = stored("Talk/Hanna Lindqvist.vcf")
        print("NOTE card: \(card.replacingOccurrences(of: "\r\n", with: " | "))")
        XCTAssertTrue(card.contains("BEGIN:VCARD"), "card not on the server")
        XCTAssertTrue(card.contains("Hanna Lindqvist"))
        XCTAssertTrue(card.contains("+49 30 23125 117"))
        XCTAssertFalse(card.contains("23125 118"), "the unticked number went out")
        XCTAssertTrue(card.contains("hanna@example.org"))
        XCTAssertTrue(card.contains("Am Hof 3"))
        XCTAssertTrue(card.contains("Gärtnerei am Hof"))
        XCTAssertFalse(card.contains("Nie mitsenden"), "note went out")
        XCTAssertFalse(card.contains("BDAY"), "birthday went out")

        // The chat list shows the own contact.
        app.navigationBars.buttons.firstMatch.tap()
        let preview = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "👤 Hanna Lindqvist")).firstMatch
        XCTAssertTrue(preview.waitForExistence(timeout: 10), "chat list shows no 👤")
        shot("10-list")
    }

    /// Test 14: an entry with neither number nor address; a sample contact of the
    /// simulator shows its lines.
    func testContactWithoutLines() {
        open("Sommerfest")
        pick("Theo Brinkmann")
        let empty = app.staticTexts["contact-empty"]
        XCTAssertTrue(empty.waitForExistence(timeout: 10), "no hint")
        XCTAssertEqual(empty.label, "Dieser Kontakt hat keine Nummer und keine Adresse")
        XCTAssertFalse(app.buttons["contact-send"].isEnabled, "button on")
        XCTAssertFalse(any.matching(NSPredicate(format: "label CONTAINS 'Ohne Nummer'")).firstMatch.exists, "note shown")
        shot("11-empty")
        app.buttons["contact-close"].tap()
        sleep(1)

        pick("Kate Bell")
        XCTAssertTrue(app.buttons["contact-send"].waitForExistence(timeout: 10), "no sheet")
        XCTAssertGreaterThan(lines.count, 0, "sample contact without lines")
        XCTAssertTrue(app.buttons["contact-send"].isEnabled)
        sleep(1)
        shot("12-sample")
        app.buttons["contact-close"].tap()
        XCTAssertFalse(tile("Kate Bell").waitForExistence(timeout: 2), "closing sent it")
    }
}
