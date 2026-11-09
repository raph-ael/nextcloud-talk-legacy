package org.sirplus.talk

import android.app.Application

class TalkApplication : Application() {
    lateinit var sessions: SessionManager
        private set

    override fun onCreate() {
        super.onCreate()
        Notifications.createChannels(this)
        org.sirplus.talk.call.CallNotifications.createChannels(this)
        sessions = SessionManager(this)
    }
}
