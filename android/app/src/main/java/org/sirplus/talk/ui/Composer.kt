package org.sirplus.talk.ui

import android.content.Context
import android.net.Uri
import android.view.ContextThemeWrapper
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.imeAnimationTarget
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.emoji2.emojipicker.EmojiPickerView
import kotlinx.coroutines.flow.distinctUntilChanged
import android.Manifest
import android.content.pm.PackageManager
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.sirplus.talk.VoiceManager
import uniffi.talk_ffi.MessageView

/** What replaces the keyboard below the input row. */
enum class InputPanel { NONE, EMOJI, ATTACH }

private const val PREFS = "ui"
private const val KEY_KB_HEIGHT = "keyboard_height_px"

/**
 * Signal's input row: [emoji | text | camera | mic] in one pill, next to a round button that
 * opens the attachment panel and turns into "send" once there is text. Emoji and attachment
 * panels take the keyboard's place at the keyboard's height.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun Composer(
    token: String,
    replyTo: MessageView?,
    onCancelReply: () -> Unit,
    onSend: (String) -> Unit,
    onOpenCamera: () -> Unit,
    onImagePicked: (Uri) -> Unit,
    voice: VoiceManager,
    onVoiceSent: () -> Unit,
) {
    val colors = LocalTalkColors.current
    val context = LocalContext.current
    val density = LocalDensity.current
    val keyboard = LocalSoftwareKeyboardController.current
    var value by rememberSaveable(token, stateSaver = TextFieldValue.Saver) { mutableStateOf(TextFieldValue()) }
    var panel by rememberSaveable(token) { mutableStateOf(InputPanel.NONE) }
    // Keeps the panel's space while the keyboard slides in, so the input row does not jump.
    var reserve by remember { mutableStateOf(false) }
    val focus = remember { FocusRequester() }

    val prefs = remember { context.getSharedPreferences(PREFS, Context.MODE_PRIVATE) }
    val defaultHeight = with(density) { 300.dp.roundToPx() }
    var panelHeight by remember { mutableIntStateOf(prefs.getInt(KEY_KB_HEIGHT, defaultHeight)) }
    val ime = WindowInsets.ime
    val imeTarget = WindowInsets.imeAnimationTarget
    val nav = WindowInsets.navigationBars

    // Learn the keyboard height; close a panel as soon as the keyboard comes up.
    LaunchedEffect(Unit) {
        snapshotFlow { imeTarget.getBottom(density) - nav.getBottom(density) }
            .distinctUntilChanged()
            .collect { h ->
                if (h > defaultHeight / 2) {
                    if (h != panelHeight) {
                        panelHeight = h
                        prefs.edit().putInt(KEY_KB_HEIGHT, h).apply()
                    }
                    if (panel != InputPanel.NONE) {
                        reserve = true
                        panel = InputPanel.NONE
                    }
                }
            }
    }
    LaunchedEffect(reserve) {
        if (!reserve) return@LaunchedEffect
        snapshotFlow { ime.getBottom(density) - nav.getBottom(density) >= panelHeight - 4 }
            .collect { if (it) reserve = false }
    }
    BackHandler(enabled = panel != InputPanel.NONE) { panel = InputPanel.NONE }

    fun open(p: InputPanel) {
        if (panel == p) {
            panel = InputPanel.NONE
            if (p == InputPanel.EMOJI) {
                focus.requestFocus()
                keyboard?.show()
            }
        } else {
            keyboard?.hide()
            panel = p
        }
    }

    val pickImage = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) {
            panel = InputPanel.NONE
            onImagePicked(uri)
        }
    }

    val hasText = value.text.isNotBlank()

    // --- voice messages ---------------------------------------------------------------
    val voiceUi by voice.state.collectAsStateWithLifecycle()
    val rec = voiceUi.recording?.takeIf { it.token == token }
    val draft = voiceUi.drafts[token]
    val haptics = LocalHapticFeedback.current
    var drag by remember { mutableStateOf(Offset.Zero) }
    val cancelDistance = with(density) { (LocalConfiguration.current.screenWidthDp * 0.4f).dp.toPx() }
    val askMic = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        Toast.makeText(
            context,
            if (granted) "Zum Aufnehmen das Mikrofon gedrückt halten" else "Für Sprachnachrichten braucht die App Zugriff auf das Mikrofon.",
            Toast.LENGTH_LONG,
        ).show()
    }
    LaunchedEffect(token) { voice.loadDraft(token) }
    // Leaving the chat or the app keeps what was recorded as draft (like Signal).
    DisposableEffect(token) { onDispose { voice.interrupt() } }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, e -> if (e == Lifecycle.Event.ON_PAUSE) voice.interrupt() }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    // Long recordings: the screen must not turn off (that would end the recording).
    val view = LocalView.current
    DisposableEffect(rec != null) {
        view.keepScreenOn = rec != null
        onDispose { view.keepScreenOn = false }
    }
    fun voiceSent() {
        onCancelReply()
        onVoiceSent()
    }

    Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
        replyTo?.let { r ->
            Row(
                Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(
                    Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(8.dp))
                        .background(colors.chip)
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                ) {
                    Text(if (r.isOwn) "Du" else r.author, color = colors.accent, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                    Text(
                        withAttachment(r.text, r.attachment),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        fontSize = 13.sp,
                    )
                }
                IconButton(onClick = onCancelReply) { Icon(TalkIcons.Close, "Antwort abbrechen") }
            }
        }

        if (rec?.locked == true) {
            LockedRecordingBar(
                rec,
                onCancel = {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    voice.cancel()
                },
                onSend = { voice.sendRecording(replyTo, ::voiceSent) },
            )
        } else if (rec == null && draft != null) {
            VoiceDraftBar(
                voice,
                draft,
                onDelete = { voice.discardDraft(token) },
                onSend = { voice.sendDraft(token, replyTo, ::voiceSent) },
            )
        } else {
        // Metrics from Signal's conversation_input_panel.xml.
        Row(
            Modifier.fillMaxWidth().padding(start = 12.dp, end = 14.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            Row(
                Modifier
                    .weight(1f)
                    .heightIn(min = 44.dp)
                    .clip(RoundedCornerShape(20.dp))
                    .background(MaterialTheme.colorScheme.surfaceVariant),
                verticalAlignment = Alignment.Bottom,
            ) {
                if (rec != null) {
                    HeldRecording(rec, drag.x, Modifier.weight(1f))
                } else {
                PillIcon(
                    if (panel == InputPanel.EMOJI) TalkIcons.Keyboard else TalkIcons.Mood,
                    if (panel == InputPanel.EMOJI) "Tastatur" else "Emoji",
                    width = 48.dp,
                    onClick = { open(InputPanel.EMOJI) },
                )
                BasicTextField(
                    value = value,
                    onValueChange = { value = it },
                    textStyle = TextStyle(fontSize = 16.sp, lineHeight = 21.sp, color = MaterialTheme.colorScheme.onSurface),
                    cursorBrush = SolidColor(colors.accent),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    maxLines = 5,
                    modifier = Modifier
                        .weight(1f)
                        .focusRequester(focus)
                        .heightIn(min = 44.dp)
                        .padding(top = 10.dp, bottom = 10.dp, end = if (hasText) 12.dp else 0.dp),
                    decorationBox = { inner ->
                        Box {
                            if (value.text.isEmpty()) Text("Nachricht", color = colors.muted, fontSize = 16.sp, lineHeight = 21.sp)
                            inner()
                        }
                    },
                )
                AnimatedVisibility(visible = !hasText, enter = fadeIn(), exit = fadeOut()) {
                    PillIcon(TalkIcons.Camera, "Kamera", width = 40.dp, endPadding = 16.dp, onClick = {
                        panel = InputPanel.NONE
                        onOpenCamera()
                    })
                }
                }
                // Same call site while holding, so the press gesture survives the swap above.
                AnimatedVisibility(visible = !hasText, enter = fadeIn(), exit = fadeOut()) {
                    MicButton(
                        onPress = {
                            if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) !=
                                PackageManager.PERMISSION_GRANTED
                            ) {
                                askMic.launch(Manifest.permission.RECORD_AUDIO)
                                false
                            } else {
                                keyboard?.hide()
                                panel = InputPanel.NONE
                                voice.start(token).also {
                                    if (it) haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                }
                            }
                        },
                        onDrag = { dx, dy -> drag = Offset(dx, dy) },
                        onCancel = {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            voice.cancel()
                        },
                        onLock = {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            voice.lock()
                        },
                        onRelease = { voice.sendRecording(replyTo, ::voiceSent) },
                        cancelDistancePx = cancelDistance,
                    )
                }
            }
            Spacer(Modifier.width(12.dp))
            val rotation by animateFloatAsState(if (panel == InputPanel.ATTACH && !hasText) 45f else 0f, label = "plus")
            Box {
            if (rec != null) LockHint(Modifier.align(Alignment.BottomCenter).offset(y = (-56).dp), drag.y)
            Box(
                Modifier
                    .padding(bottom = 2.dp)
                    .size(40.dp)
                    .clip(CircleShape)
                    .background(colors.accent)
                    .clickable {
                        if (hasText) {
                            val text = value.text.trim()
                            value = TextFieldValue()
                            onSend(text)
                        } else {
                            open(InputPanel.ATTACH)
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                AnimatedContent(
                    targetState = hasText,
                    transitionSpec = { (scaleIn() + fadeIn()) togetherWith (scaleOut() + fadeOut()) },
                    label = "send",
                ) { send ->
                    if (send) {
                        Icon(TalkIcons.Send, "Senden", tint = Color.White, modifier = Modifier.size(22.dp))
                    } else {
                        Icon(TalkIcons.Plus, "Anhang", tint = Color.White, modifier = Modifier.size(22.dp).rotate(rotation))
                    }
                }
            }
            }
        }
        }

        val shown = panel != InputPanel.NONE || reserve
        if (shown) {
            // Height = keyboard height minus the part of the keyboard already on screen, so
            // panel and keyboard hand over without moving the input row.
            Box(
                Modifier
                    .fillMaxWidth()
                    .clipToBounds()
                    .layout { measurable, constraints ->
                        val visibleIme = (ime.getBottom(this) - nav.getBottom(this)).coerceAtLeast(0)
                        val h = (panelHeight - visibleIme).coerceAtLeast(0)
                        val placeable = measurable.measure(constraints.copy(minHeight = panelHeight, maxHeight = panelHeight))
                        layout(constraints.maxWidth, h) { placeable.place(0, 0) }
                    },
            ) {
                when (panel) {
                    InputPanel.EMOJI -> EmojiPanel(
                        onEmoji = { value = value.insert(it) },
                        onBackspace = { value = value.backspace() },
                    )
                    InputPanel.ATTACH -> AttachPanel(
                        onImage = { uri ->
                            panel = InputPanel.NONE
                            onImagePicked(uri)
                        },
                        onGallery = {
                            pickImage.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                        },
                    )
                    InputPanel.NONE -> Box(Modifier.fillMaxSize())
                }
            }
        }
    }
}

/** Icon inside the input pill; [width] includes Signal's spacing around the 24dp glyph. */
@Composable
private fun PillIcon(
    icon: ImageVector,
    description: String,
    width: Dp,
    endPadding: Dp = 0.dp,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            .height(44.dp)
            .width(width)
            .clickable(
                interactionSource = remember { MutableInteractionSource() },
                indication = ripple(bounded = false, radius = 20.dp),
                onClick = onClick,
            )
            .padding(end = endPadding),
        contentAlignment = if (endPadding > 0.dp) Alignment.CenterEnd else Alignment.Center,
    ) {
        Icon(icon, description, tint = LocalTalkColors.current.muted, modifier = Modifier.size(24.dp))
    }
}

