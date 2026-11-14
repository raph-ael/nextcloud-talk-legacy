package org.wolkenfunk.app

import android.content.Intent
import android.content.pm.ApplicationInfo
import org.wolkenfunk.app.call.CallActivity
import android.os.Bundle
import android.content.res.Configuration
import android.graphics.drawable.ColorDrawable
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.wolkenfunk.app.ui.TalkApp
import org.wolkenfunk.app.ui.TalkTheme

class MainActivity : ComponentActivity() {
    private val sessions get() = (application as TalkApplication).sessions
    private val appearance get() = (application as TalkApplication).appearance

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The chosen design (docs/settings.md „Darstellung“) already for the first frame:
        // window background and system bars, which otherwise follow the system.
        val systemDark = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        applyDesign(appearance.state.value.isDark(systemDark))
        if (savedInstanceState == null) openFromIntent(intent)
        setContent {
            val look by appearance.state.collectAsStateWithLifecycle()
            val dark = look.isDark(isSystemInDarkTheme())
            LaunchedEffect(dark) { applyDesign(dark) }
            TalkTheme(dark = dark, accent = look.accent) { TalkApp(sessions) }
        }
    }

    private fun Appearance.isDark(systemDark: Boolean) = when (theme) {
        ThemeMode.SYSTEM -> systemDark
        ThemeMode.LIGHT -> false
        ThemeMode.DARK -> true
    }

    /** System bar icons and window background follow the design in effect. */
    private fun applyDesign(dark: Boolean) {
        val transparent = android.graphics.Color.TRANSPARENT
        // Navigation bar scrims as in enableEdgeToEdge() (only shown with 3-button navigation).
        val lightScrim = android.graphics.Color.argb(0xe6, 0xFF, 0xFF, 0xFF)
        val darkScrim = android.graphics.Color.argb(0x80, 0x1b, 0x1b, 0x1b)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.auto(transparent, transparent) { dark },
            navigationBarStyle = SystemBarStyle.auto(lightScrim, darkScrim) { dark },
        )
        window.setBackgroundDrawable(ColorDrawable(if (dark) 0xFF121212.toInt() else android.graphics.Color.WHITE))
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
        // Debug builds only (adb tests): `--ez org.wolkenfunk.app.CALL_SILENT true` starts a
        // silent call in that conversation; nobody is rung.
        val debug = (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
        if (debug && intent.getBooleanExtra(EXTRA_CALL_SILENT, false)) {
            intent.removeExtra(EXTRA_CALL_SILENT)
            sessions.startCall(token, silent = true)
            startActivity(Intent(this, CallActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }

    private companion object {
        const val EXTRA_CALL_SILENT = "org.wolkenfunk.app.CALL_SILENT"
    }
}
