import XCTest

/// Silent messages of somebody else (docs/chat.md "Stille Nachrichten") against the demo
/// server (`cargo run -p talk-demo`, every login works): a status that changes in place and
/// leaves nothing behind once it is deleted. Skipped unless `TALK_DEMO=1`; the server URL
/// comes in `TALK_DEV_SERVER`. Screenshots are attachments named after the scene.
final class QuietMessageTests: XCTestCase {
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
    }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private var list: XCUIElement { app.collectionViews["messages"] }
    private var composer: XCUIElement { app.descendants(matching: .any)["composer"].firstMatch }

    /// A bubble text containing `text`.
    private func bubble(_ text: String) -> XCUIElement {
        list.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    private func count(_ text: String) -> Int {
        list.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", text)).count
    }

    private func until(_ e: XCUIElement, _ format: String, timeout: TimeInterval = 20, _ message: String) {
        let ok = XCTWaiter().wait(for: [expectation(for: NSPredicate(format: format), evaluatedWith: e)], timeout: timeout)
        XCTAssertEqual(ok, .completed, message)
    }

    private func open(_ room: String) {
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "\(room) not listed")
        row.tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
    }

    /// The bubble ends above the input row and is on the screen: the list stands at its end.
    private func atTheEnd(_ e: XCUIElement, _ what: String) {
        XCTAssertTrue(e.frame.maxY <= composer.frame.minY + 1 && e.frame.minY >= list.frame.minY,
                      "\(what) at \(e.frame.minY)…\(e.frame.maxY), input row at \(composer.frame.minY)")
    }

    @discardableResult
    private func post(_ path: String, _ form: [String: String]) -> Int {
        guard let server = env["TALK_DEV_SERVER"] else { XCTFail("no TALK_DEV_SERVER"); return 0 }
        var req = URLRequest(url: URL(string: "\(server)\(path)")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.httpBody = Data(form.map { k, v in
            "\(k)=\(v.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? v)"
        }.joined(separator: "&").utf8)
        var id = 0
        let done = expectation(description: path)
        URLSession.shared.dataTask(with: req) { body, response, _ in
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            XCTAssertTrue((200..<300).contains(status), "\(path): \(status)")
            if let body, let json = try? JSONSerialization.jsonObject(with: body) as? [String: Any],
               let ocs = json["ocs"] as? [String: Any], let data = ocs["data"] as? [String: Any] {
                id = data["id"] as? Int ?? 0
            }
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
        return id
    }

    func testStatusChangesAndGoes() {
        XCUIDevice.shared.appearance = .light
        open("Lena Hoffmann")
        sleep(2)
        let edited = count("bearbeitet")
        let deleted = count("Diese Nachricht wurde gelöscht")

        // A silent message: a status, subdued and small.
        let status = post("/demo/incoming", ["token": "lena", "text": "Arbeitet …", "silent": "1"])
        XCTAssertGreaterThan(status, 0, "no id")
        XCTAssertTrue(bubble("Arbeitet …").waitForExistence(timeout: 20), "status not shown")
        atTheEnd(bubble("Arbeitet …"), "status")
        shot("01-status")

        // It grows in place, without "bearbeitet", and the list stays at its end.
        post("/demo/edit", ["token": "lena", "id": "\(status)", "text": "Arbeitet …\n🛠️ Exec: sucht in den Bestellungen"])
        XCTAssertTrue(bubble("sucht in den Bestellungen").waitForExistence(timeout: 20), "first step not shown")
        post("/demo/edit", ["token": "lena", "id": "\(status)",
                            "text": "Arbeitet …\n🛠️ Exec: sucht in den Bestellungen\n📖 Read: Bericht.md\n▸ Antwort schreiben"])
        XCTAssertTrue(bubble("Antwort schreiben").waitForExistence(timeout: 20), "second step not shown")
        sleep(1)
        atTheEnd(bubble("Antwort schreiben"), "grown status")
        XCTAssertEqual(count("bearbeitet"), edited, "a status does not say that it changed")
        shot("02-status-grown")
        // The same from a page, in the dark design (the argument wins over the stored choice).
        let arguments = app.launchArguments
        app.terminate()
        app.launchArguments = arguments + ["-appearance.theme", "dark"]
        app.launch()
        open("Lena Hoffmann")
        XCTAssertTrue(bubble("Antwort schreiben").waitForExistence(timeout: 20), "status missing after a restart")
        XCTAssertEqual(count("bearbeitet"), edited)
        shot("03-status-dark")
        app.terminate()
        app.launchArguments = arguments
        app.launch()
        open("Lena Hoffmann")
        XCTAssertTrue(bubble("Antwort schreiben").waitForExistence(timeout: 20))

        // The answer is a message like any other.
        post("/demo/incoming", ["token": "lena", "text": "Fertig."])
        XCTAssertTrue(bubble("Fertig.").waitForExistence(timeout: 20), "answer not shown")
        atTheEnd(bubble("Fertig."), "answer")
        shot("04-answer")

        // Deleted, the status is gone: no "deleted" bubble.
        post("/demo/delete", ["token": "lena", "id": "\(status)"])
        until(bubble("Antwort schreiben"), "exists == false", "the status stays")
        sleep(1)
        XCTAssertEqual(count("Diese Nachricht wurde gelöscht"), deleted, "a deleted status leaves a bubble")
        atTheEnd(bubble("Fertig."), "answer after the status went")
        shot("05-status-gone")

        // A message that was not silent leaves its bubble, as before.
        let loud = post("/demo/incoming", ["token": "lena", "text": "Vertippt"])
        XCTAssertTrue(bubble("Vertippt").waitForExistence(timeout: 20), "message not shown")
        post("/demo/delete", ["token": "lena", "id": "\(loud)"])
        until(bubble("Vertippt"), "exists == false", "the deleted message keeps its text")
        sleep(1)
        XCTAssertEqual(count("Diese Nachricht wurde gelöscht"), deleted + 1, "a deleted message leaves its bubble")
        shot("06-deleted")

        // The same after opening the chat again.
        app.navigationBars.buttons.firstMatch.tap()
        open("Lena Hoffmann")
        sleep(2)
        XCTAssertTrue(bubble("Fertig.").exists, "answer missing after reopening")
        XCTAssertFalse(bubble("Arbeitet").exists, "the status is back")
        XCTAssertEqual(count("Diese Nachricht wurde gelöscht"), deleted + 1)
        shot("07-reopened")
    }
}
