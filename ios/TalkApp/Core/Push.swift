import Foundation
import Security
import UIKit
import UserNotifications

/// Push registration at the server and the push proxy (docs/calls.md, push-proxy/).
///
/// iOS registers one token `"<APNs hex> <PushKit hex>"`: calls come as VoIP pushes
/// (PushKit, see `VoipPush`), deletions as background pushes. The server encrypts every
/// push with this device's key; the key never leaves the Keychain and is readable after the
/// first unlock, so a call can be decrypted while the phone is locked.
@MainActor
final class Push {
    static let shared = Push()

    /// From the build config (`TALK_PUSH_PROXY_URL` when generating the project); empty: no push.
    static let proxyURL: String = {
        let v = Bundle.main.object(forInfoDictionaryKey: "TalkPushProxyURL") as? String ?? ""
        return v.hasPrefix("https://") ? v : ""
    }()

    private static let apnsKey = "push.apns"
    private static let voipKey = "push.voip"
    private static let registeredKey = "push.registered"

    private var registering = false
    private var again = false

    /// Called on every session start and whenever a token changes.
    func update() {
        if registering { again = true; return }
        guard !Self.proxyURL.isEmpty,
              let session = SessionStore.shared.session, SessionStore.shared.phase == .ready,
              let apns = UserDefaults.standard.string(forKey: Self.apnsKey),
              let voip = UserDefaults.standard.string(forKey: Self.voipKey),
              let key = PushKey.loadOrCreate()
        else { return }
        registering = true
        Task {
            defer {
                self.registering = false
                if self.again { self.again = false; self.update() }
            }
            do {
                let r = try await session.registerPush(pushToken: "\(apns) \(voip)", proxyUrl: Self.proxyURL, deviceKeyPem: key)
                UserDefaults.standard.set(true, forKey: Self.registeredKey)
                Log.push.info("registered at \(r.proxy, privacy: .public)")
                #if DEBUG
                LogDump.write()
                #endif
            } catch {
                // Normal passwords cannot register (only app passwords from the login flow).
                Log.push.error("register: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    func apnsToken(_ data: Data) {
        set(Self.apnsKey, data.map { String(format: "%02x", $0) }.joined())
    }

    func voipToken(_ hex: String?) {
        set(Self.voipKey, hex)
    }

    private func set(_ key: String, _ value: String?) {
        guard UserDefaults.standard.string(forKey: key) != value else { return }
        UserDefaults.standard.set(value, forKey: key)
        update()
    }

    /// Logout: the server stops pushing to this device, the key is thrown away.
    func unregister(_ session: TalkSession) async {
        if UserDefaults.standard.bool(forKey: Self.registeredKey) {
            do { try await session.unregisterPush() } catch {
                Log.push.error("unregister: \(error.localizedDescription, privacy: .public)")
            }
        }
        UserDefaults.standard.removeObject(forKey: Self.registeredKey)
        PushKey.delete()
    }

    /// Background push (deletions: a call that ended rings no longer).
    nonisolated static func handleBackground(_ info: [AnyHashable: Any]) async {
        guard let subject = info["subject"] as? String, let key = PushKey.load(),
              let msg = try? decryptPush(deviceKeyPem: key, subject: subject) else {
            Log.push.info("background push unreadable")
            return
        }
        Log.push.info("background push: \(String(describing: msg).prefix(40), privacy: .public)")
        defer {
            #if DEBUG
            LogDump.write()
            #endif
        }
        await removeDelivered(msg)
        guard let session = await SessionStore.shared.session else { return }
        _ = await session.handlePush(message: msg, callId: nil)
    }

    /// Read or deleted on the server: the extension's chat notifications (they carry `nid`) go.
    nonisolated private static func removeDelivered(_ msg: PushMessage) async {
        let ids: Set<Int64>?
        switch msg {
        case .delete(let n): ids = Set(n)
        case .deleteAll: ids = nil
        default: return
        }
        let center = UNUserNotificationCenter.current()
        let gone = await center.deliveredNotifications().filter { n in
            guard let nid = (n.request.content.userInfo["nid"] as? NSNumber)?.int64Value else { return false }
            return ids?.contains(nid) ?? true
        }
        center.removeDeliveredNotifications(withIdentifiers: gone.map(\.request.identifier))
    }
}
