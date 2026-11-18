import XCTest

/// Full-text search and opening a chat at a message (docs/search.md, Tests 1 to 13) against
/// talk-demo (`TALK_DEV_SERVER`, user mira). The archived "Hofladen-Archiv" holds 1,200
/// numbered notes ("Notiz 618"); "Quittenmarmelade" (37), "Hüpfburg" (412) and
/// "Rhabarberschorle" (905) occur once each.
final class SearchTests: XCTestCase {
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
    private var list: XCUIElement { app.collectionViews["messages"] }
    private var hits: XCUIElementQuery { app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'hit:'")) }

    private func hit(_ text: String) -> XCUIElement {
        hits.matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    /// A bubble text containing `text`.
    private func bubble(_ text: String) -> XCUIElement {
        list.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    private func note(_ n: Int) -> XCUIElement { bubble("(Notiz \(n))") }

    private func waitForList() {
        let first = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'room:'")).firstMatch
        XCTAssertTrue(first.waitForExistence(timeout: 30), "no chat list")
    }

    /// The search field of the chat list with exactly `text` in it.
    private func search(_ text: String) {
        let field = app.searchFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 10), "no search field")
        if !field.isHittable { app.swipeDown(velocity: 600) }
        field.tap()
        let old = (field.value as? String) ?? ""
        if !old.isEmpty && old != field.placeholderValue {
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: old.count))
        }
        field.typeText(text)
    }

    private func chatSearch(_ text: String) {
        let field = app.textFields["chat-search-field"]
        XCTAssertTrue(field.waitForExistence(timeout: 5), "no search field in the chat")
        field.tap()
        let old = (field.value as? String) ?? ""
        if !old.isEmpty && old != "Suchen" {
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: old.count))
        }
        field.typeText(text)
    }

    private var count: XCUIElement { app.staticTexts["chat-search-count"] }

    /// In the upper third of the list (docs/search.md "Position"), a little slack for the
    /// bubble's padding.
    private func inUpperThird(_ e: XCUIElement, _ what: String) {
        XCTAssertTrue(e.waitForExistence(timeout: 15), "\(what) not shown")
        let l = list.frame
        XCTAssertTrue(e.frame.minY >= l.minY - 4 && e.frame.minY < l.minY + l.height / 3 + 24,
                      "\(what) at \(e.frame.minY), list \(l.minY)…\(l.maxY)")
    }

    @discardableResult
    private func post(_ path: String, _ form: [String: String]) -> [String: Any] {
        guard let server = env["TALK_DEV_SERVER"] else { XCTFail("no TALK_DEV_SERVER"); return [:] }
        var req = URLRequest(url: URL(string: "\(server)\(path)")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.httpBody = Data(form.map { k, v in
            "\(k)=\(v.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? v)"
        }.joined(separator: "&").utf8)
        var data: [String: Any] = [:]
        let done = expectation(description: path)
        URLSession.shared.dataTask(with: req) { body, response, _ in
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            XCTAssertTrue((200..<300).contains(status), "\(path): \(status)")
            if let body, let json = try? JSONSerialization.jsonObject(with: body) as? [String: Any],
               let ocs = json["ocs"] as? [String: Any] {
                data = ocs["data"] as? [String: Any] ?? [:]
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return data
    }

    /// Tests 1, 2, 4 to 7 and 9: chat list with archived section and message hits.
    func testSearchAllChats() {
        waitForList()
        // Test 9: right after the first sign-in the history is still loading.
        search("notiz")
        let loading = app.staticTexts["search-loading"]
        if loading.waitForExistence(timeout: 5) {
            XCTAssertTrue(loading.label.hasPrefix("Ältere Nachrichten werden noch geladen ("), loading.label)
            shot("01-loading")
            wait(loading, "exists == false", timeout: 90, "the history never finished loading")
        } else {
            print("NOTE history loaded before the first look")
        }

        // Test 1: the archived chat by name, under it message hits.
        search("hofladen")
        XCTAssertTrue(app.staticTexts["section:Archiviert"].waitForExistence(timeout: 5), "no Archiviert")
        XCTAssertTrue(app.buttons["room:Hofladen-Archiv"].exists, "archived chat not found by name")
        XCTAssertTrue(app.staticTexts["section:Nachrichten"].waitForExistence(timeout: 5), "no Nachrichten")
        XCTAssertTrue(hits.firstMatch.waitForExistence(timeout: 5), "no message hits")
        XCTAssertLessThan(app.buttons["room:Hofladen-Archiv"].frame.minY, app.staticTexts["section:Nachrichten"].frame.minY)
        shot("02-hofladen")

        // Test 2: one hit, at once.
        search("quitten")
        let quitten = hit("Quittenmarmelade")
        XCTAssertTrue(quitten.waitForExistence(timeout: 3), "no hit for quitten")
        XCTAssertTrue(quitten.label.contains("Hofladen-Archiv"), quitten.label)
        XCTAssertEqual(hits.count, 1)
        XCTAssertFalse(app.staticTexts["section:Archiviert"].exists)
        shot("03-quitten")

        // Test 5: words in any order, without case and accents.
        search("buro HUPF")
        XCTAssertTrue(hit("Hüpfburg").waitForExistence(timeout: 3), "buro HUPF found nothing")
        XCTAssertEqual(hits.count, 1)
        search("Hüpfburg Büro")
        XCTAssertTrue(hit("Hüpfburg").waitForExistence(timeout: 3))
        XCTAssertEqual(hits.count, 1)
        shot("05-hupfburg")

        // Test 7: a contact's name and a place's name.
        search("karla brandt")
        let karla = hit("Karla Brandt")
        XCTAssertTrue(karla.waitForExistence(timeout: 3), "contact not found")
        XCTAssertTrue(karla.label.contains("Sommerfest"), karla.label)
        search("volkspark")
        XCTAssertTrue(hits.firstMatch.waitForExistence(timeout: 3), "place not found")
        shot("07-volkspark")

        // Test 6: 50 hits, more when scrolling.
        search("notiz")
        XCTAssertTrue(hits.firstMatch.waitForExistence(timeout: 3))
        // The list only builds the rows on screen: collect them while scrolling down.
        var ids = Set<String>()
        for _ in 0..<40 where ids.count <= 60 {
            ids.formUnion(hits.allElementsBoundByIndex.map(\.identifier))
            let rows = app.collectionViews.firstMatch
            rows.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.4))
                .press(forDuration: 0.05, thenDragTo: rows.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1)))
        }
        XCTAssertGreaterThan(ids.count, 60, "no further hits while scrolling")
        shot("06-notiz")

        // Test 4: back from the chat, the search is still there.
        search("quitten")
        XCTAssertTrue(quitten.waitForExistence(timeout: 3))
        quitten.tap()
        XCTAssertTrue(app.buttons["jump-to-bottom"].waitForExistence(timeout: 15), "no window")
        app.navigationBars.buttons.firstMatch.tap()
        XCTAssertTrue(quitten.waitForExistence(timeout: 5), "hit gone after back")
        XCTAssertEqual(app.searchFields.firstMatch.value as? String, "quitten")
        shot("04-back")
    }

    /// Test 3 and 12: a hit opens the chat at the message in a window; it loads down to the
    /// end without jumps, new messages wait in the button.
    func testOpenAtHit() {
        waitForList()
        search("rhabarber")
        let h = hit("Rhabarberschorle")
        XCTAssertTrue(h.waitForExistence(timeout: 90), "no hit for rhabarber")
        h.tap()
        let target = note(905)
        inUpperThird(target, "Notiz 905")
        XCTAssertTrue(app.buttons["jump-to-bottom"].exists, "no Zum Ende springen in the window")
        XCTAssertFalse(bubble("Der Hofladen zieht um").exists, "the end flashed up")
        shot("10-open-at-hit")

        // Pages coming in below keep the screen still.
        let before = target.frame.minY
        sleep(3)
        XCTAssertEqual(target.frame.minY, before, accuracy: 2, "the list moved by itself")
        // A slow drag moves it by the distance dragged, also over rows measured on the way.
        let start = list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.7))
        start.press(forDuration: 0.1, thenDragTo: start.withOffset(CGVector(dx: 0, dy: -200)), withVelocity: 300, thenHoldForDuration: 0.5)
        sleep(1)
        if target.exists {
            XCTAssertEqual(before - target.frame.minY, 200, accuracy: 30, "jumped while dragging")
        }

        // Test 12: a new message in the window waits in the button.
        post("/demo/incoming", ["token": "hofladen", "text": "Neue Nachricht im Archiv (Fensterprobe)", "from": "jonas"])
        let jump = app.buttons["jump-to-bottom"]
        let counted = XCTWaiter().wait(for: [expectation(for: NSPredicate(format: "label == %@", "Zum Ende springen, 1 neue Nachricht"), evaluatedWith: jump)], timeout: 20)
        if counted != .completed { shot("12-window-new-failed") }
        XCTAssertEqual(jump.label, "Zum Ende springen, 1 neue Nachricht")
        XCTAssertFalse(bubble("Fensterprobe").exists, "the new message shows in the window")
        shot("12-window-new")

        // Down to the end by scrolling: no gap, no message twice.
        for _ in 0..<60 {
            // Flipped list: up leads to the newer messages.
            list.swipeUp(velocity: 2500)
            if bubble("Fensterprobe").exists { break }
        }
        XCTAssertTrue(bubble("Fensterprobe").waitForExistence(timeout: 10), "end not reached")
        XCTAssertTrue(bubble("Der Hofladen zieht um").exists)
        let last = list.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "Fensterprobe"))
        XCTAssertEqual(last.count, 1, "the held message shows twice")
        // Near the end the notes follow without a gap.
        list.swipeDown(velocity: 400)
        sleep(1)
        let visible = list.staticTexts.matching(NSPredicate(format: "label CONTAINS 'Notiz 1'")).allElementsBoundByIndex
            .compactMap { e -> Int? in
                guard let r = e.label.range(of: "(Notiz ") else { return nil }
                return Int(e.label[r.upperBound...].prefix { $0.isNumber })
            }
        XCTAssertEqual(visible.count, Set(visible).count, "a note twice: \(visible)")
        if let lo = visible.min(), let hi = visible.max() {
            XCTAssertEqual(Set(lo...hi), Set(visible), "gap among \(visible.sorted())")
        }
        shot("13-window-end")

        // "Zum Ende springen" straight from a window.
        app.navigationBars.buttons.firstMatch.tap()
        search("quitten")
        XCTAssertTrue(hit("Quittenmarmelade").waitForExistence(timeout: 5))
        hit("Quittenmarmelade").tap()
        inUpperThird(note(37), "Notiz 37")
        XCTAssertTrue(jump.waitForExistence(timeout: 5))
        jump.tap()
        XCTAssertTrue(bubble("Fensterprobe").waitForExistence(timeout: 10), "Zum Ende springen did not reach the end")
        wait(jump, "exists == false", timeout: 10, "button stays at the end")
        shot("14-jumped-to-end")
    }

    /// Test 8: searching in the chat, counter and arrows, marks; back closes the search first.
    func testSearchInChat() {
        waitForList()
        search("hofladen")
        let room = app.buttons["room:Hofladen-Archiv"]
        XCTAssertTrue(room.waitForExistence(timeout: 10))
        room.tap()
        XCTAssertTrue(bubble("Der Hofladen zieht um").waitForExistence(timeout: 15), "chat end not shown")
        app.buttons["chat-search"].tap()
        XCTAssertTrue(app.textFields["chat-search-field"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["chat-search-older"].isEnabled)
        XCTAssertFalse(count.exists && !count.label.isEmpty, "a count for an empty field")

        // "notiz 41": both words anywhere, so 41, 141 … 1141 and 410 to 419; newest first.
        chatSearch("notiz 41")
        wait(count, "label == '1 von 22'", timeout: 90, "count for notiz 41")
        XCTAssertEqual(count.label, "1 von 22")
        inUpperThird(note(1141), "Notiz 1141")
        XCTAssertFalse(app.buttons["chat-search-newer"].isEnabled, "newer at the newest hit")
        shot("20-search-in-chat")
        app.buttons["chat-search-older"].tap()
        XCTAssertEqual(count.label, "2 von 22")
        inUpperThird(note(1041), "Notiz 1041")
        for _ in 0..<7 { app.buttons["chat-search-older"].tap() }
        XCTAssertEqual(count.label, "9 von 22")
        inUpperThird(note(419), "Notiz 419")
        for _ in 0..<13 { app.buttons["chat-search-older"].tap() }
        XCTAssertEqual(count.label, "22 von 22")
        inUpperThird(note(41), "Notiz 41")
        XCTAssertFalse(app.buttons["chat-search-older"].isEnabled, "older at the oldest hit")
        shot("21-oldest-hit")
        app.buttons["chat-search-newer"].tap()
        XCTAssertEqual(count.label, "21 von 22")
        inUpperThird(note(141), "Notiz 141")

        chatSearch("schlüssel")
        wait(count, "label == '1 von 1'", timeout: 10, "count for schlüssel")
        XCTAssertEqual(count.label, "1 von 1")
        inUpperThird(note(412), "Notiz 412")
        shot("22-schluessel")
        // Test 8 of the contract: "anhänger", about 130 hits.
        chatSearch("anhänger")
        wait(count, "label BEGINSWITH '1 von '", timeout: 10, "count for anhänger")
        let total = Int(count.label.dropFirst("1 von ".count)) ?? 0
        XCTAssertGreaterThan(total, 100, count.label)
        anhaengerInUpperThird()
        for i in 2...4 {
            app.buttons["chat-search-older"].tap()
            XCTAssertEqual(count.label, "\(i) von \(total)")
            anhaengerInUpperThird()
        }
        shot("24-anhaenger")
        chatSearch("gibtsnicht")
        wait(count, "label == 'Keine Treffer'", timeout: 10, "no hits")
        XCTAssertEqual(count.label, "Keine Treffer")

        // Back closes the search, the chat stays where it is; a second back leaves the chat.
        let staying = list.staticTexts.matching(NSPredicate(format: "label CONTAINS 'Anhänger'")).firstMatch
        let stayingLabel = staying.label
        app.buttons["search-close"].tap()
        XCTAssertTrue(app.buttons["call-button"].waitForExistence(timeout: 5), "header not back")
        XCTAssertTrue(bubble(stayingLabel).exists, "the chat moved on closing")
        XCTAssertTrue(app.buttons["jump-to-bottom"].exists, "no Zum Ende springen away from the end")
        XCTAssertTrue(app.buttons["composer"].exists || any["composer"].exists, "no input row")
        shot("23-search-closed")
        app.navigationBars.buttons.firstMatch.tap()
        XCTAssertTrue(app.searchFields.firstMatch.waitForExistence(timeout: 5), "chat not left")
    }

    /// The chosen "Anhänger" hit stands in the upper third, marked.
    private func anhaengerInUpperThird() {
        sleep(1)
        let l = list.frame
        let shown = list.staticTexts.matching(NSPredicate(format: "label CONTAINS 'Anhänger'")).allElementsBoundByIndex
        XCTAssertTrue(shown.contains { $0.frame.minY >= l.minY - 4 && $0.frame.minY < l.minY + l.height / 3 + 24 },
                      "no Anhänger hit in the upper third")
    }

    /// Test 13 and 11: a quote leads to the quoted message; a new message is found, its edit
    /// and its deletion follow.
    func testQuoteAndChanges() {
        waitForList()
        let familie = app.buttons["room:Familie"]
        XCTAssertTrue(familie.waitForExistence(timeout: 10))
        familie.tap()
        let quote = list.descendants(matching: .any).matching(identifier: "quote").firstMatch
        for _ in 0..<6 where !(quote.exists && quote.isHittable) { list.swipeDown(velocity: 600) }
        XCTAssertTrue(quote.exists, "no quote")
        // Away from it first, so the jump has somewhere to go.
        let quoted = bubble("Funktioniert das auch auf meinem Tablet")
        quote.tap()
        XCTAssertTrue(quoted.waitForExistence(timeout: 5), "quoted message not shown")
        inUpperThird(quoted, "quoted message")
        shot("30-quote")
        app.navigationBars.buttons.firstMatch.tap()

        // Test 11: a new message is found.
        let sent = post("/demo/incoming", ["token": "lena", "text": "Zitronenkuchen für den Lesekreis", "from": "lena"])
        let id = (sent["id"] as? Int) ?? 0
        XCTAssertGreaterThan(id, 0, "no id")
        search("zitronen")
        XCTAssertTrue(hit("Zitronenkuchen").waitForExistence(timeout: 20), "new message not found")
        hit("Zitronenkuchen").tap()
        XCTAssertTrue(bubble("Zitronenkuchen").waitForExistence(timeout: 10))
        // Edited and deleted in the open chat.
        post("/demo/edit", ["token": "lena", "id": "\(id)", "text": "Zitronentarte für den Lesekreis"])
        XCTAssertTrue(bubble("Zitronentarte").waitForExistence(timeout: 20), "edit not shown")
        app.buttons["chat-search"].tap()
        chatSearch("zitronen")
        wait(count, "label == '1 von 1'", timeout: 10, "edited message not found")
        XCTAssertEqual(count.label, "1 von 1")
        app.buttons["search-close"].tap()
        app.navigationBars.buttons.firstMatch.tap()
        XCTAssertTrue(hit("Zitronentarte").waitForExistence(timeout: 10), "the list shows the old text")
        shot("31-edited")
        hit("Zitronentarte").tap()
        post("/demo/delete", ["token": "lena", "id": "\(id)"])
        XCTAssertTrue(bubble("Diese Nachricht wurde gelöscht").waitForExistence(timeout: 20), "delete not shown")
        app.navigationBars.buttons.firstMatch.tap()
        wait(hit("Zitronen"), "exists == false", timeout: 10, "deleted message still found")
        shot("32-deleted")
    }

    /// Test 10: without network the search goes on; a jump into a part never opened says
    /// "Keine Verbindung". Last (it restarts the server, which resets the demo).
    func testZOffline() {
        waitForList()
        search("notiz")
        let loading = app.staticTexts["search-loading"]
        _ = loading.waitForExistence(timeout: 3)
        if loading.exists { wait(loading, "exists == false", timeout: 90, "history still loading") }
        print("NOTE stop-server")
        sleep(4)
        // Note 618 lies far from every part the other tests opened.
        search("notiz 618")
        let h = hit("(Notiz 618)")
        XCTAssertTrue(h.waitForExistence(timeout: 5), "no hit offline")
        XCTAssertEqual(hits.count, 1)
        shot("40-offline-search")
        h.tap()
        let toast = any["toast"]
        XCTAssertTrue(toast.waitForExistence(timeout: 20), "no toast")
        XCTAssertTrue(toast.label.contains("Keine Verbindung"), toast.label)
        shot("41-offline-jump")
        print("NOTE start-server")
        sleep(3)
    }
}
