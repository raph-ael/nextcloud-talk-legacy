import BackgroundTasks
import UIKit

/// Keeps the process alive while the outbox delivers (iOS counterpart of Android's
/// `OutboxWorker`). The core does the sending; this only buys time:
/// - leaving the app with unsent messages: a background task (about 30 s on iOS),
/// - afterwards / after a restart: a `BGProcessingTask` that needs network and waits until
///   the outbox is idle, rescheduled while messages are still waiting.
@MainActor
final class BackgroundSender {
    static let shared = BackgroundSender()
    static let taskId = "org.sirplus.talk.outbox"

    private var task: UIBackgroundTaskIdentifier = .invalid
    private var waiter: Task<Void, Never>?

    nonisolated(unsafe) private static var registered = false

    /// Called from `App.init` (must happen before launch ends and before any submit).
    nonisolated static func register() {
        guard !registered else { return }
        registered = BGTaskScheduler.shared.register(forTaskWithIdentifier: taskId, using: nil) { task in
            Task { @MainActor in BackgroundSender.shared.run(task) }
        }
    }

    private var session: TalkSession? { SessionStore.shared.session }

    /// Takes the session directly: runs while `SessionStore.shared` is still being created.
    func sessionStarted(_ s: TalkSession) {
        if s.outboxUnsent() > 0 { schedule() }
    }

    /// Something was queued: if the app goes away now, the system gets a task for it.
    func queued() {
        schedule()
    }

    /// The scene left the foreground.
    func enteredBackground() {
        guard let s = session, s.outboxUnsent() > 0 else { return }
        schedule()
        guard task == .invalid else { return }
        task = UIApplication.shared.beginBackgroundTask(withName: "outbox") { [weak self] in
            Task { @MainActor in self?.endTask() }
        }
        waiter = Task {
            _ = await s.waitOutboxIdle()
            self.endTask()
        }
    }

    func enteredForeground() {
        endTask()
        session?.networkChanged()
    }

    private func endTask() {
        waiter?.cancel()
        waiter = nil
        if task != .invalid {
            UIApplication.shared.endBackgroundTask(task)
            task = .invalid
        }
    }

    private func schedule() {
        guard Self.registered else { return }
        let request = BGProcessingTaskRequest(identifier: Self.taskId)
        request.requiresNetworkConnectivity = true
        request.requiresExternalPower = false
        try? BGTaskScheduler.shared.submit(request)
    }

    private func run(_ task: BGTask) {
        guard let s = session, s.outboxUnsent() > 0 else {
            task.setTaskCompleted(success: true)
            return
        }
        s.networkChanged()
        let work = Task {
            let idle = await s.waitOutboxIdle()
            if idle.waiting > 0 { self.schedule() }
            task.setTaskCompleted(success: idle.waiting == 0)
        }
        task.expirationHandler = {
            work.cancel()
            Task { @MainActor in BackgroundSender.shared.schedule() }
            task.setTaskCompleted(success: false)
        }
    }
}
