import XCTest

/// Editing and deleting (docs/chat.md "Bearbeiten und Löschen") against the demo server as
/// "mira": "Familie" is her own group (she may delete others' messages there), "Lena
/// Hoffmann" a 1:1. Environment as in `ChatFlowTests`; `TALK_EDIT_GROUP`, `TALK_EDIT_ONE`,
/// `TALK_EDIT_OTHER` (somebody else's message in the group), `TALK_EDIT_OLD` (an own message
/// older than a day) and `TALK_EDIT_ONE_OTHER` (the other's message in the 1:1) point it elsewhere.
final class EditDeleteTests: XCTestCase {
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

    private var group: String { env["TALK_EDIT_GROUP"] ?? "Familie" }
    private var oneToOne: String { env["TALK_EDIT_ONE"] ?? "Lena Hoffmann" }
    private var other: String { env["TALK_EDIT_OTHER"] ?? "Ich habe dir das Rezept aufgesprochen, das Tippen dauert bei mir so lange 😊" }
    private var old: String { env["TALK_EDIT_OLD"] ?? "Fand ich auch! Oma, das Rezept musst du mir unbedingt geben 😋" }
    private var oneToOneOther: String { env["TALK_EDIT_ONE_OTHER"] ?? "Das war so schön heute früh. Nächste Woche wieder? ☀️" }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private var composer: XCUIElement { app.descendants(matching: .any)["composer"].firstMatch }

