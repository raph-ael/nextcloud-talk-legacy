package org.sirplus.talk

import android.content.Intent
import android.content.pm.ApplicationInfo
import org.sirplus.talk.call.CallActivity
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
        // Debug builds only (adb tests): `--ez org.sirplus.talk.CALL_SILENT true` starts a
        // silent call in that conversation; nobody is rung.
        val debug = (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
        if (debug && intent.getBooleanExtra(EXTRA_CALL_SILENT, false)) {
            intent.removeExtra(EXTRA_CALL_SILENT)
            sessions.startCall(token, silent = true)
            startActivity(Intent(this, CallActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }

    private companion object {
        const val EXTRA_CALL_SILENT = "org.sirplus.talk.CALL_SILENT"
    }
}
