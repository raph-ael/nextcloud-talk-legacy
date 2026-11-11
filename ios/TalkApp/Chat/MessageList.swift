import SwiftUI
import UIKit

/// Actions the list hands back to the chat screen.
struct MessageListActions {
    var onReply: (MessageView) -> Void
    var onLongPress: (UiMessage) -> Void
    var onRetry: (MessageView) -> Void
    var onOpenImage: (String, UIImage?) -> Void
    var onNearTop: () -> Void
    /// The newest message became visible (true) or was scrolled away from (false).
    var onBottomChanged: (Bool) -> Void
    /// The list moved more than half its height away from the newest message (true) or came
    /// back (false): shows the "Zum Ende springen" button. Independent of `onBottomChanged`.
    var onFarChanged: (Bool) -> Void = { _ in }
}

/// The message list: a vertically flipped `UICollectionView` (newest message at the bottom,
/// index 0) with SwiftUI cells. Flipped, older pages are appended at the end, so loading
/// them never moves what is on screen – the approach Signal iOS uses as well.
struct MessageList: UIViewRepresentable {
    let messages: [UiMessage]
    let isGroup: Bool
    let lastCommonRead: Int64
    let footer: Footer
    let highlighted: String?
    /// Changes whenever the chat wants to jump to the newest message (own send).
    let scrollToBottom: Int
    let actions: MessageListActions

