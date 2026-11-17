import XCTest

/// Files and places (docs/attachments.md) against talk-demo (`TALK_DEV_SERVER`, user mira).
/// The script prepares the simulator: the position (`simctl location … set 52.5283,13.4356`),
/// a fresh install (the tests answer the permission question; `testPlaceDenied` says
/// "Nicht erlauben" and runs alone, with `TALK_PLACE_DENIED=1`) and, for `testSendFiles`,
/// the files Einkaufsliste.pdf, Testvideo.mp4, Notizen.wfx and Weitwinkel.jpg in
/// "Auf meinem iPhone".
final class AttachmentTests: XCTestCase {
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

    /// A chat row, scrolled into view (the demo list is longer than the screen).
    private func row(_ room: String) -> XCUIElement {
        let row = app.buttons["room:\(room)"].firstMatch
        let any = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room:'")).firstMatch
        XCTAssertTrue(any.waitForExistence(timeout: 30), "no chat list")
        for _ in 0..<6 where !(row.exists && row.isHittable && row.frame.maxY < app.frame.height - 120) {
            app.swipeUp(velocity: 600)
            sleep(1)
        }
        XCTAssertTrue(row.exists, "\(room) not listed")
        return row
    }

    private func open(_ room: String) {
        row(room).tap()
        XCTAssertTrue(app.descendants(matching: .any)["composer"].firstMatch.waitForExistence(timeout: 10))
        sleep(1)
    }