    private func open(_ room: String) {
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "\(room) not listed")
        row.tap()
        XCTAssertTrue(composer.waitForExistence(timeout: 10))
    }

    private func menu(_ text: String) {
        menu(on: app.staticTexts[text].firstMatch, text)
    }

    private func menu(on bubble: XCUIElement, _ text: String) {
        XCTAssertTrue(bubble.waitForExistence(timeout: 20), "\(text) not shown")
        bubble.press(forDuration: 0.8)
        XCTAssertTrue(app.buttons["message-menu-dim"].waitForExistence(timeout: 5)
            || app.descendants(matching: .any)["message-menu-dim"].waitForExistence(timeout: 1), "no menu")
        sleep(1)
    }

    private func closeMenu() {
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2)).tap()
        gone(app.descendants(matching: .any)["message-menu-dim"].firstMatch, "menu closed")
        sleep(1)
    }

    private func gone(_ e: XCUIElement, _ what: String) {
        let x = expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: e)
        x.expectationDescription = what
        wait(for: [x], timeout: 10)
    }

    /// Deletes the field's text (key events get lost now and then, so until it is empty).
    private func clear() {
        for _ in 0..<5 {
            let v = composer.value as? String ?? ""
            if v.isEmpty || v == "Nachricht" { return }
            composer.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: v.count))
            sleep(1)
        }
    }

    private func send(_ text: String) {
        composer.tap()
        composer.typeText(text)
        app.buttons["send"].tap()
        XCTAssertTrue(app.staticTexts[text].firstMatch.waitForExistence(timeout: 20))
        sleep(2)
    }

    func testEditAndDelete() {
        open(group)
        let stamp = Int(Date().timeIntervalSince1970) % 100000
        let first = "Bis morgen um zehn \(stamp)"
        send(first)

        // Menu of an own message: Antworten, Bearbeiten, Kopieren, Löschen (last, red).
        menu(first)
        let ids = app.buttons.allElementsBoundByIndex.map(\.identifier).filter { $0.hasPrefix("menu:") }
        XCTAssertEqual(ids, ["menu:reply", "menu:edit", "menu:copy", "menu:delete"])
        shot("menu-own")

        // A draft first; editing puts the message's text in the field, the check mark is off.
        closeMenu()
        composer.tap()
        composer.typeText("Entwurf")
        menu(first)
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["edit-bar"].waitForExistence(timeout: 5), "no edit bar")
        XCTAssertEqual(composer.value as? String, first)
        let save = app.buttons["save-edit"]
        XCTAssertFalse(save.isEnabled, "unchanged text can be saved")
        XCTAssertFalse(app.buttons["attach"].exists)
        XCTAssertFalse(app.buttons["camera"].exists)
        shot("edit-bar")

        // Empty: off as well.
        clear()
        XCTAssertFalse(save.isEnabled, "empty text can be saved")
        let edited = "Bis morgen um elf \(stamp)"
        composer.typeText(edited)
        XCTAssertTrue(save.isEnabled)
        save.tap()
        gone(app.descendants(matching: .any)["edit-bar"].firstMatch, "edit mode ends")
        XCTAssertTrue(app.staticTexts[edited].firstMatch.waitForExistence(timeout: 10), "new text not shown")
        XCTAssertEqual(composer.value as? String, "Entwurf", "draft not back")
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'bearbeitet'")).firstMatch.exists, "no 'bearbeitet'")
        shot("edited")

        // ✕ cancels and the draft comes back too.
        menu(edited)
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(app.buttons["edit-cancel"].waitForExistence(timeout: 5))
        app.buttons["edit-cancel"].tap()
        XCTAssertEqual(composer.value as? String, "Entwurf")

        // The back button and the swipe from the edge first end editing; the chat stays open.
        let bar = app.descendants(matching: .any)["edit-bar"].firstMatch
        menu(edited)
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(app.buttons["edit-back"].waitForExistence(timeout: 5), "no back button that cancels")
        app.buttons["edit-back"].tap()
        gone(bar, "back ends editing")
        XCTAssertTrue(composer.exists, "chat left")
        menu(edited)
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(bar.waitForExistence(timeout: 5))
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.005, dy: 0.45))
            .press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.45)))
        gone(bar, "swipe ends editing")
        XCTAssertTrue(composer.exists, "chat left")
        XCTAssertEqual(composer.value as? String, "Entwurf")
        composer.tap()
        clear()

        // Delete: the question, then the bubble says so.
        menu(edited)
        app.buttons["menu:delete"].tap()
        XCTAssertTrue(app.staticTexts["Sie wird für alle im Chat gelöscht."].waitForExistence(timeout: 5), "no question")
        shot("delete-own")
        app.buttons.matching(NSPredicate(format: "label == 'Löschen'")).firstMatch.tap()
        XCTAssertTrue(app.staticTexts["Diese Nachricht wurde gelöscht."].firstMatch.waitForExistence(timeout: 10))
        shot("deleted")

        // Somebody else's message in the own group: only "Löschen" (red), with the name.
        menu(other)
        XCTAssertFalse(app.buttons["menu:edit"].exists)
        XCTAssertTrue(app.buttons["menu:delete"].exists)
        app.buttons["menu:delete"].tap()
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH 'Die Nachricht von '")).firstMatch
            .waitForExistence(timeout: 5), "no question with the name")
        shot("delete-other")
        // iOS 26 shows the question as a popover: "Abbrechen" is a tap beside it.
        let cancel = app.buttons.matching(NSPredicate(format: "label == 'Abbrechen'")).firstMatch
        if cancel.exists { cancel.tap() } else { app.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.6)).tap() }
        sleep(1)

        // Own message older than 24 hours: "Löschen" without "Bearbeiten".
        if !old.isEmpty {
            menu(old)
            XCTAssertFalse(app.buttons["menu:edit"].exists, "edit offered after 24 hours")
            XCTAssertTrue(app.buttons["menu:delete"].exists)
            closeMenu()
        }

        // 1:1: no "Löschen" for the other's messages.
        app.navigationBars.buttons.element(boundBy: 0).tap()
        open(oneToOne)
        menu(oneToOneOther)
        XCTAssertFalse(app.buttons["menu:delete"].exists, "delete offered for the other's message in a 1:1")
        shot("menu-other-1to1")
        closeMenu()
    }

    /// Against the real server (`TALK_EDIT_REAL` = the run's number): the script has put these
    /// into the group beforehand – "Edit-Quelle N" from the other side, "Hallo @… N" with a
    /// mention and a photo without caption (both ours), in the 1:1 an own voice message – and
    /// edits "Edit-Quelle N" as soon as "Antwort geändert N" arrives.
    func testRealServer() throws {
        guard let n = env["TALK_EDIT_REAL"] else { throw XCTSkip("only against the real server") }
        open(group)

        // 4. Own reply with own reaction, then edited: quote and reaction stay.
        menu("Edit-Quelle \(n)")
        app.buttons["menu:reply"].tap()
        send("Antwort \(n)")
        menu("Antwort \(n)")
        app.buttons["react:👍"].tap()
        sleep(3)
        menu("Antwort \(n)")
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["edit-bar"].waitForExistence(timeout: 5))
        clear()
        composer.typeText("Antwort geändert \(n)")
        app.buttons["save-edit"].tap()
        let reply = app.cells.containing(NSPredicate(format: "label == %@", "Antwort geändert \(n)")).firstMatch
        XCTAssertTrue(reply.waitForExistence(timeout: 15), "edited reply not shown")
        XCTAssertTrue(reply.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "Edit-Quelle \(n)")).firstMatch.exists,
                      "quote lost")
        XCTAssertTrue(reply.buttons.matching(NSPredicate(format: "label BEGINSWITH '👍 1'")).firstMatch.exists, "own reaction lost")
        shot("reply-edited")

        // 5. The other side edits the quoted message: the quote follows.
        let quote = reply.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", "Edit-Quelle geändert \(n)")).firstMatch
        XCTAssertTrue(quote.waitForExistence(timeout: 90), "quote not updated")
        shot("quote-updated")

        // 6. Mention: the field holds the server's notation, and after saving it still does.
        let mention = app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH 'Hallo' AND label ENDSWITH %@", " \(n)")).firstMatch
        menu(on: mention, "mention")
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["edit-bar"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5), "keyboard did not open with the mode")
        XCTAssertTrue((composer.value as? String ?? "").contains("@\""), "mention not in the server's notation")
        composer.typeText(" ok")
        app.buttons["save-edit"].tap()
        gone(app.descendants(matching: .any)["edit-bar"].firstMatch, "edit mode ends")
        let mentionEdited = app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH 'Hallo' AND label ENDSWITH %@", " \(n) ok")).firstMatch
        menu(on: mentionEdited, "mention edited")
        app.buttons["menu:edit"].tap()
        XCTAssertTrue((composer.value as? String ?? "").contains("@\""), "mention lost after saving")
        app.buttons["edit-cancel"].tap()

        // 10. Photo: "Bearbeiten" sets the caption (voice message: in the 1:1 below).
        let photo = app.descendants(matching: .any).matching(identifier: "chat-photo").allElementsBoundByIndex.last
        XCTAssertNotNil(photo, "no photo")
        menu(on: photo!, "photo")
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["edit-bar"].waitForExistence(timeout: 5))
        shot("edit-photo")
        composer.typeText("Bildunterschrift \(n)")
        app.buttons["save-edit"].tap()
        XCTAssertTrue(app.staticTexts["Bildunterschrift \(n)"].firstMatch.waitForExistence(timeout: 15), "caption not shown")

        // 7. Deleting the newest message: the chat list says "Nachricht gelöscht".
        send("Zum Löschen \(n)")
        menu("Zum Löschen \(n)")
        app.buttons["menu:delete"].tap()
        sleep(1)
        app.buttons.matching(NSPredicate(format: "label == 'Löschen'")).firstMatch.tap()
        XCTAssertTrue(app.staticTexts["Diese Nachricht wurde gelöscht."].firstMatch.waitForExistence(timeout: 15))
        app.navigationBars.buttons.element(boundBy: 0).tap()
        let row = app.buttons["room:\(group)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        let preview = expectation(for: NSPredicate(format: "label CONTAINS 'Nachricht gelöscht'"), evaluatedWith: row)
        wait(for: [preview], timeout: 15)
        shot("list-deleted")

        open(oneToOne)
        // 10. Voice message (ours, in the 1:1): only "Löschen".
        let voice = app.buttons.matching(NSPredicate(format: "label == 'Abspielen'")).allElementsBoundByIndex.last
        XCTAssertNotNil(voice, "no voice message")
        // Pressed on the waveform: the play button takes its own touches.
        voice!.coordinate(withNormalizedOffset: CGVector(dx: 4, dy: 0.5)).press(forDuration: 0.8)
        XCTAssertTrue(app.descendants(matching: .any)["message-menu-dim"].waitForExistence(timeout: 5), "no menu")
        sleep(1)
        shot("menu-voice")
        XCTAssertFalse(app.buttons["menu:edit"].exists, "edit offered for a voice message")
        XCTAssertTrue(app.buttons["menu:delete"].exists)
        closeMenu()
    }

    /// 11. Without a connection (`TALK_EDIT_OFFLINE`, demo server): the script stops the server
    /// while the edit is open. Saving then shows the notice; mode and text stay.
    func testOffline() throws {
        guard env["TALK_EDIT_OFFLINE"] != nil else { throw XCTSkip("needs the script that stops the server") }
        open(group)
        send("Offline-Test")
        menu("Offline-Test")
        app.buttons["menu:edit"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["edit-bar"].waitForExistence(timeout: 5))
        clear()
        composer.typeText("Offline geändert")
        // The script stops the server 60 s after the start; offline follows within seconds.
        sleep(50)
        shot("offline-header")
        app.buttons["save-edit"].tap()
        let toast = app.staticTexts["Keine Verbindung – Nachricht nicht geändert"].firstMatch
        XCTAssertTrue(toast.waitForExistence(timeout: 15), "no notice")
        shot("offline")
        XCTAssertTrue(app.descendants(matching: .any)["edit-bar"].exists, "edit mode ended")
        XCTAssertEqual(composer.value as? String, "Offline geändert")
    }
}
