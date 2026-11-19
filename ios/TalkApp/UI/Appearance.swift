import SwiftUI
import UIKit

/// "Darstellung" (docs/settings.md): design, accent colour and "Gruppen und Direktchats
/// trennen", per device in UserDefaults (`appearance.theme`, `appearance.accent`,
/// `chatlist.split`), kept on logout. Every change applies at once:
/// `Talk.accent`/`Talk.bubbleOwn` read it, the root sets `.tint`/`.preferredColorScheme`,
/// and the app's windows get `overrideUserInterfaceStyle` and `tintColor`, so sheets, alerts
/// and the extra windows (message menu, toasts) follow too. The call window stays dark.
@MainActor
@Observable
final class Appearance {
    static let shared = Appearance()

    enum Theme: String, CaseIterable, Identifiable {
        case system, light, dark

        var id: String { rawValue }

        var label: String {
            switch self {
            case .system: "System"
            case .light: "Hell"
            case .dark: "Dunkel"
            }
        }
    }

    private static let themeKey = "appearance.theme"
    private static let accentKey = "appearance.accent"
    private static let splitKey = "chatlist.split"

    var theme: Theme {
        didSet {
            UserDefaults.standard.set(theme.rawValue, forKey: Self.themeKey)
            applyToWindows()
        }
    }

    /// Always a known id (`Accent.byID` falls back to blue).
    var accentID: String {
        didSet {
            UserDefaults.standard.set(accentID, forKey: Self.accentKey)
            applyToWindows()
        }
    }

    /// "Gruppen" and "Direktchats" instead of "Chats" in the list (docs/categories.md); on by
    /// default. The list reads it in `SessionStore.sections`, so it redraws at once.
    var splitChats: Bool {
        didSet { UserDefaults.standard.set(splitChats, forKey: Self.splitKey) }
    }

    private init() {
        let d = UserDefaults.standard
        theme = Theme(rawValue: d.string(forKey: Self.themeKey) ?? "") ?? .system
        accentID = Accent.byID(d.string(forKey: Self.accentKey)).id
        splitChats = d.object(forKey: Self.splitKey) as? Bool ?? true
    }

    var accent: Accent { Accent.byID(accentID) }

    var colorScheme: ColorScheme? {
        switch theme {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }

    var interfaceStyle: UIUserInterfaceStyle {
        switch theme {
        case .system: .unspecified
        case .light: .light
        case .dark: .dark
        }
    }

    /// Accent of the current mode (resolved per trait collection).
    var accentUIColor: UIColor {
        let a = accent
        return UIColor { $0.userInterfaceStyle == .dark ? UIColor(hex: a.dark.accent) : UIColor(hex: a.light.accent) }
    }

    /// Design and tint for one of our windows (also new ones: message menu, toasts).
    func style(_ window: UIWindow) {
        window.overrideUserInterfaceStyle = interfaceStyle
        window.tintColor = accentUIColor
    }

    /// All windows of the app but the call window, which keeps its dark style.
    func applyToWindows() {
        for scene in UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }) {
            for w in scene.windows where !(w is CallUIWindow) { style(w) }
        }
    }
}

/// The call screen's window: always dark, left alone by `Appearance`.
final class CallUIWindow: UIWindow {}
