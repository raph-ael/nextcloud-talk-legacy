package org.sirplus.talk.call

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.telephony.PhoneStateListener
import android.telephony.TelephonyCallback
import android.telephony.TelephonyManager
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import uniffi.talk_ffi.CallDirection
import uniffi.talk_ffi.CallState
import uniffi.talk_ffi.CallView
import uniffi.talk_ffi.EndReason

/** What the call screen and the in-app bar need besides the [CallView]. */
interface CallActions {
    fun accept(token: String)
    fun decline(token: String)
    fun hangUp()
    fun setMuted(muted: Boolean)
}

/**
 * Turns the core's call states into everything around the media on Android: ringing,
 * ringback and short tones, notifications, the foreground service, call audio and the
 * call screen. The core makes all protocol decisions; this only follows `CallView`.
 * Main thread, except the [CallAudioHost] entry points.
 */
class CallController(
    private val context: Context,
    private val scope: CoroutineScope,
    private val actions: CallActions,
    /** Stops voice message playback and recording (rule 12). */
    private val stopVoice: () -> Unit,
) : CallAudioHost {
    val audio = CallAudio(context)
    private val tones = CallTones(context)
    private val main = Handler(Looper.getMainLooper())

    private val _ui = MutableStateFlow<CallView?>(null)
    /** The call to show; an ended call stays ~1.5 s with its end reason, then `null`. */
    val ui: StateFlow<CallView?> = _ui

    private var last: CallView? = null
    private var endHold: Job? = null
    private var audioActive = false
    /** The service runs only to keep the process awake while ringing. */
    private var ringService = false
    private var phoneMuted = false
    private var telephonyCallback: Any? = null

    init {
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStop(owner: LifecycleOwner) {
                // Ringing while the app goes to the background: the notification takes over.
                last?.takeIf { it.state == CallState.INCOMING }?.let { ringInBackground(it) }
            }
        })
    }

    // --- core events --------------------------------------------------------------

    fun onCall(call: CallView?) {
        val prev = last
        last = call
        if (call == null) {
            tones.stopAll()
            stopRingService()
            CallNotifications.cancelIncoming(context)
            if (_ui.value?.state != CallState.ENDED) _ui.value = null
            if (audioActive) {
                Log.w(TAG, "call gone without stopAudio: releasing audio")
                releaseAudio()
            }
            return
        }
        if (call.state == CallState.ENDED) {
            if (prev?.state != CallState.ENDED) onEnded(call, prev)
            return
        }
        endHold?.cancel()
        _ui.value = call

        if (call.state == CallState.INCOMING) {
            if (prev?.id != call.id || prev.state != CallState.INCOMING) onIncoming(call)
        } else {
            tones.stopRinging()
            stopRingService()
            CallNotifications.cancelIncoming(context)
        }
        if (call.state == CallState.RINGING && !call.isGroup && call.direction == CallDirection.OUTGOING) {
            tones.startRingback()
        } else {
            tones.stopRingback()
        }
        if (call.state == CallState.CONNECTED && prev?.state != CallState.CONNECTED && prev?.state != CallState.RECONNECTING) {
            tones.playConnected()
        }
        audio.setInCall(call.state == CallState.CONNECTING || call.state == CallState.CONNECTED || call.state == CallState.RECONNECTING)
        if (audioActive) CallNotifications.updateOngoing(context, call)
    }

    private fun onIncoming(call: CallView) {
        tones.startRinging()
        if (ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) {
            context.startActivity(
                Intent(context, CallActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            )
        } else {
            ringInBackground(call)
        }
    }

    private fun ringInBackground(call: CallView) {
        CallNotifications.showIncoming(context, call)
        if (!audioActive && !ringService) ringService = CallService.startRinging(context)
    }

    private fun stopRingService() {
        if (!ringService) return
        ringService = false
        if (!audioActive) CallService.stop(context)
    }

    private fun onEnded(call: CallView, prev: CallView?) {
        tones.stopAll()
        stopRingService()
        CallNotifications.cancelIncoming(context)
        val reason = call.endReason
        val rangHere = prev?.state == CallState.INCOMING
        // Missed calls are announced by the core's CallMissed event.
        val silentEnd = reason is EndReason.AnsweredElsewhere || reason is EndReason.Declined || reason is EndReason.Missed
        if (!rangHere && !silentEnd) tones.playEnded()
        _ui.value = call
        endHold?.cancel()
        endHold = scope.launch {
            delay(END_HOLD_MS)
            if (_ui.value?.id == call.id && _ui.value?.state == CallState.ENDED) _ui.value = null
        }
    }

    // --- user actions -------------------------------------------------------------

    fun accept(token: String) {
        tones.stopRinging()
        CallNotifications.cancelIncoming(context)
        actions.accept(token)
    }

    fun decline(token: String) {
        tones.stopRinging()
        CallNotifications.cancelIncoming(context)
        actions.decline(token)
    }

    fun hangUp() = actions.hangUp()

    fun setMuted(muted: Boolean) {
        phoneMuted = false
        actions.setMuted(muted)
    }

    // --- CallAudioHost (core thread) ----------------------------------------------

    override fun startAudio(callId: String) {
        main.post {
            if (audioActive) return@post
            Log.i(TAG, "startAudio $callId")
            audioActive = true
            ringService = false
            stopVoice()
            tones.stopRinging()
            audio.start()
            CallService.start(context)
            watchPhoneCalls(true)
        }
    }

    override fun stopAudio(callId: String) {
        main.post {
            if (!audioActive) return@post
            Log.i(TAG, "stopAudio $callId")
            releaseAudio()
        }
    }

    private fun releaseAudio() {
        audioActive = false
        watchPhoneCalls(false)
        tones.stopRingback()
        audio.stop()
        CallService.stop(context)
    }

    // --- GSM call during a Talk call: mute, unmute afterwards ---------------------

    private fun onPhoneState(state: Int) = main.post {
        val call = last ?: return@post
        when (state) {
            TelephonyManager.CALL_STATE_OFFHOOK -> if (!call.muted) {
                Log.i(TAG, "phone call taken: muting")
                phoneMuted = true
                actions.setMuted(true)
            }
            TelephonyManager.CALL_STATE_IDLE -> if (phoneMuted) {
                phoneMuted = false
                actions.setMuted(false)
            }
        }
    }

    private fun watchPhoneCalls(on: Boolean) {
        val tm = context.getSystemService(TelephonyManager::class.java) ?: return
        if (!on) {
            when (val cb = telephonyCallback) {
                null -> Unit
                is PhoneStateListener -> @Suppress("DEPRECATION") tm.listen(cb, PhoneStateListener.LISTEN_NONE)
                else -> if (Build.VERSION.SDK_INT >= 31) tm.unregisterTelephonyCallback(cb as TelephonyCallback)
            }
            telephonyCallback = null
            phoneMuted = false
            return
        }
        if (telephonyCallback != null) return
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.READ_PHONE_STATE) != PackageManager.PERMISSION_GRANTED) return
        try {
            if (Build.VERSION.SDK_INT >= 31) {
                val cb = object : TelephonyCallback(), TelephonyCallback.CallStateListener {
                    override fun onCallStateChanged(state: Int) { onPhoneState(state) }
                }
                tm.registerTelephonyCallback(context.mainExecutor, cb)
                telephonyCallback = cb
            } else {
                @Suppress("DEPRECATION")
                val l = object : PhoneStateListener() {
                    @Deprecated("Deprecated in Java")
                    override fun onCallStateChanged(state: Int, phoneNumber: String?) { onPhoneState(state) }
                }
                @Suppress("DEPRECATION")
                tm.listen(l, PhoneStateListener.LISTEN_CALL_STATE)
                telephonyCallback = l
            }
        } catch (e: SecurityException) {
            Log.w(TAG, "phone state not readable", e)
        }
    }

    private companion object {
        const val TAG = "TalkCall"
        const val END_HOLD_MS = 1500L
    }
}
