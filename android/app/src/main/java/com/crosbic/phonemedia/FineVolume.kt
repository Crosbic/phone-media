package com.crosbic.phonemedia

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import kotlin.math.abs

class FineVolume(context: Context, private val audio: AudioManager) {
    private val prefs = context.getSharedPreferences("volume", Context.MODE_PRIVATE)
    private val key = "fine:" + Build.FINGERPRINT
    private val main = Handler(Looper.getMainLooper())
    private var supported: Boolean? = when {
        !Build.MANUFACTURER.equals("samsung", ignoreCase = true) -> false
        prefs.contains(key) -> prefs.getBoolean(key, false)
        else -> null
    }
    var probing = false
        private set
    private var last: Int? = null

    val summary: String
        get() = when (supported) {
            true -> "тонкая регулировка Samsung, ${musicMax * STEPS} шагов"
            false -> "$musicMax шагов"
            null -> "проверю тонкую регулировку Samsung, когда музыка не играет"
        }

    private val musicMax get() = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)

    fun active(stream: Int) = stream == AudioManager.STREAM_MUSIC && supported == true && !wireless()

    fun output(): String {
        val device = outputs().firstOrNull() ?: return "телефон"
        return when (device.type) {
            AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "динамик телефона"
            else -> device.productName?.toString()?.takeIf { it.isNotBlank() && it != Build.MODEL } ?: "наушники"
        }
    }

    private fun wireless() = outputs().any { it.type in WIRELESS }

    private fun outputs(): List<AudioDeviceInfo> = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        audio.getAudioDevicesForAttributes(MEDIA)
    } else {
        audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
            .filter { it.type in WIRELESS && @Suppress("DEPRECATION") audio.isBluetoothA2dpOn }
    }

    fun probe(evenWhilePlaying: Boolean, done: () -> Unit) {
        if (supported != null || probing) return
        val stream = AudioManager.STREAM_MUSIC
        if (!evenWhilePlaying && audio.isMusicActive) return
        if (audio.isStreamMute(stream)) return
        val coarse = audio.getStreamVolume(stream)
        if (coarse < PROBE_FROM) return
        probing = true
        audio.setStreamVolume(stream, coarse, FLAG_FINE_VOLUME)
        if (honored(stream)) {
            finish(true, coarse, done)
            return
        }
        main.postDelayed({ finish(honored(stream), coarse, done) }, CHECK_DELAY_MS)
    }

    private fun honored(stream: Int) = audio.getStreamVolume(stream) <= HONORED_UP_TO

    private fun finish(result: Boolean, coarse: Int, done: () -> Unit) {
        if (result) {
            audio.setStreamVolume(AudioManager.STREAM_MUSIC, coarse * STEPS, FLAG_FINE_VOLUME)
            last = coarse * STEPS
        }
        supported = result
        probing = false
        prefs.edit().putBoolean(key, result).apply()
        MediaLog.add("громкость: $summary")
        done()
    }

    fun get(stream: Int): Int {
        val coarse = audio.getStreamVolume(stream)
        last = last?.takeIf { abs(it - coarse * STEPS) <= STEPS / 2 }
        return last ?: (coarse * STEPS)
    }

    fun set(stream: Int, level: Int) {
        val fine = level.coerceIn(0, audio.getStreamMaxVolume(stream) * STEPS)
        audio.setStreamVolume(stream, fine, FLAG_FINE_VOLUME)
        last = fine
    }

    companion object {
        const val STEPS = 10
        private const val FLAG_FINE_VOLUME = 0x100000
        private const val CHECK_DELAY_MS = 300L
        private const val PROBE_FROM = 4
        private const val HONORED_UP_TO = 2
        private val MEDIA = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).build()
        private val WIRELESS = setOf(
            AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
            AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
            AudioDeviceInfo.TYPE_BLE_HEADSET,
            AudioDeviceInfo.TYPE_BLE_SPEAKER,
            AudioDeviceInfo.TYPE_BLE_BROADCAST,
            AudioDeviceInfo.TYPE_HEARING_AID,
        )
    }
}