    enum Footer: Equatable { case none, loading, start }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> UICollectionView {
        let c = context.coordinator
        let layout = UICollectionViewCompositionalLayout { _, _ in
            let size = NSCollectionLayoutSize(widthDimension: .fractionalWidth(1), heightDimension: .estimated(60))
            let group = NSCollectionLayoutGroup.vertical(layoutSize: size, subitems: [NSCollectionLayoutItem(layoutSize: size)])
            let section = NSCollectionLayoutSection(group: group)
            section.contentInsets = NSDirectionalEdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12)
            return section
        }
        let view = UICollectionView(frame: .zero, collectionViewLayout: layout)
        view.transform = CGAffineTransform(scaleX: 1, y: -1)
        view.backgroundColor = .clear
        // Flipped: interactive dismissal would track the wrong direction.
        view.keyboardDismissMode = .onDrag
        view.alwaysBounceVertical = true
        view.showsHorizontalScrollIndicator = false
        // SwiftUI already keeps the list out of the safe areas; insets would swap when flipped.
        view.contentInsetAdjustmentBehavior = .never
        view.delegate = c
        view.accessibilityIdentifier = "messages"
        c.attach(view)
        return view
    }

    func updateUIView(_ view: UICollectionView, context: Context) {
        context.coordinator.update(self)
    }

    @MainActor
    final class Coordinator: NSObject, UICollectionViewDelegate, UIGestureRecognizerDelegate {
        private enum Item: Hashable { case message(String), footer }

        private var source: UICollectionViewDiffableDataSource<Int, Item>!
        private weak var view: UICollectionView?
        private var parent: MessageList?
        /// Newest first (index 0 at the bottom).
        private var rows: [UiMessage] = []
        private var index: [String: Int] = [:]
        private var lastScrollRequest = 0
        private var atBottom = true
        private var farFromBottom = false
        private let replyIcon = UIImageView(image: UIImage(systemName: "arrowshape.turn.up.left.fill"))
        private var swipeCell: UICollectionViewCell?
        private var swipeMessage: MessageView?
        private var swipeTriggered = false
        private let threshold: CGFloat = 64

        func attach(_ view: UICollectionView) {
            self.view = view
            let cell = UICollectionView.CellRegistration<UICollectionViewCell, Item> { [weak self] cell, indexPath, item in
                self?.configure(cell, indexPath: indexPath, item: item)
            }
            source = UICollectionViewDiffableDataSource(collectionView: view) { cv, indexPath, item in
                cv.dequeueConfiguredReusableCell(using: cell, for: indexPath, item: item)
            }
            let long = UILongPressGestureRecognizer(target: self, action: #selector(longPress(_:)))
            long.minimumPressDuration = 0.4
            view.addGestureRecognizer(long)
            let pan = UIPanGestureRecognizer(target: self, action: #selector(pan(_:)))
            pan.delegate = self
            view.addGestureRecognizer(pan)
            // Vertical drags fail the swipe at once and scroll as usual.
            view.panGestureRecognizer.require(toFail: pan)
            replyIcon.tintColor = UIColor(Talk.muted)
            replyIcon.alpha = 0
            replyIcon.transform = CGAffineTransform(scaleX: 1, y: -1)
            view.addSubview(replyIcon)
        }

        private func configure(_ cell: UICollectionViewCell, indexPath: IndexPath, item: Item) {
            guard let parent else { return }
            // The list is flipped; each row flips back inside SwiftUI (a transform on the
            // content view would be lost when the hosting configuration replaces it).
            cell.contentView.transform = .identity
            switch item {
            case .footer:
                cell.contentConfiguration = UIHostingConfiguration {
                    ListFooter(footer: parent.footer).scaleEffect(x: 1, y: -1)
                }
                .margins(.all, 0)
            case .message(let key):
                guard let i = index[key] else { return }
                let m = rows[i]
                // rows: i+1 is older (previous), i-1 is newer (next)
                let prev = i + 1 < rows.count ? rows[i + 1] : nil
                let next = i > 0 ? rows[i - 1] : nil
                let read = m.msg.isOwn && m.status == nil && m.msg.id <= parent.lastCommonRead
                let actions = parent.actions
                cell.contentConfiguration = UIHostingConfiguration {
                    MessageRow(
                        m: m, prev: prev, next: next, isGroup: parent.isGroup, read: read,
                        highlighted: parent.highlighted == m.key,
                        onRetry: actions.onRetry, onOpenImage: actions.onOpenImage
                    )
                    .scaleEffect(x: 1, y: -1)
                }
                .margins(.all, 0)
                .minSize(height: 0)
            }
        }

        func update(_ new: MessageList) {
            let old = parent
            parent = new
            let newRows = Array(new.messages.reversed())
            let changed = old == nil || old!.messages != new.messages || old!.footer != new.footer
                || old!.lastCommonRead != new.lastCommonRead || old!.highlighted != new.highlighted
                || old!.isGroup != new.isGroup
            let newestBefore = rows.first?.key
            rows = newRows
            index = Dictionary(uniqueKeysWithValues: rows.enumerated().map { ($1.key, $0) })
            if changed {
                var snap = NSDiffableDataSourceSnapshot<Int, Item>()
                snap.appendSections([0])
                snap.appendItems(rows.map { .message($0.key) })
                if new.footer != .none { snap.appendItems([.footer]) }
                let existing = Set(source.snapshot().itemIdentifiers)
                // Neighbours decide grouping and day chips: refresh every surviving row.
                snap.reconfigureItems(snap.itemIdentifiers.filter { existing.contains($0) })
                let animate = old != nil && abs(old!.messages.count - new.messages.count) <= 3
                source.apply(snap, animatingDifferences: animate)
            }
            // Keep the newest message visible when sending / receiving while at the bottom.
            let newestNow = rows.first?.key
            if new.scrollToBottom != lastScrollRequest {
                lastScrollRequest = new.scrollToBottom
                scrollToNewest(animated: true)
            } else if newestNow != newestBefore, newestBefore != nil, atBottom {
                scrollToNewest(animated: true)
            }
        }

        /// Far away (more than ~15 rows), the list first jumps to about a screen before the end
        /// without animation, then scrolls the rest smoothly (docs/chat.md, "Zum Ende springen").
        private func scrollToNewest(animated: Bool) {
            guard let view, !rows.isEmpty else { return }
            let nearest = view.indexPathsForVisibleItems.map(\.item).min() ?? 0
            if animated, nearest > 15 || view.contentOffset.y > view.bounds.height * 4 {
                view.setContentOffset(CGPoint(x: 0, y: view.bounds.height), animated: false)
                view.layoutIfNeeded()
            }
            view.setContentOffset(.zero, animated: animated)
        }

        // Rows are not selectable: a highlight would re-render the hosted cell on every touch.
        func collectionView(_ collectionView: UICollectionView, shouldHighlightItemAt indexPath: IndexPath) -> Bool { false }

        func collectionView(_ collectionView: UICollectionView, shouldSelectItemAt indexPath: IndexPath) -> Bool { false }

        // MARK: Scrolling

        func scrollViewDidScroll(_ scrollView: UIScrollView) {
            let bottom = scrollView.contentOffset.y <= 40
            if bottom != atBottom { parent?.actions.onBottomChanged(bottom) }
            atBottom = bottom
            let far = scrollView.contentOffset.y > scrollView.bounds.height / 2
            if far != farFromBottom { parent?.actions.onFarChanged(far) }
            farFromBottom = far
            let remaining = scrollView.contentSize.height - scrollView.contentOffset.y - scrollView.bounds.height
            // Roughly 40 messages before the top (~60 pt each).
            if remaining < 2400 { parent?.actions.onNearTop() }
        }

        func collectionView(_ collectionView: UICollectionView, willDisplay cell: UICollectionViewCell, forItemAt indexPath: IndexPath) {
            if indexPath.item >= rows.count - 40 { parent?.actions.onNearTop() }
        }

        // MARK: Long press → actions

        private func message(at point: CGPoint) -> (UiMessage, UICollectionViewCell)? {
            guard let view, let indexPath = view.indexPathForItem(at: point),
                  case .message(let key)? = source.itemIdentifier(for: indexPath),
                  let i = index[key], let cell = view.cellForItem(at: indexPath) else { return nil }
            return (rows[i], cell)
        }

        @objc private func longPress(_ g: UILongPressGestureRecognizer) {
            guard g.state == .began, let view, let (m, _) = message(at: g.location(in: view)),
                  m.msg.kind != .system else { return }
            Haptics.tap()
            parent?.actions.onLongPress(m)
        }

        // MARK: Swipe right to reply

        private func replyable(_ m: UiMessage) -> Bool {
            m.msg.kind == .text && m.status == nil && m.msg.attachment?.isAudio != true
        }

        func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
            guard let pan = g as? UIPanGestureRecognizer, let view else { return true }
            let v = pan.velocity(in: view)
            guard v.x > 0, abs(v.x) > abs(v.y) * 1.5 else { return false }
            guard let (m, _) = message(at: pan.location(in: view)) else { return false }
            return replyable(m)
        }

        @objc private func pan(_ g: UIPanGestureRecognizer) {
            guard let view else { return }
            switch g.state {
            case .began:
                let hit = message(at: g.location(in: view))
                swipeCell = hit?.1
                swipeMessage = hit?.0.msg
                swipeTriggered = false
            case .changed:
                guard let cell = swipeCell else { return }
                let dx = min(max(0, g.translation(in: view).x), threshold * 1.3)
                cell.contentView.transform = CGAffineTransform(translationX: dx, y: 0)
                replyIcon.frame = CGRect(x: 16, y: cell.frame.midY - 11, width: 24, height: 22)
                replyIcon.alpha = min(1, max(0, (dx - 8) / threshold))
                if !swipeTriggered && dx >= threshold {
                    swipeTriggered = true
                    Haptics.light()
                }
            default:
                if let cell = swipeCell {
                    if swipeTriggered, let m = swipeMessage { parent?.actions.onReply(m) }
                    UIView.animate(withDuration: 0.2) {
                        cell.contentView.transform = .identity
                        self.replyIcon.alpha = 0
                    }
                }
                swipeCell = nil
                swipeMessage = nil
                swipeTriggered = false
            }
        }
    }
}

private struct ListFooter: View {
    let footer: MessageList.Footer

    var body: some View {
        switch footer {
        case .loading:
            ProgressView().frame(maxWidth: .infinity).padding(16)
        case .start:
            Text("Beginn der Unterhaltung")
                .font(.system(size: 12))
                .foregroundStyle(Talk.muted)
                .frame(maxWidth: .infinity)
                .padding(16)
        case .none:
            EmptyView()
        }
    }
}
