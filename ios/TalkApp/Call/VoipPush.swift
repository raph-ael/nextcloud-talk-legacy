import Foundation
import PushKit

/// VoIP pushes (PushKit) for incoming calls, also when the app is not running.
///
/// iOS terminates apps that receive a VoIP push without reporting a call to CallKit, so
/// every push is reported at once – also one that cannot be decrypted (it ends at once).
/// Order (docs/calls.md): `decryptPush` → `reportNewIncomingCall` → `handlePush` with the
/// same UUID, so the core's `incoming` is this CallKit call.
@MainActor
final class VoipPush: NSObject, PKPushRegistryDelegate {
    static let shared = VoipPush()

    private var registry: PKPushRegistry?

    /// In `didFinishLaunching`: a cold start from a push delivers it right after this.
    func register() {
        guard registry == nil else { return }
        let r = PKPushRegistry(queue: .main)
        r.delegate = self
        r.desiredPushTypes = [.voIP]
        registry = r
    }

    nonisolated func pushRegistry(_ registry: PKPushRegistry, didUpdate credentials: PKPushCredentials, for type: PKPushType) {
        let hex = credentials.token.map { String(format: "%02x", $0) }.joined()
        MainActor.assumeIsolated {
            Log.push.info("voip token updated")
            Push.shared.voipToken(hex)
        }
    }

    nonisolated func pushRegistry(_ registry: PKPushRegistry, didInvalidatePushTokenFor type: PKPushType) {
        MainActor.assumeIsolated { Push.shared.voipToken(nil) }
    }

    nonisolated func pushRegistry(
        _ registry: PKPushRegistry, didReceiveIncomingPushWith payload: PKPushPayload,
        for type: PKPushType, completion: @escaping () -> Void
    ) {
        let subject = payload.dictionaryPayload["subject"] as? String
        // Synchronous and without network: works before any session exists.
        let message: PushMessage? = subject.flatMap { s in
            PushKey.load().flatMap { try? decryptPush(deviceKeyPem: $0, subject: s) }
        }
        MainActor.assumeIsolated {
            Log.push.info("voip push: \(Self.describe(message), privacy: .public)")
            CallController.shared.reportPushed(message, completion: completion)
        }
    }

    private static func describe(_ m: PushMessage?) -> String {
        switch m {
        case .none: "unreadable"
        case .call: "call"
        case .chat: "chat"
        case .delete: "delete"
        case .deleteAll: "deleteAll"
        case .other(let app, _): "other \(app)"
        }
    }
}
