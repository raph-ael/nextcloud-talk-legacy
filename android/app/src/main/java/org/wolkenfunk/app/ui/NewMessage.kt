package org.wolkenfunk.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.wolkenfunk.app.SessionManager
import uniffi.talk_ffi.UserView

private enum class Step { Contacts, Members, Name }

/**
 * „Neue Nachricht“ (docs/groups.md): contacts open a 1:1 chat; „Neue Gruppe“ leads through
 * „Mitglieder hinzufügen“ and „Gruppe benennen“. Back goes one step back.
 */
@Composable
fun NewMessageFlow(sessions: SessionManager, onClose: () -> Unit) {
    var step by remember { mutableStateOf(Step.Contacts) }
    val selected = remember { mutableStateListOf<UserView>() }
    var photo by remember { mutableStateOf<PickedPhoto?>(null) }
    var name by remember { mutableStateOf("") }
    BackHandler {
        when (step) {
            Step.Contacts -> onClose()
            Step.Members -> step = Step.Contacts
            Step.Name -> step = Step.Members
        }
    }
    AnimatedContent(
        targetState = step,
        transitionSpec = {
            if (targetState > initialState) slideInHorizontally { it } togetherWith slideOutHorizontally { -it / 4 }
            else slideInHorizontally { -it / 4 } togetherWith slideOutHorizontally { it }
        },
        label = "new-message",
    ) { s ->
        when (s) {
            Step.Contacts -> ContactsStep(sessions, onBack = onClose, onNewGroup = { step = Step.Members }, onOpened = onClose)
            Step.Members -> MemberPicker(
                sessions,
                title = "Mitglieder hinzufügen",
                excludeToken = null,
                selected = selected,
                confirmLabel = if (selected.isEmpty()) "Überspringen" else "Weiter",
                confirmEnabled = true,
                busy = false,
                onBack = { step = Step.Contacts },
                onConfirm = { step = Step.Name },
            )
            Step.Name -> NameStep(
                sessions,
                members = selected,
                name = name,
                onName = { name = it },
                photo = photo,
                onPhoto = { photo = it },
                onBack = { step = Step.Members },
                onCreated = onClose,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ContactsStep(sessions: SessionManager, onBack: () -> Unit, onNewGroup: () -> Unit, onOpened: () -> Unit) {
    var query by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    val search = rememberUserSearch(sessions, query)
    val canCreate = remember { sessions.canCreateGroups() }
    val scope = rememberCoroutineScope()
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Neue Nachricht") },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück") } },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            NameSearchField(query, { query = it })
            LazyColumn(Modifier.fillMaxSize().imePadding()) {
                if (canCreate && query.isEmpty()) {
                    item(key = "new-group") { AccentActionRow(TalkIcons.Group, "Neue Gruppe", onNewGroup) }
                }
                item(key = "h") { SectionTitle("Kontakte") }
                userResults(search) { user ->
                    PersonRow(sessions, user.name, user.id, user.subline, onClick = {
                        if (busy) return@PersonRow
                        busy = true
                        scope.launch {
                            if (sessions.openDirect(user.id)) onOpened()
                            busy = false
                        }
                    })
                }
            }
        }
    }
}

/** Search results, placeholders, „Keine Treffer“ or the retry row. */
private fun androidx.compose.foundation.lazy.LazyListScope.userResults(
    search: UserSearchState,
    row: @Composable (UserView) -> Unit,
) {
    val users = search.users
    when {
        search.failed -> item(key = "failed") { RetryRow("Keine Verbindung", search.retry) }
        users == null -> item(key = "loading") { PlaceholderRows() }
        users.isEmpty() -> item(key = "empty") {
            Text("Keine Treffer", color = LocalTalkColors.current.muted, modifier = Modifier.fillMaxWidth().padding(24.dp))
        }
        else -> items(users, key = { "u-" + it.id }) { row(it) }
    }
}

