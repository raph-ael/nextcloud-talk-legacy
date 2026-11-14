package org.wolkenfunk.app.push

import android.content.Context
import android.util.Log
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging
import kotlinx.coroutines.suspendCancellableCoroutine
import org.wolkenfunk.app.BuildConfig
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * FCM access. Push is on only when the build has a Firebase config (google-services.json)
 * and a push proxy URL; otherwise calls ring only while the app runs (notify_push).
 */
object Push {
    const val TAG = "TalkPush"

    val proxyUrl: String get() = BuildConfig.PUSH_PROXY_URL

    fun enabled(context: Context): Boolean =
        proxyUrl.isNotBlank() && runCatching { FirebaseApp.getApps(context).isNotEmpty() }.getOrDefault(false)

    /** The current FCM registration token. */
    suspend fun token(): String = suspendCancellableCoroutine { cont ->
        FirebaseMessaging.getInstance().token.addOnCompleteListener { task ->
            val token = task.result.takeIf { task.isSuccessful }
            if (token != null) cont.resume(token)
            else cont.resumeWithException(task.exception ?: IllegalStateException("no FCM token"))
        }
    }

    /** After a logout: the next account gets a fresh token. */
    fun deleteToken(context: Context) {
        if (!enabled(context)) return
        FirebaseMessaging.getInstance().deleteToken().addOnFailureListener { Log.w(TAG, "deleteToken", it) }
    }
}
