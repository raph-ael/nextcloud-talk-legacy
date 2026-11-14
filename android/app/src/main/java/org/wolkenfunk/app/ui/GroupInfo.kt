package org.wolkenfunk.app.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ExitToApp
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import org.wolkenfunk.app.SessionManager
import uniffi.talk_ffi.MemberView
import uniffi.talk_ffi.RoomView
import uniffi.talk_ffi.UserView

private const val SHOWN_MEMBERS = 10
private const val DESCRIPTION_MAX = 2000

private sealed interface InfoPage {
    data object AddMembers : InfoPage
    data object Edit : InfoPage
}

private sealed interface InfoDialog {
    data object Notifications : InfoDialog
    data object Leave : InfoDialog
    data object NewModerator : InfoDialog
    data object Delete : InfoDialog
    data class Remove(val member: MemberView) : InfoDialog
}

/** Group settings (docs/groups.md „Gruppeneinstellungen“), a screen of its own over the chat. */
@Composable
fun GroupInfoScreen(sessions: SessionManager, room: RoomView, onBack: () -> Unit) {
    var page by remember(room.token) { mutableStateOf<InfoPage?>(null) }
    BackHandler { if (page != null) page = null else onBack() }
    when (page) {
        null -> GroupInfoMain(sessions, room, onBack, onOpen = { page = it })
        InfoPage.AddMembers -> AddMembersPage(sessions, room) { page = null }
        InfoPage.Edit -> EditGroupPage(sessions, room) { page = null }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun GroupInfoMain(sessions: SessionManager, room: RoomView, onBack: () -> Unit, onOpen: (InfoPage) -> Unit) {
    val colors = LocalTalkColors.current
    val scope = rememberCoroutineScope()
    var members by remember(room.token) { mutableStateOf<List<MemberView>?>(null) }
    var failed by remember(room.token) { mutableStateOf(false) }
    var attempt by remember { mutableIntStateOf(0) }
    var showAll by remember { mutableStateOf(false) }
    var sheet by remember { mutableStateOf<MemberView?>(null) }
    var dialog by remember { mutableStateOf<InfoDialog?>(null) }
    var busy by remember { mutableStateOf(false) }

    LaunchedEffect(room.token, attempt) {
        failed = false
        val loaded = sessions.members(room.token)
        if (loaded != null) members = loaded else if (members == null) failed = true
    }
    LaunchedEffect(room.token) {
        sessions.participantsChanged.collect { t -> if (t == room.token) sessions.members(t)?.let { members = it } }
    }
    fun run(action: suspend () -> Boolean, then: () -> Unit = {}) {
        if (busy) return
        busy = true
        scope.launch {
            if (action()) then()
            busy = false
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {},
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück") } },
            )
        },
    ) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize()) {
            item(key = "head") { GroupHeader(sessions, room, members?.size) { onOpen(InfoPage.Edit) } }

            item(key = "notify") {
                SectionDivider()
                SettingRow(Icons.Filled.Notifications, "Benachrichtigungen", notificationLabel(room.notificationLevel.toInt()), onClick = {
                    dialog = InfoDialog.Notifications
                })
            }

            item(key = "members-h") {
                SectionDivider()
                SectionTitle(members?.let { memberCount(it.size) } ?: "Mitglieder")
            }
            if (room.isModerator) {
                item(key = "add") { AccentActionRow(TalkIcons.PersonAdd, "Mitglieder hinzufügen") { onOpen(InfoPage.AddMembers) } }
            }
            val list = members
            when {
                list == null && failed -> item(key = "m-failed") { RetryRow("Konnte nicht geladen werden") { attempt++ } }
                list == null -> item(key = "m-loading") { PlaceholderRows() }
                else -> {
                    val shown = if (showAll || list.size <= SHOWN_MEMBERS) list else list.take(SHOWN_MEMBERS)
                    items(shown, key = { "m-" + it.attendeeId }) { m ->
                        MemberRow(sessions, m, onClick = if (m.isSelf) null else ({ sheet = m }))
                    }
                    if (shown.size < list.size) {
                        item(key = "all") {
                            Text(
                                "Alle anzeigen",
                                color = colors.accent,
                                style = ListType.title,
                                modifier = Modifier.fillMaxWidth().clickable { showAll = true }.padding(horizontal = 24.dp, vertical = 16.dp),
                            )
                        }
                    }
                }
            }

            if (room.isModerator || room.isPublic) {
                item(key = "link") {
                    SectionDivider()
                    LinkSection(sessions, room, busy) { on -> run({ sessions.groupAction { it.setGroupLink(room.token, on) } }) }
                }
            }

            item(key = "danger") {
                SectionDivider()
                SettingRow(Icons.AutoMirrored.Filled.ExitToApp, "Gruppe verlassen", tint = colors.danger, onClick = {
                    run({
                        val needs = sessions.leaveNeedsModerator(room.token)
                        if (needs != null) dialog = if (needs) InfoDialog.NewModerator else InfoDialog.Leave
                        needs != null
                    })
                })
                if (room.canDelete) {
                    SettingRow(TalkIcons.Delete, "Gruppe löschen", tint = colors.danger, onClick = { dialog = InfoDialog.Delete })
                }
                Spacer(Modifier.navigationBarsPadding().height(24.dp))
            }
        }
    }

    sheet?.let { m ->
        MemberSheet(
            sessions, room, m,
            onDismiss = { sheet = null },
            onMessage = { sheet = null; run({ sessions.openDirect(m.actorId) }) },
            onModerator = { sheet = null; run({ sessions.groupAction { it.setModerator(room.token, m.attendeeId, !m.isModerator) } }) },
            onRemove = { sheet = null; dialog = InfoDialog.Remove(m) },
        )
    }

    when (val d = dialog) {
        null -> Unit
        InfoDialog.Notifications -> NotificationDialog(room.notificationLevel.toInt(), onDismiss = { dialog = null }) { level ->
            dialog = null
            run({ sessions.groupAction { it.setNotificationLevel(room.token, level.toUByte()) } })
        }
        InfoDialog.Leave -> ConfirmDialog(
            title = "Gruppe verlassen?",
            text = "Du erhältst keine Nachrichten mehr aus dieser Gruppe.",
            confirm = "Verlassen",
            onDismiss = { dialog = null },
        ) { dialog = null; run({ sessions.leaveOrDelete(room.token, delete = false) }) }
        InfoDialog.Delete -> ConfirmDialog(
            title = "Gruppe für alle löschen?",
            text = "Alle Nachrichten gehen verloren.",
            confirm = "Löschen",
            onDismiss = { dialog = null },
        ) { dialog = null; run({ sessions.leaveOrDelete(room.token, delete = true) }) }
        is InfoDialog.Remove -> ConfirmDialog(
            title = "${d.member.name} aus der Gruppe entfernen?",
            text = null,
            confirm = "Entfernen",
            onDismiss = { dialog = null },
        ) { dialog = null; run({ sessions.groupAction { it.removeMember(room.token, d.member.attendeeId) } }) }
        InfoDialog.NewModerator -> NewModeratorDialog(
            sessions,
            candidates = members.orEmpty().filter { !it.isSelf },
            onDismiss = { dialog = null },
        ) { m ->
            dialog = null
            run({
                sessions.groupAction { it.setModerator(room.token, m.attendeeId, true) } &&
                    sessions.leaveOrDelete(room.token, delete = false)
            })
        }
    }

    if (busy) {
        Box(Modifier.fillMaxSize().clickable(enabled = false) {}, contentAlignment = Alignment.Center) {
            CircularProgressIndicator()
        }
    }
}

