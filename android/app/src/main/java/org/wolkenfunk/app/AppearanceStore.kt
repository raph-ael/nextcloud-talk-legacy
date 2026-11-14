package org.wolkenfunk.app

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.wolkenfunk.app.ui.ACCENTS
import org.wolkenfunk.app.ui.DEFAULT_ACCENT

/** Design „System“, „Hell“ or „Dunkel“ (docs/settings.md „Darstellung“). */
enum class ThemeMode(val key: String) {
    SYSTEM("system"),
    LIGHT("light"),
    DARK("dark"),
}

data class Appearance(val theme: ThemeMode = ThemeMode.SYSTEM, val accent: String = DEFAULT_ACCENT)

/**
 * Design and accent colour of this device (SharedPreferences `ui`, keys `theme` and
 * `accent`). Not part of the account: they stay after signing out. Unknown or missing
 * values fall back to `system` and `blau`.
 */
class AppearanceStore(context: Context) {
    private val prefs = context.getSharedPreferences("ui", Context.MODE_PRIVATE)
    private val _state = MutableStateFlow(read())
    val state: StateFlow<Appearance> = _state.asStateFlow()

    private fun read(): Appearance {
        val theme = ThemeMode.entries.firstOrNull { it.key == prefs.getString("theme", null) } ?: ThemeMode.SYSTEM
        val accent = prefs.getString("accent", null)?.takeIf { id -> ACCENTS.any { it.id == id } } ?: DEFAULT_ACCENT
        return Appearance(theme, accent)
    }

    fun setTheme(theme: ThemeMode) {
        prefs.edit().putString("theme", theme.key).apply()
        _state.value = _state.value.copy(theme = theme)
    }

    fun setAccent(id: String) {
        if (ACCENTS.none { it.id == id }) return
        prefs.edit().putString("accent", id).apply()
        _state.value = _state.value.copy(accent = id)
    }
}
