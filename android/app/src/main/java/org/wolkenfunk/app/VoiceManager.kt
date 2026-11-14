package org.wolkenfunk.app

import android.content.Context
import android.widget.Toast
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.wolkenfunk.app.media.VoiceCapture
import org.wolkenfunk.app.media.VoicePlayer
import uniffi.talk_ffi.AudioInfo
import uniffi.talk_ffi.CoreException
import uniffi.talk_ffi.MessageView
import uniffi.talk_ffi.ReplyPreview
import uniffi.talk_ffi.TalkSession
import uniffi.talk_ffi.VoiceDraft
import uniffi.talk_ffi.VoiceRecording
import uniffi.talk_ffi.newReferenceId
import uniffi.talk_ffi.voiceMaxDurationMs
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap

/** A recording in progress. [locked]: hands-free (slid up), else the finger holds the mic. */
data class RecordingUi(
    val token: String,
    val durationMs: Long = 0,
    /** Last levels 0..1, oldest first. */
    val levels: List<Float> = emptyList(),
    val locked: Boolean = false,
    /** Close to the one-hour limit. */
    val warn: Boolean = false,
)

data class VoiceUi(
    val recording: RecordingUi? = null,
    /** Unsent recording per conversation (absent: not loaded yet). */
    val drafts: Map<String, VoiceDraft?> = emptyMap(),
)

/**
 * Voice messages: recording (Signal's hold / slide-to-cancel / slide-up-to-lock flow),
 * drafts that survive interruptions and restarts, and playback.
 *
 * Like Signal, a recording stops and stays as draft when the app goes to the background
 * (screen off, app switch), another app takes the audio focus or the microphone, or one
 * hour is reached; the screen stays on while recording.
 */