private fun memberCount(n: Int) = if (n == 1) "1 Mitglied" else "$n Mitglieder"

internal fun notificationLabel(level: Int) = when (level) {
    1 -> "Alle Nachrichten"
    2 -> "Nur Erwähnungen"
    else -> "Aus"
}

@Composable
private fun GroupHeader(sessions: SessionManager, room: RoomView, count: Int?, onEdit: () -> Unit) {
    val colors = LocalTalkColors.current
    var expanded by remember { mutableStateOf(false) }
    var overflows by remember { mutableStateOf(false) }
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Avatar(room.name, 80.dp, room.token, room.avatarVersion, sessions)
        Spacer(Modifier.height(12.dp))
        Text(room.name, fontSize = 24.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.Center)
        val kind = if (room.isPublic) "Gruppe mit Link" else "Gruppe"
        Text(
            if (count != null) "$kind · ${memberCount(count)}" else kind,
            color = colors.muted,
            style = ListType.body,
            modifier = Modifier.padding(top = 4.dp),
        )
        if (room.description.isNotBlank()) {
            Text(
                room.description,
                style = ListType.body,
                textAlign = TextAlign.Center,
                maxLines = if (expanded) Int.MAX_VALUE else 3,
                overflow = TextOverflow.Ellipsis,
                onTextLayout = { if (!expanded) overflows = it.hasVisualOverflow },
                modifier = Modifier.padding(top = 12.dp),
            )
            if (overflows && !expanded) {
                TextButton(onClick = { expanded = true }) { Text("Mehr") }
            }
        } else if (room.isModerator) {
            TextButton(onClick = onEdit, modifier = Modifier.padding(top = 4.dp)) { Text("Beschreibung hinzufügen") }
        }
        if (room.isModerator) {
            OutlinedButton(onClick = onEdit, modifier = Modifier.padding(top = 8.dp)) {
                Icon(Icons.Filled.Edit, null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text("Bearbeiten")
            }
        }
        Spacer(Modifier.height(8.dp))
    }
}

