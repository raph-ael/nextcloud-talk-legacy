package org.wolkenfunk.app

import android.content.Context
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.ForegroundInfo
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.BackoffPolicy
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.withTimeoutOrNull
import java.util.concurrent.TimeUnit

/**
 * Keeps delivering the outbox while the app is in the background, and brings it back
 * after the process was killed or the phone rebooted (WorkManager persists the request
 * and waits for a network). The actual sending happens in the Rust core; this worker
 * only keeps the process alive until nothing is left to do.
 *
 * Like Signal, large uploads (≥ [FOREGROUND_BYTES]) run as a foreground service with a
 * visible notification so Android does not stop them.
 */
class OutboxWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        // The outboxes of all accounts, with one shared time limit (docs/accounts.md).
        val sessions = (applicationContext as TalkApplication).sessions.allSessions()
        val busy = sessions.filter { runCatching { it.outboxUnsent() > 0u }.getOrDefault(false) }
        if (busy.isEmpty()) return Result.success() // nothing left, or signed out
        val bytes = busy.sumOf { runCatching { it.outboxUploadBytes().toLong() }.getOrDefault(0L) }
        if (bytes >= FOREGROUND_BYTES) {
            // May be refused when started from the background (Android 12+): sending
            // continues anyway, just without the foreground guarantee.
            runCatching { setForeground(foregroundInfo()) }
        }
        // Workers get about ten minutes; stop a bit earlier and continue in a new run.
        val idle = withTimeoutOrNull(TimeUnit.MINUTES.toMillis(9)) {
            coroutineScope {
                // A session signed out meanwhile counts as done.
                busy.map { s -> async { runCatching { s.waitOutboxIdle() }.getOrNull() } }.awaitAll()
            }
        } ?: return Result.retry()
        // Only waiting items left: WorkManager runs us again once a network is there.
        return if (idle.any { (it?.waiting ?: 0u) > 0u }) Result.retry() else Result.success()
    }

    override suspend fun getForegroundInfo(): ForegroundInfo = foregroundInfo()

    private fun foregroundInfo(): ForegroundInfo {
        val notification = Notifications.sending(applicationContext)
        return if (Build.VERSION.SDK_INT >= 29) {
            ForegroundInfo(Notifications.ID_SENDING, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            ForegroundInfo(Notifications.ID_SENDING, notification)
        }
    }

    companion object {
        private const val NAME = "outbox"
        const val FOREGROUND_BYTES = 10L * 1024 * 1024

        /** Makes sure a worker will deliver the outbox (no-op when one is queued or running). */
        fun schedule(context: Context) {
            val request = OneTimeWorkRequestBuilder<OutboxWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork(NAME, ExistingWorkPolicy.KEEP, request)
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(NAME)
        }
    }
}
