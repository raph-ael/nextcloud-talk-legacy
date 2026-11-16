package org.wolkenfunk.app.ui

import android.os.SystemClock
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.fadeIn
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import org.wolkenfunk.app.UiState
import uniffi.talk_ffi.ConnectionState

/** What the header shows for the account's connection (docs/connection.md). */
enum class ConnectionHint { NONE, CONNECTING, OFFLINE }

/** The spinner appears once the connection has been away this long; shorter gaps stay invisible. */
private const val SPINNER_DELAY_MS = 500L
private val STATUS_LINE = 16.sp

/**
 * `CONNECTING` only after the state has lasted 500 ms, counted from when the account left
 * `online` (so a change of screen or account does not start the wait again). Back to `NONE` at once.
 */
@Composable
fun connectionHint(state: UiState): ConnectionHint {
    val since = state.connectionSince
    var held by remember(state.accountId, since) {
        mutableStateOf(SystemClock.elapsedRealtime() - since >= SPINNER_DELAY_MS)
    }
    val waiting = state.connection != ConnectionState.ONLINE
    LaunchedEffect(state.accountId, since, waiting) {
        if (waiting && !held) {
            delay(SPINNER_DELAY_MS - (SystemClock.elapsedRealtime() - since))
            held = true
        }
    }
    return when {
        state.connection == ConnectionState.OFFLINE -> ConnectionHint.OFFLINE
        waiting && held -> ConnectionHint.CONNECTING
        else -> ConnectionHint.NONE
    }
}

/** Small, thin and muted; its place is always taken, so nothing next to it moves. */
@Composable
fun ConnectionSpinner(visible: Boolean, modifier: Modifier = Modifier, size: Dp = 14.dp, stroke: Dp = 1.5.dp) {
    Box(modifier.size(size)) {
        AnimatedVisibility(visible, enter = fadeIn(), exit = ExitTransition.None) {
            CircularProgressIndicator(
                Modifier.size(size).semantics { contentDescription = "Verbinde …" },
                color = LocalTalkColors.current.muted,
                strokeWidth = stroke,
            )
        }
    }
}

/**
 * Title of a top bar with „Keine Verbindung“ as a second line while [offline]. The title
 * keeps the place it has alone: the line hangs below it, and only where the bar is too low
 * for both (large fonts) the title moves up by what is missing.
 */
@Composable
fun TitleWithConnection(
    offline: Boolean,
    modifier: Modifier = Modifier,
    /** Chat list: the spinner sits in front of the text; in a chat the text stands alone. */
    spinner: Boolean = false,
    title: @Composable () -> Unit,
) {
    val reserved = with(LocalDensity.current) { STATUS_LINE.roundToPx() }
    Layout(
        modifier = modifier,
        content = {
            Box { title() }
            Box {
                if (offline) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (spinner) {
                            ConnectionSpinner(true, size = 12.dp)
                            Spacer(Modifier.width(6.dp))
                        }
                        Text(
                            "Keine Verbindung",
                            color = LocalTalkColors.current.muted,
                            fontSize = 13.sp,
                            lineHeight = STATUS_LINE,
                            maxLines = 1,
                        )
                    }
                }
            }
        },
    ) { measurables, constraints ->
        val loose = constraints.copy(minWidth = 0, minHeight = 0)
        val t = measurables[0].measure(loose)
        val s = measurables[1].measure(loose)
        val line = maxOf(s.height, reserved)
        val width = maxOf(t.width, s.width).coerceAtLeast(constraints.minWidth)
        // Room for the line below and as much above, so the title is centred with or without it.
        val height = (t.height + 2 * line).coerceIn(constraints.minHeight, constraints.maxHeight)
        var top = (height - t.height) / 2
        if (offline) top = (top - (top + t.height + line - height).coerceAtLeast(0)).coerceAtLeast(0)
        layout(width, height) {
            t.place(0, top)
            s.place(0, top + t.height)
        }
    }
}
