import SwiftUI
import UIKit

/// Design tokens shared with Android (`ui/Theme.kt`) and desktop (`styles.css`).
enum Talk {
    /// The chosen accent colour (docs/settings.md "Darstellung"); reading it in a view body
    /// makes the view follow changes.
    @MainActor static var accent: Color {
        let a = Appearance.shared.accent
        return dynamic(light: a.light.accent, dark: a.dark.accent)
    }
    /// Own bubble: the same in both modes (white text stays readable).
    @MainActor static var bubbleOwn: Color { Color(hex: Appearance.shared.accent.light.bubble) }
    static let bubbleOwnText = Color.white
    static let bubbleOther = dynamic(light: 0xE9E9EB, dark: 0x2C2C2E)
    static let bubbleOtherText = dynamic(light: 0x1B1B1D, dark: 0xE9E9EB)
    static let muted = dynamic(light: 0x6B6B70, dark: 0x9A9AA0)
    static let chip = dynamic(light: 0xF1F1F3, dark: 0x232325)
    static let danger = dynamic(light: 0xD6342C, dark: 0xFF6B63)
    static let background = dynamic(light: 0xFFFFFF, dark: 0x121212)
    static let surfaceVariant = dynamic(light: 0xE9E9EB, dark: 0x2C2C2E)
    static let surface = dynamic(light: 0xFFFFFF, dark: 0x1B1B1C)
    static let onSurface = dynamic(light: 0x1B1B1D, dark: 0xE9E9EB)
    /// Floating controls above content (desktop `--floating`).
    static let floating = dynamic(light: 0xFFFFFF, dark: 0x2E2E2E)
    /// Settings pages (docs/groups.md): grouped sections on a page like the system settings,
    /// built from the tokens above (light: chip page, surface rows; dark: background page,
    /// chip rows).
    static let groupedBackground = dynamic(light: 0xF1F1F3, dark: 0x121212)
    static let groupedRow = dynamic(light: 0xFFFFFF, dark: 0x232325)
    static let callDot = Color(hex: 0x2FB35A)
    static let offlineText = Color(hex: 0x6A5000)
    static let offlineBackground = Color(hex: 0xFFF3CD)

    static let uiBackground = UIColor { $0.userInterfaceStyle == .dark ? UIColor(hex: 0x121212) : .white }

    private static func dynamic(light: UInt32, dark: UInt32) -> Color {
        Color(UIColor { $0.userInterfaceStyle == .dark ? UIColor(hex: dark) : UIColor(hex: light) })
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(UIColor(hex: hex))
    }
}

extension UIColor {
    convenience init(hex: UInt32) {
        self.init(
            red: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: 1
        )
    }
}

/// "Keine Verbindung" strip above lists.
struct OfflineBanner: View {
    var body: some View {
        Text("Keine Verbindung – wird erneut versucht …")
            .font(.system(size: 13))
            .foregroundStyle(Talk.offlineText)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(10)
            .background(Talk.offlineBackground, in: RoundedRectangle(cornerRadius: 10))
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
    }
}

enum Haptics {
    static func tap() {
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    }

    static func light() {
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }
}
