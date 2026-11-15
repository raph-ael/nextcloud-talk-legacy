import Foundation
import Security
import UIKit
import UserNotifications

/// Push registration at the server and the push proxy (docs/calls.md, push-proxy/).
///
/// iOS registers one token `"<APNs hex> <PushKit hex>"`: calls come as VoIP pushes
/// (PushKit, see `VoipPush`), deletions as background pushes. Every account registers it
/// with its own device key (docs/accounts.md "Push"), and the server encrypts every push
/// with it; the keys never leave the Keychain and are readable after the first unlock, so
/// a call can be decrypted while the phone is locked.
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
    /// Per account: `push.registered.<id>` (the earlier version: `push.registered`).
    private static func registeredKey(_ id: String) -> String { "push.registered.\(id)" }
    private static let legacyRegisteredKey = "push.registered"

    /// Takeover of the earlier version's account: its registration stays valid.
    static func adoptLegacyRegistration(for id: String) {
        let d = UserDefaults.standard
        if d.bool(forKey: legacyRegisteredKey) { d.set(true, forKey: registeredKey(id)) }
        d.removeObject(forKey: legacyRegisteredKey)
    }

    /// A token changed: every account registers it again (docs/accounts.md "Push").
    func update() {
        for rt in SessionStore.shared.allRuntimes { update(rt) }
    }

    /// Called on every start of an account and whenever a token changes. Each account
    /// registers the same token with its own key.
    func update(_ rt: AccountRuntime) {
        if rt.registering { rt.registerAgain = true; return }
        let id = rt.account.id
        guard !Self.proxyURL.isEmpty,
              let apns = UserDefaults.standard.string(forKey: Self.apnsKey),
              let voip = UserDefaults.standard.string(forKey: Self.voipKey),
              let key = DeviceKeys.loadOrCreate(for: id, line: rt.account.line)
        else { return }
        rt.registering = true
        let session = rt.session
        Task {
            defer {
                rt.registering = false
                if rt.registerAgain { rt.registerAgain = false; self.update(rt) }
            }
            do {
                let r = try await session.registerPush(pushToken: "\(apns) \(voip)", proxyUrl: Self.proxyURL, deviceKeyPem: key)
                UserDefaults.standard.set(true, forKey: Self.registeredKey(id))
                Log.push.info("registered \(id, privacy: .public) at \(r.proxy, privacy: .public)")
                #if DEBUG
                LogDump.write()
                #endif
            } catch {
                // Normal passwords cannot register (only app passwords from the login flow).
                Log.push.error("register \(id, privacy: .public): \(error.localizedDescription, privacy: .public)")
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

    /// Logout of one account: the server stops pushing to it, its key is thrown away.
    func unregister(_ rt: AccountRuntime) async {
        let id = rt.account.id
        if UserDefaults.standard.bool(forKey: Self.registeredKey(id)) {
            do { try await rt.session.unregisterPush() } catch {
                Log.push.error("unregister \(id, privacy: .public): \(error.localizedDescription, privacy: .public)")
            }
        }
        forget(rt)
    }

    /// The account is gone (also after `authFailed`, without asking the server).
    func forget(_ rt: AccountRuntime) {
        UserDefaults.standard.removeObject(forKey: Self.registeredKey(rt.account.id))
        DeviceKeys.delete(for: rt.account.id)
    }

    /// Background push (deletions: a call that ended rings no longer). The key that
    /// decrypts names the account.
    nonisolated static func handleBackground(_ info: [AnyHashable: Any]) async {
        guard let subject = info["subject"] as? String,
              let found = decryptPushFor(keys: DeviceKeys.decryptKeys(), subject: subject) else {
            Log.push.info("background push unreadable")
            return
        }
        let msg = found.message
        Log.push.info("background push: \(String(describing: msg).prefix(40), privacy: .public)")
        defer {
            #if DEBUG
            LogDump.write()
            #endif
        }
        let (account, legacy, session) = await MainActor.run { () -> (String?, Bool, TalkSession?) in
            let rt = SessionStore.shared.runtime(found.accountId)
            return (rt?.account.id, rt?.account.dir == SessionStore.legacyDir, rt?.session)
        }
        guard let account, let session else { return }
        await removeDelivered(msg, account: account, legacy: legacy)
        _ = await session.handlePush(message: msg, callId: nil)
    }

    /// Read or deleted on the server: the extension's chat notifications (they carry `nid`)
    /// of this account go. Notification ids of different servers overlap.
    nonisolated private static func removeDelivered(_ msg: PushMessage, account: String, legacy: Bool) async {
        let ids: Set<Int64>?
        switch msg {
        case .delete(let n): ids = Set(n)
        case .deleteAll: ids = nil
        default: return
        }
        let center = UNUserNotificationCenter.current()
        let gone = await center.deliveredNotifications().filter { n in
            let info = n.request.content.userInfo
            let owner = info["account"] as? String
            guard owner == account || (legacy && (owner == nil || owner == DeviceKeys.legacyId)),
                  let nid = (info["nid"] as? NSNumber)?.int64Value else { return false }
            return ids?.contains(nid) ?? true
        }
        center.removeDeliveredNotifications(withIdentifiers: gone.map(\.request.identifier))
    }
}
