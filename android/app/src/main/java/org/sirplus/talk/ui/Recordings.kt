package org.sirplus.talk.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.sirplus.talk.SessionManager
import org.sirplus.talk.call.RecordingNotifications
import org.sirplus.talk.call.RecordingNotifications.failed
import uniffi.talk_ffi.RecordingNotice

/**
 * Finished recordings after a call: "Aufnahme verfügbar · ‹Datei›" with [In Chat teilen]
 * and [Verwerfen], failures with [OK]. [token]: only this conversation (chat), otherwise
 * all with the room name (conversation list; tapping opens the conversation).
 */
@Composable
fun RecordingBanners(sessions: SessionManager, token: String? = null, onOpen: ((String) -> Unit)? = null) {
    val all by sessions.recordings.collectAsStateWithLifecycle()
    val busy by sessions.recordingsBusy.collectAsStateWithLifecycle()
    val items = if (token == null) all else all.filter { it.token == token }
    if (items.isEmpty()) return
    Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        for (n in items.sortedByDescending { it.timestamp }) {
            Banner(n, showRoom = token == null, busy = n.id in busy, sessions, onOpen)
        }
    }
}

@Composable
private fun Banner(n: RecordingNotice, showRoom: Boolean, busy: Boolean, sessions: SessionManager, onOpen: ((String) -> Unit)?) {
    val colors = LocalTalkColors.current
    val scheme = MaterialTheme.colorScheme
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(if (n.failed) scheme.errorContainer else scheme.secondaryContainer)
            .then(if (onOpen != null) Modifier.clickable { onOpen(n.token) } else Modifier)
            .padding(start = 14.dp, end = 6.dp, top = 10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(
                if (n.failed) TalkIcons.Close else TalkIcons.Record, null,
                tint = if (n.failed) scheme.onErrorContainer else scheme.onSecondaryContainer,
                modifier = Modifier.size(20.dp),
            )
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    RecordingNotifications.title(n),
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Medium,
                    color = if (n.failed) scheme.onErrorContainer else scheme.onSecondaryContainer,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                if (showRoom) Text(n.roomName, fontSize = 13.sp, color = colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            if (n.failed) {
                TextButton(onClick = { sessions.dismissRecording(n.id) }, enabled = !busy) { Text("OK") }
            } else {
                TextButton(onClick = { sessions.dismissRecording(n.id) }, enabled = !busy) { Text("Verwerfen") }
                if (n.canShare) {
                    TextButton(onClick = { sessions.shareRecording(n.id) }, enabled = !busy) { Text("In Chat teilen") }
                }
            }
        }
    }
}