@Composable
private fun EmojiPanel(onEmoji: (String) -> Unit, onBackspace: () -> Unit) {
    val dark = isSystemInDarkTheme()
    val background = MaterialTheme.colorScheme.surface
    Box(Modifier.fillMaxSize().background(background)) {
        AndroidView(
            factory = { ctx ->
                val themed = ContextThemeWrapper(
                    ctx,
                    if (dark) android.R.style.Theme_Material_NoActionBar else android.R.style.Theme_Material_Light_NoActionBar,
                )
                EmojiPickerView(themed).apply {
                    setBackgroundColor(background.toArgb())
                    setOnEmojiPickedListener { onEmoji(it.emoji) }
                }
            },
            modifier = Modifier.fillMaxSize(),
        )
        Box(
            Modifier
                .align(Alignment.BottomEnd)
                .padding(12.dp)
                .size(48.dp)
                .clip(CircleShape)
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .clickable(onClick = onBackspace),
            contentAlignment = Alignment.Center,
        ) {
            Icon(TalkIcons.Backspace, "Löschen", tint = MaterialTheme.colorScheme.onSurface, modifier = Modifier.size(22.dp))
        }
    }
}

/** Replaces the selection with [s] and puts the cursor behind it. */
private fun TextFieldValue.insert(s: String): TextFieldValue {
    val start = selection.min
    val text = text.replaceRange(start, selection.max, s)
    return TextFieldValue(text, TextRange(start + s.length))
}

/** Deletes the selection or the grapheme (emoji incl. modifiers) before the cursor. */
private fun TextFieldValue.backspace(): TextFieldValue {
    if (!selection.collapsed) return TextFieldValue(text.removeRange(selection.min, selection.max), TextRange(selection.min))
    val pos = selection.start
    if (pos == 0) return this
    val it = android.icu.text.BreakIterator.getCharacterInstance()
    it.setText(text)
    val prev = it.preceding(pos).coerceAtLeast(0)
    return TextFieldValue(text.removeRange(prev, pos), TextRange(prev))
}
