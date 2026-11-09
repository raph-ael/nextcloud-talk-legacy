import Foundation
import PushKit

/// VoIP pushes (PushKit) for incoming calls while the app is not running.
///
/// iOS terminates apps that receive a VoIP push without reporting a call to CallKit, so
/// every push is reported at once – also when it cannot be decrypted (then it ends at once).
/// Decryption and registration with the push proxy are done by the core (phase 3); until
/// then the token is only kept.
@MainActor
final class VoipPush: NSObject, PKPushRegistryDelegate {
    static let shared = VoipPush()
    nonisolated private static let tokenKey = "voip.token"

    private var registry: PKPushRegistry?

    /// The current PushKit token (hex), for the push proxy registration.
    static var token: String? { UserDefaults.standard.string(forKey: tokenKey) }

    func register() {
        guard registry == nil else { return }
        let r = PKPushRegistry(queue: .main)
        r.delegate = self
        r.desiredPushTypes = [.voIP]
        registry = r
    }

    nonisolated func pushRegistry(_ registry: PKPushRegistry, didUpdate credentials: PKPushCredentials, for type: PKPushType) {
        let hex = credentials.token.map { String(format: "%02x", $0) }.joined()
        UserDefaults.standard.set(hex, forKey: Self.tokenKey)
        Log.call.info("voip token updated")
    }

    nonisolated func pushRegistry(_ registry: PKPushRegistry, didInvalidatePushTokenFor type: PKPushType) {
        UserDefaults.standard.removeObject(forKey: Self.tokenKey)
    }

    nonisolated func pushRegistry(
        _ registry: PKPushRegistry, didReceiveIncomingPushWith payload: PKPushPayload,
        for type: PKPushType, completion: @escaping () -> Void
    ) {
        MainActor.assumeIsolated {
            // Phase 3: `session.incomingPush(subject)` decrypts and returns token + caller.
            // Until the core can read it, the call is reported and ended right away.
            Log.call.info("voip push received")
            CallController.shared.reportPushedIncoming(token: nil, name: "Talk-Anruf", completion: completion)
        }
    }
}
