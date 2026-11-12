import XCTest

/// Video (docs/calls.md "Video") against the real server, in the bots' 1:1 (Bot A here).
/// Bot B answers with its fake camera (`talk-cli -a b call <token> --answer --video
/// --video-after 5 --video-off-after 50 --expect-video`, started by the test script) and
/// checks that our video arrives. In the simulator our camera is `FakeVideoCapturer`.
/// Environment as in `ChatFlowTests`; `TALK_BOT_ROOM` names the 1:1 in Bot A's list.
final class VideoCallTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        for key in ["TALK_DEV_SERVER", "TALK_DEV_USER", "TALK_DEV_PASS", "TALK_FAKE_MIC"] {
            if let v = env[key] { app.launchEnvironment[key] = v }
        }
        app.launch()
    }

    override func tearDown() {
        let hangup = app.buttons["call-hangup"].firstMatch
        if hangup.exists && hangup.isHittable { hangup.tap() }
    }

    private var room: String { env["TALK_BOT_ROOM"] ?? "Talk Bot B" }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func wait(_ element: XCUIElement, _ format: String, timeout: TimeInterval, _ message: String) {
        let e = expectation(for: NSPredicate(format: format), evaluatedWith: element)
        e.expectationDescription = message
        wait(for: [e], timeout: timeout)
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }

    /// Calls the bot and waits until the call is connected (timer instead of a status text).
    private func callBot() {
        let row = app.buttons["room:\(room)"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 30), "bot room not listed")
        row.tap()
        let call = app.buttons["call-button"].firstMatch
        XCTAssertTrue(call.waitForExistence(timeout: 10))
        call.tap()
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let allow = springboard.buttons.matching(NSPredicate(
            format: "label IN {'Erlauben', 'OK', 'Allow', 'Beim Verwenden der App erlauben'}"
        )).firstMatch
        if allow.waitForExistence(timeout: 3) { allow.tap() }
        let status = app.staticTexts["call-status"].firstMatch
        XCTAssertTrue(status.waitForExistence(timeout: 10), "call screen did not open")
        wait(status, "label MATCHES '[0-9]+:[0-9]{2}'", timeout: 40, "call connected")
    }

    func testCameraBothWays() {
        callBot()

        // Every call starts without camera.
        let camera = app.buttons["call-camera"].firstMatch
        XCTAssertEqual(camera.label, "Kamera einschalten")
        XCTAssertFalse(element("call-local-preview").exists)

        // 1. Bot B's camera comes on: its video fills the stage.
        let remote = element("call-remote-video")
        XCTAssertTrue(remote.waitForExistence(timeout: 30), "no remote video")
        shot("remote-video")

        // 2. Our camera: preview (mirrored) and the button turns white.
        camera.tap()
        XCTAssertTrue(element("call-local-preview").waitForExistence(timeout: 10), "no own preview")
        XCTAssertEqual(camera.label, "Kamera ausschalten")
        sleep(6)
        shot("both-video")

        // 3. Back camera and front again.
        app.buttons["call-switch-camera"].firstMatch.tap()
        sleep(3)
        shot("back-camera")
        app.buttons["call-switch-camera"].firstMatch.tap()
        sleep(2)

        // 4. Minimised: the camera pauses; back in the call screen it runs again.
        app.buttons["call-minimize"].firstMatch.tap()
        let bar = app.buttons["call-bar"].firstMatch
        XCTAssertTrue(bar.waitForExistence(timeout: 5))
        sleep(5)
        bar.tap()
        XCTAssertTrue(element("call-local-preview").waitForExistence(timeout: 10), "preview not back")
        XCTAssertEqual(camera.label, "Kamera ausschalten")
        sleep(5)

        // 5. Camera off: preview gone.
        camera.tap()
        wait(element("call-local-preview"), "exists == false", timeout: 5, "preview gone")
        XCTAssertEqual(camera.label, "Kamera einschalten")

        // 6. Bot B switches its camera off: the avatar returns.
        wait(remote, "exists == false", timeout: 40, "remote video gone")
        shot("remote-off")

        app.buttons["call-hangup"].firstMatch.tap()
        wait(app.buttons["call-hangup"].firstMatch, "exists == false", timeout: 10, "call closed")
    }

    /// Bot B forces a full rejoin (`--rejoin-after 25`): both videos run on afterwards.
    func testVideoAfterRejoin() {
        callBot()
        let remote = element("call-remote-video")
        XCTAssertTrue(remote.waitForExistence(timeout: 30), "no remote video")
        app.buttons["call-camera"].firstMatch.tap()
        XCTAssertTrue(element("call-local-preview").waitForExistence(timeout: 10), "no own preview")
        shot("before-rejoin")
        // The rejoin replaces Bot B's connection; its video may drop for a moment.
        sleep(40)
        XCTAssertTrue(remote.waitForExistence(timeout: 20), "remote video not back after the rejoin")
        XCTAssertTrue(element("call-local-preview").exists, "own camera lost")
        shot("after-rejoin")
        app.buttons["call-hangup"].firstMatch.tap()
        wait(app.buttons["call-hangup"].firstMatch, "exists == false", timeout: 10, "call closed")
    }
}
