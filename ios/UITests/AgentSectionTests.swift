import XCTest

/// The section "KI-Agenten" (docs/categories.md "KI-Agenten") against the demo server
/// (`cargo run -p talk-demo`, every login works): who wears the status emoji 🤖 has their
/// chat in a section of its own, right below the favourites. Skipped unless `TALK_DEMO=1`;
/// the server URL comes in `TALK_DEV_SERVER`. Screenshots are attachments named after the scene.
final class AgentSectionTests: XCTestCase {
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
    }

    override func tearDown() {
        // Lena is a person again, whatever happened.
        if env["TALK_DEMO"] == "1" { mark(agent: false) }
    }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func head(_ title: String) -> XCUIElement { app.buttons["section:\(title)"] }
    private var lenas: XCUIElementQuery { app.buttons.matching(identifier: "room:Lena Hoffmann") }

    private func until(_ e: XCUIElement, _ format: String, timeout: TimeInterval = 20, _ message: String) {
        let ok = XCTWaiter().wait(for: [expectation(for: NSPredicate(format: format), evaluatedWith: e)], timeout: timeout)
        XCTAssertEqual(ok, .completed, message)
    }

    /// Lena sets or clears the status emoji, as an agent does.
    private func mark(agent: Bool) {
        guard let server = env["TALK_DEV_SERVER"] else { XCTFail("no TALK_DEV_SERVER"); return }
        var req = URLRequest(url: URL(string: "\(server)/demo/agent")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.httpBody = Data("user=lena&on=\(agent ? 1 : 0)".utf8)
        let done = expectation(description: "demo/agent")
        URLSession.shared.dataTask(with: req) { _, response, _ in
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            XCTAssertTrue((200..<300).contains(status), "/demo/agent: \(status)")
            done.fulfill()
        }.resume()
        wait(for: [done], timeout: 10)
    }

    /// The titles of the section heads on the screen, from the top.
    private func heads() -> [String] {
        app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'section:'")).allElementsBoundByIndex
            .filter(\.exists)
            .sorted { $0.frame.minY < $1.frame.minY }
            .map { String($0.identifier.dropFirst("section:".count)) }
    }

    func testAgentsHaveTheirOwnSection() {
        XCUIDevice.shared.appearance = .light
        mark(agent: true)
        app.launch()

        // The list tells it with its first full fetch.
        let agents = head("KI-Agenten")
        XCTAssertTrue(agents.waitForExistence(timeout: 40), "no section KI-Agenten")
        let family = head("Familie & Freunde")
        XCTAssertTrue(family.waitForExistence(timeout: 5), "no section Familie & Freunde")
        XCTAssertEqual(Array(heads().prefix(3)), ["Favoriten", "KI-Agenten", "Familie & Freunde"])
        XCTAssertEqual(agents.label, "KI-Agenten")
        XCTAssertEqual(agents.value as? String, "ausgeklappt")

        // Lena is in it, and still in her category.
        until(lenas.element(boundBy: 1), "exists == true", "Lena is not listed twice")
        XCTAssertEqual(lenas.count, 2)
        let rows = lenas.allElementsBoundByIndex.map(\.frame.minY).sorted()
        XCTAssertTrue(agents.frame.minY < rows[0] && rows[0] < family.frame.minY, "Lena is not under KI-Agenten")
        XCTAssertGreaterThan(rows[1], family.frame.minY, "Lena left her category")
        shot("01-ki-agenten")

        // Collapsed, the section is empty and its head says how many chats it has.
        agents.tap()
        until(agents, "value == 'eingeklappt'", "does not collapse")
        XCTAssertEqual(agents.label, "KI-Agenten, 1 Chat")
        XCTAssertEqual(lenas.count, 1, "the collapsed section still shows its chat")
        shot("02-eingeklappt")
        agents.tap()
        until(agents, "value == 'ausgeklappt'", "does not expand")
        until(lenas.element(boundBy: 1), "exists == true", "Lena is not back")

        // Without the status the section is gone after the next start.
        mark(agent: false)
        app.terminate()
        app.launch()
        XCTAssertTrue(head("Favoriten").waitForExistence(timeout: 40))
        until(agents, "exists == false", timeout: 40, "the section stays without an agent")
        XCTAssertEqual(lenas.count, 1)
        XCTAssertEqual(Array(heads().prefix(2)), ["Favoriten", "Familie & Freunde"])
        shot("03-ohne-agent")
    }
}
