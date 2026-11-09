package org.sirplus.talk.call

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** Per-device call settings. */
class CallPrefs(context: Context) {
    private val prefs = context.getSharedPreferences("call", Context.MODE_PRIVATE)

    /** Hardware echo cancellation / noise suppression (off: WebRTC's software AEC/NS). */
    var hardwareAec: Boolean
        get() = prefs.getBoolean("hardware_aec", true)
        set(v) = prefs.edit().putBoolean("hardware_aec", v).apply()
}

enum class AudioRoute { EARPIECE, SPEAKER, WIRED, BLUETOOTH }

/** An output the user can pick. [id]: AudioDeviceInfo id (API 31+), else the route's ordinal. */
data class AudioOutput(val id: Int, val route: AudioRoute, val name: String)

data class AudioRouting(
    val current: AudioRoute = AudioRoute.EARPIECE,
    val currentId: Int = -1,
    val outputs: List<AudioOutput> = emptyList(),
) {
    /** More than earpiece + speaker: the output button opens a picker instead of toggling. */
    val needsPicker: Boolean get() = outputs.any { it.route == AudioRoute.BLUETOOTH || it.route == AudioRoute.WIRED }
}

/**
 * Call audio like Signal: MODE_IN_COMMUNICATION, the communication device chosen by
 * priority (Bluetooth > wired > earpiece; speaker by button), the user's choice re-applied
 * when the system switches on its own, exclusive transient focus, proximity and Wi-Fi
 * locks, and the previous state restored afterwards. Main thread only.
 */
class CallAudio(private val context: Context) {
    private val audio = context.getSystemService(AudioManager::class.java)
    private val power = context.getSystemService(PowerManager::class.java)
    private val wifi = context.applicationContext.getSystemService(WifiManager::class.java)
    private val main = Handler(Looper.getMainLooper())

    private val _routing = MutableStateFlow(AudioRouting())
    val routing: StateFlow<AudioRouting> = _routing

    private var active = false
    /** Ringing (incoming) before accepting: no proximity lock yet. */
    private var inCall = false
    /** What the user picked; `null`: automatic by priority. */
    private var chosen: AudioOutput? = null

