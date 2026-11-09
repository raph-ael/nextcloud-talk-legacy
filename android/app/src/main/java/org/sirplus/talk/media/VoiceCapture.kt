package org.sirplus.talk.media

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioRecordingConfiguration
import android.media.MediaRecorder
import android.os.Build
import android.os.Handler
import android.os.Looper
import uniffi.talk_ffi.CoreException
import uniffi.talk_ffi.RecordingStatus
import uniffi.talk_ffi.VoiceRecording
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Microphone capture for a voice message: 16-bit mono PCM at [RATE] from [AudioRecord] on
 * its own thread, handed to the core every 100 ms. The core encodes MP3 and appends it to
 * the file right away, so a crash or kill loses at most the last block.
 *
 * Like Signal, the recording ends (and stays as draft) when another app takes audio focus
 * or the microphone (a call), see [onInterrupted].
 */
class VoiceCapture(
    context: Context,
    private val recording: VoiceRecording,
    /** On the capture thread, every 100 ms. */
    private val onStatus: (RecordingStatus) -> Unit,
    /** On the capture or main thread: stop and keep the draft; the text says why. */
    private val onInterrupted: (String) -> Unit,
) {
    companion object {
        /** The only rate every Android device supports. */
        const val RATE = 44_100
        private const val BLOCK = RATE / 10
    }

    private val audio = context.getSystemService(AudioManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private var record: AudioRecord? = null
    private var thread: Thread? = null
    @Volatile private var running = false
    private var focus: AudioFocusRequest? = null
    private var silencedCallback: AudioManager.AudioRecordingCallback? = null

    /** The microphone is taken by a call. */
    fun micBusy(): Boolean =
        audio.mode == AudioManager.MODE_IN_CALL || audio.mode == AudioManager.MODE_IN_COMMUNICATION

    /** Starts capturing; `false` when the microphone cannot be opened. Needs RECORD_AUDIO. */
    @SuppressLint("MissingPermission")
    fun start(): Boolean {
        val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        if (min <= 0) return false
        val rec = try {
            AudioRecord(
                MediaRecorder.AudioSource.MIC,
                RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                maxOf(min, BLOCK * 2 * 4),
            )
        } catch (e: Exception) {
            return false
        }
        if (rec.state != AudioRecord.STATE_INITIALIZED) {
            rec.release()
            return false
        }
        // Other audio pauses; losing focus (a call, another recorder) ends the recording.
        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setOnAudioFocusChangeListener({ change ->
                if (change == AudioManager.AUDIOFOCUS_LOSS || change == AudioManager.AUDIOFOCUS_LOSS_TRANSIENT) {
                    onInterrupted("Aufnahme unterbrochen – als Entwurf gespeichert.")
                }
            }, main)
            .build()
        audio.requestAudioFocus(request)
        focus = request
        try {
            rec.startRecording()
        } catch (e: IllegalStateException) {
            rec.release()
            abandonFocus()
            return false
        }
        if (rec.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
            rec.release()
            abandonFocus()
            return false
        }
        // Android 10+: another app took the microphone, we would only record silence.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val session = rec.audioSessionId
            val cb = object : AudioManager.AudioRecordingCallback() {
                override fun onRecordingConfigChanged(configs: MutableList<AudioRecordingConfiguration>) {
                    if (configs.any { it.clientAudioSessionId == session && it.isClientSilenced }) {
                        onInterrupted("Das Mikrofon wird von einer anderen App benutzt – Aufnahme als Entwurf gespeichert.")
                    }
                }
            }
            audio.registerAudioRecordingCallback(cb, main)
            silencedCallback = cb
        }
        record = rec
        running = true
        thread = Thread(::loop, "voice-capture").apply {
            priority = Thread.MAX_PRIORITY
            start()
        }
        return true
    }

    private fun loop() {
        val rec = record ?: return
        val block = ShortArray(BLOCK)
        val bytes = ByteBuffer.allocate(BLOCK * 2).order(ByteOrder.LITTLE_ENDIAN)
        while (running) {
            var n = 0
            while (n < BLOCK && running) {
                val r = rec.read(block, n, BLOCK - n)
                if (r < 0) {
                    if (running) onInterrupted("Mikrofon-Fehler ($r) – Aufnahme als Entwurf gespeichert.")
                    running = false
                    break
                }
                n += r
            }
            if (n == 0) continue
            bytes.clear()
            bytes.asShortBuffer().put(block, 0, n)
            val status = try {
                recording.push(bytes.array().copyOf(n * 2))
            } catch (e: CoreException) {
                // Disk full or similar: what is on disk stays.
                onInterrupted("Aufnahme gestoppt: ${e.message}")
                break
            }
            onStatus(status)
            if (status.limitReached) {
                onInterrupted("Sprachnachrichten sind auf 60 Minuten begrenzt. Die Aufnahme liegt als Entwurf bereit.")
                break
            }
        }
    }

    /** Stops the microphone; afterwards the recording can be finished or cancelled. */
    fun stop() {
        running = false
        val t = thread
        if (t != null && t !== Thread.currentThread()) t.join(1500)
        thread = null
        record?.let {
            runCatching { it.stop() }
            it.release()
        }
        record = null
        silencedCallback?.let { audio.unregisterAudioRecordingCallback(it) }
        silencedCallback = null
        abandonFocus()
    }

    private fun abandonFocus() {
        focus?.let { audio.abandonAudioFocusRequest(it) }
        focus = null
    }
}
