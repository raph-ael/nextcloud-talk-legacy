package org.wolkenfunk.app.ui

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LifecycleResumeEffect
import org.wolkenfunk.app.MessageNotifications

/** Asked before sending or calling (earlier versions). */
private const val ASKED_ON_USE = "asked_notifications"
/** Asked at a start with an account (docs/notifications.md); the earlier marker does not count for it. */
private const val ASKED_AT_START = "asked_notifications_start"

private fun prefs(context: Context) = context.getSharedPreferences("ui", Context.MODE_PRIVATE)

private fun missing(context: Context) = Build.VERSION.SDK_INT >= 33 &&
    ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED

/**
 * Returns a function that asks once for the notification permission (Android 13+), so
 * background uploads can show their progress and failed sends can be announced.
 * Without it everything still works, just silently.
 */
@Composable
fun rememberNotificationAsker(): () -> Unit {
    val context = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    return {
        val prefs = prefs(context)
        if (missing(context) && !prefs.getBoolean(ASKED_ON_USE, false) && !prefs.getBoolean(ASKED_AT_START, false)) {
            prefs.edit().putBoolean(ASKED_ON_USE, true).apply()
            launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
}

/**
 * Asks for the notification permission at the next start with an account, once
 * (docs/notifications.md „Erlaubnis“). Also in installations that were asked before sending.
 */
@Composable
fun AskNotificationsAtStart(signedIn: Boolean) {
    val context = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    LaunchedEffect(signedIn) {
        val prefs = prefs(context)
        if (signedIn && missing(context) && !prefs.getBoolean(ASKED_AT_START, false)) {
            prefs.edit().putBoolean(ASKED_AT_START, true).apply()
            launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
}

/** Whether the app may notify; checked again whenever the app comes back to the front. */
@Composable
fun notificationsEnabled(): Boolean {
    val context = LocalContext.current
    var on by remember { mutableStateOf(MessageNotifications.enabled(context)) }
    LifecycleResumeEffect(Unit) {
        on = MessageNotifications.enabled(context)
        onPauseOrDispose {}
    }
    return on
}

private fun Context.activity(): Activity? {
    var c: Context? = this
    while (c is ContextWrapper) {
        if (c is Activity) return c
        c = c.baseContext
    }
    return null
}

fun openNotificationSettings(context: Context) {
    val intent = Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
    runCatching { context.startActivity(intent) }
}

/**
 * „Einschalten“: asks where the system still shows its question, else opens the system
 * settings (the question is gone after two refusals, and older versions have none).
 */
@Composable
fun rememberNotificationEnabler(): () -> Unit {
    val context = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    return {
        val prefs = prefs(context)
        val asked = prefs.getBoolean(ASKED_AT_START, false) || prefs.getBoolean(ASKED_ON_USE, false)
        val again = context.activity()?.let {
            Build.VERSION.SDK_INT >= 33 && ActivityCompat.shouldShowRequestPermissionRationale(it, Manifest.permission.POST_NOTIFICATIONS)
        } == true
        if (missing(context) && (!asked || again)) {
            prefs.edit().putBoolean(ASKED_AT_START, true).apply()
            launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else {
            openNotificationSettings(context)
        }
    }
}
