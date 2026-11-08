package org.sirplus.talk

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import org.sirplus.talk.ui.TalkApp
import org.sirplus.talk.ui.TalkTheme

class MainActivity : ComponentActivity() {
    private val sessions get() = (application as TalkApplication).sessions

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null) openFromIntent(intent)
        setContent {
            TalkTheme { TalkApp(sessions) }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openFromIntent(intent)
    }

    /** A notification about a conversation (e.g. a failed send) opens that conversation. */
    private fun openFromIntent(intent: Intent?) {
        val token = intent?.getStringExtra(Notifications.EXTRA_TOKEN) ?: return
        intent.removeExtra(Notifications.EXTRA_TOKEN)
        sessions.openRoom(token)
    }
}