    private var savedMode = AudioManager.MODE_NORMAL
    private var savedSpeaker = false
    private var savedMicMute = false
    private var focus: AudioFocusRequest? = null
    private var proximity: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var reapplyCount = 0

    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) {
            if (!active) return
            // A headset plugged in or connected during the call takes over (like Signal).
            if (added.any { routeOf(it.type).let { r -> r == AudioRoute.BLUETOOTH || r == AudioRoute.WIRED } }) chosen = null
            main.postDelayed({ apply() }, 300)
        }

        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) {
            if (!active) return
            if (chosen != null && removed.any { matches(it, chosen!!) }) chosen = null
            main.post { apply() }
        }
    }

    private val commListener: Any? = if (Build.VERSION.SDK_INT >= 31) {
        AudioManager.OnCommunicationDeviceChangedListener { device -> onSystemSwitched(device) }
    } else {
        null
    }

    @SuppressLint("WakelockTimeout")
    fun start() {
        if (active) return
        active = true
        chosen = null
        reapplyCount = 0
        savedMode = audio.mode
        @Suppress("DEPRECATION")
        savedSpeaker = audio.isSpeakerphoneOn
        savedMicMute = audio.isMicrophoneMute

        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setAcceptsDelayedFocusGain(false)
            .setOnAudioFocusChangeListener { change -> Log.i(TAG, "audio focus: $change") }
            .build()
        focus = request
        audio.requestAudioFocus(request)
        audio.mode = AudioManager.MODE_IN_COMMUNICATION
        audio.isMicrophoneMute = false

        audio.registerAudioDeviceCallback(deviceCallback, main)
        if (Build.VERSION.SDK_INT >= 31) {
            audio.addOnCommunicationDeviceChangedListener(context.mainExecutor, commListener as AudioManager.OnCommunicationDeviceChangedListener)
        }

        wifiLock = wifi?.createWifiLock(
            if (Build.VERSION.SDK_INT >= 29) WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            else @Suppress("DEPRECATION") WifiManager.WIFI_MODE_FULL_HIGH_PERF,
            "talk:call",
        )?.apply {
            setReferenceCounted(false)
            acquire()
        }
        apply()
        Log.i(TAG, "call audio started (previous mode $savedMode)")
    }

    /** The call is up (not just dialing/ringing): proximity lock on the earpiece. */
    fun setInCall(value: Boolean) {
        inCall = value
        updateProximity()
    }

    fun stop() {
        if (!active) return
        active = false
        inCall = false
        audio.unregisterAudioDeviceCallback(deviceCallback)
        if (Build.VERSION.SDK_INT >= 31) {
            audio.removeOnCommunicationDeviceChangedListener(commListener as AudioManager.OnCommunicationDeviceChangedListener)
            audio.clearCommunicationDevice()
        } else {
            @Suppress("DEPRECATION")
            if (audio.isBluetoothScoOn) {
                audio.stopBluetoothSco()
                audio.isBluetoothScoOn = false
            }
            @Suppress("DEPRECATION")
            audio.isSpeakerphoneOn = savedSpeaker
        }
        audio.isMicrophoneMute = savedMicMute
        audio.mode = savedMode
        focus?.let { audio.abandonAudioFocusRequest(it) }
        focus = null
        releaseProximity()
        wifiLock?.takeIf { it.isHeld }?.release()
        wifiLock = null
        _routing.value = AudioRouting()
        Log.i(TAG, "call audio stopped, mode restored to $savedMode")
    }

    /** User picked an output in the sheet (or toggled the speaker). */
    fun select(output: AudioOutput) {
        if (!active) return
        chosen = output
        reapplyCount = 0
        apply()
    }

    /** Speaker on/off (the button when no headset is connected). */
    fun toggleSpeaker() {
        val r = _routing.value
        val target = if (r.current == AudioRoute.SPEAKER) {
            best(r.outputs.filter { it.route != AudioRoute.SPEAKER })
        } else {
            r.outputs.firstOrNull { it.route == AudioRoute.SPEAKER }
        } ?: return
        select(target)
    }

    // --- routing ------------------------------------------------------------------

    private fun outputs(): List<AudioOutput> {
        if (Build.VERSION.SDK_INT >= 31) {
            return audio.availableCommunicationDevices
                .mapNotNull { d ->
                    val route = routeOf(d.type) ?: return@mapNotNull null
                    AudioOutput(d.id, route, nameOf(d, route))
                }
                // One entry per route (e.g. a BT device shows up as SCO and BLE).
                .distinctBy { it.route to it.name }
        }
        val list = ArrayList<AudioOutput>()
        val devices = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        val wired = devices.firstOrNull { routeOf(it.type) == AudioRoute.WIRED }
        if (wired != null) list += AudioOutput(AudioRoute.WIRED.ordinal, AudioRoute.WIRED, nameOf(wired, AudioRoute.WIRED))
        else if (devices.any { it.type == AudioDeviceInfo.TYPE_BUILTIN_EARPIECE }) {
            list += AudioOutput(AudioRoute.EARPIECE.ordinal, AudioRoute.EARPIECE, "Telefon")
        }
        list += AudioOutput(AudioRoute.SPEAKER.ordinal, AudioRoute.SPEAKER, "Lautsprecher")
        devices.firstOrNull { it.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO }?.let {
            list += AudioOutput(AudioRoute.BLUETOOTH.ordinal, AudioRoute.BLUETOOTH, nameOf(it, AudioRoute.BLUETOOTH))
        }
        return list
    }

    private fun best(outputs: List<AudioOutput>): AudioOutput? =
        outputs.firstOrNull { it.route == AudioRoute.BLUETOOTH }
            ?: outputs.firstOrNull { it.route == AudioRoute.WIRED }
            ?: outputs.firstOrNull { it.route == AudioRoute.EARPIECE }
            ?: outputs.firstOrNull { it.route == AudioRoute.SPEAKER }

    /** Applies the chosen (or best) output and publishes the state. */
    private fun apply() {
        if (!active) return
        val outputs = outputs()
        val wanted = chosen?.let { c -> outputs.firstOrNull { it.id == c.id } ?: outputs.firstOrNull { it.route == c.route } }
            ?: best(outputs)
            ?: return
        if (Build.VERSION.SDK_INT >= 31) {
            val device = audio.availableCommunicationDevices.firstOrNull { it.id == wanted.id }
            if (device != null && audio.communicationDevice?.id != device.id) {
                if (!audio.setCommunicationDevice(device)) Log.w(TAG, "setCommunicationDevice(${wanted.name}) refused")
            }
        } else {
            @Suppress("DEPRECATION")
            when (wanted.route) {
                AudioRoute.BLUETOOTH -> {
                    audio.isSpeakerphoneOn = false
                    audio.startBluetoothSco()
                    audio.isBluetoothScoOn = true
                }
                AudioRoute.SPEAKER -> {
                    if (audio.isBluetoothScoOn) { audio.stopBluetoothSco(); audio.isBluetoothScoOn = false }
                    audio.isSpeakerphoneOn = true
                }
                else -> {
                    if (audio.isBluetoothScoOn) { audio.stopBluetoothSco(); audio.isBluetoothScoOn = false }
                    audio.isSpeakerphoneOn = false
                }
            }
        }
        _routing.value = AudioRouting(wanted.route, wanted.id, outputs)
        Log.i(TAG, "audio route: ${wanted.route} (${wanted.name})")
        updateProximity()
    }

    /** The system changed the device on its own (e.g. a BT profile flip): put ours back. */
    private fun onSystemSwitched(device: AudioDeviceInfo?) {
        if (!active || Build.VERSION.SDK_INT < 31) return
        val current = _routing.value
        if (device != null && device.id == current.currentId) return
        if (reapplyCount++ >= MAX_REAPPLY) {
            // Keep what the system insists on and show it truthfully.
            val route = device?.let { routeOf(it.type) } ?: return
            _routing.value = current.copy(current = route, currentId = device.id)
            updateProximity()
            return
        }
        Log.i(TAG, "system switched to ${device?.type}, re-applying ${current.current}")
        main.postDelayed({ apply() }, 200)
    }

    @SuppressLint("WakelockTimeout")
    private fun updateProximity() {
        val wantLock = active && inCall && _routing.value.current == AudioRoute.EARPIECE
        if (wantLock) {
            if (proximity == null && power?.isWakeLockLevelSupported(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK) == true) {
                proximity = power.newWakeLock(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK, "talk:proximity")
                    .apply { setReferenceCounted(false) }
            }
            proximity?.takeIf { !it.isHeld }?.acquire()
        } else {
            releaseProximity()
        }
    }

    private fun releaseProximity() {
        // Wait for the face to move away before turning the screen back on.
        proximity?.takeIf { it.isHeld }?.release(PowerManager.RELEASE_FLAG_WAIT_FOR_NO_PROXIMITY)
    }

    private fun matches(d: AudioDeviceInfo, o: AudioOutput) =
        if (Build.VERSION.SDK_INT >= 31) d.id == o.id else routeOf(d.type) == o.route

    private fun nameOf(d: AudioDeviceInfo, route: AudioRoute): String = when (route) {
        AudioRoute.EARPIECE -> "Telefon"
        AudioRoute.SPEAKER -> "Lautsprecher"
        AudioRoute.WIRED -> "Kopfhörer"
        AudioRoute.BLUETOOTH -> d.productName?.toString()?.takeIf { it.isNotBlank() && it != Build.MODEL } ?: "Bluetooth"
    }

    private companion object {
        const val TAG = "TalkCallAudio"
        const val MAX_REAPPLY = 3

        fun routeOf(type: Int): AudioRoute? = when (type) {
            AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> AudioRoute.EARPIECE
            AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> AudioRoute.SPEAKER
            AudioDeviceInfo.TYPE_WIRED_HEADSET,
            AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
            AudioDeviceInfo.TYPE_USB_HEADSET -> AudioRoute.WIRED
            AudioDeviceInfo.TYPE_BLUETOOTH_SCO -> AudioRoute.BLUETOOTH
            AudioDeviceInfo.TYPE_HEARING_AID -> AudioRoute.BLUETOOTH
            else -> if (Build.VERSION.SDK_INT >= 31 &&
                (type == AudioDeviceInfo.TYPE_BLE_HEADSET || type == AudioDeviceInfo.TYPE_BLE_SPEAKER)
            ) AudioRoute.BLUETOOTH else null
        }
    }
}
