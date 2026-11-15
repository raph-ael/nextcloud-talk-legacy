package org.wolkenfunk.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.wolkenfunk.app.AccountView
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.UiState

/** „Konto hinzufügen“ is offered up to this many accounts (docs/accounts.md). */
const val MAX_ACCOUNTS = 5

/** „99+“ above 99. */
private fun count(n: Int) = if (n > 99) "99+" else n.toString()

/** „1 Chat“, „3 Chats“ (docs/accounts.md „Barrierefreiheit“). */
private fun chats(n: Int) = if (n == 1) "1 Chat" else "$n Chats"

/**
 * Own avatar at the top left of the chat list (docs/accounts.md „Avatar oben links“): with
 * one account it opens the settings, from two on the account menu, with the number of
 * chats with something new in the **other** accounts.
 */
@Composable
fun OwnAvatarButton(sessions: SessionManager, state: UiState, onSettings: () -> Unit, onAccounts: () -> Unit) {
    val several = state.accounts.size >= 2
    val others = state.accounts.filter { it.id != state.accountId }.sumOf { it.chats }
    Box(
        Modifier
            .padding(start = 12.dp, end = 8.dp)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = ripple(bounded = false, radius = 22.dp),
                onClick = if (several) onAccounts else onSettings,
            )
            .clearAndSetSemantics {
                contentDescription = when {
                    !several -> "Einstellungen"
                    others > 0 -> "Konten, Neues in ${chats(others)}"
                    else -> "Konten"
                }
                role = Role.Button
            },
    ) {
        UserAvatar(sessions, state.userId.ifEmpty { state.user }, state.userId, 32.dp)
        if (several && others > 0) {
            CountBadge(count(others), Modifier.align(Alignment.TopEnd).offset(x = 8.dp, y = (-6).dp))
        }
    }
}

/** Number in `accent` with white text, like the unread count in the chat list. */
@Composable
private fun CountBadge(text: String, modifier: Modifier = Modifier) {
    Box(
        modifier
            .heightIn(min = 18.dp)
            .widthIn(min = 18.dp)
            .clip(RoundedCornerShape(9.dp))
            .background(LocalTalkColors.current.accent)
            .padding(horizontal = 5.dp, vertical = 1.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = Color.White, fontSize = 11.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium)
    }
}

/**
 * One account as in the account menu: avatar 36, name, the server muted below; on the right
 * a tick for the active one, else its number. [locked]: a call runs, other accounts greyed.
 */
@Composable
fun AccountRow(sessions: SessionManager, account: AccountView, active: Boolean, locked: Boolean, onClick: () -> Unit) {
    val colors = LocalTalkColors.current
    val disabled = locked && !active
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clickable(onClick = onClick)
            .clearAndSetSemantics {
                contentDescription = buildString {
                    append("${account.name}, ${account.server}")
                    if (active) append(", aktiv") else if (account.chats > 0) append(", ${chats(account.chats)} mit Neuem")
                    if (disabled) append(", nicht verfügbar")
                }
                role = Role.Button
            }
            .padding(horizontal = 24.dp, vertical = 8.dp)
            .alpha(if (disabled) 0.38f else 1f),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CompositionLocalProvider(LocalAvatarAccount provides account.id) {
            UserAvatar(sessions, account.name, account.userId, 36.dp)
        }
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f)) {
            Text(account.name, style = ListType.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(account.server, style = ListType.body, color = colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        when {
            active -> Icon(Icons.Filled.Check, null, tint = colors.accent)
            account.chats > 0 -> CountBadge(count(account.chats))
        }
    }
}

/** Tap on an account row: the active one only closes, another switches (not during a call). */
fun pickAccount(sessions: SessionManager, state: UiState, id: String, onDone: () -> Unit) {
    if (id == state.accountId) {
        onDone()
        return
    }
    if (sessions.switchAccount(id)) onDone()
}

/** Account menu at the own avatar from two accounts on (docs/accounts.md „Kontomenü“). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountMenu(sessions: SessionManager, state: UiState, onDismiss: () -> Unit, onAdd: () -> Unit, onSettings: () -> Unit) {
    val call by sessions.call.collectAsState()
    val locked = call != null && sessions.inCall()
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(Modifier.fillMaxWidth().padding(bottom = 16.dp)) {
            for (a in state.accounts) {
                AccountRow(sessions, a, active = a.id == state.accountId, locked = locked) {
                    pickAccount(sessions, state, a.id, onDismiss)
                }
            }
            HorizontalDivider(Modifier.padding(vertical = 4.dp), color = LocalTalkColors.current.chip)
            if (state.accounts.size < MAX_ACCOUNTS) {
                SettingRow(Icons.Filled.Add, "Konto hinzufügen", onClick = { onDismiss(); onAdd() })
            }
            SettingRow(Icons.Filled.Settings, "Einstellungen", onClick = { onDismiss(); onSettings() })
        }
    }
}

/** „Abmelden?“ (docs/accounts.md „Abmelden“): names the account; only it is signed out. */
@Composable
fun LogoutDialog(name: String, server: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Abmelden?") },
        text = {
            Text(
                "$name ($server) wird auf diesem Gerät abgemeldet. Ungesendete Nachrichten und der " +
                    "Offline-Speicher dieses Kontos werden gelöscht.",
            )
        },
        confirmButton = {
            TextButton(onClick = onConfirm) { Text("Abmelden", color = LocalTalkColors.current.danger) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Abbrechen") } },
    )
}
