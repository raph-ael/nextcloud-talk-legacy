package org.wolkenfunk.app.ui

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat

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
        val prefs = context.getSharedPreferences("ui", Context.MODE_PRIVATE)
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED &&
            !prefs.getBoolean("asked_notifications", false)
        ) {
            prefs.edit().putBoolean("asked_notifications", true).apply()
            launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
}
