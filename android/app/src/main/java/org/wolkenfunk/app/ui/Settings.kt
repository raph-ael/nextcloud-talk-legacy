package org.wolkenfunk.app.ui

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ExitToApp
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.graphics.drawable.toBitmap
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.TalkApplication
import org.wolkenfunk.app.ThemeMode
import org.wolkenfunk.app.UiState

private enum class SettingsPage { APPEARANCE, NOTIFICATIONS, ABOUT }

/**
 * „Einstellungen“ (docs/settings.md): a screen of its own over the chat list; sub-pages
 * slide in within it, the back key goes up one level.
 */
@Composable
fun SettingsScreen(sessions: SessionManager, state: UiState, onBack: () -> Unit) {
    var page by rememberSaveable { mutableStateOf<SettingsPage?>(null) }
    BackHandler { if (page != null) page = null else onBack() }
    AnimatedContent(
        targetState = page,
        transitionSpec = {
            if (targetState != null) {
                slideInHorizontally { it } togetherWith slideOutHorizontally { -it / 4 }
            } else {
                slideInHorizontally { -it / 4 } togetherWith slideOutHorizontally { it }
            }
        },
        label = "settings",
    ) { p ->
        when (p) {
            null -> SettingsHome(sessions, state, onBack, onOpen = { page = it })
            SettingsPage.APPEARANCE -> AppearancePage { page = null }
            SettingsPage.NOTIFICATIONS -> NotificationsPage { page = null }
            SettingsPage.ABOUT -> AboutPage { page = null }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsScaffold(title: String, onBack: () -> Unit, content: @Composable () -> Unit) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title, fontSize = 22.sp) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück") } },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
            content()
            Spacer(Modifier.navigationBarsPadding().height(24.dp))
        }
    }
}

@Composable
private fun SettingsHome(sessions: SessionManager, state: UiState, onBack: () -> Unit, onOpen: (SettingsPage) -> Unit) {
    val colors = LocalTalkColors.current
    SettingsScaffold("Einstellungen", onBack) {
        // Head: only shown, nothing to edit here.
        Column(
            Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            UserAvatar(sessions, state.userId.ifEmpty { state.user }, state.userId, 64.dp)
            Spacer(Modifier.height(12.dp))
            Text(state.user, fontSize = 20.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.Center)
            Text(
                state.server.removePrefix("https://"),
                color = colors.muted,
                style = ListType.body,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(top = 2.dp),
            )
        }
        SectionDivider()
        NavRow(TalkIcons.Palette, "Darstellung") { onOpen(SettingsPage.APPEARANCE) }
        NavRow(Icons.Filled.Notifications, "Benachrichtigungen") { onOpen(SettingsPage.NOTIFICATIONS) }
        NavRow(Icons.Filled.Info, "Über Wolkenfunk") { onOpen(SettingsPage.ABOUT) }
        SectionDivider()
        SettingRow(Icons.AutoMirrored.Filled.ExitToApp, "Abmelden", tint = colors.danger, onClick = { sessions.logout() })
    }
}

/** A row of the start page: muted icon, title, arrow; read as a button with its title. */
@Composable
private fun NavRow(icon: ImageVector, title: String, onClick: () -> Unit) {
    val colors = LocalTalkColors.current
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clickable(onClick = onClick)
            .clearAndSetSemantics {
                contentDescription = title
                role = Role.Button
            }
            .padding(horizontal = 24.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = colors.muted, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(24.dp))
        Text(title, style = ListType.title, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null, tint = colors.muted)
    }
}

// --- Darstellung ---------------------------------------------------------------------

@Composable
private fun AppearancePage(onBack: () -> Unit) {
    val store = (LocalContext.current.applicationContext as TalkApplication).appearance
    val look by store.state.collectAsStateWithLifecycle()
    val dark = isDarkTheme()
    SettingsScaffold("Darstellung", onBack) {
        // Preview with the real bubble styles; the own one in the chosen colour.
        Column(
            Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            PreviewBubble("Wie gefällt dir die neue Farbe?", own = false)
            PreviewBubble("Sieht gut aus!", own = true)
        }
        SectionDivider()

        Column(Modifier.selectableGroup().semantics { contentDescription = "Design" }) {
            SectionTitle("Design", Modifier.semantics { heading() })
            listOf(ThemeMode.SYSTEM to "System", ThemeMode.LIGHT to "Hell", ThemeMode.DARK to "Dunkel").forEach { (mode, label) ->
                val selected = look.theme == mode
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = 56.dp)
                        .selectable(selected = selected, role = Role.RadioButton) { store.setTheme(mode) }
                        .padding(horizontal = 24.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = selected, onClick = null)
                    Spacer(Modifier.width(24.dp))
                    Text(label, style = ListType.title)
                }
            }
        }
        SectionDivider()

        SectionTitle("Akzentfarbe", Modifier.semantics { heading() })
        Column(
            Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            ACCENTS.chunked(5).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    row.forEach { a ->
                        Swatch(
                            color = if (dark) a.dark.accent else a.light.accent,
                            ring = MaterialTheme.colorScheme.onSurface,
                            selected = a.id == look.accent,
                            name = a.name,
                        ) { store.setAccent(a.id) }
                    }
                }
            }
        }
    }
}