@Composable
internal fun SectionDivider() {
    HorizontalDivider(Modifier.padding(vertical = 8.dp), color = LocalTalkColors.current.chip, thickness = 8.dp)
}

@Composable
internal fun SettingRow(
    icon: ImageVector,
    label: String,
    value: String? = null,
    tint: Color? = null,
    onClick: (() -> Unit)? = null,
    trailing: @Composable () -> Unit = {},
) {
    val colors = LocalTalkColors.current
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 24.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = tint ?: colors.muted, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(24.dp))
        Column(Modifier.weight(1f)) {
            Text(label, style = ListType.title, color = tint ?: MaterialTheme.colorScheme.onSurface, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (value != null) Text(value, style = ListType.body, color = colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        trailing()
    }
}

@Composable
private fun MemberRow(sessions: SessionManager, m: MemberView, onClick: (() -> Unit)?) {
    val name = if (m.isSelf) "Du" else m.name
    val sub = when (m.actorType) {
        "guests" -> "Gast"
        "groups" -> "Nextcloud-Gruppe"
        "emails" -> "E-Mail"
        else -> ""
    }
    PersonRow(
        sessions, name, if (m.actorType == "users") m.actorId else "", sub,
        modifier = Modifier.semantics(mergeDescendants = true) {
            contentDescription = if (m.isModerator) "$name, Moderator" else name
        },
        onClick = onClick,
    ) {
        if (m.isModerator) Text("Moderator", style = ListType.body, color = LocalTalkColors.current.muted)
    }
}

@Composable
private fun LinkSection(sessions: SessionManager, room: RoomView, busy: Boolean, onToggle: (Boolean) -> Unit) {
    val context = LocalContext.current
    val link = remember(room.token, room.isPublic) { if (room.isPublic) sessions.groupLink(room.token) else "" }
    if (room.isModerator) {
        SettingRow(TalkIcons.Link, "Gruppenlink", "Jeder mit dem Link kann beitreten", onClick = { if (!busy) onToggle(!room.isPublic) }) {
            Switch(checked = room.isPublic, onCheckedChange = { if (!busy) onToggle(it) })
        }
    }
    if (room.isPublic) {
        if (!room.isModerator) SettingRow(TalkIcons.Link, "Gruppenlink", link)
        else Text(
            link,
            style = ListType.body,
            color = LocalTalkColors.current.muted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.fillMaxWidth().padding(start = 72.dp, end = 24.dp, bottom = 8.dp),
        )
        SettingRow(TalkIcons.Copy, "Link kopieren", onClick = {
            context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("Gruppenlink", link))
            Toast.makeText(context, "Link kopiert", Toast.LENGTH_SHORT).show()
        })
        SettingRow(Icons.Filled.Share, "Teilen", onClick = {
            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, link)
            context.startActivity(Intent.createChooser(send, null))
        })
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MemberSheet(
    sessions: SessionManager,
    room: RoomView,
    m: MemberView,
    onDismiss: () -> Unit,
    onMessage: () -> Unit,
    onModerator: () -> Unit,
    onRemove: () -> Unit,
) {
    val colors = LocalTalkColors.current
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().padding(bottom = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            UserAvatar(sessions, m.name, if (m.actorType == "users") m.actorId else "", 80.dp)
            Spacer(Modifier.height(12.dp))
            Text(m.name, fontSize = 22.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.Center, modifier = Modifier.padding(horizontal = 24.dp))
            if (m.isModerator) Text("Moderator", color = colors.muted, style = ListType.body)
            Spacer(Modifier.height(16.dp))
            if (m.actorType == "users") SettingRow(TalkIcons.Chat, "Nachricht senden", onClick = onMessage)
            if (room.isModerator && !m.isOwner) {
                SettingRow(TalkIcons.Shield, if (m.isModerator) "Moderatorrechte entziehen" else "Zum Moderator machen", onClick = onModerator)
                SettingRow(TalkIcons.PersonRemove, "Aus der Gruppe entfernen", tint = colors.danger, onClick = onRemove)
            }
        }
    }
}

