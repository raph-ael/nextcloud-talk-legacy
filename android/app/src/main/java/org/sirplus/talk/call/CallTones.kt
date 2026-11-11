package org.sirplus.talk.call

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.os.Build
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Log
import kotlin.math.PI
import kotlin.math.exp
import kotlin.math.min
import kotlin.math.sin

/**
 * Call sounds, all synthesized here (no recorded or third-party sounds): the ringtone with
 * vibration for incoming calls, the 425 Hz ringback tone (1 s on / 4 s off) while it rings
 * on the other side, and short connect / hang-up tones. Main thread only.
 */
class CallTones(private val context: Context) {
    private val audio = context.getSystemService(AudioManager::class.java)
    private val vibrator: Vibrator? = if (Build.VERSION.SDK_INT >= 31) {
        context.getSystemService(VibratorManager::class.java)?.defaultVibrator
    } else {
        @Suppress("DEPRECATION")
        context.getSystemService(Vibrator::class.java)
    }

    private var ring: AudioTrack? = null
    private var ringback: AudioTrack? = null
    private var vibrating = false

    /** Incoming call: ringtone and vibration according to the ringer mode. */
    fun startRinging() {
        if (ring != null || vibrating) return
        val mode = audio.ringerMode
        if (mode == AudioManager.RINGER_MODE_NORMAL) {
            ring = loop(ringtone(), AudioAttributes.USAGE_NOTIFICATION_RINGTONE, AudioAttributes.CONTENT_TYPE_SONIFICATION, RING_LOOPS)
        }
        if (mode != AudioManager.RINGER_MODE_SILENT) vibrate()
    }

    fun stopRinging() {
        ring?.release()
        ring = null
        if (vibrating) vibrator?.cancel()
        vibrating = false
    }

    /** Outgoing 1:1 call rings on the other side. */
    fun startRingback() {
        if (ringback != null) return
        ringback = loop(ringbackTone(), AudioAttributes.USAGE_VOICE_COMMUNICATION, AudioAttributes.CONTENT_TYPE_SPEECH, RINGBACK_LOOPS)
    }

    fun stopRingback() {
        ringback?.release()
        ringback = null
    }

    /** Short rising tone when the call is connected. */
    fun playConnected() = once(sweep(listOf(660.0 to 0.09, 880.0 to 0.12)))

    /** Short falling tone when the call ends. */
    fun playEnded() = once(sweep(listOf(660.0 to 0.12, 440.0 to 0.12, 330.0 to 0.18)))

    /** Quiet two-note chime: this call is being recorded. */
    fun playRecordingNotice() = once(recordingChime())

    fun stopAll() {
        stopRinging()
        stopRingback()
    }

    // --- playback -----------------------------------------------------------------

