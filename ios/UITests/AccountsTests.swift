import XCTest

/// Several accounts (docs/accounts.md) against two demo servers: Mira on `TALK_DEV_SERVER`
/// (port 8099) and Jonas on `TALK_DEV_SERVER_2` (port 8098, `core/talk-demo/fixtures-b`).
/// Skipped unless `TALK_DEMO=1`. Screenshots are attachments named after the scene.
///
/// `testSwitchDraftsAndSettings` signs in through `TALK_DEV_*` and `TALK_DEV_*_2` (outside
/// the account list); `testLoginAddRestartLogout` uses the real login screens and the
/// stored list, and ends signed out.
final class AccountsTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment
    private var server1: String { env["TALK_DEV_SERVER"] ?? "" }
    private var server2: String { env["TALK_DEV_SERVER_2"] ?? "" }

    override func setUpWithError() throws {
        try XCTSkipUnless(env["TALK_DEMO"] == "1" && !server2.isEmpty, "needs two demo servers (TALK_DEMO=1)")
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments += ["-AppleLanguages", "(de)", "-AppleLocale", "de_DE"]
    }

    private func launchWithDevAccounts() {
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS", "TALK_DEV_SERVER_2", "TALK_DEV_USER_2", "TALK_DEV_PASS_2"] {
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

    /// The server as shown: without `https://` or `http://`.
    private func label(_ s: String) -> String {
        s.replacingOccurrences(of: "https://", with: "").replacingOccurrences(of: "http://", with: "")
    }

    /// "Konten, Neues in {n} Chats", singular for one.
    private func konten(_ n: Int) -> String { n == 1 ? "Konten, Neues in 1 Chat" : "Konten, Neues in \(n) Chats" }

    /// Somebody writes to the signed-in user of a demo server (`/demo/incoming`).
    private func incoming(_ server: String, token: String, text: String) {
        var req = URLRequest(url: URL(string: "\(server)/demo/incoming")!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        let body = ["token": token, "text": text].map { k, v in
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

    private func waitLabel(_ e: XCUIElement, _ text: String, timeout: TimeInterval = 15) {
        let p = NSPredicate(format: "label == %@", text)
        let ok = XCTWaiter().wait(for: [expectation(for: p, evaluatedWith: e)], timeout: timeout) == .completed
        XCTAssertTrue(ok, "label is '\(e.label)', expected '\(text)'")
    }

    private var avatar: XCUIElement { app.buttons["settings"] }

    /// The number in "Konten, Neues in {n} Chats" (0 for "Konten").
    private func newChats(_ label: String) -> Int {
        Int(label.split(whereSeparator: { !$0.isNumber }).first ?? "") ?? 0
    }

    private func menuSwitch(to id: String) {
        avatar.tap()
        let row = app.buttons["account:\(id)"]
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no account menu")
        row.tap()
        sleep(1)
    }

    private func openChat(_ name: String) {
        let r = app.buttons["room:\(name)"].firstMatch
        XCTAssertTrue(r.waitForExistence(timeout: 10), "\(name) not listed")
        r.tap()
        XCTAssertTrue(app.textViews["composer"].waitForExistence(timeout: 10) || app.textFields["composer"].waitForExistence(timeout: 2))
    }

    private var composer: XCUIElement {
        app.textViews["composer"].exists ? app.textViews["composer"] : app.textFields["composer"]
    }

    private func back() {
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(avatar.waitForExistence(timeout: 5))
        sleep(1)
    }

    /// Badge at the avatar, the menu, switching, `garten` on both servers, drafts per
    /// account, the "Konten" section and the logout question, "schon angemeldet".
    func testSwitchDraftsAndSettings() {
        launchWithDevAccounts()
        // A new chat counts once; "Nur Erwähnungen" counts only a mention. (Other tests of
        // the run may have left something new on the server already.)
        let start = newChats(avatar.label)
        incoming(server2, token: "mira", text: "Hallo Jonas")
        waitLabel(avatar, konten(max(start, 1)))
        let afterMira = max(start, 1)
        incoming(server2, token: "werkstatt", text: "Wer hat heute Zeit?")
        sleep(3)
        XCTAssertEqual(avatar.label, konten(afterMira), "a chat on 'Nur Erwähnungen' counted without a mention")
        incoming(server2, token: "werkstatt", text: "@jonas bringst du den Ständer mit?")
        waitLabel(avatar, konten(afterMira + 1))
        shot("avatar-zahl")

        // Account menu: rows in sign-in order, tick and number, add and settings.
        avatar.tap()
        let mira = app.buttons["account:de000001"], jonas = app.buttons["account:de000002"]
        XCTAssertTrue(mira.waitForExistence(timeout: 5))
        XCTAssertEqual(mira.label, "Mira Lange, \(label(server1)), aktiv")
        XCTAssertEqual(jonas.label, "Jonas Richter, \(label(server2)), \(afterMira + 1) Chats mit Neuem")
        XCTAssertLessThan(mira.frame.minY, jonas.frame.minY)
        XCTAssertTrue(app.buttons["account-add"].exists && app.buttons["account-settings"].exists)
        sleep(1)
        shot("kontomenue")
        // The active one only closes the menu.
        mira.tap()
        XCTAssertTrue(mira.waitForNonExistence(timeout: 5))
        menuSwitch(to: "de000002")
        XCTAssertTrue(app.buttons["room:Kleingartenverein"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["room:Gartenprojekt"].exists)
        XCTAssertTrue(avatar.label.hasPrefix("Konten, Neues in "), "Mira's chats not counted: \(avatar.label)")
        shot("chats-jonas")

        // `garten` exists on both servers: this one's name and messages.
        openChat("Kleingartenverein")
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'Wasseruhr'")).firstMatch.waitForExistence(timeout: 10))
        // A draft stays in its account and chat.
        composer.tap()
        composer.typeText("Entwurf von Jonas")
        back()
        menuSwitch(to: "de000001")
        openChat("Gartenprojekt")
        XCTAssertFalse(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'Wasseruhr'")).firstMatch.exists)
        XCTAssertFalse((composer.value as? String ?? "").contains("Entwurf von Jonas"), "draft leaked into the other account")
        back()
        menuSwitch(to: "de000002")
        openChat("Kleingartenverein")
        XCTAssertEqual(composer.value as? String, "Entwurf von Jonas", "draft lost")
        shot("entwurf-jonas")
        composer.tap()
        composer.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 17))
        back()

        // Settings: the active account in the head, the others under "Konten".
        avatar.tap()
        app.buttons["account-settings"].tap()
        XCTAssertTrue(app.navigationBars["Einstellungen"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Jonas Richter"].exists)
        XCTAssertTrue(app.buttons["account:de000001"].exists)
        XCTAssertFalse(app.buttons["account:de000002"].exists)
        XCTAssertTrue(app.buttons["settings-add-account"].exists)
        sleep(1)
        shot("einstellungen-konten")
        // A tap switches, the settings stay open.
        app.buttons["account:de000001"].tap()
        XCTAssertTrue(app.staticTexts["Mira Lange"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.navigationBars["Einstellungen"].exists)
        XCTAssertTrue(app.buttons["account:de000002"].exists)

        // "Abmelden" asks with the account's name.
        app.buttons["settings-logout"].tap()
        let text = "Mira Lange (\(label(server1))) wird auf diesem Gerät abgemeldet. Ungesendete Nachrichten und der Offline-Speicher dieses Kontos werden gelöscht."
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label == %@", text)).firstMatch.waitForExistence(timeout: 5), "logout question")
        sleep(1)
        shot("abmelden")
        app.alerts["Abmelden?"].buttons["Abbrechen"].tap()
        sleep(1)

        // "Konto hinzufügen": the login screen with its title and "Abbrechen".
        app.buttons["account:de000002"].tap()
        XCTAssertTrue(app.staticTexts["Jonas Richter"].waitForExistence(timeout: 5))
        app.buttons["settings-add-account"].tap()
        XCTAssertTrue(app.staticTexts["Konto hinzufügen"].waitForExistence(timeout: 5))
        shot("konto-hinzufuegen")
        app.buttons["login-cancel"].tap()
        XCTAssertTrue(app.navigationBars["Einstellungen"].waitForExistence(timeout: 5))

        // The same account again: a toast, no second entry, it becomes active.
        app.buttons["settings-add-account"].tap()
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText(server1)
        app.buttons["Anmelden"].tap()
        XCTAssertTrue(app.staticTexts["Dieses Konto ist schon angemeldet"].waitForExistence(timeout: 30), "no toast")
        shot("schon-angemeldet")
        XCTAssertTrue(app.staticTexts["Mira Lange"].waitForExistence(timeout: 5), "not switched to the known account")
        XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'account:'")).count, 1)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(avatar.waitForExistence(timeout: 5))
    }

    /// The real way: sign in, add a second account, restart (both and the active one are
    /// still there), sign out one after the other.
    func testLoginAddRestartLogout() {
        app.launch()
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 20), "not signed out at the start")
        field.tap()
        field.typeText(server1)
        app.buttons["Anmelden"].tap()
        XCTAssertTrue(avatar.waitForExistence(timeout: 30), "first login failed")
        waitLabel(avatar, "Einstellungen")

        // With one account the avatar opens the settings; "Konten" offers adding one.
        avatar.tap()
        XCTAssertTrue(app.buttons["settings-add-account"].waitForExistence(timeout: 5))
        app.buttons["settings-add-account"].tap()
        let field2 = app.textFields.firstMatch
        XCTAssertTrue(field2.waitForExistence(timeout: 5))
        field2.tap()
        field2.typeText(server2)
        app.buttons["Anmelden"].tap()
        // The new account is active; the settings stayed underneath.
        XCTAssertTrue(app.staticTexts["Jonas Richter"].waitForExistence(timeout: 30), "second login failed")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(app.buttons["room:Kleingartenverein"].waitForExistence(timeout: 10))
        XCTAssertTrue(avatar.label.hasPrefix("Konten"))

        // Restart: both accounts and the active one are still there.
        app.terminate()
        app.launch()
        XCTAssertTrue(app.buttons["room:Kleingartenverein"].waitForExistence(timeout: 30), "active account not kept")
        avatar.tap()
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'account:'")).firstMatch.waitForExistence(timeout: 5))
        XCTAssertEqual(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'account:'")).count, 2)
        shot("nach-neustart")
        app.buttons["account-settings"].tap()

        // Sign out Jonas: Mira stays and becomes active.
        XCTAssertTrue(app.buttons["settings-logout"].waitForExistence(timeout: 5))
        app.buttons["settings-logout"].tap()
        app.alerts["Abmelden?"].buttons["Abmelden"].tap()
        XCTAssertTrue(app.staticTexts["Mira Lange"].waitForExistence(timeout: 15), "Mira not active after Jonas left")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(app.buttons["room:Gartenprojekt"].waitForExistence(timeout: 10))
        waitLabel(avatar, "Einstellungen")

        // And Mira: the login screen.
        avatar.tap()
        app.buttons["settings-logout"].tap()
        app.alerts["Abmelden?"].buttons["Abmelden"].tap()
        XCTAssertTrue(app.textFields.firstMatch.waitForExistence(timeout: 15), "no login screen after the last account")
    }

    /// App badge: the sum of `badge().messages` over both accounts. A message for the
    /// inactive account raises the number at the home screen icon by one.
    func testAppBadgeSum() {
        launchWithDevAccounts()
        // The badge needs the notification permission: it is asked on the first send.
        openChat("Lena Hoffmann")
        composer.tap()
        composer.typeText("Bis morgen!")
        app.buttons["send"].tap()
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let allow = springboard.alerts.buttons["Erlauben"]
        if allow.waitForExistence(timeout: 10) { allow.tap() }
        back()
        incoming(server2, token: "mira", text: "Erste Nachricht")
        waitLabel(avatar, "Konten, Neues in 1 Chat")
        avatar.tap()
        let jonas = app.buttons["account:de000002"]
        XCTAssertTrue(jonas.waitForExistence(timeout: 5))
        XCTAssertEqual(jonas.label, "Jonas Richter, \(label(server2)), 1 Chat mit Neuem")
        app.buttons["account:de000001"].tap()
        XCTAssertTrue(jonas.waitForNonExistence(timeout: 5))
        sleep(2)
        let before = homeBadge(springboard)
        app.activate()
        XCTAssertTrue(avatar.waitForExistence(timeout: 10))
        incoming(server2, token: "mira", text: "Zweite Nachricht")
        sleep(4)
        let after = homeBadge(springboard)
        shot("app-badge")
        XCTAssertGreaterThan(before, 0, "no app badge")
        XCTAssertEqual(after, before + 1, "the inactive account's message not in the app badge")
        app.activate()
    }

    /// While a call runs, switching is locked: the other rows are greyed out ("nicht
    /// verfügbar") and a tap only shows the toast. A mock call stands in for a real one.
    func testSwitchLockedDuringCall() {
        app.launchEnvironment["TALK_CALL_MOCK"] = "connected"
        launchWithDevAccounts()
        let minimize = app.buttons["call-minimize"]
        XCTAssertTrue(minimize.waitForExistence(timeout: 15), "no mock call")
        minimize.tap()
        XCTAssertTrue(avatar.waitForExistence(timeout: 5))
        avatar.tap()
        let jonas = app.buttons["account:de000002"]
        XCTAssertTrue(jonas.waitForExistence(timeout: 5))
        XCTAssertTrue(jonas.label.hasSuffix("nicht verfügbar"), jonas.label)
        XCTAssertFalse(app.buttons["account:de000001"].label.contains("nicht verfügbar"))
        jonas.tap()
        XCTAssertTrue(app.staticTexts["Während eines Anrufs nicht möglich"].waitForExistence(timeout: 5), "no toast")
        sleep(1)
        shot("gesperrt-im-anruf")
        XCTAssertTrue(app.buttons["account:de000001"].label.hasSuffix("aktiv"), "switched during a call")
    }

    /// The number at the app's home screen icon (0 without one).
    private func homeBadge(_ springboard: XCUIApplication) -> Int {
        XCUIDevice.shared.press(.home)
        sleep(2)
        let icon = springboard.icons["Wolkenfunk"]
        guard icon.waitForExistence(timeout: 10) else { return -1 }
        let text = "\(icon.value ?? "") \(icon.label)"
        let digits = text.split(whereSeparator: { !$0.isNumber }).first.map(String.init) ?? ""
        print("home icon: \(text)")
        return Int(digits) ?? 0
    }
}
