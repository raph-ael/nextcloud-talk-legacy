import BackgroundTasks
import UIKit

/// Keeps the process alive while the outboxes of all accounts deliver (iOS counterpart of
/// Android's `OutboxWorker`, docs/accounts.md "Postausgang"). The core does the sending;
/// this only buys time, one deadline for all:
/// - leaving the app with unsent messages: a background task (about 30 s on iOS),
/// - afterwards / after a restart: a `BGProcessingTask` that needs network and waits until
///   the outbox is idle, rescheduled while messages are still waiting.
@MainActor
final class BackgroundSender {
    static let shared = BackgroundSender()
    static let taskId = "org.wolkenfunk.app.outbox"

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

    /// Sessions with something unsent.
    private var sending: [TalkSession] {
        SessionStore.shared.allRuntimes.map(\.session).filter { $0.outboxUnsent() > 0 }
    }

    /// Waits until every given outbox is idle; the number still waiting.
    nonisolated private static func waitIdle(_ sessions: [TalkSession]) async -> UInt32 {
        await withTaskGroup(of: UInt32.self) { group in
            for s in sessions { group.addTask { await s.waitOutboxIdle().waiting } }
            return await group.reduce(0, +)
        }
    }

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
        keepAlive(sending)
    }

    /// A notification action ("Antworten") queued a message; iOS may have started the app
    /// in the background just for it.
    func actionQueued(_ s: TalkSession) {
        s.networkChanged()
        keepAlive(sending)
    }

    /// A background task (about 30 s) until these outboxes are idle, the processing task
    /// for what is left.
    private func keepAlive(_ sessions: [TalkSession]) {
        guard !sessions.isEmpty else { return }
        schedule()
        if task == .invalid {
            task = UIApplication.shared.beginBackgroundTask(withName: "outbox") { [weak self] in
                Task { @MainActor in self?.endTask() }
            }
        }
        waiter?.cancel()
        waiter = Task {
            _ = await Self.waitIdle(sessions)
            guard !Task.isCancelled else { return }
            self.endTask()
        }
    }

    func enteredForeground() {
        endTask()
        for rt in SessionStore.shared.allRuntimes { rt.session.networkChanged() }
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
        let sessions = sending
        guard !sessions.isEmpty else {
            task.setTaskCompleted(success: true)
            return
        }
        for s in sessions { s.networkChanged() }
        let work = Task {
            let waiting = await Self.waitIdle(sessions)
            if waiting > 0 { self.schedule() }
            task.setTaskCompleted(success: waiting == 0)
        }
        task.expirationHandler = {
            work.cancel()
            Task { @MainActor in BackgroundSender.shared.schedule() }
            task.setTaskCompleted(success: false)
        }
    }
}