    private fun track(pcm: ShortArray, usage: Int, content: Int): AudioTrack? = try {
        AudioTrack.Builder()
            .setAudioAttributes(AudioAttributes.Builder().setUsage(usage).setContentType(content).build())
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(RATE)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build(),
            )
            .setTransferMode(AudioTrack.MODE_STATIC)
            .setBufferSizeInBytes(pcm.size * 2)
            .build()
            .also { it.write(pcm, 0, pcm.size) }
    } catch (e: Exception) {
        Log.w(TAG, "tone not playable", e)
        null
    }

    /**
     * Repeats [pcm] [loops] more times. Never endless: if the process is frozen in the
     * background, the system keeps playing a looping track on its own.
     */
    private fun loop(pcm: ShortArray, usage: Int, content: Int, loops: Int): AudioTrack? =
        track(pcm, usage, content)?.apply {
            setLoopPoints(0, pcm.size, loops)
            play()
        }

    private fun once(pcm: ShortArray) {
        val t = track(pcm, AudioAttributes.USAGE_VOICE_COMMUNICATION, AudioAttributes.CONTENT_TYPE_SONIFICATION) ?: return
        t.setNotificationMarkerPosition(pcm.size - 1)
        t.setPlaybackPositionUpdateListener(object : AudioTrack.OnPlaybackPositionUpdateListener {
            override fun onMarkerReached(track: AudioTrack) = track.release()
            override fun onPeriodicNotification(track: AudioTrack) = Unit
        })
        t.play()
    }

    private fun vibrate() {
        val v = vibrator ?: return
        if (!v.hasVibrator()) return
        // 1 s on / 1 s off for about a minute, not endlessly (see [loop]).
        val pattern = LongArray(1 + 2 * VIBRATE_CYCLES) { if (it == 0) 0L else 1000L }
        val effect = VibrationEffect.createWaveform(pattern, -1)
        if (Build.VERSION.SDK_INT >= 33) {
            v.vibrate(effect, VibrationAttributes.createForUsage(VibrationAttributes.USAGE_RINGTONE))
        } else {
            @Suppress("DEPRECATION")
            v.vibrate(effect, AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE).build())
        }
        vibrating = true
    }

    // --- synthesis ----------------------------------------------------------------

    private companion object {
        const val TAG = "TalkTones"
        const val RATE = 24_000
        /** Ringtone loop is 3 s: ~63 s in total. */
        const val RING_LOOPS = 20
        /** Ringback loop is 5 s: ~65 s in total (the core gives up after 60 s). */
        const val RINGBACK_LOOPS = 12
        const val VIBRATE_CYCLES = 32

        /** Telephone ringback: 425 Hz, 1 s on, 4 s off (soft edges against clicks). */
        fun ringbackTone(): ShortArray {
            val out = ShortArray(RATE * 5)
            val on = RATE
            val fade = RATE / 100
            for (i in 0 until on) {
                val env = min(1.0, min(i, on - i).toDouble() / fade)
                out[i] = (sin(2 * PI * 425 * i / RATE) * env * 0.35 * Short.MAX_VALUE).toInt().toShort()
            }
            return out
        }

        /**
         * Ringtone: a bell-like phrase of four struck notes (sine partials with exponential
         * decay), twice, then a pause; 3 s per loop.
         */
        fun ringtone(): ShortArray {
            val out = DoubleArray(RATE * 3)
            val notes = listOf(0.0 to 784.0, 0.18 to 988.0, 0.36 to 1175.0, 0.54 to 988.0)
            for (rep in 0..1) {
                val base = rep * 0.95
                for ((at, f) in notes) strike(out, base + at, f, 0.9)
            }
            return normalize(out, 0.8)
        }

        fun strike(out: DoubleArray, atSec: Double, freq: Double, lenSec: Double) {
            val start = (atSec * RATE).toInt()
            val len = (lenSec * RATE).toInt()
            for (i in 0 until len) {
                val idx = start + i
                if (idx >= out.size) break
                val t = i.toDouble() / RATE
                val attack = min(1.0, i / (RATE * 0.004))
                val v = sin(2 * PI * freq * t) * exp(-t * 5.0) +
                    0.35 * sin(2 * PI * freq * 2.0 * t) * exp(-t * 9.0) +
                    0.12 * sin(2 * PI * freq * 3.01 * t) * exp(-t * 14.0)
                out[idx] += v * attack
            }
        }

        /** Two soft struck notes (C6, E6), well below the other tones. */
        fun recordingChime(): ShortArray {
            val out = DoubleArray((RATE * 0.9).toInt())
            strike(out, 0.0, 1047.0, 0.5)
            strike(out, 0.22, 1319.0, 0.65)
            return normalize(out, 0.12)
        }

        /** Consecutive short sine tones (frequency to seconds). */
        fun sweep(parts: List<Pair<Double, Double>>): ShortArray {
            val total = parts.sumOf { (it.second * RATE).toInt() }
            val out = DoubleArray(total)
            var pos = 0
            val fade = RATE / 200
            for ((f, sec) in parts) {
                val n = (sec * RATE).toInt()
                for (i in 0 until n) {
                    val env = min(1.0, min(i, n - i).toDouble() / fade)
                    out[pos + i] = sin(2 * PI * f * i / RATE) * env
                }
                pos += n
            }
            return normalize(out, 0.3)
        }

        fun normalize(x: DoubleArray, peak: Double): ShortArray {
            val max = x.maxOf { kotlin.math.abs(it) }.takeIf { it > 0 } ?: 1.0
            return ShortArray(x.size) { (x[it] / max * peak * Short.MAX_VALUE).toInt().toShort() }
        }
    }
}