@Composable
private fun ConfirmDialog(title: String, text: String?, confirm: String, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = text?.let { { Text(it) } },
        confirmButton = { TextButton(onClick = onConfirm) { Text(confirm, color = LocalTalkColors.current.danger) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Abbrechen") } },
    )
}

@Composable
private fun NotificationDialog(current: Int, onDismiss: () -> Unit, onPick: (Int) -> Unit) {
    val shown = current
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Benachrichtigungen") },
        text = {
            Column {
                listOf(1, 2, 3).forEach { level ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 48.dp)
                            .selectable(selected = level == shown, role = Role.RadioButton) { onPick(level) },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = level == shown, onClick = null)
                        Spacer(Modifier.width(16.dp))
                        Text(notificationLabel(level), style = ListType.title)
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Abbrechen") } },
    )
}

@Composable
private fun NewModeratorDialog(sessions: SessionManager, candidates: List<MemberView>, onDismiss: () -> Unit, onPick: (MemberView) -> Unit) {
    var chosen by remember { mutableStateOf<MemberView?>(null) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Neuen Moderator wählen") },
        text = {
            Column {
                Text("Bevor du gehst, ernenne jemanden zum Moderator.")
                Spacer(Modifier.height(8.dp))
                LazyColumn(Modifier.heightIn(max = 320.dp)) {
                    items(candidates, key = { it.attendeeId }) { m ->
                        val selected = chosen?.attendeeId == m.attendeeId
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = 56.dp)
                                .selectable(selected = selected, role = Role.RadioButton) { chosen = m },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            UserAvatar(sessions, m.name, if (m.actorType == "users") m.actorId else "", 36.dp)
                            Spacer(Modifier.width(12.dp))
                            Text(m.name, style = ListType.title, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                            RoundCheck(selected)
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { chosen?.let(onPick) }, enabled = chosen != null) {
                Text("Verlassen", color = if (chosen != null) LocalTalkColors.current.danger else LocalTalkColors.current.muted)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Abbrechen") } },
    )
}

@Composable
private fun AddMembersPage(sessions: SessionManager, room: RoomView, onDone: () -> Unit) {
    val selected = remember { mutableStateListOf<UserView>() }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    MemberPicker(
        sessions,
        title = "Mitglieder hinzufügen",
        excludeToken = room.token,
        selected = selected,
        confirmLabel = "Hinzufügen (${selected.size})",
        confirmEnabled = selected.isNotEmpty(),
        busy = busy,
        onBack = onDone,
        onConfirm = {
            busy = true
            scope.launch {
                val ids = selected.map { it.id }
                if (sessions.groupAction { it.addMembers(room.token, ids) }) onDone()
                busy = false
            }
        },
    )
}

private sealed interface PhotoChange {
    data object Unchanged : PhotoChange
    data object Removed : PhotoChange
    class New(val photo: PickedPhoto) : PhotoChange
}

/** „Gruppe bearbeiten“: photo, name, description; saves only what changed, one after another. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun EditGroupPage(sessions: SessionManager, room: RoomView, onDone: () -> Unit) {
    var name by remember { mutableStateOf(room.name) }
    var description by remember { mutableStateOf(room.description) }
    var photo by remember { mutableStateOf<PhotoChange>(PhotoChange.Unchanged) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val pick = rememberPhotoPicker { photo = PhotoChange.New(it) }
    val canSave = name.isNotBlank() && !busy
    val save: () -> Unit = save@{
        if (!canSave) return@save
        busy = true
        scope.launch {
            val token = room.token
            var ok = true
            if (ok && name.trim() != room.name) ok = sessions.groupAction { it.renameGroup(token, name.trim()) }
            if (ok && description.trim() != room.description.trim()) ok = sessions.groupAction { it.setDescription(token, description.trim()) }
            val p = photo
            if (ok && p is PhotoChange.New) ok = sessions.groupAction { it.setGroupPhoto(token, p.photo.bytes) }
            if (ok && p is PhotoChange.Removed) ok = sessions.groupAction { it.setGroupPhoto(token, null) }
            busy = false
            if (ok) onDone()
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Gruppe bearbeiten") },
                navigationIcon = { IconButton(onClick = onDone) { Icon(Icons.Filled.Close, "Abbrechen") } },
                actions = {
                    if (busy) CircularProgressIndicator(Modifier.padding(end = 16.dp).size(20.dp), strokeWidth = 2.dp)
                    else TextButton(onClick = save, enabled = canSave) { Text("Speichern") }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.padding(padding).fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(Modifier.height(16.dp))
            val p = photo
            GroupPhotoButton(
                (p as? PhotoChange.New)?.photo?.preview,
                fallback = {
                    if (p is PhotoChange.Removed) EmptyGroupPhoto()
                    else Avatar(room.name, 80.dp, room.token, room.avatarVersion, sessions)
                },
            ) { if (!busy) pick() }
            Row {
                TextButton(onClick = { pick() }, enabled = !busy) { Text("Ändern") }
                val hasPhoto = p is PhotoChange.New || (p is PhotoChange.Unchanged && room.hasCustomAvatar)
                if (hasPhoto) TextButton(onClick = { photo = PhotoChange.Removed }, enabled = !busy) { Text("Entfernen") }
            }
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(255) },
                label = { Text("Gruppenname") },
                singleLine = true,
                enabled = !busy,
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = description,
                onValueChange = { description = it.take(DESCRIPTION_MAX) },
                label = { Text("Beschreibung") },
                minLines = 3,
                enabled = !busy,
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                supportingText = if (description.length >= 1800) ({ Text("${description.length}/$DESCRIPTION_MAX") }) else null,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(24.dp))
        }
    }
}