/** Round colour field, 44dp (48dp touch target); the chosen one with a white tick and a ring. */
@Composable
private fun Swatch(color: Color, ring: Color, selected: Boolean, name: String, onClick: () -> Unit) {
    Box(
        Modifier
            .size(48.dp)
            .clip(CircleShape)
            .clickable(onClick = onClick)
            .clearAndSetSemantics {
                contentDescription = if (selected) "$name, ausgewählt" else name
                role = Role.Button
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(44.dp)
                .then(if (selected) Modifier.border(2.dp, ring, CircleShape) else Modifier)
                .padding(if (selected) 4.dp else 0.dp)
                .clip(CircleShape)
                .background(color),
            contentAlignment = Alignment.Center,
        ) {
            if (selected) Icon(Icons.Filled.Check, null, tint = Color.White, modifier = Modifier.size(22.dp))
        }
    }
}

/** A single-message bubble in the chat's style (ChatScreen: 18dp corners, 12/7 padding). */
@Composable
private fun PreviewBubble(text: String, own: Boolean) {
    val colors = LocalTalkColors.current
    Row(Modifier.fillMaxWidth(), horizontalArrangement = if (own) Arrangement.End else Arrangement.Start) {
        Text(
            text,
            color = if (own) colors.bubbleOwnText else colors.bubbleOtherText,
            style = TextStyle(fontSize = 15.sp, lineHeight = 21.sp),
            modifier = Modifier
                .widthIn(max = 300.dp)
                .clip(RoundedCornerShape(18.dp))
                .background(if (own) colors.bubbleOwn else colors.bubbleOther)
                .padding(horizontal = 12.dp, vertical = 7.dp),
        )
    }
}

// --- Benachrichtigungen ----------------------------------------------------------------

@Composable
private fun NotificationsPage(onBack: () -> Unit) {
    val context = LocalContext.current
    SettingsScaffold("Benachrichtigungen", onBack) {
        SettingRow(Icons.Filled.Notifications, "Systemeinstellungen öffnen", onClick = {
            val intent = Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
            runCatching { context.startActivity(intent) }
        })
        Text(
            "Für einzelne Chats: Chat in der Liste lange gedrückt halten.",
            style = ListType.body,
            color = LocalTalkColors.current.muted,
            modifier = Modifier.fillMaxWidth().padding(start = 72.dp, end = 24.dp, top = 4.dp, bottom = 8.dp),
        )
    }
}

// --- Über Wolkenfunk -----------------------------------------------------------------

@Composable
private fun AboutPage(onBack: () -> Unit) {
    val context = LocalContext.current
    val colors = LocalTalkColors.current
    val icon = remember {
        runCatching {
            val px = (64 * context.resources.displayMetrics.density).toInt()
            context.packageManager.getApplicationIcon(context.packageName).toBitmap(px, px).asImageBitmap()
        }.getOrNull()
    }
    val version = remember {
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull().orEmpty()
    }
    val core = remember { runCatching { uniffi.talk_ffi.coreVersion() }.getOrDefault("") }
    fun open(url: String) {
        runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url))) }
    }
    SettingsScaffold("Über Wolkenfunk", onBack) {
        Column(
            Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            if (icon != null) Image(icon, null, modifier = Modifier.size(64.dp))
            Spacer(Modifier.height(12.dp))
            Text("Wolkenfunk", fontSize = 22.sp, fontWeight = FontWeight.Medium)
            Text("Version $version", style = ListType.body, modifier = Modifier.padding(top = 4.dp))
            Text("Kern $core", style = ListType.body, color = colors.muted)
        }
        SectionDivider()
        LinkRow("Webseite") { open("https://wolkenfunk.org") }
        LinkRow("Quellcode") { open("https://github.com/raph-ael/nextcloud-talk-legacy") }
        Text(
            "Lizenz: AGPL-3.0",
            style = ListType.body,
            color = colors.muted,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 16.dp),
        )
    }
}

@Composable
private fun LinkRow(label: String, onClick: () -> Unit) {
    Text(
        label,
        style = ListType.title,
        color = LocalTalkColors.current.accent,
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 24.dp, vertical = 12.dp),
    )
}