class VoiceManager(
    private val context: Context,
    private val scope: CoroutineScope,
    private val session: () -> TalkSession?,
    private val chats: () -> UiState,
) {
    companion object {
        private const val LEVEL_BARS = 40
        private const val WARN_BEFORE_MS = 5 * 60 * 1000L
        /** Free space needed to start (an hour is ~29 MB). */
        private const val MIN_FREE_BYTES = 40L * 1024 * 1024
        /** Received voice messages up to this size are fetched right away (waveform). */
        const val AUTO_FETCH_BYTES = 15L * 1024 * 1024
    }

    private val _state = MutableStateFlow(VoiceUi())
    val state: StateFlow<VoiceUi> = _state

    val player = VoicePlayer(context, scope).also { p -> p.onEnded = { key -> playNextAfter(key) } }

    private var recording: VoiceRecording? = null
    private var capture: VoiceCapture? = null
    private val maxMs = voiceMaxDurationMs().toLong()
    private val infos = ConcurrentHashMap<String, AudioInfo>()

    private fun toast(text: String) = Toast.makeText(context, text, Toast.LENGTH_LONG).show()

    // --- recording ----------------------------------------------------------------

    /** Starts recording for [token] (RECORD_AUDIO must be granted). `false`: not started. */
    fun start(token: String): Boolean {
        if (recording != null) return false
        val s = session() ?: return false
        if (context.noBackupFilesDir.usableSpace < MIN_FREE_BYTES) {
            toast("Zu wenig freier Speicher für eine Aufnahme.")
            return false
        }
        player.stop()
        val rec = try {
            s.startVoice(token, VoiceCapture.RATE.toUInt())
        } catch (e: CoreException) {
            toast("Aufnahme nicht möglich: ${e.message}")
            return false
        }
        val cap = VoiceCapture(
            context,
            rec,
            onStatus = { st ->
                val ms = st.durationMs.toLong()
                _state.update { v ->
                    val r = v.recording ?: return@update v
                    v.copy(
                        recording = r.copy(
                            durationMs = ms,
                            levels = (r.levels + st.level).takeLast(LEVEL_BARS),
                            warn = ms >= maxMs - WARN_BEFORE_MS,
                        ),
                    )
                }
            },
            onInterrupted = { why -> scope.launch { stopToDraft(why) } },
        )
        if (cap.micBusy()) {
            rec.cancel()
            rec.close()
            toast("Während eines Anrufs kann nicht aufgenommen werden.")
            return false
        }
        if (!cap.start()) {
            rec.cancel()
            rec.close()
            toast("Das Mikrofon lässt sich nicht öffnen.")
            return false
        }
        recording = rec
        capture = cap
        _state.update { it.copy(recording = RecordingUi(token)) }
        return true
    }

    fun lock() {
        _state.update { v -> v.copy(recording = v.recording?.copy(locked = true)) }
    }

    /** Ends the recording as the chat's draft. Returns it (`null`: shorter than a second). */
    suspend fun stopToDraft(notice: String? = null): VoiceDraft? {
        val rec = recording ?: return null
        val token = _state.value.recording?.token ?: return null
        val cap = capture
        recording = null
        capture = null
        val draft = withContext(Dispatchers.IO) {
            cap?.stop()
            val d = runCatching { rec.finish() }.getOrNull()
            rec.close()
            d
        }
        _state.update { it.copy(recording = null, drafts = it.drafts + (token to draft)) }
        if (notice != null && draft != null) toast(notice)
        return draft
    }

    /** Fire-and-forget [stopToDraft] (app paused, chat left). */
    fun interrupt() {
        if (recording != null) scope.launch { stopToDraft() }
    }

    fun cancel() {
        val rec = recording ?: return
        val cap = capture
        recording = null
        capture = null
        _state.update { it.copy(recording = null) }
        scope.launch(Dispatchers.IO) {
            cap?.stop()
            rec.cancel()
            rec.close()
        }
    }

    /** Released finger or send button of the locked bar: stop and send. */
    fun sendRecording(replyTo: MessageView?, onSent: () -> Unit) {
        val token = _state.value.recording?.token ?: return
        scope.launch {
            val draft = stopToDraft()
            if (draft == null) toast("Zum Aufnehmen gedrückt halten")
            else sendDraft(token, replyTo, onSent)
        }
    }

    fun loadDraft(token: String) {
        if (_state.value.recording?.token == token) return
        val s = session() ?: return
        scope.launch {
            val d = withContext(Dispatchers.IO) { runCatching { s.voiceDraft(token) }.getOrNull() }
            _state.update { it.copy(drafts = it.drafts + (token to d)) }
        }
    }

    fun discardDraft(token: String) {
        val d = _state.value.drafts[token]
        if (d != null) player.stop(draftKey(d))
        _state.update { it.copy(drafts = it.drafts + (token to null)) }
        val s = session() ?: return
        scope.launch(Dispatchers.IO) { s.discardVoiceDraft(token) }
    }

    fun sendDraft(token: String, replyTo: MessageView?, onSent: () -> Unit = {}) {
        val s = session() ?: return
        _state.value.drafts[token]?.let { player.stop(draftKey(it)) }
        val name = "Sprachnachricht " + SimpleDateFormat("yyyy-MM-dd HH-mm-ss", Locale.ROOT).format(Date())
        scope.launch {
            val sent = withContext(Dispatchers.IO) {
                runCatching { s.sendVoiceDraft(token, name, replyTo?.toReply(), newReferenceId()) }
            }
            if (sent.isFailure) {
                toast("Senden nicht möglich: ${sent.exceptionOrNull()?.message}")
                return@launch
            }
            _state.update { it.copy(drafts = it.drafts + (token to null)) }
            OutboxWorker.schedule(context)
            onSent()
        }
    }

    private fun MessageView.toReply() = ReplyPreview(id, author, text, isOwn, attachment)

    // --- playback -----------------------------------------------------------------

    fun draftKey(d: VoiceDraft) = "draft:${d.id}"

    fun messageKey(m: UiMessage) = if (m.status != null) "ref:${m.msg.referenceId}" else "id:${m.msg.id}"

    /** Local file of a message's audio: the outbox copy while pending, else downloaded once. */
    suspend fun localFile(m: UiMessage): String? {
        val a = m.msg.attachment ?: return null
        m.localFile?.takeIf { m.status != null || a.id.isEmpty() }?.let { return it }
        val s = session() ?: return null
        return try {
            s.fetchFile(a.path, a.size)
        } catch (e: CoreException) {
            null
        }
    }

    /** Duration and waveform (cached in memory; the core caches on disk). */
    suspend fun info(key: String, path: String): AudioInfo? {
        infos[key]?.let { return it }
        val s = session() ?: return null
        val i = try {
            s.audioInfo(path)
        } catch (e: CoreException) {
            return null
        }
        infos[key] = i
        return i
    }

    fun cachedInfo(key: String): AudioInfo? = infos[key]

    fun toggle(m: UiMessage) {
        player.toggle(messageKey(m), "Sprachnachricht", m.msg.author.ifEmpty { "Du" }) { localFile(m) }
    }

    fun seek(m: UiMessage, fraction: Float) {
        player.seek(messageKey(m), fraction, "Sprachnachricht", m.msg.author.ifEmpty { "Du" }) { localFile(m) }
    }

    fun toggleDraft(d: VoiceDraft) = player.toggle(draftKey(d), "Entwurf", "Sprachnachricht") { d.path }

    fun seekDraft(d: VoiceDraft, fraction: Float) =
        player.seek(draftKey(d), fraction, "Entwurf", "Sprachnachricht") { d.path }

    /** After a voice message ends, the next newer one of the open chat follows. */
    private fun playNextAfter(key: String) {
        val st = chats()
        val token = st.activeToken ?: return
        val messages = st.chats[token]?.messages ?: return
        val i = messages.indexOfFirst { messageKey(it) == key }
        if (i < 0) return
        val next = messages.drop(i + 1).firstOrNull { it.status == null && it.msg.attachment?.voiceMessage == true } ?: return
        toggle(next)
    }
}