/**
 * Picking people: search, chips of the selection, list with round checks and a FAB.
 * Used for step 1 of a new group and for „Mitglieder hinzufügen“ in the group settings.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun MemberPicker(
    sessions: SessionManager,
    title: String,
    excludeToken: String?,
    selected: SnapshotStateList<UserView>,
    confirmLabel: String,
    confirmEnabled: Boolean,
    busy: Boolean,
    onBack: () -> Unit,
    onConfirm: () -> Unit,
) {
    var query by remember { mutableStateOf("") }
    val search = rememberUserSearch(sessions, query, excludeToken)
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück") } },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { if (confirmEnabled && !busy) onConfirm() },
                containerColor = if (confirmEnabled) LocalTalkColors.current.accent else LocalTalkColors.current.chip,
                contentColor = if (confirmEnabled) Color.White else LocalTalkColors.current.muted,
                modifier = Modifier.imePadding(),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(20.dp), color = Color.White, strokeWidth = 2.dp)
                else Text(confirmLabel)
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            NameSearchField(query, { query = it })
            if (selected.isNotEmpty()) {
                FlowRow(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    selected.forEach { u -> PersonChip(sessions, u) { selected.remove(u) } }
                }
            }
            LazyColumn(Modifier.fillMaxSize().imePadding()) {
                userResults(search) { user ->
                    val isSelected = selected.any { it.id == user.id }
                    PersonRow(
                        sessions, user.name, user.id, user.subline,
                        modifier = Modifier.semantics(mergeDescendants = true) {
                            contentDescription = "${user.name}, " + if (isSelected) "ausgewählt" else "nicht ausgewählt"
                        },
                        onClick = {
                            if (busy) return@PersonRow
                            if (isSelected) {
                                selected.removeAll { it.id == user.id }
                            } else {
                                selected.add(user)
                                query = ""
                            }
                        },
                    ) { RoundCheck(isSelected) }
                }
                item { Spacer(Modifier.height(88.dp)) }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NameStep(
    sessions: SessionManager,
    members: List<UserView>,
    name: String,
    onName: (String) -> Unit,
    photo: PickedPhoto?,
    onPhoto: (PickedPhoto?) -> Unit,
    onBack: () -> Unit,
    onCreated: () -> Unit,
) {
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val pick = rememberPhotoPicker { onPhoto(it) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val ready = name.isNotBlank()
    val create = {
        if (ready && !busy) {
            busy = true
            scope.launch {
                if (sessions.createGroup(name.trim(), members.map { it.id }, photo?.bytes)) onCreated()
                busy = false
            }
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Gruppe benennen") },
                navigationIcon = {
                    IconButton(onClick = { if (!busy) onBack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Zurück") }
                },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { create() },
                containerColor = if (ready) LocalTalkColors.current.accent else LocalTalkColors.current.chip,
                contentColor = if (ready) Color.White else LocalTalkColors.current.muted,
                modifier = Modifier.imePadding(),
            ) {
                if (busy) CircularProgressIndicator(Modifier.size(20.dp), color = Color.White, strokeWidth = 2.dp)
                else Text("Erstellen")
            }
        },
    ) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize().imePadding()) {
            item(key = "photo") {
                Column(Modifier.fillMaxWidth().padding(top = 16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    GroupPhotoButton(photo?.preview, { EmptyGroupPhoto() }) { if (!busy) pick() }
                    if (photo != null) {
                        TextButton(onClick = { onPhoto(null) }, enabled = !busy) { Text("Entfernen") }
                    } else {
                        Spacer(Modifier.height(12.dp))
                    }
                }
            }
            item(key = "name") {
                OutlinedTextField(
                    value = name,
                    onValueChange = { onName(it.take(255)) },
                    label = { Text("Gruppenname (erforderlich)") },
                    singleLine = true,
                    enabled = !busy,
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp).focusRequester(focus),
                )
            }
            item(key = "h") { SectionTitle("Mitglieder · ${members.size}", Modifier.padding(top = 8.dp)) }
            items(members, key = { "m-" + it.id }) { u -> PersonRow(sessions, u.name, u.id, u.subline) }
            item { Box(Modifier.height(88.dp)) }
        }
    }
}
