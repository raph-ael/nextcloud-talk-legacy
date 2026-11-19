package org.wolkenfunk.app.ui

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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Edit
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
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalWindowInfo
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
import kotlinx.coroutines.flow.first
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
import androidx.compose.ui.focus.onFocusChanged
import org.wolkenfunk.app.SessionManager
import org.wolkenfunk.app.TextDraft
import uniffi.talk_ffi.MentionCandidate
import uniffi.talk_ffi.MentionPick
import uniffi.talk_ffi.MessageView
import uniffi.talk_ffi.RoomView

/**
 * A message being edited: [source] is what the input field starts with (`editSource()`),
 * [picks] who the names in it stand for.
 */
class EditTarget(val msg: MessageView, val source: String, val picks: List<MentionPick>)

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
    sessions: SessionManager,
    room: RoomView,
    replyTo: MessageView?,
    onCancelReply: () -> Unit,
    /** Edit mode (docs/chat.md „Bearbeiten und Löschen“): the bar replaces the reply bar meanwhile. */
    editing: EditTarget?,
    saving: Boolean,
    onCancelEdit: () -> Unit,
    onSaveEdit: (String, List<MentionPick>) -> Unit,
    onSend: (String, List<MentionPick>) -> Unit,
    onOpenCamera: () -> Unit,
    onImagePicked: (Uri) -> Unit,
    onFilesPicked: (List<Uri>) -> Unit,
    /** „Standort“ in the attachment menu; the caller runs `sent` after sending, which closes the menu. */
    onLocation: (sent: () -> Unit) -> Unit,
    /** „Kontakt“ in the attachment menu; `sent` as for the place. */
    onContact: (sent: () -> Unit) -> Unit,
    onVoiceSent: () -> Unit,
) {
    val token = room.token
    val voice = sessions.voice
    val colors = LocalTalkColors.current
    val context = LocalContext.current
    val density = LocalDensity.current
    val keyboard = LocalSoftwareKeyboardController.current
    val focusManager = LocalFocusManager.current
    // The draft outlives the chat screen (docs/mentions.md „Entwurf“): text, cursor and picks.
    var value by rememberSaveable(token, stateSaver = TextFieldValue.Saver) {
        mutableStateOf(sessions.draft(token)?.let { TextFieldValue(it.text, TextRange(it.cursor)) } ?: TextFieldValue())
    }
    // The mentions picked from the suggestions; which places of the text they are, the text alone tells.
    var picks by rememberSaveable(token, stateSaver = PicksSaver) { mutableStateOf(sessions.draft(token)?.picks.orEmpty()) }
    LaunchedEffect(token, value.text, value.selection, picks) {
        sessions.setDraft(token, if (value.text.isEmpty()) null else TextDraft(value.text, value.selection.end, picks))
    }
    // The text being edited lives next to the draft: it never becomes the draft, and the draft
    // (like a started reply) is back untouched once the edit mode ends.
    var editValue by remember(editing?.msg?.id) {
        val source = editing?.source.orEmpty()
        mutableStateOf(TextFieldValue(source, TextRange(source.length)))
    }
    var editPicks by remember(editing?.msg?.id) { mutableStateOf(editing?.picks.orEmpty()) }
    val field = if (editing != null) editValue else value
    val fieldPicks = if (editing != null) editPicks else picks
    /** Every change of the field passes here: deleting backwards behind a mention takes it as a whole. */
    fun setField(v: TextFieldValue) {
        val next = deleteWholeMention(field, v, fieldPicks)
        if (editing != null) editValue = next else value = next
    }
    val editText = editValue.text.trim()
    val canSave = editing != null && !saving && editText.isNotEmpty() && editText != editing.source.trim()
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
    // Back cancels the edit mode before it leaves the chat; an open panel closes first.
    BackHandler(enabled = editing != null, onBack = onCancelEdit)
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

    val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) {
            panel = InputPanel.NONE
            onFilesPicked(uris)
        }
    }

    val hasText = field.text.isNotBlank()

    // --- suggestions after an „@“ (docs/mentions.md „Vorschlagsliste“) -----------------
    var focused by remember { mutableStateOf(false) }
    val query = remember(field.text, field.selection, fieldPicks) { mentionQuery(field, fieldPicks) }
    var candidates by remember(token) { mutableStateOf(emptyList<MentionCandidate>()) }
    // The core answers from memory: asked on every change, a later question drops the earlier one.
    LaunchedEffect(token, query?.text) {
        val q = query?.text
        candidates = if (q == null) emptyList() else sessions.mentionCandidates(token, q)
    }
    // „Zurück“ closes the list for this „@“ until the text behind it changes.
    var dismissed by remember(token) { mutableStateOf<MentionQuery?>(null) }
    val suggesting = query != null && focused && candidates.isNotEmpty() && dismissed != query
    BackBeforeKeyboard(enabled = suggesting) { dismissed = query }
    fun pick(c: MentionCandidate) {
        val q = query ?: return
        val chosen = fieldPicks.filter { it.label != c.label } + MentionPick(c.label, c.mentionId)
        val next = acceptMention(field, q, c.label)
        // Closed for this „@“ as after „Zurück“: a longer name that starts alike („Lena Brandt“) does not open it again.
        dismissed = mentionQuery(next, chosen)
        if (editing != null) {
            editPicks = chosen
            editValue = next
        } else {
            picks = chosen
            value = next
        }
    }

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
    // Entering the edit mode: a running recording becomes its draft, the keyboard opens.
    val windowInfo = LocalWindowInfo.current
    LaunchedEffect(editing?.msg?.id) {
        if (editing == null) return@LaunchedEffect
        voice.interrupt()
        panel = InputPanel.NONE
        // The message menu is a window of its own: the keyboard opens once ours has the focus again.
        snapshotFlow { windowInfo.isWindowFocused }.first { it }
        focus.requestFocus()
        keyboard?.show()
    }

    Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
        if (suggesting) {
            // Over the message list, which keeps its place: the list takes no room of its own.
            Box(
                Modifier.layout { measurable, constraints ->
                    val placeable = measurable.measure(constraints.copy(minHeight = 0))
                    layout(placeable.width, 0) { placeable.place(0, -placeable.height) }
                },
            ) {
                MentionSuggestions(sessions, room, candidates, ::pick)
            }
        }
        if (editing != null) {
            ContextBar(
                title = "Nachricht bearbeiten",
                icon = Icons.Filled.Edit,
                // A file without caption shows its name.
                text = editing.msg.text.ifEmpty { editing.msg.attachment?.name.orEmpty() },
                closeLabel = "Bearbeiten abbrechen",
                onClose = onCancelEdit,
            )
        } else if (replyTo != null) {
            ContextBar(
                title = if (replyTo.isOwn) "Du" else replyTo.author,
                icon = null,
                text = withAttachment(replyTo.text, replyTo.attachment, replyTo.location),
                closeLabel = "Antwort abbrechen",
                onClose = onCancelReply,
            )
        }

        if (editing == null && rec?.locked == true) {
            LockedRecordingBar(
                rec,
                onCancel = {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    voice.cancel()
                },
                onSend = { voice.sendRecording(replyTo, ::voiceSent) },
            )
        } else if (editing == null && rec == null && draft != null) {
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
                if (rec != null && editing == null) {
                    HeldRecording(rec, drag.x, Modifier.weight(1f))
                } else {
                // docs/mentions.md „Farben“: the field's text colour at 12 %.
                val mentionMark = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.12f)
                PillIcon(
                    if (panel == InputPanel.EMOJI) TalkIcons.Keyboard else TalkIcons.Mood,
                    if (panel == InputPanel.EMOJI) "Tastatur" else "Emoji",
                    width = 48.dp,
                    onClick = { open(InputPanel.EMOJI) },
                )
                BasicTextField(
                    value = field,
                    onValueChange = { setField(it) },
                    textStyle = TextStyle(fontSize = 16.sp, lineHeight = 21.sp, color = MaterialTheme.colorScheme.onSurface),
                    cursorBrush = SolidColor(colors.accent),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    maxLines = 5,
                    visualTransformation = remember(fieldPicks, mentionMark) { MentionHighlight(fieldPicks, mentionMark) },
                    modifier = Modifier
                        .weight(1f)
                        .focusRequester(focus)
                        .onFocusChanged { focused = it.isFocused }
                        .heightIn(min = 44.dp)
                        .padding(top = 10.dp, bottom = 10.dp, end = if (hasText || editing != null) 12.dp else 0.dp),
                    decorationBox = { inner ->
                        Box {
                            if (field.text.isEmpty()) Text("Nachricht", color = colors.muted, fontSize = 16.sp, lineHeight = 21.sp)
                            inner()
                        }
                    },
                )
                AnimatedVisibility(visible = !hasText && editing == null, enter = fadeIn(), exit = fadeOut()) {
                    PillIcon(TalkIcons.Camera, "Kamera", width = 40.dp, endPadding = 16.dp, onClick = {
                        panel = InputPanel.NONE
                        onOpenCamera()
                    })
                }
                }
                // Same call site while holding, so the press gesture survives the swap above.
                AnimatedVisibility(visible = !hasText && editing == null, enter = fadeIn(), exit = fadeOut()) {
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
                    // The check mark is off while there is nothing to save.
                    .background(colors.accent.copy(alpha = if (editing != null && !canSave) 0.4f else 1f))
                    .clickable(enabled = editing == null || canSave) {
                        if (editing != null) {
                            onSaveEdit(editText, editPicks)
                        } else if (hasText) {
                            val text = value.text.trim()
                            val chosen = picks
                            value = TextFieldValue()
                            picks = emptyList()
                            onSend(text, chosen)
                        } else {
                            open(InputPanel.ATTACH)
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                AnimatedContent(
                    targetState = if (editing != null) SendButton.SAVE else if (hasText) SendButton.SEND else SendButton.ATTACH,
                    transitionSpec = { (scaleIn() + fadeIn()) togetherWith (scaleOut() + fadeOut()) },
                    label = "send",
                ) { button ->
                    when (button) {
                        SendButton.SAVE -> Icon(Icons.Filled.Check, "Änderung speichern", tint = Color.White, modifier = Modifier.size(22.dp))
                        SendButton.SEND -> Icon(TalkIcons.Send, "Senden", tint = Color.White, modifier = Modifier.size(22.dp))
                        SendButton.ATTACH -> Icon(TalkIcons.Plus, "Anhang", tint = Color.White, modifier = Modifier.size(22.dp).rotate(rotation))
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
                        onEmoji = { setField(field.insert(it)) },
                        onBackspace = { setField(field.backspace()) },
                    )
                    InputPanel.ATTACH -> AttachPanel(
                        onImage = { uri ->
                            panel = InputPanel.NONE
                            onImagePicked(uri)
                        },
                        onGallery = {
                            pickImage.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                        },
                        onFile = { pickFiles.launch(arrayOf("*/*")) },
                        onLocation = {
                            // Without the focus the keyboard stays away when the sheet closes.
                            focusManager.clearFocus()
                            onLocation { panel = InputPanel.NONE }
                        },
                        onContact = {
                            focusManager.clearFocus()
                            onContact { panel = InputPanel.NONE }
                        },
                    )
                    InputPanel.NONE -> Box(Modifier.fillMaxSize())
                }
            }
        }
    }
}

/** What the round button next to the input pill does. */
private enum class SendButton { ATTACH, SEND, SAVE }

/** The bar above the input row: what is being answered, or the message being edited. */
@Composable
private fun ContextBar(title: String, icon: ImageVector?, text: String, closeLabel: String, onClose: () -> Unit) {
    val colors = LocalTalkColors.current
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
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (icon != null) {
                    Icon(icon, null, tint = colors.accent, modifier = Modifier.size(14.dp))
                    Spacer(Modifier.width(6.dp))
                }
                Text(title, color = colors.accent, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
            }
            Text(text, maxLines = 1, overflow = TextOverflow.Ellipsis, fontSize = 13.sp)
        }
        IconButton(onClick = onClose) { Icon(TalkIcons.Close, closeLabel) }
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
    val dark = isDarkTheme()
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
