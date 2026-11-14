package org.wolkenfunk.app

import android.app.Application

class TalkApplication : Application() {
    lateinit var sessions: SessionManager
        private set
    lateinit var appearance: AppearanceStore
        private set

    override fun onCreate() {
        super.onCreate()
        // Core logs to logcat (tag "talk-core"); debug builds also log the call details.
        val debug = (applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0
        uniffi.talk_ffi.initLogging(LogcatSink, if (debug) "talk_core=info,talk_core::call=debug" else "talk_core=info")
        Notifications.createChannels(this)
        org.wolkenfunk.app.call.CallNotifications.createChannels(this)
        org.wolkenfunk.app.call.RecordingNotifications.createChannel(this)
        appearance = AppearanceStore(this)
        sessions = SessionManager(this)
    }
}

private object LogcatSink : uniffi.talk_ffi.LogSink {
    override fun log(level: uniffi.talk_ffi.LogLevel, target: String, message: String) {
        val text = "$target: $message"
        when (level) {
            uniffi.talk_ffi.LogLevel.ERROR -> android.util.Log.e(TAG, text)
            uniffi.talk_ffi.LogLevel.WARN -> android.util.Log.w(TAG, text)
            uniffi.talk_ffi.LogLevel.INFO -> android.util.Log.i(TAG, text)
            uniffi.talk_ffi.LogLevel.DEBUG -> android.util.Log.d(TAG, text)
            uniffi.talk_ffi.LogLevel.TRACE -> android.util.Log.v(TAG, text)
        }
    }

    private const val TAG = "talk-core"
}
