import SwiftUI
import UIKit

/// One entry of the long-press menu.
struct MenuAction: Identifiable {
    let id: String
    let label: String
    let icon: String
    var destructive = false
    let run: () -> Void
}

/// Window of the long-press menu, above the app (the navigation bar is dimmed too) and
/// below a call. It becomes key only for the emoji keyboard (＋).
@MainActor
final class MessageMenuWindow {
    static let shared = MessageMenuWindow()
    private var window: UIWindow?
    private weak var previousKey: UIWindow?

    var safeArea: UIEdgeInsets {
        activeScene?.keyWindow?.safeAreaInsets ?? .zero
    }

    private var activeScene: UIWindowScene? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        return scenes.first { $0.activationState == .foregroundActive } ?? scenes.first
    }

    func show(_ content: some View) {
        close()
        guard let scene = activeScene else { return }
        let w = UIWindow(windowScene: scene)
        w.windowLevel = UIWindow.Level(rawValue: UIWindow.Level.normal.rawValue + 0.5)
        let host = UIHostingController(rootView: content)
        host.view.backgroundColor = .clear
        w.rootViewController = host
        previousKey = scene.keyWindow
        w.isHidden = false
        window = w
    }

    func makeKey() { window?.makeKey() }

    func close() {
        guard let w = window else { return }
        window = nil
        w.isHidden = true
        previousKey?.makeKey()
    }
}

/// Long-press menu like Signal (docs/chat.md "Reaktionen", iOS): the screen is dimmed, the
/// message is drawn again where it is in the list, the quick reactions float above it and
/// the actions sit below. Bar, bubble and menu move together so nothing overlaps or leaves
/// the screen. ＋ swaps bar and menu for the emoji keyboard.
struct MessageMenuOverlay: View {
    let target: MessageMenuTarget
    let showBar: Bool
    let actions: [MenuAction]
    let safe: UIEdgeInsets
    let onReact: (String) -> Void
    /// Called once the overlay has faded out.
    let onClose: () -> Void

    @State private var shown = false
    @State private var picking = false
    @State private var closing = false
    @State private var bubbleHeight: CGFloat = 0
    @State private var menuHeight: CGFloat = 0
    @State private var keyboard: CGFloat = 0

    private let gap: CGFloat = 8
    private let barHeight: CGFloat = 52

    var body: some View {
        GeometryReader { geo in
            let own = target.m.msg.isOwn
            let layout = place(height: geo.size.height)
            ZStack(alignment: .topLeading) {
                Color.black.opacity(shown ? 0.4 : 0)
                    .contentShape(Rectangle())
                    .onTapGesture { dismiss() }
                    .accessibilityElement()
                    .accessibilityLabel("Menü schließen")
                    .accessibilityAddTraits(.isButton)
                    .accessibilityAction { dismiss() }
                    .accessibilityIdentifier("message-menu-dim")
                VStack(alignment: own ? .trailing : .leading, spacing: gap) {
                    if showBar && !picking {
                        ReactionBar(mine: target.m.msg.reactionsSelf, onReact: react, onMore: more)
                            .frame(height: barHeight)
                            .scaleEffect(shown ? 1 : 0.8, anchor: own ? .bottomTrailing : .bottomLeading)
                            .opacity(shown ? 1 : 0)
                    }
                    Bubble(
                        m: target.m, first: target.first, last: target.last, isGroup: target.isGroup,
                        read: target.read, highlighted: false, onRetry: { _ in }, onOpenImage: { _, _ in }
                    )
                    .fixedSize(horizontal: false, vertical: true)
                    .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { bubbleHeight = $0 }
                    .frame(height: layout.bubble > 0 ? layout.bubble : nil, alignment: .top)
                    .clipped()
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                    if !actions.isEmpty && !picking {
                        menu
                            .fixedSize()
                            .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { menuHeight = $0 }
                            .scaleEffect(shown ? 1 : 0.9, anchor: own ? .topTrailing : .topLeading)
                            .opacity(shown ? 1 : 0)
                    }
                }
                .padding(.horizontal, 12)
                .frame(width: geo.size.width, alignment: own ? .trailing : .leading)
                .offset(y: layout.top)
                .opacity(bubbleHeight > 0 && !closing ? 1 : 0)
                if picking {
                    EmojiCatcher(active: true, onPick: react, onCancel: { dismiss() })
                        .frame(width: 1, height: 1)
                        .opacity(0.02)
                }
            }
        }
        .ignoresSafeArea()
        .accessibilityAction(.escape) { dismiss() }
        .onAppear {
            withAnimation(.spring(duration: 0.25, bounce: 0.2)) { shown = true }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillChangeFrameNotification)) { n in
            guard let frame = n.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else { return }
            let screen = UIScreen.main.bounds.height
            withAnimation(.easeOut(duration: 0.25)) { keyboard = max(0, screen - frame.minY) }
        }
    }

    /// Top of the stack and the (maybe clipped) bubble height: the bubble stays where it is
    /// in the list unless bar or menu would leave the screen; a very tall bubble is cut.
    private func place(height: CGFloat) -> (top: CGFloat, bubble: CGFloat) {
        let top = safe.top + 8
        let bottom = height - max(safe.bottom, keyboard) - 8
        let bar = showBar && !picking ? barHeight + gap : 0
        let below = !actions.isEmpty && !picking ? menuHeight + gap : 0
        let room = max(80, bottom - top - bar - below)
        let bubble = min(bubbleHeight, room)
        let total = bar + bubble + below
        var y = target.frame.maxY - bubble - bar
        y = min(y, bottom - total)
        y = max(y, top)
        return (y, bubble < bubbleHeight ? bubble : 0)
    }

    private var menu: some View {
        VStack(spacing: 0) {
            ForEach(Array(actions.enumerated()), id: \.element.id) { i, a in
                if i > 0 { Divider() }
                Button {
                    dismiss(then: a.run)
                } label: {
                    HStack(spacing: 16) {
                        Text(a.label).font(.system(size: 17))
                        Spacer(minLength: 0)
                        Image(systemName: a.icon).font(.system(size: 17))
                    }
                    .foregroundStyle(a.destructive ? Talk.danger : Talk.onSurface)
                    .padding(.horizontal, 16)
                    .frame(width: 240, height: 46)
                    .contentShape(Rectangle())
                }
                .buttonStyle(MenuRowStyle())
                .accessibilityIdentifier("menu:\(a.id)")
            }
        }
        .background(Talk.floating, in: RoundedRectangle(cornerRadius: 14))
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .shadow(color: .black.opacity(0.18), radius: 12, y: 4)
    }

    private func react(_ emoji: String) {
        Haptics.light()
        onReact(emoji)
        dismiss()
    }

    private func more() {
        MessageMenuWindow.shared.makeKey()
        withAnimation(.easeOut(duration: 0.15)) { picking = true }
    }

    private func dismiss(then action: (() -> Void)? = nil) {
        guard !closing else { return }
        withAnimation(.easeOut(duration: 0.15)) {
            shown = false
            closing = true
        }
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(160))
            onClose()
            action?()
        }
    }
}

private struct MenuRowStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(configuration.isPressed ? Talk.muted.opacity(0.15) : Color.clear)
    }
}