    /// Somebody else shares a place (`/demo/incoming` with `lat` and `lon`).
    private func incomingPlace(_ token: String, name: String, from: String) {
        guard let server = env["TALK_DEV_SERVER"] else { return XCTFail("no TALK_DEV_SERVER") }
        var req = URLRequest(url: URL(string: "\(server)/demo/incoming")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        let body = ["token": token, "text": name, "lat": "52.5283", "lon": "13.4356", "from": from].map { k, v in
            "\(k)=\(v.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? v)"
        }.joined(separator: "&")
        req.httpBody = Data(body.utf8)
        let done = expectation(description: "incoming")
        URLSession.shared.dataTask(with: req) { _, response, _ in
            XCTAssertEqual((response as? HTTPURLResponse)?.statusCode, 201, "demo/incoming")
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
    }

    private func place(_ name: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: "place-bubble")
            .matching(NSPredicate(format: "label == %@", "Standort: \(name)")).firstMatch
    }

    /// The system asks once ("beim Verwenden der App" or not at all).
    private func answerLocationAlert(allow: Bool) {
        let alert = XCUIApplication(bundleIdentifier: "com.apple.springboard").alerts.firstMatch
        guard alert.waitForExistence(timeout: 5) else { return }
        shot(allow ? "00-permission" : "00-permission-deny")
        let labels = allow ? ["Beim Verwenden der App erlauben", "Allow While Using App"] : ["Nicht erlauben", "Don’t Allow", "Don't Allow"]
        let b = alert.buttons.matching(NSPredicate(format: "label IN %@", labels)).firstMatch
        XCTAssertTrue(b.exists, "permission button: \(alert.debugDescription)")
        b.tap()
    }

    private func closeMenu() {
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.98)).tap()
        sleep(1)
    }

    /// docs/attachments.md Test 9: a place from somebody else in the list, the bubble, the
    /// menu ("Kopieren", no "Bearbeiten") and Apple Maps.
    func testIncomingPlace() {
        _ = row("Sommerfest")
        incomingPlace("sommerfest", name: "Treffpunkt am Brunnen", from: "lena")
        let preview = app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "📍 Treffpunkt am Brunnen")).firstMatch
        XCTAssertTrue(preview.waitForExistence(timeout: 20), "chat list shows no 📍")
        shot("01-list")

        open("Sommerfest")
        let bubble = place("Treffpunkt am Brunnen")
        XCTAssertTrue(bubble.waitForExistence(timeout: 10), "no place bubble")
        XCTAssertEqual(bubble.frame.width, 240, accuracy: 1, "map width")
        sleep(3)
        shot("02-chat")

        // The fixture's place further up.
        let older = place("Volkspark Friedrichshain, Berlin")
        for _ in 0..<4 where !older.isHittable {
            app.collectionViews.firstMatch.swipeDown(velocity: 500)
            sleep(1)
        }
        XCTAssertTrue(older.exists, "fixture place missing")
        sleep(2)
        shot("03-fixture")

        reveal(bubble)
        bubble.press(forDuration: 0.8)
        XCTAssertTrue(app.buttons["menu:copy"].waitForExistence(timeout: 5), "no Kopieren")
        XCTAssertTrue(app.buttons["menu:reply"].exists)
        XCTAssertFalse(app.buttons["menu:edit"].exists, "places cannot be edited")
        shot("04-menu")
        app.buttons["menu:copy"].tap()
        sleep(1)
        // What "Kopieren" put there, pasted into the input field.
        let composer = app.descendants(matching: .any)["composer"].firstMatch
        composer.tap()
        composer.press(forDuration: 1.2)
        let paste = app.menuItems.matching(NSPredicate(format: "label IN %@", ["Einsetzen", "Paste"])).firstMatch
        XCTAssertTrue(paste.waitForExistence(timeout: 5), "no Einsetzen")
        paste.tap()
        sleep(1)
        let link = "https://www.openstreetmap.org/?mlat=52.528300&mlon=13.435600#map=16/52.528300/13.435600"
        XCTAssertEqual(composer.value as? String, "Treffpunkt am Brunnen\n" + link)
        shot("04b-pasted")
        composer.press(forDuration: 1.2)
        let all = app.menuItems.matching(NSPredicate(format: "label IN %@", ["Alles auswählen", "Select All"])).firstMatch
        if all.waitForExistence(timeout: 3) {
            all.tap()
            composer.typeText(XCUIKeyboardKey.delete.rawValue)
        }
        app.collectionViews.firstMatch.swipeDown(velocity: 300)
        sleep(1)

        reveal(older)
        older.tap()
        let maps = XCUIApplication(bundleIdentifier: "com.apple.Maps")
        XCTAssertTrue(maps.wait(for: .runningForeground, timeout: 20), "Apple Maps did not open")
        sleep(4)
        shot("05-maps")
        maps.terminate()
        app.activate()
    }

    /// Test 7 and 8: the sheet with map, name and accuracy; sending with a reply bar open
    /// sends no quote and leaves the bar.
    func testSendPlace() {
        open("Sommerfest")
        let last = app.staticTexts["Danke euch allen, das war ein toller Tag!"].firstMatch
        XCTAssertTrue(last.waitForExistence(timeout: 10))
        last.press(forDuration: 0.8)
        XCTAssertTrue(app.buttons["menu:reply"].waitForExistence(timeout: 5))
        app.buttons["menu:reply"].tap()
        XCTAssertTrue(app.buttons["Antwort abbrechen"].waitForExistence(timeout: 5), "no reply bar")

        app.buttons["attach"].tap()
        XCTAssertTrue(app.buttons["attach-place"].waitForExistence(timeout: 5))
        sleep(1)
        shot("05b-panel")
        app.buttons["attach-place"].tap()
        answerLocationAlert(allow: true)
        let send = app.buttons["place-send"]
        XCTAssertTrue(send.waitForExistence(timeout: 10), "no sheet")
        wait(send, "isEnabled == true", timeout: 30, "no position")
        let name = app.staticTexts["place-name"]
        wait(name, "label != 'Standort'", timeout: 8, "no address")
        let accuracy = app.staticTexts["place-accuracy"]
        XCTAssertTrue(accuracy.label.hasPrefix("Auf etwa "), accuracy.label)
        sleep(2)
        shot("06-sheet")
        let sent = name.label
        send.tap()
        XCTAssertFalse(send.waitForExistence(timeout: 2) && send.isHittable, "sheet still open")
        let bubble = place(sent)
        XCTAssertTrue(bubble.waitForExistence(timeout: 15), "sent place missing")
        XCTAssertTrue(app.buttons["Antwort abbrechen"].exists, "reply bar went away")
        sleep(3)
        shot("07-sent")
    }

    /// Test 7, first half: without the permission the sheet says so and offers the settings.
    func testPlaceDenied() throws {
        try XCTSkipUnless(env["TALK_PLACE_DENIED"] == "1", "needs the permission revoked")
        open("Sommerfest")
        app.buttons["attach"].tap()
        app.buttons["attach-place"].tap()
        answerLocationAlert(allow: false)
        let text = app.staticTexts["place-message"]
        XCTAssertTrue(text.waitForExistence(timeout: 10))
        XCTAssertEqual(text.label, "Wolkenfunk darf den Standort nicht verwenden")
        XCTAssertEqual(app.buttons["place-action"].label, "Einstellungen öffnen")
        shot("08-denied")
        app.buttons["place-action"].tap()
        let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
        XCTAssertTrue(settings.wait(for: .runningForeground, timeout: 15), "Settings did not open")
        sleep(2)
        shot("09-settings")
        settings.terminate()
        app.activate()
    }

    /// Picks files in "Auf meinem iPhone" (multiple selection). The picker opens where it
    /// was last; going back and in again there loses its selection mode, so it only
    /// navigates when the files are not shown yet.
    private func pick(_ names: [String]) {
        app.buttons["attach"].tap()
        app.buttons["attach-file"].tap()
        func cell(_ n: String) -> XCUIElement {
            let base = (n as NSString).deletingPathExtension
            return app.cells.matching(NSPredicate(format: "identifier BEGINSWITH %@ OR identifier BEGINSWITH %@", base + ",", n + ",")).firstMatch
        }
        if !cell(names[0]).waitForExistence(timeout: 5) {
            let browse = app.tabBars.buttons.matching(NSPredicate(format: "label IN %@", ["Durchsuchen", "Browse"])).firstMatch
            if browse.exists { browse.tap() }
            let local = app.descendants(matching: .any)
                .matching(NSPredicate(format: "label IN %@", ["Auf meinem iPhone", "On My iPhone"])).firstMatch
            if local.waitForExistence(timeout: 5) && local.isHittable { local.tap() }
        }
        sleep(2)
        shot("09-picker-open")
        let openButton = app.buttons.matching(NSPredicate(format: "label IN %@", ["Öffnen", "Open"])).firstMatch
        for n in names {
            XCTAssertTrue(cell(n).waitForExistence(timeout: 10), "\(n) not in the picker")
            cell(n).tap()
        }
        shot("10-picker")
        // With multiple selection the first tap selects; "Öffnen" hands them over.
        XCTAssertTrue(openButton.waitForExistence(timeout: 5), "no Öffnen")
        openButton.tap()
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

    /// A file tile; the server may have renamed the file ("Notizen (2).wfx").
    private func fileRow(_ name: String) -> XCUIElement {
        let base = (name as NSString).deletingPathExtension
        return app.descendants(matching: .any).matching(identifier: "file-row")
            .matching(NSPredicate(format: "label BEGINSWITH %@ OR label BEGINSWITH %@", name + ",", base + " (")).firstMatch
    }

    /// Tests 1, 2, 5 and 6 as far as the demo goes: three files with a quote and caption in
    /// the chosen order, ✕ removes one, a photo as a file stays a photo, own files open in
    /// the preview, an unknown type gets the hint.
    func testSendFiles() {
        open("Gartenprojekt")
        let quoted = app.staticTexts["Wer gießt am Wochenende? Ich könnte am Sonntag."].firstMatch
        // The chat opens at the unread line; the newest message is further down.
        for _ in 0..<4 where !(quoted.exists && quoted.isHittable) {
            app.collectionViews.firstMatch.swipeUp(velocity: 600)
            sleep(1)
        }
        XCTAssertTrue(quoted.waitForExistence(timeout: 5))
        quoted.press(forDuration: 0.8)
        XCTAssertTrue(app.buttons["menu:reply"].waitForExistence(timeout: 5))
        app.buttons["menu:reply"].tap()
        XCTAssertTrue(app.buttons["Antwort abbrechen"].waitForExistence(timeout: 5), "no reply bar")
        pick(["Einkaufsliste.pdf", "Testvideo.mp4", "Notizen.wfx", "Weitwinkel.jpg"])
        let title = app.navigationBars["4 Dateien senden"]
        XCTAssertTrue(title.waitForExistence(timeout: 10), "no send screen")
        shot("11-send-four")
        // The picker hands the files over in its own order, not the tapping order; the
        // send screen and the chat keep that order.
        let rows = app.descendants(matching: .any).matching(identifier: "file-send-row")
        rows.containing(NSPredicate(format: "label == 'Weitwinkel.jpg'")).firstMatch.buttons["file-remove"].tap()
        XCTAssertTrue(app.navigationBars["3 Dateien senden"].waitForExistence(timeout: 5), "✕ did not remove")
        let order = rows.allElementsBoundByIndex.map { r in
            r.staticTexts.allElementsBoundByIndex.map(\.label).first { $0.contains(".") } ?? ""
        }
        XCTAssertEqual(Set(order), ["Einkaufsliste.pdf", "Testvideo.mp4", "Notizen.wfx"])
        let caption = app.textFields["caption"].exists ? app.textFields["caption"] : app.textViews["caption"]
        caption.tap()
        caption.typeText("Unterlagen fürs Fest")
        shot("12-send-three")
        app.buttons["send-files"].tap()
        let tiles = order.map { fileRow($0) }
        XCTAssertTrue(tiles[2].waitForExistence(timeout: 20), "file bubble missing")
        let caps = app.staticTexts["Unterlagen fürs Fest"].firstMatch
        XCTAssertTrue(caps.waitForExistence(timeout: 10))
        wait(tiles[2], "isHittable == true", timeout: 10, "last file not visible")
        XCTAssertLessThan(tiles[0].frame.minY, tiles[1].frame.minY, "order")
        XCTAssertLessThan(tiles[1].frame.minY, tiles[2].frame.minY, "order")
        XCTAssertGreaterThan(caps.frame.minY, tiles[2].frame.minY, "caption not on the last file")
        // The quote hangs on the first file only.
        let quote = app.staticTexts.matching(NSPredicate(format: "label == %@", "Wer gießt am Wochenende? Ich könnte am Sonntag."))
        XCTAssertEqual(quote.count, 2, "one quote expected")
        if quote.count == 2 {
            let q = quote.element(boundBy: 1).frame
            XCTAssertLessThan(q.maxY, tiles[0].frame.minY + 1, "quote not on the first file")
            XCTAssertGreaterThan(q.minY, quote.element(boundBy: 0).frame.maxY, "quote order")
        }
        XCTAssertFalse(app.buttons["Antwort abbrechen"].exists, "reply bar stayed")
        sleep(2)
        shot("13-sent")

        // A photo picked as a file is shown as a photo.
        pick(["Weitwinkel.jpg"])
        XCTAssertTrue(app.navigationBars["Datei senden"].waitForExistence(timeout: 10))
        app.buttons["send-files"].tap()
        // A photo bubble below the caption of the three files.
        let photos = app.descendants(matching: .any).matching(identifier: "chat-photo")
        var shown = false
        for _ in 0..<20 where !shown {
            sleep(1)
            let below = caps.exists ? caps.frame.maxY : 0
            shown = photos.allElementsBoundByIndex.contains { $0.frame.minY > below && $0.frame.width > 200 }
        }
        XCTAssertTrue(shown, "the photo sent as a file is not shown as a photo")
        sleep(2)
        shot("14-photo")

        // Own PDF: spinner, then QuickLook under its name.
        let pdfRow = fileRow("Einkaufsliste.pdf")
        reveal(pdfRow)
        pdfRow.tap()
        let ql = app.navigationBars.matching(NSPredicate(format: "identifier CONTAINS 'Einkaufsliste' OR label CONTAINS 'Einkaufsliste'")).firstMatch
        let anyQl = app.otherElements["QLPreviewControllerView"]
        XCTAssertTrue(ql.waitForExistence(timeout: 15) || anyQl.exists, "no preview: \(app.debugDescription)")
        sleep(2)
        shot("15-quicklook")
        let done = app.buttons.matching(NSPredicate(format: "label IN %@", ["Fertig", "Done", "Schließen", "Close"])).firstMatch
        if done.exists { done.tap() } else { app.swipeDown(velocity: .fast) }
        sleep(1)

        // Unknown type.
        reveal(fileRow("Notizen.wfx"))
        fileRow("Notizen.wfx").tap()
        sleep(3)
        shot("16-unknown")
        let toast = app.staticTexts["toast"]
        if toast.exists {
            XCTAssertEqual(toast.label, "Keine App zum Öffnen von Notizen.wfx")
        }
    }
}
