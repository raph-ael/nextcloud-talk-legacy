package org.sirplus.talk.call

import android.content.Intent
import android.content.pm.ActivityInfo
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.collectAsState
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import uniffi.talk_ffi.CallView
import kotlinx.coroutines.delay
import org.sirplus.talk.MainActivity
import org.sirplus.talk.TalkApplication
import org.sirplus.talk.ui.CallScreen
import org.sirplus.talk.ui.TalkTheme
import org.sirplus.talk.ui.presenting
import org.sirplus.talk.ui.rememberCallPermissions
import uniffi.talk_ffi.CallState

/**
 * The call screen, in its own task (singleTask), also over the lock screen. Shows
 * [CallController.ui] and closes itself once the call is gone.
 */
class CallActivity : ComponentActivity() {
    private val sessions get() = (application as TalkApplication).sessions
    private var acceptRequested by mutableStateOf(false)
    /** A call was shown since the last (re)start: its end closes the screen. */
    private var seen = false
    private var startWait: Job? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        showOverLockScreen()
        acceptRequested = intent?.action == ACTION_ACCEPT
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                // Ringing: back does not decline (like Signal); otherwise back to the chats.
                if (sessions.calls.ui.value?.state != CallState.INCOMING) minimize()
            }
        })
        // Not tied to the lifecycle: also when the screen is in the background, the end
        // of the call has to close it (a stale instance would come back on the next call).
        lifecycleScope.launch { sessions.calls.ui.collect { onCall(it) } }
        setContent {
            TalkTheme(dark = true) {
                val call by sessions.calls.ui.collectAsState()
                val routing by sessions.calls.audio.routing.collectAsState()
                val withPermissions = rememberCallPermissions()
                val current = call
                // Rotating only while a shared screen is shown (the system's rotation
                // lock applies), otherwise portrait (docs/calls.md "Mobil").
                val rotate = current != null && presenting(current)
                LaunchedEffect(rotate) {
                    requestedOrientation = if (rotate) ActivityInfo.SCREEN_ORIENTATION_FULL_USER else ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
                }

                // Accept from the notification: once the microphone is allowed.
                LaunchedEffect(acceptRequested, current?.id) {
                    if (acceptRequested && current?.state == CallState.INCOMING) {
                        acceptRequested = false
                        val token = current.token
                        withPermissions { sessions.calls.accept(token) }
                    }
                }
                if (current != null) {
                    CallScreen(
                        call = current,
                        routing = routing,
                        sessions = sessions,
                        onAccept = { withPermissions { sessions.calls.accept(current.token) } },
                        onDecline = { sessions.calls.decline(current.token) },
                        onHangUp = { sessions.calls.hangUp() },
                        onMute = { sessions.calls.setMuted(!current.muted) },
                        onCamera = { sessions.calls.setCamera(it) },
                        onSpeaker = { sessions.calls.audio.toggleSpeaker() },
                        onOutput = { sessions.calls.audio.select(it) },
                        onMinimize = { minimize() },
                    )
                }
            }
        }
    }

    // Camera only while the call screen is visible (docs/calls.md "Mobil").
    override fun onStart() {
        super.onStart()
        sessions.calls.resumeCamera()
    }

    override fun onStop() {
        super.onStop()
        sessions.calls.pauseCamera()
    }

    private fun onCall(call: CallView?) {
        if (call != null) {
            seen = true
            startWait?.cancel()
        } else if (seen) {
            finishAndRemoveTask()
        } else if (startWait?.isActive != true) {
            // Opened for a call that is still starting: give it a moment.
            startWait = lifecycleScope.launch {
                delay(STARTUP_WAIT_MS)
                if (sessions.calls.ui.value == null) finishAndRemoveTask()
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        seen = false
        onCall(sessions.calls.ui.value)
        if (intent.action == ACTION_ACCEPT) acceptRequested = true
    }

    private fun minimize() {
        startActivity(
            Intent(this, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT),
        )
        finish()
    }

    private fun showOverLockScreen() {
        if (Build.VERSION.SDK_INT >= 27) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON)
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    companion object {
        const val ACTION_ACCEPT = "org.sirplus.talk.call.ACCEPT"
        private const val STARTUP_WAIT_MS = 5_000L
    }
}
