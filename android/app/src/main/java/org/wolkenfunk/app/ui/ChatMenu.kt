package org.wolkenfunk.app.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import org.wolkenfunk.app.SessionManager
import uniffi.talk_ffi.RoomView

private enum class MenuSheet { MAIN, CATEGORY, NOTIFICATIONS }

/** One-to-one chats (incl. note-to-self and former one-to-one) have no „Nur Erwähnungen“. */
private fun isOneToOne(room: RoomView) = room.roomType.toInt() in setOf(1, 5, 6)

/**
 * Chat menu after a long press in the list (docs/settings.md „Chat-Menü“): avatar and
 * name, then the entries; „Kategorie“ and „Benachrichtigungen“ open a second sheet.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatMenu(sessions: SessionManager, room: RoomView, onDismiss: () -> Unit, onGroupSettings: () -> Unit) {
    var sheet by remember(room.token) { mutableStateOf(MenuSheet.MAIN) }
    var naming by remember(room.token) { mutableStateOf(false) }
    val supported = remember(room.token) { sessions.categoriesSupported() }

    if (naming) {
        NewCategoryDialog(sessions, room.token, onDone = { naming = false; onDismiss() }, onCancel = { naming = false })
        return
    }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(Modifier.fillMaxWidth().padding(bottom = 16.dp)) {
            when (sheet) {
                MenuSheet.MAIN -> MainEntries(
                    sessions, room, supported,
                    onDismiss = onDismiss,
                    onOpen = { sheet = it },
                    onGroupSettings = onGroupSettings,
                )
                MenuSheet.CATEGORY -> CategoryEntries(sessions, room, onNew = { naming = true })
                MenuSheet.NOTIFICATIONS -> NotificationEntries(room) { level ->
                    onDismiss()
                    sessions.chatAction { it.setNotificationLevel(room.token, level.toUByte()) }
                }
            }
        }
    }
}

@Composable
private fun MenuHeader(sessions: SessionManager, room: RoomView) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Avatar(room.name, 40.dp, room.token, room.avatarVersion, sessions)
        Spacer(Modifier.width(16.dp))
        Text(
            room.name,
            style = ListType.title,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.semantics { heading() },
        )
    }
}

@Composable
private fun MainEntries(
    sessions: SessionManager,
    room: RoomView,
    categoriesSupported: Boolean,
    onDismiss: () -> Unit,
    onOpen: (MenuSheet) -> Unit,
    onGroupSettings: () -> Unit,
) {
    val muted = LocalTalkColors.current.muted
    val arrow: @Composable () -> Unit = { Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null, tint = muted) }
    MenuHeader(sessions, room)
    HorizontalDivider(Modifier.padding(vertical = 4.dp), color = LocalTalkColors.current.chip)
    if (room.unreadMessages > 0) {
        SettingRow(TalkIcons.MarkRead, "Als gelesen markieren", onClick = {
            onDismiss()
            sessions.chatAction { it.markRoomRead(room.token) }
        })
    }
    val favorite = room.isFavorite
    SettingRow(
        if (favorite) TalkIcons.StarBorder else Icons.Filled.Star,
        if (favorite) "Aus Favoriten entfernen" else "Zu Favoriten hinzufügen",
        onClick = {
            onDismiss()
            sessions.chatAction { it.setFavorite(room.token, !favorite) }
        },
    )
    if (categoriesSupported) {
        SettingRow(TalkIcons.Label, "Kategorie", onClick = { onOpen(MenuSheet.CATEGORY) }, trailing = arrow)
    }
    SettingRow(
        Icons.Filled.Notifications,
        "Benachrichtigungen",
        notificationLabel(room.notificationLevel.toInt()),
        onClick = { onOpen(MenuSheet.NOTIFICATIONS) },
        trailing = arrow,
    )
    if (room.roomType.toInt() == 2 || room.roomType.toInt() == 3) {
        SettingRow(Icons.Filled.Settings, "Gruppeneinstellungen", onClick = onGroupSettings)
    }
}

/** Own categories with ticks; a tap toggles and the sheet stays open. */
@Composable
private fun CategoryEntries(sessions: SessionManager, room: RoomView, onNew: () -> Unit) {
    val custom = remember(room.token) { sessions.categories().filter { it.kind == "custom" } }
    val ids = remember(custom) { custom.map { it.id }.toSet() }
    // Shown at once; follows the room again once the core reports the change.
    var checked by remember(room.tagIds) { mutableStateOf(room.tagIds.filter { it in ids }.toSet()) }
    SheetTitle("Kategorie")
    for (c in custom) {
        val on = c.id in checked
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 56.dp)
                .toggleable(value = on, role = Role.Checkbox) {
                    val next = if (on) checked - c.id else checked + c.id
                    checked = next
                    sessions.chatAction { s -> s.setRoomCategories(room.token, custom.map { it.id }.filter { it in next }) }
                }
                .padding(horizontal = 24.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(c.name, style = ListType.title, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            Checkbox(checked = on, onCheckedChange = null)
        }
    }
    if (custom.isNotEmpty()) HorizontalDivider(Modifier.padding(vertical = 4.dp), color = LocalTalkColors.current.chip)
    SettingRow(Icons.Filled.Add, "Neue Kategorie …", onClick = onNew)
}

@Composable
private fun NotificationEntries(room: RoomView, onPick: (Int) -> Unit) {
    val current = room.notificationLevel.toInt()
    val levels = if (isOneToOne(room)) listOf(1, 3) else listOf(1, 2, 3)
    SheetTitle("Benachrichtigungen")
    Column(Modifier.selectableGroup()) {
        for (level in levels) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = 56.dp)
                    .selectable(selected = level == current, role = Role.RadioButton) { onPick(level) }
                    .padding(horizontal = 24.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = level == current, onClick = null)
                Spacer(Modifier.width(24.dp))
                Text(notificationLabel(level), style = ListType.title)
            }
        }
    }
}

@Composable
private fun SheetTitle(text: String) {
    Text(
        text,
        fontSize = 18.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 12.dp).semantics { heading() },
    )
}

/** „Neue Kategorie …“: name field as on the desktop page „Kategorien“, then created with this chat. */
@Composable
private fun NewCategoryDialog(sessions: SessionManager, token: String, onDone: () -> Unit, onCancel: () -> Unit) {
    var name by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val valid = name.isNotBlank() && !busy
    fun create() {
        if (!valid) return
        busy = true
        scope.launch {
            if (sessions.createCategory(name.trim(), listOf(token))) onDone() else busy = false
        }
    }
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text("Neue Kategorie") },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(250) },
                label = { Text("Name der Kategorie") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
            )
        },
        confirmButton = { TextButton(onClick = { create() }, enabled = valid) { Text("Erstellen") } },
        dismissButton = { TextButton(onClick = onCancel) { Text("Abbrechen") } },
    )
}
